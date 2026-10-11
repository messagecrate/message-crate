//! Pool construction for the SQLite database file.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// The statement that begins a write transaction. IMMEDIATE takes the write
/// lock at once, so overlapping writers wait on the busy timeout instead of
/// failing at their first write.
pub const BEGIN_IMMEDIATE_SQL: &str = "BEGIN IMMEDIATE TRANSACTION";

/// The server's historical pragma set, applied to each new connection:
/// busy timeout first (overlapping auth and UI writes wait), foreign keys on,
/// synchronous NORMAL, `temp_store` MEMORY, `cache_size` -200000.
fn with_pragmas(pool: SqlitePoolOptions) -> SqlitePoolOptions {
    pool.after_connect(|conn, _meta| {
        Box::pin(async move {
            sqlx::query("PRAGMA busy_timeout = 15000")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA synchronous = NORMAL")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA temp_store = MEMORY")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA cache_size = -200000")
                .execute(&mut *conn)
                .await?;
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

/// Pool options for SQLite: four connections plus the pragmas. Every
/// SQLite pool comes through here, so this is also where the server's SQL
/// functions are registered for the connections the pool will open
/// ([`crate::db::sqlite_functions`]), and, in tests, the guard that refuses
/// a write to messages outside a transaction (`crate::db::write_guard`).
fn sqlite_pool_options() -> SqlitePoolOptions {
    crate::db::sqlite_functions::register();
    #[cfg(test)]
    crate::db::write_guard::register();
    with_pragmas(SqlitePoolOptions::new().max_connections(4))
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

/// Ask SQLite for write-ahead logging, and return the mode the database ends
/// in. A hot rollback journal or another process holding the database can
/// make the request fail. SQLite also turns it down without an error where
/// the database cannot use it (one held in memory, say), and answers the
/// mode it kept. Either way the pool stays usable, and a warning in the log
/// says so. `None` when the mode cannot be read at all. That is warned of
/// too, and [`open_pool_for_path`] still returns the pool.
async fn try_enable_wal(pool: &SqlitePool) -> Option<JournalMode> {
    let mode = match sqlx::query_scalar::<_, String>("PRAGMA journal_mode = WAL")
        .fetch_one(pool)
        .await
    {
        Ok(name) => mode_or_warn(JournalMode::parse(&name))?,
        Err(err) => {
            tracing::warn!(error = %err, "Write-ahead logging could not be turned on, so the database goes on without it");
            return journal_mode_or_warn(pool).await;
        }
    };
    if mode != JournalMode::Wal {
        tracing::warn!(
            "SQLite kept the database in journal mode {mode} rather than write-ahead logging, so the database goes on without it"
        );
    }
    Some(mode)
}

/// Open the configured pool for a SQLite file, naming the file in the error
/// context. The file is created when missing.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or created.
pub async fn open_pool_for_path(path: &Path) -> Result<SqlitePool> {
    let pool = sqlite_pool_options()
        .connect_with(connect_options(path))
        .await
        .with_context(|| format!("failed to open database {}", path.display()))?;
    try_enable_wal(&pool).await;
    Ok(pool)
}

/// Shared test pool: file-backed SQLite in a fresh temp dir, returned with
/// the pool so the test's files live there too. It runs in WAL mode, as
/// [`open_pool_for_path`] does, because a write that lands between another
/// transaction's read and its write fails differently under WAL than under a
/// rollback journal, and a test must see what the server sees.
#[cfg(test)]
pub(crate) async fn test_pool() -> (SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("messagecrate.db");
    let pool = sqlite_pool_options()
        .connect_with(connect_options(&path))
        .await
        .unwrap();
    assert_eq!(try_enable_wal(&pool).await, Some(JournalMode::Wal));
    (pool, dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_sqlite_pool_and_applies_pragmas() {
        let (pool, _dir) = test_pool().await;
        // All five pragmas, read back through their pragma table
        // functions (values must match with_pragmas).
        let busy_timeout: i64 = sqlx::query_scalar("SELECT timeout FROM pragma_busy_timeout")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(busy_timeout, 15000, "busy_timeout");
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
        assert_eq!(cache_size, -200000, "cache_size");
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
        let pool = sqlite_pool_options()
            .connect_with(SqliteConnectOptions::new().in_memory(true))
            .await
            .unwrap();
        assert_eq!(try_enable_wal(&pool).await, Some(JournalMode::Memory));
        assert_eq!(journal_mode(&pool).await.unwrap(), JournalMode::Memory);
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
