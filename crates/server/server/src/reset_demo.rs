//! Generate the demo bundle, clear the demo account's data, import, load the
//! demo's address book, and process media.
//!
//! Three callers: `reset-demo`; `serve` on a database that does not exist
//! yet ([`seed_new_database`]), which is how every new Message Crate starts
//! with the Demo Account; and `PUT /v1/server/demo-account`
//! ([`build_demo_account`]), the owner's rebuild on a running server.
//!
//! This module sequences each caller's run. The build every caller runs is
//! in `build`, the files a reset and a seeding move in `snapshot`, the
//! checks before a reset's swap in `verify`, and what a build says in
//! `console`. The SQL is in [`crate::db::demo`].

mod build;
mod console;
mod snapshot;
mod verify;

use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use demo_seed::DemoSize;
use sqlx::SqlitePool;

use crate::config::Config;
use crate::db::address_book::LoadCounts;
use crate::db::audit_trail::AuditActor;
use crate::db::demo_account_build::DemoDataSize;
use crate::db::schema;
use crate::imports_api;
use crate::open_db::OpenDb;
use crate::process_assets;
use crate::progress::Progress;
use build::{
    BuildBy, BuildRun, ResetPreparedStats, build_demo_account_with, generate_demo_data,
    rebuild_demo_account, validate_prepared_bundle,
};
pub use build::{
    BundleGenerator, build_demo_account, generate_bundle, remove_failed_demo_build,
    remove_part_built_demo_account, remove_stopped_demo_build,
};
use snapshot::{
    ResetPaths, checkpoint_and_clean_sidecars, install_reset_state_or_keep_work, parent_dir_or_cwd,
    prepare_database_snapshot, refuse_another_layout, reset_account_work_dir, seed_into_place,
};
use verify::{
    other_account_directories, verify_non_demo_state_preserved,
    verify_other_account_directories_preserved,
};

/// Stable id of the Demo Account, which every demo build writes.
pub use crate::db::account_profile::DEMO_ACCOUNT_ID;

/// Counts reported when a demo reset finishes.
#[derive(Debug)]
pub struct ResetDemoStats {
    /// Stats from regenerating the demo bundle.
    pub seed: demo_seed::GenStats,
    /// Stats from importing the regenerated bundle.
    pub import: imports_api::ImportCounts,
    /// What loading the bundle's address book changed, after the imports.
    pub address_book: LoadCounts,
    /// Dedupe content keys filled during the reset (one per message; not a duplicate count).
    pub dedupe_keys_filled: u64,
    /// Stats from the post-import media processing pass.
    pub process_assets: process_assets::ProcessAssetsStats,
}

/// Generate the Demo Data set of `size` and rebuild the demo account from it
/// in the database `cfg` names. The config file is the operator's: the caller
/// reads it, and nothing here writes it (#1216).
///
/// # Errors
///
/// Returns an error when generation fails, the database cannot be replaced,
/// or import / media processing fails.
pub async fn run_reset_demo(size: DemoSize, cfg: &Config) -> Result<ResetDemoStats> {
    let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
    let bundle = work.path().join("bundle");
    let progress = BuildRun::Command.progress();
    progress.heading(format_args!("Generating the {size} Demo Data set"));
    // `reset-demo` runs in the foreground and stops with its process, so
    // nothing sets the flag.
    let seed_stats = generate_demo_data(size, &bundle, &AtomicBool::new(false), progress)
        .context("generate demo bundle (demo-seed)")?;
    for line in demo_seed::summary_lines(&seed_stats) {
        progress.say(line);
    }
    let reset_stats = reset_prepared_bundle(cfg, &bundle, DEMO_ACCOUNT_ID, size.into()).await?;

    Ok(ResetDemoStats {
        seed: seed_stats,
        import: reset_stats.import,
        address_book: reset_stats.address_book,
        dedupe_keys_filled: reset_stats.dedupe_keys_filled,
        process_assets: reset_stats.process_assets,
    })
}

/// Whether the database `cfg` names does not exist yet: a file that is not
/// there, or an empty one ([`schema::DatabaseKind::Empty`]). `serve` seeds
/// such a database and no other, so one that was ever started, or made
/// empty with `create-database`, is left as it is. Another program's file is
/// not new either: seeding would move the new database over it, so `serve`
/// goes on to refuse it by name.
///
/// # Errors
///
/// Returns an error when the file cannot be read as SQLite.
pub async fn database_is_new(cfg: &Config) -> Result<bool> {
    Ok(matches!(
        crate::open_db::inspect(&cfg.paths.db).await?,
        None | Some(schema::DatabaseKind::Empty)
    ))
}

/// Add the Demo Account, with the medium data set, to a database that does
/// not exist yet. `serve` calls this before it listens. The database is
/// written beside the configured path and moved into place once it is whole
/// ([`seed_into_place`]).
///
/// A failure is reported and not returned: the unfinished database is
/// removed and `serve` creates an empty one, so the Message Crate starts
/// without the Demo Account, since a person can still claim it and import,
/// and the owner can add the Demo Account later.
pub async fn seed_new_database(cfg: &Config) {
    let size = DemoSize::Medium;
    tracing::info!(
        "The database is new, so the Demo Account is added, with the {size} Demo Data set…"
    );
    let started = std::time::Instant::now();
    // Seeding runs before the server listens, on this thread, so no stop
    // can arrive while it generates and nothing sets the flag.
    match seed_new_database_with(cfg, size.into(), |bundle| {
        generate_demo_data(size, bundle, &AtomicBool::new(false), Progress::Log).map(|_| ())
    })
    .await
    {
        Ok(messages) => tracing::info!(
            "The Demo Account was added, with {messages} messages, in {:.1} s",
            started.elapsed().as_secs_f64()
        ),
        Err(error) => tracing::warn!(
            error = %crate::server::error_chain(&error),
            "The Demo Account could not be added. This Message Crate starts without it. `message-crate-server reset-demo` adds it"
        ),
    }
}

/// [`seed_new_database`] with the step that writes the bundle of the `size`
/// set injected, so a test can seed from a few conversations, or from a
/// bundle that fails partway. Returns the number of messages imported, or why
/// seeding failed; the Demo Account was removed again by then.
async fn seed_new_database_with<G>(cfg: &Config, size: DemoDataSize, generate: G) -> Result<u64>
where
    G: FnOnce(&Path) -> Result<()>,
{
    seed_into_place(cfg, async |seeding: &Config, db: &SqlitePool| {
        build_demo_account_with(seeding, db, size, generate).await
    })
    .await
}

/// The `reset-demo` command's build from the bundle of the `size` set: build
/// the new state in a prepared database next to the active one, recording
/// the build in its Audit Trail as the command's before the deletion of the
/// old Demo Account, as a reset from Owner Home is recorded, prove nothing
/// outside the demo account changed, then swap it in.
async fn reset_prepared_bundle(
    cfg: &Config,
    bundle: &Path,
    account_id: i64,
    size: DemoDataSize,
) -> Result<ResetPreparedStats> {
    reset_prepared_bundle_with(cfg, bundle, account_id, size, async |_| Ok(())).await
}

/// [`reset_prepared_bundle`] with `after_rebuild` run on the prepared
/// database once the rebuild has finished, so a test can change a row the
/// rebuild must not change and see the reset refused.
async fn reset_prepared_bundle_with(
    cfg: &Config,
    bundle: &Path,
    account_id: i64,
    size: DemoDataSize,
    after_rebuild: impl AsyncFnOnce(&SqlitePool) -> Result<()>,
) -> Result<ResetPreparedStats> {
    let prepared = validate_prepared_bundle(bundle)?;
    // Refused here, by its own name, before the lock file, the snapshot or
    // the swap touch anything beside it.
    if crate::open_db::inspect(&cfg.paths.db).await? == Some(schema::DatabaseKind::Foreign) {
        return Err(schema::not_a_message_crate_database(&cfg.paths.db));
    }
    refuse_another_layout(&cfg.paths.db).await?;
    let _operation_lock = crate::operation_lock::acquire_for_reset(&cfg.paths.db)?;
    let mut ready = crate::operation_lock::ReadyWhileRebuilding::clear(&cfg.paths.db)?;
    let db_parent = parent_dir_or_cwd(&cfg.paths.db);
    file_io::create_dir_all_owner_only(db_parent)
        .with_context(|| format!("create database parent {}", db_parent.display()))?;
    let db_work = tempfile::Builder::new()
        .prefix(".reset-demo-db-")
        .tempdir_in(db_parent)
        .context("create temporary demo database directory")?;
    // Keep the prepared account tree on the same mount as data_dir so
    // install can rename into data_dir/<account>. A work directory on
    // another mount (tmp, a nested bind, a named volume) fails with EXDEV.
    let data_work = reset_account_work_dir(&cfg.paths.data_dir)?;
    let other_directories = other_account_directories(&cfg.paths.data_dir, account_id)?;
    let prepared_db = db_work.path().join("messagecrate.db");
    checkpoint_and_clean_sidecars(&cfg.paths.db, "before creating the reset snapshot").await?;
    prepare_database_snapshot(&cfg.paths.db, &prepared_db).await?;

    let mut temporary_cfg = cfg.clone();
    temporary_cfg.paths.db = prepared_db.clone();
    temporary_cfg.paths.data_dir = data_work.path().to_path_buf();
    let opened = OpenDb::create_or_open(temporary_cfg.clone()).await?;
    let stats = match rebuild_demo_account(
        &temporary_cfg,
        &opened.db,
        &prepared,
        account_id,
        BuildBy {
            actor: AuditActor::CommandLine,
            record_with_size: Some(size),
        },
        BuildRun::Command,
        &AtomicBool::new(false),
    )
    .await
    {
        Ok(stats) => after_rebuild(&opened.db).await.map(|()| stats),
        Err(error) => Err(error),
    };
    // Closed before anything checks, checkpoints or renames the file.
    opened.close().await;
    let stats = stats?;

    verify_non_demo_state_preserved(&cfg.paths.db, &prepared_db, account_id).await?;
    verify_other_account_directories_preserved(
        &cfg.paths.data_dir,
        account_id,
        &other_directories,
    )?;
    let active_account = cfg.paths.data_dir.join(account_id.to_string());
    let prepared_account = temporary_cfg.paths.data_dir.join(account_id.to_string());
    let paths = ResetPaths {
        active_db: &cfg.paths.db,
        prepared_db: &prepared_db,
        active_account: &active_account,
        prepared_account: &prepared_account,
    };
    install_reset_state_or_keep_work(&paths, db_work, data_work, &mut ready).await?;
    ready.mark_ready()?;
    Ok(stats)
}

#[cfg(test)]
pub(crate) mod tests;
