//! The import pipeline, which runs without HTTP. The HTTP routes, the
//! server's `import` command and the Demo Account build all run it.
//!
//! [`import_jsonl_files_on_conn`] imports JSONL files into an account in two
//! stages: `staging`, then `promote`. [`OwnedImportRun`] is an Import Run
//! that a process opens and finishes itself. A completed run gets a Saved
//! Search and a Contact Group ([`complete_run_at`]). The end of a run,
//! completed or discarded, hides the account's duplicates
//! ([`dedupe_account`]). [`discard_running_import_and_dedupe`] is the
//! server's `imports discard` command.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use api_types::ImportMode;
use serde::Serialize;
use sqlx::SqliteConnection;
use tempfile::TempDir;

use super::failure::ImportError;
use super::promote;
use super::staging::{self, StagingInserts};
use crate::assets_api::AssetStats;
use crate::config::validate_source_id;
use crate::counts::words;
#[cfg(test)]
use crate::db::engine;
use crate::db::imports::{self, CompleteImportArgs};
use crate::db::maintenance;
use crate::dedupe;
use crate::progress::Progress;
use media::{MediaMode, MediaTools, NoTools};

/// Full import settings: paths, mode, and media handling.
#[derive(Debug, Clone)]
pub struct ImportOptions<'a> {
    /// The account's content-addressed asset store, shared by every source.
    pub assets_dir: &'a Path,
    /// Root for resolving relative attachment paths in JSONL. `None` when
    /// the import carries no files, as an HTTP batch does: a path is then
    /// only a label, still checked as safe, and only a stated SHA-256 finds
    /// an Asset (#2259).
    pub asset_root: Option<&'a Path>,
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Fixed source id (HTTP / `--source` override). Ignored when
    /// `source_from_jsonl`. The source is stamped on each message and does
    /// not decide where files go: every source shares [`Self::assets_dir`].
    pub source: &'a str,
    /// Account the import writes into.
    pub account_id: i64,
    /// Optional Import Run id (messages stamped on promote).
    pub import_id: Option<i64>,
    /// When true, stamp `messages.source` from each conversation's IR `export.source`.
    pub source_from_jsonl: bool,
    /// Attachment handling mode: copy, none, convert, compress.
    pub media: MediaMode,
    /// The ffmpeg and ffprobe that convert and compress run.
    pub media_tools: &'a dyn MediaTools,
    /// When `source_from_jsonl` + Replace: wipe these sources before import.
    pub wipe_sources: Option<Vec<String>>,
    /// The country the run states for every phone number its files write
    /// without a `+` code; `None` keeps such a number's digits as written
    /// (#1676).
    pub phone_country: Option<&'static phone::Country>,
    /// Where the import says how far it has got: standard output for the
    /// `import` command, the server's log for everything else.
    pub progress: Progress,
    /// The instant the import stamps on each contact it changes: when the
    /// batch, or the `import` command's run, started, however long it then
    /// takes.
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Path/mode fields for [`ImportOptions::fixed`].
#[derive(Debug, Clone, Copy)]
pub struct FixedImportArgs<'a> {
    /// Content-addressed asset store directory.
    pub assets_dir: &'a Path,
    /// As [`ImportOptions::asset_root`] says.
    pub asset_root: Option<&'a Path>,
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Fixed source id applied to every conversation.
    pub source: &'a str,
    /// Account the import writes into.
    pub account_id: i64,
    /// Optional Import Run id (messages stamped on promote).
    pub import_id: Option<i64>,
    /// The country the run states for phone numbers written without a `+`
    /// code, as [`ImportOptions::phone_country`] says.
    pub phone_country: Option<&'static phone::Country>,
    /// As [`ImportOptions::now`] says.
    pub now: chrono::DateTime<chrono::Utc>,
}

/// The tools an import that copies its media as it is
/// ([`ImportOptions::fixed`]) is given: none, since it runs neither program.
static COPIED_AS_IS: NoTools = NoTools;

impl<'a> ImportOptions<'a> {
    /// HTTP / tests / reset-demo: fixed source + assets dir, copy media,
    /// progress in the server's log.
    pub fn fixed(args: FixedImportArgs<'a>) -> Self {
        Self {
            assets_dir: args.assets_dir,
            asset_root: args.asset_root,
            mode: args.mode,
            source: args.source,
            account_id: args.account_id,
            import_id: args.import_id,
            source_from_jsonl: false,
            media: MediaMode::Clone,
            media_tools: &COPIED_AS_IS,
            wipe_sources: None,
            phone_country: args.phone_country,
            progress: Progress::Log,
            now: args.now,
        }
    }
}

/// Counters for one import run (staging and promote results).
#[derive(Debug, Default, Clone, Serialize, utoipa::ToSchema)]
pub struct ImportCounts {
    /// Conversations imported.
    pub conversations: u64,
    /// Participant rows imported.
    pub participants: u64,
    /// Messages imported.
    pub messages: u64,
    /// Attachment records (message–media links) imported.
    pub attachments: u64,
    /// Tapback reactions imported.
    pub tapbacks: u64,
    /// JSONL files imported.
    pub files: u64,
    /// Unique media files written to the asset store.
    pub assets_copied: u64,
    /// Media files already present under the same fingerprint, skipped.
    pub assets_deduped: u64,
    /// Attachment files referenced but not found on disk.
    pub assets_missing: u64,
    /// Contacts the import created for participants nothing else owned.
    pub contacts_created: u64,
    /// Messages hidden as duplicates within this import.
    pub messages_deduped: u64,
    /// Messages added by an append-mode import.
    pub messages_appended: u64,
    /// Import mode.
    pub mode: ImportMode,
    /// Flagged phone identities (ambiguous; review note set) inserted by this import.
    pub phones_needing_review: u64,
    /// Identities of type `other` this import met for people: a name the
    /// backup gave with no address, or a sender such as `AMAZON`. Each one is
    /// a person the exporter could not tie to an address, so a count above
    /// zero says the import is incomplete.
    pub other_identities: u64,
}

impl ImportCounts {
    /// Add one staged file's counts onto the running import totals.
    pub(super) fn merge_file(&mut self, other: &ImportCounts) {
        self.conversations += other.conversations;
        self.participants += other.participants;
        self.messages += other.messages;
        self.attachments += other.attachments;
        self.tapbacks += other.tapbacks;
        self.contacts_created += other.contacts_created;
        self.messages_deduped += other.messages_deduped;
        self.phones_needing_review += other.phones_needing_review;
        self.other_identities += other.other_identities;
    }

    /// Add a whole import run's counts onto a running total, files and assets
    /// included; the demo reset imports three sources in turn.
    pub fn add_run(&mut self, other: &ImportCounts) {
        self.merge_file(other);
        self.files += other.files;
        self.assets_copied += other.assets_copied;
        self.assets_deduped += other.assets_deduped;
        self.assets_missing += other.assets_missing;
        self.messages_appended += other.messages_appended;
    }
}

/// An Import Run this process opened, as opposed to one a client
/// (the `import` crate) owns and closes itself. Whoever starts one must
/// finish it whatever the import does, so the Settings import table never
/// shows a run stuck in progress.
pub(crate) struct OwnedImportRun {
    account_id: i64,
    /// The `imports` row id; the import stamps it on every message.
    pub id: i64,
}

impl OwnedImportRun {
    /// Record a run at the parse stage. Nothing client-side (staging
    /// directory, device, form) is known for a run started here.
    ///
    /// # Errors
    ///
    /// Returns an error when the account already has a running Import Run or the
    /// row cannot be inserted.
    pub(crate) async fn start(
        conn: &mut SqliteConnection,
        account_id: i64,
        source: &str,
        mode: ImportMode,
        tool: &str,
    ) -> std::result::Result<Self, imports::StartImportError> {
        let id = imports::start_import(
            conn,
            &imports::StartImportArgs::new(account_id, source, mode.as_str(), Some(tool)),
            chrono::Utc::now(),
        )
        .await?;
        Ok(Self { account_id, id })
    }

    /// Mark the run succeeded with its counts, or failed. Not
    /// being able to record the outcome is a warning on stderr, never an
    /// error: the import's own result is what the caller returns.
    pub(crate) async fn finish(self, conn: &mut SqliteConnection, result: &Result<ImportCounts>) {
        let outcome = match result {
            Ok(counts) => CompleteImportArgs::succeeded(counts.messages, counts.attachments),
            Err(_) => CompleteImportArgs::failed(),
        };
        // The import itself is done either way; a failure to record that is
        // worth a log line, not an error the caller would have to unwind.
        if let Err(error) = complete_run(conn, self.account_id, self.id, &outcome).await {
            tracing::warn!(import_id = self.id, error = %error, "The Import Run could not be recorded as finished");
        }
    }
}

/// [`complete_run_at`] at the current instant, for the server's `import`
/// command and the Demo Account build (through [`OwnedImportRun::finish`]).
///
/// # Errors
///
/// The errors of [`complete_run_at`].
pub(crate) async fn complete_run(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
    outcome: &CompleteImportArgs,
) -> std::result::Result<imports::ImportRow, imports::ImportLookupError> {
    complete_run_at(conn, account_id, import_id, outcome, chrono::Utc::now()).await
}

/// Record an Import Run's outcome at the instant `now`, then make the run's
/// Saved Search and Contact Group, named after `now`'s day. Every path that
/// completes a run reaches this: the HTTP `complete_import`, which queues the
/// run's Assets at the same instant, and the server's `import` command and
/// the Demo Account build through [`complete_run`]. So each run gets the
/// same shortcuts however it was made.
///
/// # Errors
///
/// Returns the error of recording the outcome ([`imports::complete_import`]):
/// a run that is not found or not running, kept apart from a database fault
/// so the handler maps each to its status. A shortcut that cannot be made is
/// a warning, never an error.
pub(super) async fn complete_run_at(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
    outcome: &CompleteImportArgs,
    now: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<imports::ImportRow, imports::ImportLookupError> {
    let row = imports::complete_import(conn, account_id, import_id, outcome, now).await?;
    create_import_saved_search(conn, account_id, &row).await;
    create_import_contact_group(conn, account_id, &row, now).await;
    Ok(row)
}

/// Test helper: open a configured database, make sure of its schema, and run
/// one import.
///
/// Production paths run [`import_jsonl_files_on_conn`] on their own
/// connection: HTTP serve, the `import` command and the Demo Account build.
#[cfg(test)]
pub(crate) async fn import_jsonl_files(
    db_path: &Path,
    paths: &[PathBuf],
    opts: &ImportOptions<'_>,
) -> Result<ImportCounts> {
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let pool = engine::open_pool_for_path(db_path)
        .await
        .with_context(|| format!("failed to open database {}", db_path.display()))?;
    let mut conn = pool.acquire().await?;
    crate::db::schema::ensure_schema(&mut conn).await?;
    // The import itself never makes the account (#2061); a test that names
    // one gets it here, as the `import` command makes it.
    crate::db::account_profile::create_account_row_if_missing(&mut conn, opts.account_id).await?;
    opts.progress
        .say(format_args!("Opened the database at {}", db_path.display()));
    Ok(import_jsonl_files_on_conn(&mut conn, paths, opts).await?)
}

/// Import onto an existing connection: HTTP serve, the `import` command, the
/// Demo Account build, and tests.
///
/// A command line that reports the error turns it into an `anyhow::Error`
/// and prints it with `{:#}`, which names the file a refusal was in and the
/// whole chain of an internal failure.
///
/// # Errors
///
/// Returns [`ImportError::Rejected`] when the file breaks a rule the sender
/// can fix, [`ImportError::AccountGone`] when the account has no row,
/// [`ImportError::Run`] when the import run is no longer running,
/// and [`ImportError::Internal`] when the options are invalid or
/// staging or promote fails for any other reason.
pub async fn import_jsonl_files_on_conn(
    conn: &mut SqliteConnection,
    paths: &[PathBuf],
    opts: &ImportOptions<'_>,
) -> Result<ImportCounts, ImportError> {
    let wipe_sources = prepare_import(conn, opts)
        .await
        .map_err(ImportError::Internal)?;
    opts.progress.say(format_args!(
        "Importing {}",
        crate::counts::words(paths.len() as u64, "1 JSONL file", "{n} JSONL files")
    ));
    if opts.mode == ImportMode::Replace && !wipe_sources.is_empty() {
        let names = wipe_sources.join(", ");
        opts.progress.say(crate::counts::words(
            wipe_sources.len() as u64,
            &format!(
                "The account's messages from source {names} are deleted if the import succeeds"
            ),
            &format!(
                "The account's messages from the {{n}} sources {names} are deleted if the import succeeds"
            ),
        ));
    }

    let mut counts = ImportCounts {
        mode: opts.mode,
        ..Default::default()
    };
    let started = Instant::now();
    // Stats on already-committed rows so promote's guid join can use the
    // indexes. Outside the transaction, because a failed ANALYZE is only a
    // warning.
    maintenance::analyze_import_tables(conn, opts.progress).await;

    // Staging and promote share one transaction. Staging makes contacts and,
    // by ADR-0013, discards trashed ones; none of that may outlive a promote
    // that fails. The write lock is taken up front (IMMEDIATE) so two
    // imports for different accounts cannot race into SQLITE_BUSY at the
    // first INSERT. The account and the run are checked again under that
    // lock: an account deleted, or a run discarded or completed, while the
    // batch uploaded or waited for its turn takes no messages (#2061).
    let mut tx = crate::db::begin_write(conn)
        .await
        .map_err(|err| ImportError::Internal(err.into()))?;
    if !crate::db::account_profile::account_exists(&mut tx, opts.account_id)
        .await
        .map_err(ImportError::Internal)?
    {
        return Err(ImportError::AccountGone);
    }
    if let Some(import_id) = opts.import_id {
        crate::db::imports::require_running_import(&mut tx, opts.account_id, import_id).await?;
    }
    let asset_stats = stage_all_files(&mut tx, paths, opts, &mut counts, started).await?;

    opts.progress.say(format_args!(
        "Writing what the files hold into the account, {:.0} s after the import started…",
        started.elapsed().as_secs_f64()
    ));
    promote_step(&mut tx, opts, &wipe_sources, &mut counts).await?;
    crate::db::staging::reset_for_account(&mut tx, opts.account_id)
        .await
        .map_err(ImportError::Internal)?;
    tx.commit()
        .await
        .map_err(|err| ImportError::Internal(err.into()))?;

    counts.assets_copied = asset_stats.copied;
    counts.assets_deduped = asset_stats.deduped;
    counts.assets_missing = asset_stats.missing;
    opts.progress.say(format_args!(
        "The import finished in {:.1} s, with {}, {}, {} and {}",
        started.elapsed().as_secs_f64(),
        words(counts.files, "1 file", "{n} files"),
        words(counts.messages, "1 message", "{n} messages"),
        words(counts.attachments, "1 attachment", "{n} attachments"),
        words(counts.assets_copied, "1 Asset copied", "{n} Assets copied"),
    ));
    Ok(counts)
}

/// Everything an import does before its transaction: the asset store
/// directory, an empty staging
/// area, and the source ids a replace-mode import wipes, which it returns.
/// None of it reads the files, so every error here is the server's.
///
/// # Errors
///
/// Returns an error when a directory or row cannot be written, or a source
/// id is invalid.
async fn prepare_import(
    conn: &mut SqliteConnection,
    opts: &ImportOptions<'_>,
) -> Result<Vec<String>> {
    fs::create_dir_all(opts.assets_dir)
        .with_context(|| format!("failed to create {}", opts.assets_dir.display()))?;

    opts.progress.say("Clearing the account's staging tables…");
    crate::db::staging::reset_for_account(conn, opts.account_id).await?;
    sources_to_wipe(opts)
}

/// The source ids a replace-mode import wipes once staging succeeds: the
/// ones found in the files, or the single `--source` override. Append mode
/// wipes nothing.
///
/// # Errors
///
/// Returns an error when a source id is invalid.
fn sources_to_wipe(opts: &ImportOptions<'_>) -> Result<Vec<String>> {
    let wipe_sources = match (opts.mode, opts.source_from_jsonl) {
        (ImportMode::Replace, true) => opts.wipe_sources.clone().unwrap_or_default(),
        (ImportMode::Replace, false) => vec![opts.source.to_string()],
        (ImportMode::Append, _) => Vec::new(),
    };
    for source in &wipe_sources {
        validate_source_id(source)?;
    }
    Ok(wipe_sources)
}

/// Stage every file into the staging tables inside the import's transaction
/// `tx`, and print progress along the way.
///
/// # Errors
///
/// Returns [`ImportError::Rejected`], naming the file, when a line breaks a
/// rule the sender can fix, and [`ImportError::Internal`] when a file cannot
/// be read or a row cannot be written.
async fn stage_all_files(
    tx: &mut SqliteConnection,
    paths: &[PathBuf],
    opts: &ImportOptions<'_>,
    counts: &mut ImportCounts,
    started: Instant,
) -> Result<AssetStats, ImportError> {
    let total_files = paths.len();
    let progress_every = if total_files <= 20 {
        1usize
    } else {
        (total_files / 40).max(10)
    };
    let media_work = TempDir::new()
        .context("temp dir for import-time media rewrite")
        .map_err(ImportError::Internal)?;
    let mut asset_stats = AssetStats::default();
    let identities = crate::db::account_profile::account_identity_keys(tx, opts.account_id)
        .await
        .map_err(ImportError::Internal)?;
    let mut stmts = StagingInserts::new(
        opts.account_id,
        opts.import_id,
        identities,
        opts.phone_country,
        opts.now,
    );

    for (idx, path) in paths.iter().enumerate() {
        let file_counts = staging::import_file_to_staging(
            tx,
            &mut stmts,
            opts,
            path,
            &mut asset_stats,
            media_work.path(),
        )
        .await
        .map_err(|err| ImportError::staging(path, err))?;
        counts.merge_file(&file_counts);
        counts.files += 1;

        let n = idx + 1;
        if n == 1 || n == total_files || n % progress_every == 0 {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
            opts.progress.say(format_args!(
                "Read {name}, file {n} of {total_files}. So far: {}, {}, {} and {}, in {:.0} s",
                words(counts.messages, "1 message", "{n} messages"),
                words(counts.attachments, "1 attachment", "{n} attachments"),
                words(asset_stats.copied, "1 Asset copied", "{n} Assets copied"),
                words(asset_stats.missing, "1 Asset missing", "{n} Assets missing"),
                started.elapsed().as_secs_f64()
            ));
        }
    }
    if let Some(reason) = asset_stats.hevc_fallback.reason() {
        // The reason names the files ffmpeg was given as `<input>` and
        // `<output>`, never by their paths, so the log holds no file name
        // (`media::HevcFallback::reason`).
        tracing::warn!(
            import_id = opts.import_id,
            reason,
            "HEVC encoding failed, so videos were compressed as H.264 instead"
        );
    }
    Ok(asset_stats)
}

/// Move staged rows into production and fold the promote counts into `counts`.
///
/// In append mode the promote step is the only place the final row counts
/// are known, so they replace the staging counts.
///
/// # Errors
///
/// Returns [`PromoteError`](promote::PromoteError) when a promote statement
/// fails.
async fn promote_step(
    tx: &mut SqliteConnection,
    opts: &ImportOptions<'_>,
    wipe_sources: &[String],
    counts: &mut ImportCounts,
) -> Result<(), promote::PromoteError> {
    let promote_stats = promote::promote_append(
        tx,
        opts.mode,
        opts.account_id,
        opts.import_id,
        wipe_sources,
        opts.progress,
    )
    .await?;
    counts.messages_deduped += promote_stats.messages_deduped;
    counts.messages_appended = promote_stats.messages_appended;
    if opts.mode == ImportMode::Append {
        counts.conversations = promote_stats.conversations;
        counts.participants = promote_stats.participants;
        counts.messages = promote_stats.messages;
        counts.attachments = promote_stats.attachments;
        counts.tapbacks = promote_stats.tapbacks;
    }
    Ok(())
}

/// The full dedupe of `account`, within one source and across sources,
/// as the end of an Import Run runs it.
pub(super) async fn dedupe_account(
    conn: &mut SqliteConnection,
    account: i64,
    progress: Progress,
) -> Result<dedupe::DedupeStats> {
    dedupe::dedupe_cross_source(conn, account, None, dedupe::NEAR_WINDOW_SECS, progress).await
}

/// Discard `account`'s running Import Run `import_id` at the instant `now`
/// with its `issues` and `notes`, then hide the duplicates of the account.
/// Every discard comes here: the route's and the `imports discard`
/// command's. The run has ended before the dedupe starts, so a dedupe that
/// fails is a warning, never an error: the discard stands, and the next
/// run's end hides the duplicates.
///
/// # Errors
///
/// The errors of [`imports::discard_import`].
pub(super) async fn discard_and_dedupe(
    conn: &mut SqliteConnection,
    account: i64,
    import_id: i64,
    issues: &[imports::ImportIssueInput],
    notes: &[imports::ImportNoteRow],
    now: chrono::DateTime<chrono::Utc>,
    progress: Progress,
) -> std::result::Result<(), imports::ImportLookupError> {
    imports::discard_import(conn, account, import_id, issues, notes, now).await?;
    if let Err(error) = dedupe_account(conn, account, progress).await {
        tracing::warn!(
            account,
            error = %error,
            "The duplicates of a discarded Import Run could not be hidden"
        );
    }
    Ok(())
}

/// Discard `account`'s running Import Run, whichever it is, at the instant
/// `now`, then hide the duplicates of the account ([`discard_and_dedupe`]),
/// and return the row as it was before the discard; `None`, with no
/// dedupe, when the account has no running run. This is the server's
/// `imports discard` command, the way out of a run a killed client left
/// open: the operator knows the account, not the run id. The run's batches
/// did not hide their duplicates (#2040), so its end does, as the discard
/// route's does.
///
/// # Errors
///
/// Returns the error of the lookup or of the discard.
pub(crate) async fn discard_running_import_and_dedupe(
    conn: &mut SqliteConnection,
    account: i64,
    now: chrono::DateTime<chrono::Utc>,
    progress: Progress,
) -> Result<Option<imports::ImportRow>> {
    let Some(running) = imports::running_import(conn, account).await? else {
        return Ok(None);
    };
    discard_and_dedupe(conn, account, running.id, &[], &[], now, progress)
        .await
        .map_err(|err| anyhow::anyhow!(err))?;
    Ok(Some(running))
}

/// Add the sidebar shortcut to the messages this run brought in.
///
/// Only runs that stored something get one: a saved search matching nothing is
/// worse than no saved search, and a run that stored nothing is still visible
/// in Import History either way.
///
/// The shortcut is a convenience, not a record, so a failure here is logged and
/// the import still reports success. The person may delete the saved search
/// afterwards; the `imports` row it points at is permanent.
async fn create_import_saved_search(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    row: &crate::db::imports::ImportRow,
) {
    if row.message_count <= 0 {
        return;
    }
    let date_ymd = import_date_ymd(row);
    if let Err(e) = crate::db::saved_searches::create_for_import(
        conn,
        account_id,
        row.id,
        &row.source,
        &date_ymd,
    )
    .await
    {
        tracing::warn!(
            import_id = row.id,
            messages = row.message_count,
            error = ?e,
            "The Import Run's Saved Search could not be created"
        );
    }
}

/// Create the Contact Group naming the contacts an import run touched.
///
/// The group is a shortcut pointing at the run: the person may delete it, and
/// the `imports` row it describes is permanent either way. Membership is
/// a snapshot, because what a run brought in is a historical fact that should
/// not silently rewrite itself as contacts change later.
///
/// A failure here is reported and swallowed, the same as the saved search: the
/// messages are already in the database, and losing a shortcut is not a reason to
/// call the import failed. Each member's `last_modified` is set to `now`.
async fn create_import_contact_group(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    row: &crate::db::imports::ImportRow,
    now: chrono::DateTime<chrono::Utc>,
) {
    let touched = match crate::db::import_contacts::contact_ids(conn, row.id).await {
        Ok(ids) if ids.is_empty() => return,
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(
                import_id = row.id,
                error = %crate::api_error::error_chain(&e),
                "The Import Run could not list the contacts it touched"
            );
            return;
        }
    };
    let group_id = match crate::db::contacts::create_import_group(
        conn,
        account_id,
        &import_contact_group_name(row),
    )
    .await
    {
        Ok((id, _)) => id,
        Err(e) => {
            tracing::warn!(
                import_id = row.id,
                error = %crate::api_error::error_chain(&e),
                "The Import Run's Contact Group could not be created"
            );
            return;
        }
    };
    if let Err(e) = crate::db::named_membership::patch_members(
        crate::db::named_membership::group_spec(),
        conn,
        account_id,
        group_id,
        &touched,
        &[],
        now,
    )
    .await
    {
        tracing::warn!(
            import_id = row.id,
            contacts = touched.len(),
            error = ?e,
            "The Import Run's Contact Group was created, but its contacts could not be added"
        );
    }
}

/// Name an Import Run's Contact Group starts from: the source and the day
/// the run finished. `contact_groups.name` is unique per account, and each
/// run gets a group of its own, so `create_import_group` adds " 2", " 3", …
/// when the name is taken.
pub(super) fn import_contact_group_name(row: &crate::db::imports::ImportRow) -> String {
    format!("{} import {}", row.source, import_date_ymd(row))
}

/// [`import_contact_group_name`] as an SQL expression over an `imports` row
/// aliased `i`, for a test query that finds a run's Contact Group. It falls
/// back from `finished_at` to `started_at` as [`import_date_ymd`] does, and
/// leaves out the fall back to today, which only a timestamp shorter than a
/// date takes, and the " 2" a taken name gets.
#[cfg(test)]
pub(crate) const IMPORT_CONTACT_GROUP_NAME_SQL: &str =
    "i.source || ' import ' || substr(coalesce(i.finished_at, i.started_at), 1, 10)";

/// Calendar date to name an import's saved search after: the day the run
/// finished, falling back to the day it started, then to today. All three are
/// UTC, because that is what `imports` stores.
fn import_date_ymd(row: &crate::db::imports::ImportRow) -> String {
    row.finished_at
        .as_deref()
        .or(Some(row.started_at.as_str()))
        .and_then(|rfc3339| rfc3339.get(..10))
        .filter(|d| d.len() == 10)
        .map_or_else(
            || chrono::Utc::now().format("%Y-%m-%d").to_string(),
            str::to_string,
        )
}
