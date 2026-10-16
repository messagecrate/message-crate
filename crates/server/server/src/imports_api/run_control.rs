//! How the server schedules an import's work: the import slots that bound
//! how many imports run at once, the batch that runs in a slot
//! ([`run_import_path`]), and the end of an Import Run, which takes a slot
//! as a batch does.

use std::path::PathBuf;
use std::sync::Arc;

use api_types::ImportMode;
use sqlx::SqliteConnection;
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

use super::dto::CreateImportBatchResponse;
use super::pipeline::{
    FixedImportArgs, ImportOptions, complete_run_at, dedupe_account, discard_and_dedupe,
    import_jsonl_files_on_conn,
};
use crate::api_error::{ApiError, run_blocking};
use crate::config::Config;
use crate::db::imports::{self, CompleteImportArgs};
use crate::extract::Json;
use crate::media_queue::MediaQueue;
use crate::progress::Progress;
use crate::request_body::UploadTimeLimits;
use crate::server::{AppState, ImportState};

/// What a handler that runs or ends an Import Run reads: the database, the
/// configuration, the import locks and slots, the upload time limits, and
/// the queue the run's Assets go on for their Thumbnails and Previews. Cut
/// from [`AppState`] for each request, so it never holds a copy of its own.
#[derive(Debug, Clone)]
pub(crate) struct Importer {
    pub(crate) db: sqlx::SqlitePool,
    pub(crate) cfg: Arc<Config>,
    pub(crate) imports: ImportState,
    pub(crate) upload_time_limits: UploadTimeLimits,
    pub(crate) media_queue: MediaQueue,
}

impl axum::extract::FromRef<AppState> for Importer {
    fn from_ref(state: &AppState) -> Self {
        Self {
            db: state.db.clone(),
            cfg: Arc::clone(&state.cfg),
            imports: state.imports.clone(),
            upload_time_limits: state.upload_time_limits,
            media_queue: state.media_queue.clone(),
        }
    }
}

/// What one batch imports under. Every field is read from the Import Run's
/// row, never from the request: the run was created with them once, so two
/// batches cannot disagree.
#[derive(Debug, Clone)]
pub(crate) struct BatchContext {
    pub(crate) account: i64,
    pub(crate) import_id: i64,
    pub(crate) source: String,
    pub(crate) mode: ImportMode,
    pub(crate) phone_country: Option<&'static phone::Country>,
}

impl BatchContext {
    /// The context a running row gives a batch. A mode the row spells in a
    /// way the enum does not know is read as `append`, the mode that never
    /// removes anything.
    pub(crate) fn from_row(row: &crate::db::imports::ImportRow) -> Self {
        let mode = match row.mode.as_str() {
            "replace" => ImportMode::Replace,
            _ => ImportMode::Append,
        };
        Self {
            account: row.account_id,
            import_id: row.id,
            source: row.source.clone(),
            mode,
            // The code was checked when the run was created, so one the
            // table no longer holds can only come from a rebuilt server:
            // the run then states none.
            phone_country: row.phone_country.as_deref().and_then(phone::country),
        }
    }
}

/// How an Import Run ends: completed with its outcome, or discarded with
/// the Import Errors and notes it recorded before it was given up.
pub(super) enum RunEnd {
    Complete(CompleteImportArgs),
    Discard {
        issues: Vec<crate::db::imports::ImportIssueInput>,
        notes: Vec<crate::db::imports::ImportNoteRow>,
    },
}

/// End `account`'s running Import Run `import_id` as `end` says at the
/// instant `now`, hide the duplicates of the account, and return the run's
/// row as it now stands. Both routes that end a run come here.
///
/// The full dedupe runs once per Import Run, as it ends, and not after each
/// batch: each run of it works over the whole account, so one after every
/// batch made an import's cost grow with the account for every batch
/// (#2040). Duplicates may show between batches while a run is open.
///
/// A completion hides the duplicates first, so a dedupe that fails leaves
/// the run running and the client can complete it again, rather than a run
/// that completed with its duplicates shown. A discard ends the run first
/// and then hides them ([`discard_and_dedupe`]), so a dedupe that fails
/// every time never keeps the account's one running run, and with it every
/// later import, from being given up.
///
/// It holds an import slot and the account's import lock, as a batch does,
/// in the same order (slot, account, pool): the pass holds one pooled
/// connection and the write lock for as long as it takes, and no batch of
/// the run can land while the run ends. The run is checked first, so a
/// request for a run that is not running is refused before the pass. Like a
/// batch, it runs off the async workers, because hashing every content key
/// again is CPU work. The task also does what follows a run's end
/// ([`after_run_ended`]) once the end is written, so it happens even when
/// the client gave up waiting and the request is gone.
///
/// # Errors
///
/// [`ApiError::ServerStopping`] when the server is stopping, the
/// [`imports::ImportLookupError`] of a run that is not the account's or not
/// running, and the error of a completion's dedupe or of the write that
/// ends the run.
pub(super) async fn end_run(
    importer: &Importer,
    account: i64,
    import_id: i64,
    end: RunEnd,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<imports::ImportRow, ApiError> {
    let permit = take_import_slot(&importer.imports).await?;
    let importer = importer.clone();
    let handle = tokio::runtime::Handle::current();
    run_blocking("end import run", move || {
        handle.block_on(async move {
            // The slot, the lock and the connection go at the end of this
            // block, before the work that follows a run's end.
            {
                let _permit = permit;
                let _guard = importer
                    .imports
                    .account_locks
                    .lock(account.to_string())
                    .await;
                let mut conn = importer.db.acquire().await?;
                end_and_dedupe(&mut conn, account, import_id, end, now).await?;
            }
            after_run_ended(&importer, account, import_id, now).await;
            let mut conn = importer.db.acquire().await?;
            Ok::<_, ApiError>(imports::get_owned_import(&mut conn, account, import_id).await?)
        })
    })
    .await
}

/// End the running Import Run `import_id` as `end` says, with the dedupe on
/// the side [`end_run`] gives each kind of end: before a completion, after
/// a discard.
///
/// # Errors
///
/// The [`imports::ImportLookupError`] of a run that is not the account's or
/// not running, and the error of a completion's dedupe or of the write that
/// ends the run.
async fn end_and_dedupe(
    conn: &mut SqliteConnection,
    account: i64,
    import_id: i64,
    end: RunEnd,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), ApiError> {
    imports::require_running_import(conn, account, import_id).await?;
    match end {
        RunEnd::Complete(args) => {
            dedupe_account(conn, account, Progress::Log).await?;
            complete_run_at(conn, account, import_id, &args, now).await?;
        }
        RunEnd::Discard { issues, notes } => {
            discard_and_dedupe(
                conn,
                account,
                import_id,
                &issues,
                &notes,
                now,
                Progress::Log,
            )
            .await?;
        }
    }
    Ok(())
}

/// What follows the end of an Import Run, once its end is written: sweep
/// the account's files no row names, and queue the run's Assets for their
/// Thumbnails and Previews. Neither fails: each logs what it could not do.
async fn after_run_ended(
    importer: &Importer,
    account: i64,
    import_id: i64,
    now: chrono::DateTime<chrono::Utc>,
) {
    crate::asset_store::sweep_after_run(
        &importer.db,
        &importer.cfg.paths,
        account,
        importer.upload_time_limits.stall,
    )
    .await;
    crate::media_queue::queue_import_run(
        &importer.db,
        &importer.media_queue,
        account,
        import_id,
        now,
    )
    .await;
}

/// Bound on concurrent HTTP imports: each import holds one pooled connection
/// for its whole run, so at most this many may overlap and the remaining
/// connections stay available for auth, search, and export.
pub(super) const MAX_CONCURRENT_IMPORTS: usize = 2;

/// A new server's import slots, [`MAX_CONCURRENT_IMPORTS`] of them, which
/// `AppState::new` holds as `ImportState::slots`: one count per server.
pub(crate) fn import_slots() -> Arc<tokio::sync::Semaphore> {
    Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_IMPORTS))
}

/// Take one of the server's import slots, waiting while every one is held.
///
/// An import batch holds one pooled connection for its whole run (JSONL
/// parse, asset IO, promote), and so does the dedupe that ends an Import Run
/// (`end_run`), so the slots bound concurrent imports and they can
/// never drain the pool. A batch takes its slot before it reads its body,
/// so the batches waiting behind the running ones hold nothing on disk, and
/// before the per-account lock, so lock order (slot → account → pool) is
/// consistent everywhere. The slots close when the server is told to stop
/// (`AppState::on_stop_signal`), so a batch still waiting here is refused
/// and the Upload retries it, rather than making the stop wait for it.
///
/// # Errors
///
/// Returns [`ApiError::ServerStopping`] when the slots closed.
pub(super) async fn take_import_slot(
    imports: &ImportState,
) -> Result<OwnedSemaphorePermit, ApiError> {
    Arc::clone(&imports.slots)
        .acquire_owned()
        .await
        .map_err(|_| server_stopping())
}

/// The refusal of a batch, or of the end of an Import Run, that the server
/// did not start because it is stopping.
pub(super) fn server_stopping() -> ApiError {
    ApiError::ServerStopping(
        "the server is stopping, so it did not start this request; send it again once the server is running".into(),
    )
}

/// `create_import_batch` is the only entry point, and the run's `source`,
/// validated when the run was created, names an on-disk directory. The
/// caller holds the batch's import slot until this returns.
///
/// `client_gone` is canceled when the batch's client stops waiting. A
/// batch still waiting for the account's import lock then returns at once
/// and writes nothing, so its caller frees its slot: nobody waits for its
/// answer, and the Upload sends the batch again. Once the batch holds the
/// lock it no longer looks, so an import never stops part-way (#2090).
///
/// # Errors
///
/// [`ApiError::Internal`] for a batch whose client went before it took
/// the lock, which no client reads, and the error of the import.
pub(super) async fn run_import_path(
    importer: Importer,
    context: BatchContext,
    jsonl_path: PathBuf,
    client_gone: CancellationToken,
) -> Result<Json<CreateImportBatchResponse>, ApiError> {
    let cfg = Arc::clone(&importer.cfg);
    let BatchContext {
        account,
        import_id,
        source: source_id,
        mode: run_mode,
        phone_country,
    } = context;

    // `biased` looks at the client first, so a batch whose client went
    // before its turn came does not start, even when the lock is free.
    let _guard = tokio::select! {
        biased;
        () = client_gone.cancelled() => {
            tracing::info!(
                import_id,
                "The client of a batch stopped waiting before its import started, so the batch was not imported"
            );
            return Err(ApiError::Internal(anyhow::anyhow!(
                "the client stopped waiting before the batch's import started"
            )));
        }
        guard = importer.imports.account_locks.lock(account.to_string()) => guard,
    };
    #[cfg(test)]
    drop(importer.imports.after_account_lock.read().await);

    // One pooled connection held for the whole import; the batch's import
    // slot keeps enough of the pool free for other requests.
    let mut conn = importer.db.acquire().await?;

    // A `replace` run wipes the source once, on its first batch; every batch
    // after that appends. The row's stamped messages say which this is.
    let mode = if run_mode == ImportMode::Replace
        && !crate::db::imports::has_messages(&mut conn, import_id).await?
    {
        ImportMode::Replace
    } else {
        ImportMode::Append
    };

    // The import body never carries files of its own: attachments are
    // uploaded first through `/v1/assets` and found by their SHA-256. An
    // attachment's `path` is only a label and resolves nowhere, never in the
    // asset store, where it could name an upload in progress or a media type
    // sidecar and store those bytes (#2259). The path check still refuses an
    // unsafe path.
    let assets_dir = cfg.paths.assets_dir_for_account(account);

    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets_dir,
        asset_root: None,
        mode,
        source: &source_id,
        account_id: account,
        import_id: Some(import_id),
        phone_country,
        now: chrono::Utc::now(),
    });
    let import_result = import_jsonl_files_on_conn(&mut conn, &[jsonl_path], &opts).await;
    let counts = import_result?;
    // No dedupe here: the run's end hides the duplicates, once for the
    // whole run (`end_run`, #2040).

    Ok(Json(CreateImportBatchResponse {
        source: source_id,
        account,
        counts,
    }))
}
