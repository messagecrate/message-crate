//! Erasing the text a delete for good removes from the database's files, so
//! a copy of the files made afterwards does not hold it (#2108).
//!
//! A plain `DELETE` marks a row's space free and leaves its bytes in place.
//! Three places keep them, and each has its own remedy:
//!
//! - **The database file's pages.** Every pool connection runs
//!   `PRAGMA secure_delete = ON` ([`crate::db::engine`]), so SQLite writes
//!   zeros over a deleted row, and over every page it frees, as it frees
//!   them. That holds on every write path: a delete for good, the update
//!   that makes a contact Unknown, the cascade under a deleted account.
//! - **The full-text search indexes.** `messages_fts` and
//!   `message_versions_fts` are contentless-delete tables
//!   (`schema/sql/fts_virtual.sql`): deleting a row records its id as gone
//!   and leaves every word it indexed in the index's segments until they are
//!   merged. [`erase_and_commit`] merges each index into one segment, which
//!   writes it again without the deleted rows' words. The pages the old
//!   segments held are freed, so `secure_delete` zeroes them.
//! - **The write-ahead log.** It keeps the page images earlier transactions
//!   wrote, the text among them, until they are written over.
//!   [`clear_write_ahead_log`] copies the log into the database file and
//!   cuts it to zero bytes. When another connection's read keeps it from
//!   doing so, the database is remembered, and [`keep_retrying`] tries again
//!   every [`RETRY_EVERY`] until the log is cleared. A server that stops
//!   clears it too: SQLite copies the log into the file and deletes it when
//!   the last connection closes.
//!
//! A delete for good therefore ends in two calls: [`erase_and_commit`] on its
//! write transaction, then [`clear_write_ahead_log`] on the connection the
//! transaction ran on, which only the caller holds once the commit has
//! consumed the transaction.
//!
//! What the files of the database do not cover is outside this module: the
//! attachment files are unlinked, not overwritten; a copy the owner made,
//! and the `.before-rebuild-` copy a schema rebuild keeps
//! ([`crate::db::schema`]), hold what they held when they were made. Text
//! deleted before the server wrote over freed pages stays in pages freed
//! then, until they are used again or the file is vacuumed. The user guide's
//! Trash page says so.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use sqlx::{SqliteConnection, SqlitePool};

use crate::db::WriteTx;

/// The statements that merge each full-text search index into one segment.
/// FTS5's `optimize` is the only merge that takes in every segment, so it is
/// the only one that leaves no deleted row's words behind.
const OPTIMIZE_SEARCH_INDEXES: [&str; 2] = [
    "INSERT INTO messages_fts(messages_fts) VALUES('optimize')",
    "INSERT INTO message_versions_fts(message_versions_fts) VALUES('optimize')",
];

/// How long clearing the log waits for another connection's read to end.
/// The checkpoint holds the write lock while it waits, so every other writer
/// waits with it: the wait is short, and a read that outlasts it leaves the
/// clearing to [`keep_retrying`].
const CHECKPOINT_WAIT: Duration = Duration::from_secs(1);

/// How often [`keep_retrying`] tries again to clear a log a read kept.
pub(crate) const RETRY_EVERY: Duration = Duration::from_secs(30);

/// The database files whose log still holds text deleted for good, because
/// a read kept [`clear_write_ahead_log`] from clearing it. Keyed by file, so
/// two databases in one process, as in the tests, never clear each other's.
static LOGS_TO_CLEAR: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// Merge each full-text search index into one segment, so the words of the
/// rows `tx` deleted are no longer in it, then commit `tx`. The merge runs
/// in the delete's own transaction, so the delete and the erasing of its
/// words land together or not at all. The caller then clears the log with
/// [`clear_write_ahead_log`].
///
/// The merge rewrites the whole index, so its time grows with the index,
/// not with what was deleted: about 0.8 s on the large Demo Data's 636,000
/// messages, where deleting a conversation of 916 messages took 0.3 s
/// without it. An index already in one segment with nothing deleted from it
/// is left as it is, at no cost.
///
/// # Errors
///
/// Returns the database error when a merge or the commit fails; the
/// transaction then rolls back with the delete in it.
pub(crate) async fn erase_and_commit(mut tx: WriteTx<'_>) -> sqlx::Result<()> {
    for sql in OPTIMIZE_SEARCH_INDEXES {
        sqlx::query(sql).execute(&mut *tx).await?;
    }
    tx.commit().await
}

/// What [`clear_write_ahead_log`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogCleared {
    /// The log was copied into the database file and cut to zero bytes, or
    /// the database does not run with one.
    Cleared,
    /// The log was left as it was: another connection's read outlasted
    /// [`CHECKPOINT_WAIT`], or the checkpoint failed. A warning in the
    /// server's log says which, and [`keep_retrying`] tries again.
    Kept,
}

/// Copy the write-ahead log into the database file and cut it to zero bytes,
/// after a delete for good has committed, so the page images earlier
/// transactions wrote go from the log. It waits [`CHECKPOINT_WAIT`] for
/// another connection's read to end.
///
/// The delete has committed by the time this runs, so a log that is kept is
/// a warning in the server's log, never a failed request. The database is
/// then remembered for [`keep_retrying`].
pub(crate) async fn clear_write_ahead_log(conn: &mut SqliteConnection) -> LogCleared {
    let file = crate::db::schema::database_file(conn).await.ok().flatten();
    let cleared = match checkpoint_waiting_briefly(conn).await {
        Ok(true) => LogCleared::Cleared,
        Ok(false) => {
            tracing::warn!(
                "The write-ahead log could not be cleared after a delete for good, because another connection was still reading, so the server tries again every {} s",
                RETRY_EVERY.as_secs()
            );
            LogCleared::Kept
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "The write-ahead log could not be cleared after a delete for good, so the server tries again every {} s",
                RETRY_EVERY.as_secs()
            );
            LogCleared::Kept
        }
    };
    if let Some(file) = file {
        let mut pending = LOGS_TO_CLEAR.lock().unwrap_or_else(|e| e.into_inner());
        match cleared {
            LogCleared::Cleared => pending.remove(&file),
            LogCleared::Kept => pending.insert(file),
        };
    }
    cleared
}

/// Run `PRAGMA wal_checkpoint(TRUNCATE)` with `busy_timeout` at
/// [`CHECKPOINT_WAIT`], then put the connection's own timeout back. Answers
/// whether the checkpoint finished.
async fn checkpoint_waiting_briefly(conn: &mut SqliteConnection) -> sqlx::Result<bool> {
    let own: i64 = sqlx::query_scalar("SELECT timeout FROM pragma_busy_timeout")
        .fetch_one(&mut *conn)
        .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA busy_timeout = {}",
        CHECKPOINT_WAIT.as_millis()
    )))
    .execute(&mut *conn)
    .await?;
    let answer = sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&mut *conn)
        .await;
    sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA busy_timeout = {own}")))
        .execute(&mut *conn)
        .await?;
    Ok(answer?.0 == 0)
}

/// Try again to clear the log of the database behind `pool`, when a read
/// kept [`clear_write_ahead_log`] from clearing it. Does nothing, and takes
/// no connection, while no log is waiting.
pub(crate) async fn retry_clearing(pool: &SqlitePool) {
    if LOGS_TO_CLEAR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
    {
        return;
    }
    let Ok(mut conn) = pool.acquire().await else {
        return;
    };
    let Ok(Some(file)) = crate::db::schema::database_file(&mut conn).await else {
        return;
    };
    let waiting = LOGS_TO_CLEAR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&file);
    if waiting && clear_write_ahead_log(&mut conn).await == LogCleared::Cleared {
        tracing::info!("The write-ahead log is cleared of the text deleted for good");
    }
}

/// Call [`retry_clearing`] on `pool` every [`RETRY_EVERY`] while the server
/// runs. The task ends with the process.
pub(crate) fn keep_retrying(pool: SqlitePool) {
    crate::request_id::spawn_in_current_span(async move {
        let mut ticks = tokio::time::interval(RETRY_EVERY);
        loop {
            ticks.tick().await;
            retry_clearing(&pool).await;
        }
    });
}

#[cfg(test)]
mod tests;
