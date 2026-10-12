//! Pool construction for the SQLite database file.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, SqliteConnection, SqlitePool};

/// The statement that begins a write transaction. IMMEDIATE takes the write
/// lock at once, so overlapping writers wait on the busy timeout instead of
/// failing at their first write.
pub const BEGIN_IMMEDIATE_SQL: &str = "BEGIN IMMEDIATE TRANSACTION";

/// Turn `trusted_schema` OFF on `conn`, before it reads the file.
///
/// With it OFF, a view, trigger, `CHECK` constraint or index expression in
/// the file may call only a function registered innocuous, so a crafted file
/// that carries the right `application_id` cannot make the server call any
/// other function by being read. Every function the server registers is
/// innocuous ([`crate::db::sqlite_functions`]). Every connection the server
/// opens on its database runs this: the pool's ([`with_pragmas`]) and the
/// read-only one ([`connect_read_only`]).
async fn distrust_schema(conn: &mut SqliteConnection) -> sqlx::Result<()> {
    sqlx::query("PRAGMA trusted_schema = OFF")
        .execute(conn)
        .await
        .map(drop)
}

/// A read-only connection to the SQLite file at `path`, set up for a file
/// that may have been tampered with as the pool's connections are: in
/// defensive mode with the server's SQL functions
/// ([`crate::db::sqlite_functions`]), and with `trusted_schema` OFF
/// ([`distrust_schema`]). It runs none of the pool's other pragmas, and
/// never asks for write-ahead logging, so it changes nothing in the file.
///
/// # Errors
///
/// Returns an error when the file cannot be opened, or the pragma fails.
pub(crate) async fn connect_read_only(path: &Path) -> sqlx::Result<SqliteConnection> {
    crate::db::sqlite_functions::register();
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    distrust_schema(&mut conn).await?;
    Ok(conn)
}

/// How long a statement that needs the write lock waits for another
/// writer's transaction, such as an import batch's, before SQLite answers
/// `SQLITE_BUSY`. The request then answers `503 Service Unavailable`
/// (`database-busy`).
const BUSY_TIMEOUT: Duration = Duration::from_secs(15);

/// How many connections the pool opens at most. Up to two import batches
/// (`MAX_CONCURRENT_IMPORTS`) hold one each for their whole run, and the
/// sweep of unreferenced files after an Import Run
/// (`asset_store::sweep_after_run`) holds one more, so eight leave five for
/// every other request. Under write-ahead logging a reader never waits for a
/// writer, so those five serve reads side by side. Each connection keeps a
/// page cache of its own of up to [`CACHE_KIB`], so the pool's caches can
/// hold `MAX_CONNECTIONS * CACHE_KIB` in all; the two are set together. The
/// count is no higher because more connections would raise that total past
/// about 800 MB, or shrink each connection's cache further.
const MAX_CONNECTIONS: u32 = 8;

/// The most page cache one connection keeps, in KiB (`PRAGMA cache_size`
/// with a negative value). About 100 MB, so eight connections hold at most
/// about 800 MB, what four held at 200 MB each before the pool grew (#2124).
const CACHE_KIB: u32 = 100_000;

/// How long a request waits for a free connection before sqlx gives up with
/// `PoolTimedOut`. The request then answers `503 Service Unavailable`
/// (`database-busy`).
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// The server's pragma set, applied to each new connection: `trusted_schema`
/// OFF first ([`distrust_schema`]), then `busy_timeout` (overlapping auth and
/// UI writes wait that long for the write lock), foreign keys on, synchronous
/// NORMAL, `temp_store` MEMORY, `cache_size` -[`CACHE_KIB`].
fn with_pragmas(pool: SqlitePoolOptions, busy_timeout: Duration) -> SqlitePoolOptions {
    let busy_timeout_sql = format!("PRAGMA busy_timeout = {}", busy_timeout.as_millis());
    let cache_size_sql = format!("PRAGMA cache_size = -{CACHE_KIB}");
    pool.after_connect(move |conn, _meta| {
        let busy_timeout_sql = busy_timeout_sql.clone();
        let cache_size_sql = cache_size_sql.clone();
        Box::pin(async move {
            distrust_schema(conn).await?;
            sqlx::query(&busy_timeout_sql).execute(&mut *conn).await?;
            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA synchronous = NORMAL")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA temp_store = MEMORY")
                .execute(&mut *conn)
                .await?;
            sqlx::query(&cache_size_sql).execute(&mut *conn).await?;
            Ok(())
        })
    })
}

/// Connect options for a database file, created when missing.
fn connect_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
}

/// Pool options for SQLite: [`MAX_CONNECTIONS`] connections, a wait of
/// [`ACQUIRE_TIMEOUT`] for one, and the pragmas with `busy_timeout` (the
/// server's is [`BUSY_TIMEOUT`]). Every SQLite pool comes through here, so
/// this is also where the server's SQL functions are registered for the
/// connections the pool will open ([`crate::db::sqlite_functions`]), and, in
/// tests, the guard that refuses a write to messages outside a transaction
/// (`crate::db::write_guard`).
fn sqlite_pool_options(busy_timeout: Duration) -> SqlitePoolOptions {
    crate::db::sqlite_functions::register();
    #[cfg(test)]
    crate::db::write_guard::register();
    with_pragmas(
        SqlitePoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .acquire_timeout(ACQUIRE_TIMEOUT),
        busy_timeout,
    )
}

/// The journal mode SQLite runs a database in, as `PRAGMA journal_mode`
/// names it.
///
/// The server asks for `wal` on every open. In any other mode a reader waits
/// for a writer, so every page load waits while an import batch holds its
/// write transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JournalMode {
    /// A rollback journal, deleted at the end of each transaction.
    Delete,
    /// A rollback journal, cut to zero length at the end of each transaction.
    Truncate,
    /// A rollback journal whose header is zeroed at the end of each
    /// transaction.
    Persist,
    /// A rollback journal kept in memory.
    Memory,
    /// Write-ahead logging: a reader does not wait for a writer.
    Wal,
    /// No journal at all.
    Off,
}

impl JournalMode {
    /// The mode SQLite names, in any case, read through the same names the
    /// mode has on the wire.
    ///
    /// # Errors
    ///
    /// Returns an error for a name SQLite never answers.
    fn parse(name: &str) -> Result<Self> {
        use serde::de::IntoDeserializer;
        let lower: serde::de::value::StringDeserializer<serde::de::value::Error> =
            name.to_ascii_lowercase().into_deserializer();
        Self::deserialize(lower)
            .with_context(|| format!("SQLite answered an unknown journal mode, {name:?}"))
    }

    /// The name SQLite gives the mode.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Truncate => "truncate",
            Self::Persist => "persist",
            Self::Memory => "memory",
            Self::Wal => "wal",
            Self::Off => "off",
        }
    }
}

impl std::fmt::Display for JournalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The journal mode the database runs in now.
///
/// # Errors
///
/// Returns an error when the database cannot be read, or SQLite names a mode
/// this server does not know.
pub async fn journal_mode<'c, E>(executor: E) -> Result<JournalMode>
where
    E: sqlx::SqliteExecutor<'c>,
{
    let name: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(executor)
        .await
        .context("read the journal mode")?;
    JournalMode::parse(&name)
}

/// The journal mode the database runs in now, or `None`, with a warning in
/// the log, when it cannot be read.
pub async fn journal_mode_or_warn<'c, E>(executor: E) -> Option<JournalMode>
where
    E: sqlx::SqliteExecutor<'c>,
{
    mode_or_warn(journal_mode(executor).await)
}

/// The mode an answer names, or `None`, with a warning in the log, when the
/// answer is an error.
fn mode_or_warn(answer: Result<JournalMode>) -> Option<JournalMode> {
    answer
        .inspect_err(|err| {
            tracing::warn!(
                error = format!("{err:#}"),
                "The database's journal mode could not be read"
            );
        })
        .ok()
}

/// What SQLite answered when the server asked it for write-ahead logging.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WalAnswer {
    /// The mode the database ended in. `None` when it could not be read.
    mode: Option<JournalMode>,
    /// The error SQLite gave for the request, in SQLite's words. `None` when
    /// it gave none: it turned write-ahead logging on, or kept another mode
    /// without an error.
    error: Option<Arc<str>>,
}

/// Ask SQLite for write-ahead logging, and return what it answered: the mode
/// the database ends in, and the error it gave, if any. Another process
/// holding the database, or a hot rollback journal that cannot be rolled
/// back (in a read-only directory, say), can make the request fail. A hot
/// journal that can be rolled back is not a cause: SQLite rolls it back on
/// the first read. SQLite also turns it down without an error where the
/// database cannot use it (one held in memory, say), and answers the mode it
/// kept. Either way the pool stays usable, and a warning in the log says so.
/// The mode is `None` when it cannot be read at all. That is warned of too,
/// and [`open_pool_keeping_wal_error`] still returns the pool.
async fn try_enable_wal(pool: &SqlitePool) -> WalAnswer {
    let mode = match sqlx::query_scalar::<_, String>("PRAGMA journal_mode = WAL")
        .fetch_one(pool)
        .await
    {
        Ok(name) => mode_or_warn(JournalMode::parse(&name)),
        Err(err) => {
            tracing::warn!(error = %err, "Write-ahead logging could not be turned on, so the database goes on without it");
            // SQLite's own words when the error is SQLite's, without sqlx's
            // prefix and code; sqlx's whole message otherwise.
            let error: Arc<str> = err
                .as_database_error()
                .map_or_else(|| err.to_string(), |db| db.message().to_owned())
                .into();
            return WalAnswer {
                mode: journal_mode_or_warn(pool).await,
                error: Some(error),
            };
        }
    };
    if let Some(mode) = mode
        && mode != JournalMode::Wal
    {
        tracing::warn!(
            "SQLite kept the database in journal mode {mode} rather than write-ahead logging, so the database goes on without it"
        );
    }
    WalAnswer { mode, error: None }
}

/// Open the configured pool for a SQLite file, naming the file in the error
/// context. The file is created when missing.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or created.
pub async fn open_pool_for_path(path: &Path) -> Result<SqlitePool> {
    Ok(open_pool_keeping_wal_error(path).await?.pool)
}

/// A pool [`open_pool_keeping_wal_error`] opened, with the error SQLite gave
/// when the pool asked for write-ahead logging.
#[derive(Debug)]
pub struct OpenedPool {
    /// The pool, usable whatever SQLite answered.
    pub pool: SqlitePool,
    /// The error SQLite gave, in SQLite's words, such as `database is
    /// locked`. `None` when it gave none: it turned write-ahead logging on,
    /// or kept another mode without an error.
    pub wal_error: Option<Arc<str>>,
}

/// [`open_pool_for_path`], keeping the error SQLite gave when the pool
/// asked for write-ahead logging, so Owner Home can say why the database
/// runs without it.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or created.
pub async fn open_pool_keeping_wal_error(path: &Path) -> Result<OpenedPool> {
    let pool = sqlite_pool_options(BUSY_TIMEOUT)
        .connect_with(connect_options(path))
        .await
        .with_context(|| format!("failed to open database {}", path.display()))?;
    let wal_error = try_enable_wal(&pool).await.error;
    Ok(OpenedPool { pool, wal_error })
}

/// Close `pool` and every connection it opened, so none is left attached to
/// the database file when this returns.
///
/// sqlx 0.8.6's `Pool::close` alone can return with a connection still
/// open. A dropped connection goes back to the pool through a task that
/// pings it first. The close waits for every connection's permit, but each
/// idle connection it closes hands back one permit more than it held, so
/// the wait can end while that task is still pinging. The task then puts
/// the connection among the idle ones, open, after the close has taken
/// them. So this closes again, letting that task run in between, until the
/// pool counts no connection. Each pass ends: once the pool is closed a
/// connection on its way back is either closed by its task or left idle for
/// the next pass, and none can be checked out.
///
/// The connection left open matters before the file is removed, replaced or
/// renamed. When it closes later as the last connection on its file, SQLite
/// deletes the `-wal` and `-shm` files by name, and by then those names can
/// be another database's: the next start's new database lost its
/// write-ahead log, and the tables in it, that way (#2592).
///
/// This goes once a sqlx release's `Pool::close` closes every connection
/// (#2884).
#[allow(clippy::disallowed_methods)] // the one place that calls Pool::close
pub async fn close_pool(pool: &SqlitePool) {
    pool.close().await;
    while pool.size() > 0 {
        tokio::task::yield_now().await;
        pool.close().await;
    }
}

/// Shared test pool: file-backed SQLite in a fresh temp dir, returned with
/// the pool so the test's files live there too. It runs in WAL mode, as
/// [`open_pool_for_path`] does, because a write that lands between another
/// transaction's read and its write fails differently under WAL than under a
/// rollback journal, and a test must see what the server sees.
#[cfg(test)]
pub(crate) async fn test_pool() -> (SqlitePool, tempfile::TempDir) {
    test_pool_with_busy_timeout(BUSY_TIMEOUT).await
}

/// [`test_pool`] with `busy_timeout` in place of the server's, so a test
/// meets `SQLITE_BUSY` at once rather than after a wait.
#[cfg(test)]
pub(crate) async fn test_pool_with_busy_timeout(
    busy_timeout: Duration,
) -> (SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("messagecrate.db");
    let pool = sqlite_pool_options(busy_timeout)
        .connect_with(connect_options(&path))
        .await
        .unwrap();
    assert_eq!(try_enable_wal(&pool).await.mode, Some(JournalMode::Wal));
    (pool, dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_sqlite_pool_and_applies_pragmas() {
        let (pool, _dir) = test_pool().await;
        // Every pragma with_pragmas sets, read back through its pragma
        // table function (values must match with_pragmas).
        let busy_timeout: i64 = sqlx::query_scalar("SELECT timeout FROM pragma_busy_timeout")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(busy_timeout, 15000, "busy_timeout");
        // Two imports and the sweep after an Import Run leave connections
        // for every other request, and a request gives up waiting for one
        // well before a browser or a proxy gives up on it (#2124).
        assert_eq!(pool.options().get_max_connections(), 8);
        assert_eq!(
            pool.options().get_acquire_timeout(),
            Duration::from_secs(10)
        );
        let on: i64 = sqlx::query_scalar("SELECT foreign_keys FROM pragma_foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(on, 1, "foreign_keys");
        let synchronous: i64 = sqlx::query_scalar("SELECT synchronous FROM pragma_synchronous")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(synchronous, 1, "synchronous must be NORMAL");
        let temp_store: i64 = sqlx::query_scalar("SELECT temp_store FROM pragma_temp_store")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(temp_store, 2, "temp_store must be MEMORY");
        let cache_size: i64 = sqlx::query_scalar("SELECT cache_size FROM pragma_cache_size")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(cache_size, -100000, "cache_size");
        // A view, trigger, CHECK constraint or index expression in the file
        // may call only a function registered innocuous (#2249).
        let trusted_schema: i64 =
            sqlx::query_scalar("SELECT trusted_schema FROM pragma_trusted_schema")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(trusted_schema, 0, "trusted_schema must be OFF");
        // The tests run in the journal mode the server runs in, so a test can
        // show what WAL does to a write that lands between a read and a write.
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(journal_mode, "wal", "journal_mode");
        // The pool is usable for real work.
        sqlx::query("CREATE TABLE t1 (id INTEGER PRIMARY KEY, v TEXT)")
            .execute(&pool)
            .await
            .unwrap();
    }

    /// SQLite turns write-ahead logging down for a database held in memory
    /// without an error, and answers the mode it kept. The mode returned is
    /// that one, not the one asked for.
    #[tokio::test]
    async fn a_turned_down_wal_request_returns_the_mode_kept() {
        let pool = sqlite_pool_options(BUSY_TIMEOUT)
            .connect_with(SqliteConnectOptions::new().in_memory(true))
            .await
            .unwrap();
        assert_eq!(try_enable_wal(&pool).await.mode, Some(JournalMode::Memory));
        assert_eq!(journal_mode(&pool).await.unwrap(), JournalMode::Memory);
    }

    /// Another connection's write transaction keeps SQLite from turning
    /// write-ahead logging on. The answer keeps the error SQLite gave, in
    /// SQLite's words, so Owner Home can name the cause (#2691), and the
    /// mode the database stayed in.
    #[tokio::test]
    async fn a_refused_wal_request_keeps_the_error_sqlite_gave() {
        use sqlx::Connection;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("messagecrate.db");
        let mut holder = connect_options(&path).connect().await.unwrap();
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
            .execute(&mut holder)
            .await
            .unwrap();
        // The write lock, held until the end of the test.
        let tx = holder.begin_with(BEGIN_IMMEDIATE_SQL).await.unwrap();

        // No wait for the lock, so SQLite answers at once.
        let pool = sqlite_pool_options(Duration::ZERO)
            .connect_with(connect_options(&path))
            .await
            .unwrap();
        let answer = try_enable_wal(&pool).await;

        assert_eq!(answer.mode, Some(JournalMode::Delete));
        assert_eq!(answer.error.as_deref(), Some("database is locked"));
        tx.rollback().await.unwrap();
    }

    /// A request SQLite grants, or turns down without an error, has no error
    /// to keep.
    #[tokio::test]
    async fn a_granted_or_quietly_turned_down_wal_request_keeps_no_error() {
        let (pool, _dir) = test_pool().await;
        assert_eq!(try_enable_wal(&pool).await.error, None);
        let memory = sqlite_pool_options(BUSY_TIMEOUT)
            .connect_with(SqliteConnectOptions::new().in_memory(true))
            .await
            .unwrap();
        assert_eq!(try_enable_wal(&memory).await.error, None);
    }

    /// The name the start-up line prints is the one SQLite answers, which is
    /// the one the mode is read back by, in any case.
    #[test]
    fn a_mode_reads_back_from_the_name_it_prints_in_any_case() {
        for mode in [
            JournalMode::Delete,
            JournalMode::Truncate,
            JournalMode::Persist,
            JournalMode::Memory,
            JournalMode::Wal,
            JournalMode::Off,
        ] {
            assert_eq!(
                JournalMode::parse(&mode.to_string().to_ascii_uppercase()).unwrap(),
                mode
            );
        }
        assert!(JournalMode::parse("wal2").is_err());
    }
}
