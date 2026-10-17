//! The files a reset and a new database's seeding move: the copy of the
//! database the `reset-demo` command rebuilds, the checkpoint that empties
//! a database's write-ahead log before it is moved, and the swap of the
//! rebuilt database and Demo Account directory into place, undone when a
//! step fails.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sqlx::SqlitePool;

use super::console;
use crate::config::Config;
use crate::db::{engine, maintenance, schema};
use crate::open_db::OpenDb;

/// Parent directory of `path`, or `.` when the path has no parent.
pub(super) fn parent_dir_or_cwd(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Work directory for the prepared account tree. Must live on the same mount
/// as `data_dir` so the later install can `rename` into `data_dir/<account>`.
pub(super) fn reset_account_work_dir(data_dir: &Path) -> Result<tempfile::TempDir> {
    crate::owner_only::make_data_dir_owner_only(data_dir)?;
    tempfile::Builder::new()
        .prefix(".reset-demo-data-")
        .tempdir_in(data_dir)
        .with_context(|| {
            format!(
                "create temporary demo account directory in {}",
                data_dir.display()
            )
        })
}

/// Where `serve` writes a new database until seeding is complete:
/// `<database>.seeding`, beside the configured file, so the rename into
/// place stays on one file system.
pub(super) fn seeding_path(db: &Path) -> PathBuf {
    sqlite_sidecar(db, ".seeding")
}

/// Remove the `-wal` and `-shm` sidecars of the database file `db`; one that
/// is not there is not an error.
pub(super) fn remove_sidecars(db: &Path) -> Result<()> {
    remove_any_if_exists(&sqlite_sidecar(db, "-wal"))?;
    remove_any_if_exists(&sqlite_sidecar(db, "-shm"))
}

/// Create a new database with `build` at [`seeding_path`] and rename it to
/// the path `cfg` names once `build` has finished, so a database file at the
/// configured path is always fully seeded. A start that is stopped while
/// seeding, the way the desktop app kills a server that is still starting,
/// leaves only the seeding file, and the next start removes it and seeds
/// again (#1215).
///
/// When `build` fails the seeding file is removed and nothing is renamed,
/// so `serve` goes on to create an empty database at the configured path.
pub(super) async fn seed_into_place<B>(cfg: &Config, build: B) -> Result<u64>
where
    B: AsyncFnOnce(&Config, &SqlitePool) -> Result<u64>,
{
    let seeding = seeding_path(&cfg.paths.db);
    remove_any_if_exists(&seeding)?;
    remove_sidecars(&seeding)?;
    let mut seeding_cfg = cfg.clone();
    seeding_cfg.paths.db = seeding.clone();
    let seeded = async {
        let opened = OpenDb::create_or_open(seeding_cfg.clone()).await?;
        let built = build(&seeding_cfg, &opened.db).await;
        // Closed before the file is checkpointed and renamed.
        opened.close().await;
        let messages = built?;
        checkpoint_and_clean_sidecars(&seeding, "before moving the new database into place")
            .await?;
        // A configured file can be there only when it is empty
        // ([`database_is_new`]); its sidecars must not attach to the new one.
        remove_sidecars(&cfg.paths.db)?;
        fs::rename(&seeding, &cfg.paths.db).with_context(|| {
            format!(
                "move the new database {} to {}",
                seeding.display(),
                cfg.paths.db.display()
            )
        })?;
        Ok(messages)
    }
    .await;
    if seeded.is_err()
        && let Err(error) = remove_any_if_exists(&seeding).and_then(|()| remove_sidecars(&seeding))
    {
        tracing::warn!(
            error = %crate::api_error::error_chain(&error),
            "The unfinished new database at {} could not be removed",
            seeding.display()
        );
    }
    seeded
}

/// Swap the prepared state in. When the swap fails and its rollback left any
/// of the previous state in the work directories, keep those directories on
/// disk, name them in the error so nothing is lost, and leave `server.ready`
/// removed, since the active state is not whole. Every other failure here
/// leaves the previous state installed, and `ready` writes the marker back.
pub(super) async fn install_reset_state_or_keep_work(
    paths: &ResetPaths<'_>,
    db_work: tempfile::TempDir,
    data_work: tempfile::TempDir,
    ready: &mut crate::operation_lock::ReadyWhileRebuilding,
) -> Result<()> {
    let Err(error) = install_reset_state(paths).await else {
        return Ok(());
    };
    let previous_state_still_in_work = db_work.path().join("previous-messagecrate.db").exists()
        || data_work.path().join("previous-account").exists();
    if previous_state_still_in_work {
        ready.keep_cleared();
        let db_work = db_work.keep();
        let data_work = data_work.keep();
        return Err(error.context(format!(
            "reset-demo rollback was incomplete; temporary database and account state were kept at {} and {}",
            db_work.display(),
            data_work.display()
        )));
    }
    Err(error)
}

/// Refuse a Message Crate database at `db` stamped with another Schema
/// Fingerprint, before anything is written. Its snapshot would be rebuilt
/// empty, and the copy kept before that rebuild would land in the reset's
/// work directory, which is deleted when the reset ends. `serve` keeps the
/// copy beside the database and rebuilds it, and a reset can follow.
pub(super) async fn refuse_another_layout(db: &Path) -> Result<()> {
    if crate::open_db::inspect(db).await? != Some(schema::DatabaseKind::MessageCrate) {
        return Ok(());
    }
    let mut conn = engine::connect_read_only(db)
        .await
        .with_context(|| format!("failed to open database {}", db.display()))?;
    let stamped = schema::user_version(&mut conn).await;
    sqlx::Connection::close(conn).await?;
    let stamped = stamped.with_context(|| format!("failed to read database {}", db.display()))?;
    if stamped != schema::SCHEMA_FINGERPRINT {
        bail!(
            "{} was made by a server with another database layout (Schema Fingerprint {stamped}, \
             this server's is {}), so reset-demo left it as it is. Start `serve` on it first: \
             it keeps a copy of the database beside it and rebuilds it",
            db.display(),
            schema::SCHEMA_FINGERPRINT
        );
    }
    Ok(())
}

/// Copy the active database to the prepared path (checkpointing the WAL first) so the reset
/// works on a snapshot and the live file is untouched until the swap.
pub(super) async fn prepare_database_snapshot(active: &Path, prepared: &Path) -> Result<()> {
    if active.is_file() {
        let pool = engine::open_pool_for_path(active)
            .await
            .with_context(|| format!("open {} for reset snapshot", active.display()))?;
        let mut conn = pool
            .acquire()
            .await
            .with_context(|| format!("open {} for reset snapshot", active.display()))?;
        schema::copy_database(&mut conn, prepared)
            .await
            .with_context(|| {
                format!(
                    "copy database snapshot {} to {}",
                    active.display(),
                    prepared.display()
                )
            })?;
        conn.close().await?;
        pool.close().await;
    } else {
        let pool = engine::open_pool_for_path(prepared)
            .await
            .with_context(|| format!("create prepared database {}", prepared.display()))?;
        let mut conn = pool
            .acquire()
            .await
            .with_context(|| format!("create prepared database {}", prepared.display()))?;
        schema::ensure_schema(&mut conn).await?;
        conn.close().await?;
        pool.close().await;
    }
    Ok(())
}

/// Copy pending SQLite writes into the main database file, then remove the
/// write-ahead log (`-wal`) and shared-memory (`-shm`) sidecar files so the
/// database can be renamed safely.
pub(super) async fn checkpoint_and_clean_sidecars(db: &Path, operation: &str) -> Result<()> {
    if !db.is_file() {
        return Ok(());
    }
    let pool = engine::open_pool_for_path(db)
        .await
        .with_context(|| format!("open {} {operation}", db.display()))?;
    let mut conn = pool
        .acquire()
        .await
        .with_context(|| format!("open {} {operation}", db.display()))?;
    let finished = maintenance::checkpoint_truncate(&mut conn)
        .await
        .with_context(|| {
            format!(
                "checkpoint SQLite write-ahead log for {} {operation}",
                db.display()
            )
        })?;
    // Close the connection itself, so its `sqlite3_close` has run before
    // anything else touches the files: a close that lands after the TRUNCATE
    // checkpoint can read the truncated `-shm` mapping and crash the process.
    conn.close().await?;
    // Close the pool so no connection stays attached to the database while
    // reset-demo replaces or renames it.
    pool.close().await;
    if !finished {
        bail!(
            "cannot replace {} because its WAL could not be checkpointed; stop every process using the database and run reset-demo offline",
            db.display()
        );
    }

    let wal = sqlite_sidecar(db, "-wal");
    if wal.exists() {
        let length = fs::metadata(&wal)
            .with_context(|| format!("inspect SQLite WAL {}", wal.display()))?
            .len();
        if length != 0 {
            bail!(
                "cannot replace {} because {} still contains {length} bytes after WAL checkpoint; stop every process using the database and run reset-demo offline",
                db.display(),
                wal.display()
            );
        }
        fs::remove_file(&wal).with_context(|| {
            format!(
                "remove empty SQLite WAL {}; reset-demo requires offline database access",
                wal.display()
            )
        })?;
    }
    let shm = sqlite_sidecar(db, "-shm");
    if shm.exists() {
        fs::remove_file(&shm).with_context(|| {
            format!(
                "remove SQLite shared-memory sidecar {}; stop every process using the database and run reset-demo offline",
                shm.display()
            )
        })?;
    }
    Ok(())
}

/// The `-wal` or `-shm` sidecar path next to a SQLite database file.
pub(super) fn sqlite_sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut path: OsString = db.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

#[derive(Clone, Copy)]
pub(super) struct ResetPaths<'a> {
    /// The database file the server reads.
    pub(super) active_db: &'a Path,
    /// The rebuilt copy that replaces it.
    pub(super) prepared_db: &'a Path,
    /// The Demo Account's directory under the data directory.
    pub(super) active_account: &'a Path,
    /// The rebuilt directory that replaces it.
    pub(super) prepared_account: &'a Path,
}

/// Swap the prepared database and account directory into their active paths.
async fn install_reset_state(paths: &ResetPaths<'_>) -> Result<()> {
    install_reset_state_with(paths, demo_seed::move_path).await
}

/// [`install_reset_state`] with the rename step injected, so tests can simulate a rename that fails midway.
pub(super) async fn install_reset_state_with<F>(paths: &ResetPaths<'_>, rename: F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    checkpoint_and_clean_sidecars(paths.prepared_db, "before installing the prepared database")
        .await?;
    checkpoint_and_clean_sidecars(
        paths.active_db,
        "immediately before replacing the active database",
    )
    .await?;
    replace_reset_state_with(paths, rename)
}

/// One of the two things a reset swaps: the database file or the account
/// directory. Each has an active path, a prepared replacement, and a backup path
/// the active one is moved to first so the swap can be undone.
struct Swap<'a> {
    /// What the paths hold, for messages: "database" or "account directory".
    what: &'static str,
    active: &'a Path,
    prepared: &'a Path,
    backup: PathBuf,
    /// Whether an active file existed before the swap, so there is a backup to restore.
    had_active: bool,
    /// Whether the prepared file has been moved into the active path.
    installed: bool,
}

impl<'a> Swap<'a> {
    fn new(what: &'static str, active: &'a Path, prepared: &'a Path, backup: PathBuf) -> Self {
        Self {
            what,
            active,
            prepared,
            backup,
            had_active: active.exists(),
            installed: false,
        }
    }

    /// Move the active file to its backup path, when there is one.
    fn back_up(&self, rename: &mut impl FnMut(&Path, &Path) -> Result<()>) -> Result<()> {
        if !self.had_active {
            return Ok(());
        }
        rename(self.active, &self.backup).with_context(|| {
            format!(
                "move existing {} {} into backup",
                self.what,
                self.active.display()
            )
        })
    }

    /// Move the prepared file into the active path.
    fn install(&mut self, rename: &mut impl FnMut(&Path, &Path) -> Result<()>) -> Result<()> {
        rename(self.prepared, self.active).with_context(|| {
            format!(
                "install prepared {} {} at {}",
                self.what,
                self.prepared.display(),
                self.active.display()
            )
        })?;
        self.installed = true;
        Ok(())
    }

    /// Undo whatever this swap did: remove an installed file, then put the
    /// backup back. Every step is attempted; the problems met are returned
    /// rather than stopping at the first, so as much as possible is restored.
    fn roll_back(&self, rename: &mut impl FnMut(&Path, &Path) -> Result<()>) -> Vec<String> {
        let mut problems = Vec::new();
        if self.installed
            && let Err(error) = remove_any_if_exists(self.active)
        {
            problems.push(format!(
                "remove installed {} {}: {error:#}",
                self.what,
                self.active.display()
            ));
        }
        if self.had_active
            && self.backup.exists()
            && let Err(error) = rename(&self.backup, self.active)
        {
            problems.push(format!(
                "restore previous {} {}: {error:#}",
                self.what,
                self.active.display()
            ));
        }
        problems
    }
}

impl<'a> ResetPaths<'a> {
    /// The two swaps in install order: database, then account directory.
    fn swaps(&self) -> Result<[Swap<'a>; 2]> {
        let db_backup = self
            .prepared_db
            .parent()
            .context("prepared database has no parent")?
            .join("previous-messagecrate.db");
        let account_backup = self
            .prepared_account
            .parent()
            .context("prepared account has no parent")?
            .join("previous-account");
        Ok([
            Swap::new("database", self.active_db, self.prepared_db, db_backup),
            Swap::new(
                "account directory",
                self.active_account,
                self.prepared_account,
                account_backup,
            ),
        ])
    }
}

/// The swap itself: move the active state to backups, move the prepared state in, and roll
/// the backups back if any step fails.
pub(super) fn replace_reset_state_with<F>(paths: &ResetPaths<'_>, mut rename: F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    if !paths.prepared_db.is_file() || !paths.prepared_account.is_dir() {
        bail!("prepared reset state is incomplete");
    }
    if let Some(parent) = paths.active_account.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create account data parent {}", parent.display()))?;
    }
    let mut swaps = paths.swaps()?;

    let outcome = (|| -> Result<()> {
        for swap in &swaps {
            swap.back_up(&mut rename)?;
        }
        for swap in &mut swaps {
            swap.install(&mut rename)?;
        }
        Ok(())
    })();
    let Err(error) = outcome else {
        cleanup_reset_backups(&swaps);
        return Ok(());
    };

    let problems: Vec<String> = swaps
        .iter()
        .rev()
        .flat_map(|swap| swap.roll_back(&mut rename))
        .collect();
    if problems.is_empty() {
        cleanup_reset_backups(&swaps);
        return Err(error.context("replace demo account state"));
    }
    let kept: Vec<String> = swaps
        .iter()
        .map(|swap| swap.backup.display().to_string())
        .collect();
    Err(anyhow::anyhow!(
        "replace demo account state: {error:#}; rollback incomplete; backups kept at {}: {}",
        kept.join(", "),
        problems.join("; ")
    ))
}

/// Remove the backups once the active state is installed or restored. Failure
/// is a warning: the state is right, only a copy is left behind.
fn cleanup_reset_backups(swaps: &[Swap<'_>]) {
    for swap in swaps {
        if let Err(error) = remove_any_if_exists(&swap.backup) {
            console::backup_not_removed(swap.what, &swap.backup, &error);
        }
    }
}

/// Remove a file or directory tree; a missing path is not an error.
pub(super) fn remove_any_if_exists(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))?;
    } else if path.exists() {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}
