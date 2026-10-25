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
//! consumed the transaction. An Edit load of an address book
//! ([`crate::db::address_book::load`]) deletes for good the identities its
//! file no longer lists that nothing else uses, and touches no search
//! index, so it ends in [`clear_write_ahead_log`] alone.
//!
//! What the files of the database do not cover is outside this module: the
//! attachment files are unlinked, not overwritten; a copy the administrator
//! made, and the `.before-rebuild-` copy a schema rebuild keeps
//! ([`crate::db::schema`]), hold what they held when they were made. Text
//! deleted before the server wrote over freed pages stays in pages freed then,
//! until they are used again or the file is vacuumed. The user guide's Trash
//! page says so.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context as _, Result};
use sqlx::{SqliteConnection, SqlitePool};

use crate::db::WriteTx;

/// The statements that merge each full-text search index into one segment,
/// each with the step it names in an error.
/// FTS5's `optimize` is the only merge that takes in every segment, so it is
/// the only one that leaves no deleted row's words behind.
const OPTIMIZE_SEARCH_INDEXES: [(&str, &str); 2] = [
    (
        "INSERT INTO messages_fts(messages_fts) VALUES('optimize')",
        "merge the message search index into one segment",
    ),
    (
        "INSERT INTO message_versions_fts(message_versions_fts) VALUES('optimize')",
        "merge the message version search index into one segment",
    ),
];

/// How long clearing the log waits for another connection's read to end.
/// The checkpoint holds the write lock while it waits, so every other writer
/// waits with it: the wait is short, and a read that outlasts it leaves the
/// clearing to [`keep_retrying`].
const CHECKPOINT_WAIT: Duration = Duration::from_secs(1);

/// How often [`keep_retrying`] tries again to clear a log a read kept.
pub(crate) const RETRY_EVERY: Duration = Duration::from_secs(30);

/// The database files whose log still holds text deleted for good, because
/// a read kept [`clear_write_ahead_log`] from clearing it.
///
/// It is the process's, not a pool's, because the delete functions that
/// clear the log are handed a connection, not the pool or the server's
/// state. Keyed by file, so two databases in one process, as in the tests,
/// never clear each other's.
struct LogsToClear {
    /// Each waiting file, with the count of [`LogsToClear::kept`] when its
    /// log was last kept.
    waiting: BTreeMap<PathBuf, u64>,
    /// How many times a log has been kept, so a checkpoint can tell whether
    /// another was kept while it ran.
    kept: u64,
}

static LOGS_TO_CLEAR: Mutex<LogsToClear> = Mutex::new(LogsToClear {
    waiting: BTreeMap::new(),
    kept: 0,
});

/// [`LOGS_TO_CLEAR`], locked. A panic while it was held left it whole,
/// because every change to it is a single insert or remove.
fn logs_to_clear() -> std::sync::MutexGuard<'static, LogsToClear> {
    LOGS_TO_CLEAR.lock().unwrap_or_else(|e| e.into_inner())
}

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
/// Returns an error naming the step when a merge or the commit fails; the
/// transaction then rolls back with the delete in it.
pub(crate) async fn erase_and_commit(mut tx: WriteTx<'_>) -> Result<()> {
    for (sql, step) in OPTIMIZE_SEARCH_INDEXES {
        sqlx::query(sql).execute(&mut *tx).await.context(step)?;
    }
    tx.commit()
        .await
        .context("commit the delete for good with its erased search words")
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

/// Copy the write-ahead log of the database behind `conn` into the database
/// file and cut it to zero bytes, after a delete for good has committed, so
/// the page images earlier transactions wrote go from the log.
///
/// The checkpoint runs on a connection of its own, opened for it and closed
/// after it ([`crate::db::engine::connect_for_checkpoint`]), which waits
/// [`CHECKPOINT_WAIT`] for another connection's read to end. `conn` comes
/// from the pool and goes back to it unchanged, even when the request is
/// dropped while the checkpoint waits.
///
/// The delete has committed by the time this runs, so a log that is kept is
/// a warning in the server's log, never a failed request. The database is
/// then remembered for [`keep_retrying`].
pub(crate) async fn clear_write_ahead_log(conn: &mut SqliteConnection) -> LogCleared {
    let file = match crate::db::schema::database_file(conn).await {
        Ok(Some(file)) => file,
        // An in-memory database has no log to clear.
        Ok(None) => return LogCleared::Cleared,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "The write-ahead log could not be cleared after a delete for good, because the database's file could not be named, so it holds the deleted text until the server stops"
            );
            return LogCleared::Kept;
        }
    };
    clear_log_of(file).await
}

/// [`clear_write_ahead_log`] for the database at `file`, recording the
/// outcome in [`LOGS_TO_CLEAR`].
async fn clear_log_of(file: PathBuf) -> LogCleared {
    // A log kept by a checkpoint that starts after this one is not cleared
    // by this one, so only an entry this checkpoint has seen is removed.
    let seen = logs_to_clear().waiting.get(&file).copied();
    let checkpoint = checkpoint(&file).await;
    let cleared = match &checkpoint {
        Ok(true) => LogCleared::Cleared,
        Ok(false) | Err(_) => {
            let cause = match &checkpoint {
                Err(err) => format!("the checkpoint failed: {err:#}"),
                _ => "another connection was still reading".to_owned(),
            };
            tracing::warn!(
                cause,
                "The write-ahead log could not be cleared after a delete for good, so the server tries again every {} s",
                RETRY_EVERY.as_secs()
            );
            LogCleared::Kept
        }
    };
    record(&mut logs_to_clear(), file, seen, cleared);
    cleared
}

/// Record in `logs` what a checkpoint of `file` did. `seen` is the file's
/// entry when the checkpoint started: a clear removes the entry only if it
/// is still that one, because a log kept by a checkpoint that ran meanwhile
/// may hold frames this one did not clear.
fn record(logs: &mut LogsToClear, file: PathBuf, seen: Option<u64>, cleared: LogCleared) {
    match cleared {
        LogCleared::Cleared => {
            if logs.waiting.get(&file).copied() == seen {
                logs.waiting.remove(&file);
            }
        }
        LogCleared::Kept => {
            logs.kept += 1;
            let kept = logs.kept;
            logs.waiting.insert(file, kept);
        }
    }
}

/// Run `PRAGMA wal_checkpoint(TRUNCATE)` on a connection of its own to
/// `file`, and answer whether it finished.
async fn checkpoint(file: &std::path::Path) -> Result<bool> {
    use sqlx::Connection as _;
    let mut own = crate::db::engine::connect_for_checkpoint(file, CHECKPOINT_WAIT).await?;
    let finished = crate::db::maintenance::checkpoint_truncate(&mut own).await?;
    own.close()
        .await
        .context("close the checkpoint's connection")?;
    Ok(finished)
}

/// Try again to clear the log of the database behind `pool`, when a read
/// kept [`clear_write_ahead_log`] from clearing it. Does nothing, and takes
/// no connection, while no log is waiting.
pub(crate) async fn retry_clearing(pool: &SqlitePool) {
    if logs_to_clear().waiting.is_empty() {
        return;
    }
    let file = {
        let Ok(mut conn) = pool.acquire().await else {
            return;
        };
        let Ok(Some(file)) = crate::db::schema::database_file(&mut conn).await else {
            return;
        };
        file
    };
    if logs_to_clear().waiting.contains_key(&file)
        && clear_log_of(file).await == LogCleared::Cleared
    {
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
