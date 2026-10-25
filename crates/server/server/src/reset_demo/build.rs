//! The Demo Account build every caller runs: generate the Demo Data, remove
//! the old Demo Account, write its row, import the three sources, load its
//! address book, dedupe, and make its Previews and Thumbnails.

use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use demo_seed::DemoSize;
use demo_seed::bundle::{DemoSeed, IMESSAGE_SOURCE, SBR_SOURCE, WHATSAPP_SOURCE, read_seed_toml};
use ir::IdentityType;
use sqlx::SqlitePool;

use super::{DEMO_ACCOUNT_ID, console};
use crate::config::Config;
use crate::db::account_profile::{self, MessageBatch, MessagesToDelete};
use crate::db::address_book::{self, LoadCounts, LoadMode};
use crate::db::audit_trail::AuditActor;
use crate::db::demo_account_build::{self, DemoDataSize};
use crate::db::maintenance;
use crate::dedupe;
use crate::imports_api::pipeline::{self, FixedImportArgs, ImportOptions};
use crate::open_db::OpenDb;
use crate::process_assets::{self, ProcessAssetsOptions};
use crate::progress::Progress;
use api_types::ImportMode;

pub(super) struct PreparedBundle {
    pub(super) seed: DemoSeed,
    pub(super) imessage_dir: PathBuf,
    pub(super) sbr_dir: PathBuf,
    pub(super) whatsapp_dir: PathBuf,
    pub(super) contacts_csv: PathBuf,
}

/// One per-source import in a demo reset. [`import_demo_sources`] loops over
/// [`DEMO_IMPORT_SOURCES`], so the sources, their order, and their
/// Replace-then-Append modes are written down once.
pub(super) struct DemoImportSource {
    /// Label printed in the table [`console::say_reset_header`] says.
    pub(super) label: &'static str,
    /// Source id recorded on the imported conversations.
    pub(super) source: &'static str,
    /// The directory of conversation files inside the prepared bundle.
    pub(super) export_dir: fn(&PreparedBundle) -> &PathBuf,
    /// The first source replaces the demo account's data; the rest append.
    pub(super) mode: ImportMode,
}

pub(super) const DEMO_IMPORT_SOURCES: [DemoImportSource; 3] = [
    DemoImportSource {
        label: "imessage",
        source: IMESSAGE_SOURCE,
        export_dir: |bundle| &bundle.imessage_dir,
        mode: ImportMode::Replace,
    },
    DemoImportSource {
        label: "android",
        source: SBR_SOURCE,
        export_dir: |bundle| &bundle.sbr_dir,
        mode: ImportMode::Append,
    },
    DemoImportSource {
        label: "whatsapp",
        source: WHATSAPP_SOURCE,
        export_dir: |bundle| &bundle.whatsapp_dir,
        mode: ImportMode::Append,
    },
];

/// Shared post-import tail: fill dedupe content keys, convert the account's
/// media, and warn (but continue) when some attachments fail conversion.
/// Only `account_id` is converted: on a running server every other account
/// is someone's, with Uploads of its own in progress.
pub(super) async fn dedupe_and_process_assets(
    cfg: &Config,
    db: &SqlitePool,
    account_id: i64,
    stop: &AtomicBool,
    progress: Progress,
) -> Result<(dedupe::DedupeStats, process_assets::ProcessAssetsStats)> {
    let dedupe_stats = {
        let mut conn = db.acquire().await?;
        dedupe::dedupe_cross_source(&mut conn, account_id, None, 2, progress).await?
    };
    // Without ffmpeg the preview pass would fail once per attachment; say so
    // once instead (#1018). Whether each original is shown as it is needs no
    // ffmpeg for anything but an MP4, and the viewer opens an original only
    // when its rows say so, so that is still recorded.
    let tools = cfg.media_tools();
    if !media::MediaTools::available(&tools) {
        progress.heading(
            "ffmpeg was not found, so the Demo Account's attachments get no Previews or Thumbnails",
        );
        process_assets::decide_shown_as_is(cfg, &tools, db, account_id, None).await?;
        return Ok((dedupe_stats, process_assets::ProcessAssetsStats::default()));
    }
    progress.heading("Making the Demo Account's Previews and Thumbnails");
    let opened = OpenDb {
        cfg: cfg.clone(),
        db: db.clone(),
        wal_error: None,
    };
    let process_stats = process_assets::run(
        &opened,
        &tools,
        &ProcessAssetsOptions {
            force: false,
            dry_run: false,
            skip_image: false,
            skip_video: false,
            skip_audio: false,
            account: Some(account_id),
        },
        stop,
        progress,
    )
    .await
    .context("process-assets after prepared demo import")?;
    if let Some(line) = console::not_done_line(&process_stats) {
        progress.warn(line);
    }
    Ok((dedupe_stats, process_stats))
}

pub(super) struct ResetPreparedStats {
    pub(super) import: pipeline::ImportCounts,
    pub(super) address_book: LoadCounts,
    pub(super) dedupe_keys_filled: u64,
    pub(super) process_assets: process_assets::ProcessAssetsStats,
}

/// Record in `db` that a Demo Account build has started, before the build
/// writes anything else. The record stays until the build has finished or
/// what it wrote has been removed.
async fn begin_demo_build(db: &SqlitePool) -> Result<()> {
    let mut conn = db.acquire().await?;
    demo_account_build::begin(&mut conn, chrono::Utc::now())
        .await
        .context("record that the Demo Account build has started")
}

/// Remove what a failed build wrote: the Demo Account, then the build's
/// record. The record stays when the account cannot be removed, so the next
/// start tries again.
///
/// # Errors
///
/// Returns an error when the account or the record cannot be removed.
pub async fn remove_failed_demo_build(cfg: &Config, db: &SqlitePool) -> Result<()> {
    wipe_demo_account(cfg, db, DEMO_ACCOUNT_ID, AuditActor::Server, Progress::Log).await?;
    let mut conn = db.acquire().await?;
    demo_account_build::end(&mut conn).await
}

/// Remove the Demo Account a build left when the server stops during it, and
/// keep the build's record, so the next start reports the build as failed.
/// A build that has already finished has no record, and its Demo Account
/// is kept. Returns whether there was a build to stop.
///
/// # Errors
///
/// Returns an error when the record cannot be read or the account cannot be
/// removed.
pub async fn remove_part_built_demo_account(cfg: &Config, db: &SqlitePool) -> Result<bool> {
    let unfinished = {
        let mut conn = db.acquire().await?;
        demo_account_build::is_unfinished(&mut conn).await?
    };
    if unfinished {
        wipe_demo_account(cfg, db, DEMO_ACCOUNT_ID, AuditActor::Server, Progress::Log).await?;
    }
    Ok(unfinished)
}

/// On start: when the database holds the record of a Demo Account build that
/// did not finish, because the server stopped during it, remove the Demo
/// Account the build left and the record. Returns whether there was one.
///
/// # Errors
///
/// Returns an error when the record cannot be read or the account or the
/// record cannot be removed.
pub async fn remove_stopped_demo_build(cfg: &Config, db: &SqlitePool) -> Result<bool> {
    let unfinished = {
        let mut conn = db.acquire().await?;
        demo_account_build::is_unfinished(&mut conn).await?
    };
    if unfinished {
        remove_failed_demo_build(cfg, db).await?;
    }
    Ok(unfinished)
}

/// Writes a demo bundle of the given size into the given directory, and stops
/// part-way with an error once the flag is set. The server holds the real
/// one ([`generate_bundle`]); a test holds one that writes a few
/// conversations.
pub type BundleGenerator = fn(DemoSize, &Path, &AtomicBool) -> Result<()>;

/// The generator a running server uses: the built-in data set of the size.
pub fn generate_bundle(size: DemoSize, bundle: &Path, cancel: &AtomicBool) -> Result<()> {
    generate_demo_data(size, bundle, cancel, Progress::Log).map(|_| ())
}

/// Generate the built-in data set of `size` into `bundle`
/// ([`demo_seed::generate_size_to`]), and say each warning the generator
/// returns through `progress`: on standard error for `reset-demo`, in the
/// server's log under `serve` (#2903).
pub(super) fn generate_demo_data(
    size: DemoSize,
    bundle: &Path,
    cancel: &AtomicBool,
    progress: Progress,
) -> Result<demo_seed::GenStats> {
    let stats = demo_seed::generate_size_to(size, bundle, cancel)?;
    console::say_generator_warnings(&stats, progress);
    Ok(stats)
}

/// Sets the flag it holds when it is dropped. A build holds one for its
/// generator, so a build that is dropped part-way, the way the server drops
/// it when it stops, stops the generator too (#1431).
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Build the Demo Account in the server's database `db` while the server is
/// serving it: the Admin Home action. The account is removed and built again,
/// in the live database, and no other account is touched. Each source is
/// imported in batches, each its own transaction, so another account's write
/// waits for one batch at most. Returns the number of messages imported.
///
/// # Errors
///
/// Returns an error when generation, import or media processing fails, or
/// `stop` is set while it converts the Demo Data's media; the partly built
/// Demo Account is removed first. Setting `stop` kills the ffmpeg the build
/// runs, which the build's task cannot be dropped in the middle of (#1729).
pub async fn build_demo_account(
    db: SqlitePool,
    cfg: Arc<Config>,
    size: DemoSize,
    generate: BundleGenerator,
    stop: Arc<AtomicBool>,
) -> Result<u64> {
    let outcome = async {
        // Recorded before generating, so a server stopped at any point of
        // the build removes the Demo Account it was replacing.
        begin_demo_build(&db).await?;
        let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
        // A blocking task cannot be aborted, so the generator checks this
        // flag instead. Dropping the build sets it, so a server that stops
        // mid-generation waits for one file, not the whole data set.
        let cancel = Arc::new(AtomicBool::new(false));
        let _stop_generator = CancelOnDrop(Arc::clone(&cancel));
        // Generating is CPU work with no await in it, so it runs off the
        // request-serving threads. The temporary directory moves into the task
        // and comes back with its result: when the build is dropped while
        // the task runs, the directory is removed once the generator has
        // returned, never while it is still writing into it.
        let (work, generated) = crate::request_id::spawn_blocking_in_current_span(move || {
            let generated = generate(size, &work.path().join("bundle"), &cancel);
            (work, generated)
        })
        .await
        .context("the demo bundle generator stopped")?;
        generated.context("generate demo bundle (demo-seed)")?;
        build_from_bundle(
            &cfg,
            &db,
            &work.path().join("bundle"),
            BuildBy {
                actor: AuditActor::Admin,
                record_with_size: None,
            },
            &stop,
        )
        .await
    }
    .await;
    whole_demo_account_or_none(&cfg, &db, outcome).await
}

/// The build `serve` runs on a new database: generate a bundle of the `size`
/// set with `generate`, and build the Demo Account from it in the database
/// `db`, the one `cfg` names, recording the build in the Audit Trail as the
/// server's first start's.
pub(super) async fn build_demo_account_with<G>(
    cfg: &Config,
    db: &SqlitePool,
    size: DemoDataSize,
    generate: G,
) -> Result<u64>
where
    G: FnOnce(&Path) -> Result<()>,
{
    let outcome = async {
        let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
        let bundle = work.path().join("bundle");
        generate(&bundle).context("generate demo bundle (demo-seed)")?;
        // Nothing stops it: it runs before the server listens or handles a
        // signal.
        build_from_bundle(
            cfg,
            db,
            &bundle,
            BuildBy {
                actor: AuditActor::StartUp,
                record_with_size: Some(size),
            },
            &AtomicBool::new(false),
        )
        .await
    }
    .await;
    whole_demo_account_or_none(cfg, db, outcome).await
}

/// The number of messages a build imported. When the build failed, the
/// partly built account is removed first, so the Message Crate holds a whole
/// Demo Account or none.
pub(super) async fn whole_demo_account_or_none(
    cfg: &Config,
    db: &SqlitePool,
    outcome: Result<ResetPreparedStats>,
) -> Result<u64> {
    match outcome {
        Ok(stats) => Ok(stats.import.messages),
        Err(error) => {
            if let Err(error) = remove_failed_demo_build(cfg, db).await {
                tracing::warn!(
                    error = %crate::api_error::error_chain(&error),
                    "The partly built Demo Account could not be removed"
                );
            }
            Err(error)
        }
    }
}

/// Build the Demo Account in the database `db`, the one `cfg` names, from
/// the bundle at `bundle`. There is nothing to snapshot or swap: this writes
/// to the database directly, touching the Demo Account alone. It runs no
/// `VACUUM`: on a running server that rewrites every account's rows while
/// holding the write lock, and on a first start it only reclaims the
/// staging tables' churn while the desktop app waits for the server to
/// listen (#1404).
pub(super) async fn build_from_bundle(
    cfg: &Config,
    db: &SqlitePool,
    bundle: &Path,
    by: BuildBy,
    stop: &AtomicBool,
) -> Result<ResetPreparedStats> {
    let prepared = validate_prepared_bundle(bundle)?;
    rebuild_demo_account(
        cfg,
        db,
        &prepared,
        DEMO_ACCOUNT_ID,
        by,
        BuildRun::Serve,
        stop,
    )
    .await
}

/// Wipe, seed, import, load the address book, dedupe, and convert media for
/// the demo account in the database `db`, the one `cfg` names, then run
/// `VACUUM` when `run` is [`BuildRun::Command`]. A new database, a reset and
/// a build on a running server all run exactly this; what differs is what
/// the caller does around it (a reset snapshots the database first, runs as
/// [`BuildRun::Command`], and swaps it in after). Every step uses `db`, so on a
/// running server the build shares the server's pool. The build's record in
/// `demo_account_build` is written first and removed last, so a database
/// that still holds it after a stop has a part-built Demo Account (#1215).
pub(super) async fn rebuild_demo_account(
    cfg: &Config,
    db: &SqlitePool,
    prepared: &PreparedBundle,
    account_id: i64,
    by: BuildBy,
    run: BuildRun,
    stop: &AtomicBool,
) -> Result<ResetPreparedStats> {
    let progress = run.progress();
    begin_demo_build(db).await?;
    if let Some(size) = by.record_with_size {
        let mut conn = db.acquire().await?;
        crate::db::audit_trail::record_demo_build(&mut conn, by.actor, size, chrono::Utc::now())
            .await?;
    }
    wipe_demo_account(cfg, db, account_id, by.actor, progress).await?;
    console::say_reset_header(account_id, prepared, &cfg.paths.db.display(), progress);
    seed_demo_account(db, account_id, &prepared.seed).await?;
    let import = import_demo_sources(cfg, db, prepared, account_id, progress).await?;
    let address_book = load_demo_address_book(db, prepared, account_id, progress).await?;
    let (dedupe_stats, process_stats) =
        dedupe_and_process_assets(cfg, db, account_id, stop, progress).await?;
    if run == BuildRun::Command {
        vacuum_after_demo(db, progress).await;
    }
    let mut conn = db.acquire().await?;
    demo_account_build::end(&mut conn)
        .await
        .context("record that the Demo Account build has finished")?;
    drop(conn);
    Ok(ResetPreparedStats {
        import,
        address_book,
        dedupe_keys_filled: dedupe_stats.keys_filled,
        process_assets: process_stats,
    })
}

/// Who builds the Demo Account, and what the build records of itself in the
/// Audit Trail.
#[derive(Debug, Clone, Copy)]
pub(super) struct BuildBy {
    /// Who acts: the wipe's `account_deleted` entry names them, and so does
    /// the build's own entry.
    pub(super) actor: AuditActor,
    /// The set of Demo Data the build records itself with, right before the
    /// wipe ([`audit_trail::record_demo_build`]): the `reset-demo` command and
    /// the build on a new database do. `None` for the administrator's build,
    /// which the administrator's request recorded already.
    ///
    /// [`audit_trail::record_demo_build`]: crate::db::audit_trail::record_demo_build
    pub(super) record_with_size: Option<DemoDataSize>,
}

/// What runs a Demo Account build, which decides two things: whether it ends
/// by running `VACUUM`, which rewrites the whole database file and holds the
/// write lock while it does, and where its progress goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildRun {
    /// The `reset-demo` command. It runs `VACUUM`: the command serves no one
    /// while it runs, and the database it writes is a copy swapped in
    /// afterwards, so rewriting the file holds up no other writer. Its
    /// progress is its output, on standard output.
    Command,
    /// A build on a running server, and the build `serve` runs on a new
    /// database. It leaves the freed pages for the database to reuse, and
    /// its progress goes into the server's log.
    Serve,
}

impl BuildRun {
    /// Where the build's import, dedupe and `VACUUM` say how far they have
    /// got.
    pub(super) fn progress(self) -> Progress {
        match self {
            Self::Command => Progress::Print,
            Self::Serve => Progress::Log,
        }
    }
}

/// The most JSONL a build imports in one transaction: what an Upload sends
/// in one request, [`api_types::MAX_IMPORT_BATCH_BYTES`].
/// Every other write on the server waits for the transaction, so it waits
/// for one batch at most.
pub(super) const IMPORT_BATCH_BYTES: u64 = api_types::MAX_IMPORT_BATCH_BYTES as u64;

/// Group `paths`, in order, into batches whose sizes add up to `limit` bytes
/// or less. A file larger than `limit` is a batch of its own, because a
/// conversation file is never split.
///
/// # Errors
///
/// Returns an error when a file's size cannot be read.
pub(super) fn import_batches(paths: Vec<PathBuf>, limit: u64) -> Result<Vec<Vec<PathBuf>>> {
    let mut batches: Vec<Vec<PathBuf>> = Vec::new();
    let mut batch_bytes = 0u64;
    for path in paths {
        let bytes = fs::metadata(&path)
            .with_context(|| format!("read the size of {}", path.display()))?
            .len();
        match batches.last_mut() {
            Some(batch) if batch_bytes.saturating_add(bytes) <= limit => {
                batch.push(path);
                batch_bytes += bytes;
            }
            _ => {
                batches.push(vec![path]);
                batch_bytes = bytes;
            }
        }
    }
    Ok(batches)
}

/// Import the staged sources in [`DEMO_IMPORT_SOURCES`] order: the first
/// replaces the account's data and the rest append. Returns the summed counts.
pub(super) async fn import_demo_sources(
    cfg: &Config,
    db: &SqlitePool,
    prepared: &PreparedBundle,
    account_id: i64,
    progress: Progress,
) -> Result<pipeline::ImportCounts> {
    import_demo_sources_with(
        cfg,
        db,
        prepared,
        account_id,
        IMPORT_BATCH_BYTES,
        progress,
        async || Ok(()),
    )
    .await
}

/// [`import_demo_sources`] in batches of at most `batch_bytes`, calling
/// `after_batch` once each batch is committed, so a test can write for
/// another account between two batches.
///
/// Each source is one Import Run, sent as batches the way an Upload sends
/// them: each batch is its own transaction, and a source that replaces does
/// so on its first batch alone, so a later batch never removes an earlier
/// one's messages.
pub(super) async fn import_demo_sources_with(
    cfg: &Config,
    db: &SqlitePool,
    prepared: &PreparedBundle,
    account_id: i64,
    batch_bytes: u64,
    progress: Progress,
    mut after_batch: impl AsyncFnMut() -> Result<()>,
) -> Result<pipeline::ImportCounts> {
    let mut totals = pipeline::ImportCounts::default();
    for source in &DEMO_IMPORT_SOURCES {
        let export_dir = (source.export_dir)(prepared);
        let paths = crate::import_cli::list_jsonl_files(export_dir)?;
        let assets_dir = cfg.paths.assets_dir_for_account(account_id);
        let mut conn = db.acquire().await?;
        let import_run = pipeline::OwnedImportRun::start(
            &mut conn,
            account_id,
            source.source,
            source.mode,
            "message-crate-server",
        )
        .await?;
        let mut run = pipeline::ImportCounts {
            mode: source.mode,
            ..Default::default()
        };
        let mut result = Ok(());
        for (index, batch) in import_batches(paths, batch_bytes)?.iter().enumerate() {
            let mode = if index == 0 {
                source.mode
            } else {
                ImportMode::Append
            };
            let imported = pipeline::import_jsonl_files_on_conn(
                &mut conn,
                batch,
                &ImportOptions {
                    progress,
                    ..ImportOptions::fixed(FixedImportArgs {
                        assets_dir: &assets_dir,
                        asset_root: Some(export_dir),
                        mode,
                        source: source.source,
                        account_id,
                        import_id: Some(import_run.id),
                        phone_country: None,
                        now: chrono::Utc::now(),
                    })
                },
            )
            .await
            .map_err(anyhow::Error::from);
            match imported {
                Ok(counts) => run.add_run(&counts),
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
            if let Err(error) = after_batch().await {
                result = Err(error);
                break;
            }
        }
        let result = result.map(|()| run);
        import_run.finish(&mut conn, &result).await;
        totals.add_run(&result?);
    }
    Ok(totals)
}

/// Load the bundle's address book into the demo account, in Append mode,
/// once its messages are in.
///
/// The demo is built the way a person builds theirs: the imports bring the
/// people in as Unknowns, and the person exports the address book, types the
/// names onto the Unknowns' rows, and loads it back.
/// [`address_book::rewrite_ids_to_nameless`] does the typing: it
/// gives each contact of the bundle's file the id of the Unknown the imports
/// made for it. The load goes through
/// [`address_book::load`], the function `POST /v1/contacts` calls, so the
/// demo exercises the same rules a person's file does.
///
/// Naming the Unknown in place keeps it in the Contact Group of each Import
/// Run that touched it (#1511). A new contact taking the Unknown's
/// identities instead would leave the Unknown empty, so the load would
/// delete it and the run's group would lose it. The mode is Append because
/// Edit takes a contact out of every Contact Group its rows do not list, and
/// the bundle's file cannot list the groups the Import Runs make.
pub(super) async fn load_demo_address_book(
    db: &SqlitePool,
    prepared: &PreparedBundle,
    account_id: i64,
    progress: Progress,
) -> Result<LoadCounts> {
    let text = fs::read_to_string(&prepared.contacts_csv)
        .with_context(|| format!("read {}", prepared.contacts_csv.display()))?;
    let mut conn = db.acquire().await?;
    let text = address_book::rewrite_ids_to_nameless(&mut conn, account_id, &text)
        .await
        .context("match the demo address book to the Unknowns the imports made")?;
    let loaded = address_book::load(
        &mut conn,
        account_id,
        &text,
        LoadMode::Append,
        address_book::Record::Not,
        chrono::Utc::now(),
    )
    .await;
    let counts = loaded.map_err(|e| anyhow::anyhow!("load the demo address book: {e}"))?;
    console::say_address_book_loaded(&counts, progress);
    Ok(counts)
}

/// Check the bundle has its seed, the three run directories, and the contacts file, and return their paths.
pub(super) fn validate_prepared_bundle(bundle: &Path) -> Result<PreparedBundle> {
    let demo_seed = bundle.join("config/seed.toml");
    let imessage_dir = bundle.join("staging").join(IMESSAGE_SOURCE);
    let sbr_dir = bundle.join("staging").join(SBR_SOURCE);
    let whatsapp_dir = bundle.join("staging").join(WHATSAPP_SOURCE);
    let contacts_csv = bundle.join("config/contacts.csv");
    if !demo_seed.is_file()
        || !imessage_dir.is_dir()
        || !sbr_dir.is_dir()
        || !whatsapp_dir.is_dir()
        || !contacts_csv.is_file()
    {
        bail!(
            "incomplete demo bundle under {} (need config/seed.toml, \
             staging/{IMESSAGE_SOURCE}/, staging/{SBR_SOURCE}/, staging/{WHATSAPP_SOURCE}/, config/contacts.csv)",
            bundle.display()
        );
    }
    Ok(PreparedBundle {
        seed: read_seed_toml(&demo_seed)?,
        imessage_dir,
        sbr_dir,
        whatsapp_dir,
        contacts_csv,
    })
}

/// Reclaim space after the demo import replaced most rows. Best effort:
/// failures are written as warnings through `progress`, not returned,
/// because the demo rows are already
/// committed and a failed vacuum only costs disk space.
async fn vacuum_after_demo(db: &SqlitePool, progress: Progress) {
    let mut conn = match db.acquire().await {
        Ok(conn) => conn,
        Err(err) => {
            progress.warn(format_args!(
                "VACUUM did not run, because no connection to the database could be opened: {err}"
            ));
            return;
        }
    };
    maintenance::vacuum_import_tables(&mut conn, progress).await;
}

/// Seed the demo account row and profile in the database `db`.
pub(super) async fn seed_demo_account(
    db: &SqlitePool,
    account_id: i64,
    seed: &DemoSeed,
) -> Result<()> {
    let mut conn = db.acquire().await?;
    seed_demo_account_on_conn(&mut conn, account_id, seed).await
}

/// Create the demo account row, with [`account_profile::DEMO_USERNAME`], and
/// the profile fields the seed names, so the demo logs in without setup. The
/// username is the one account creation reserves, never one read from the
/// bundle (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
pub(super) async fn seed_demo_account_on_conn(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    seed: &DemoSeed,
) -> Result<()> {
    account_profile::create_account_row_if_missing(conn, account_id).await?;

    // Account creation reserves the name, but a database written before the
    // reservation may already give it to another account.
    let username = account_profile::DEMO_USERNAME;
    if let Some(holder) = account_profile::lookup_account_by_username(conn, username).await?
        && holder != account_id
    {
        let held_as = account_profile::username_for_account(conn, holder)
            .await?
            .unwrap_or_default();
        anyhow::bail!(
            "account {holder} ({held_as}) already has the username {username}, which belongs to the Demo Account; delete that account to add the Demo Account"
        );
    }

    account_profile::write_demo_account_row(conn, account_id, username, &seed.owner.display_name)
        .await?;
    // The seed creates no API token. A token the Demo Account creates cannot
    // import either, because a token's grant is narrowed to the account's.
    for (raw, handle_type) in &seed.owner.handle_specs {
        account_profile::link_account_handle(conn, account_id, raw, *handle_type).await?;
    }
    // An email is an identity like the phone, linked the same way. The
    // profile's emails are read from these links, so the profile and the
    // identities list name the same addresses (#955, #1027).
    for email in &seed.owner.emails {
        account_profile::link_account_handle(conn, account_id, email, IdentityType::Email).await?;
    }
    Ok(())
}

/// The most messages the wipe deletes in one transaction, with their
/// attachments, tapbacks and search-index rows. One batch must finish far
/// inside the 15-second busy timeout another writer waits: a batch of 5,000
/// took about 0.13 s on the medium Demo Data.
pub(super) const WIPE_BATCH_ROWS: NonZeroU32 = NonZeroU32::new(5_000).expect("5,000 is not zero");

/// Delete the demo account, `actor` acting, and its on-disk attachments.
/// Leaves the database and other accounts intact. Its messages, nearly all
/// its rows, go first, duplicates before the rest, in batches of
/// [`WIPE_BATCH_ROWS`], each its own
/// transaction, so another account's write waits for one batch at most
/// (#1404). The rest of its rows follow the account row via CASCADE; its
/// Audit Trail stays, unlinked, with an `account_deleted` entry, as any
/// account's does (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
/// Returns whether there was a Demo Account to delete.
pub(super) async fn wipe_demo_account(
    cfg: &Config,
    db: &SqlitePool,
    account_id: i64,
    actor: AuditActor,
    progress: Progress,
) -> Result<bool> {
    wipe_demo_account_with(
        cfg,
        db,
        account_id,
        actor,
        progress,
        WIPE_BATCH_ROWS,
        async || Ok(()),
    )
    .await
}

/// [`wipe_demo_account`] in batches of at most `batch_rows` messages, calling
/// `after_batch` once each batch is committed, so a test can write for
/// another account between two batches.
pub(super) async fn wipe_demo_account_with(
    cfg: &Config,
    db: &SqlitePool,
    account_id: i64,
    actor: AuditActor,
    progress: Progress,
    batch_rows: NonZeroU32,
    mut after_batch: impl AsyncFnMut() -> Result<()>,
) -> Result<bool> {
    progress.heading(format_args!(
        "Removing the Demo Account from the database at {}",
        cfg.paths.db.display()
    ));
    let mut conn = db
        .acquire()
        .await
        .with_context(|| format!("open {} for demo account wipe", cfg.paths.db.display()))?;
    for which in [MessagesToDelete::Duplicates, MessagesToDelete::Any] {
        loop {
            let batch = account_profile::delete_account_messages_batch(
                &mut conn, account_id, which, batch_rows,
            )
            .await?;
            after_batch().await?;
            if batch == MessageBatch::Done {
                break;
            }
        }
    }
    let deleted =
        account_profile::delete_account(&mut conn, account_id, actor, chrono::Utc::now()).await?;
    if deleted {
        progress.say("The Demo Account is removed from the database");
    } else {
        progress.say("The database held no Demo Account to remove");
    }
    drop(conn);

    crate::asset_store::remove_account_dir(&cfg.paths, account_id).with_context(|| {
        format!(
            "remove {}",
            crate::asset_store::account_dir(&cfg.paths, account_id).display()
        )
    })?;
    Ok(deleted)
}
