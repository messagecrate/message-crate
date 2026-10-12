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
//!   merged. [`erase_deleted_words`] merges each index into one segment,
//!   which writes it again without the deleted rows' words. The pages the
//!   old segments held are freed, so `secure_delete` zeroes them.
//! - **The write-ahead log.** It keeps the page images earlier transactions
//!   wrote, the text among them, until they are written over.
//!   [`clear_write_ahead_log`] copies the log into the database file and
//!   cuts it to zero bytes. Every pool connection also runs
//!   `PRAGMA journal_size_limit = 0`, so the first commit after the log
//!   starts over from its beginning cuts it to that commit's own frames: the
//!   log is cleared that way too when [`clear_write_ahead_log`] could not
//!   finish because another connection was still reading.
//!
//! What the files of the database do not cover is outside this module: the
//! attachment files are unlinked, not overwritten; a copy the owner made,
//! and the `.before-rebuild-` copy a schema rebuild keeps
//! ([`crate::db::schema`]), hold what they held when they were made. The
//! user guide's Trash page says so.

use sqlx::SqliteConnection;

use crate::db::WriteTx;

/// The statements that merge each full-text search index into one segment.
/// FTS5's `optimize` is the only merge that takes in every segment, so it is
/// the only one that leaves no deleted row's words behind.
const OPTIMIZE_SEARCH_INDEXES: [&str; 2] = [
    "INSERT INTO messages_fts(messages_fts) VALUES('optimize')",
    "INSERT INTO message_versions_fts(message_versions_fts) VALUES('optimize')",
];

/// Merge each full-text search index into one segment, so the words of the
/// rows `tx` deleted are no longer in it. It runs in the delete's own
/// transaction, so the delete and the erasing of its words land together or
/// not at all.
///
/// It rewrites the whole index, so its time grows with the index, not with
/// what was deleted: about 0.8 s on the large Demo Data's 636,000 messages,
/// where deleting a conversation of 916 messages took 0.3 s without it. An
/// index already in one segment with nothing deleted from it is left as it
/// is, at no cost.
///
/// # Errors
///
/// Returns the database error when a merge fails; the caller's transaction
/// then rolls back with the delete in it.
pub(crate) async fn erase_deleted_words(tx: &mut WriteTx<'_>) -> sqlx::Result<()> {
    for sql in OPTIMIZE_SEARCH_INDEXES {
        sqlx::query(sql).execute(&mut **tx).await?;
    }
    Ok(())
}

/// What a `PRAGMA wal_checkpoint(TRUNCATE)` answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogCleared {
    /// The log was copied into the database file and cut to zero bytes, or
    /// the database does not run with one.
    Cleared,
    /// Another connection was still reading an older state of the database
    /// when the wait for it ended, so the log was left as it was.
    Busy,
}

/// Copy the write-ahead log into the database file and cut it to zero bytes,
/// after a delete for good has committed, so the page images earlier
/// transactions wrote go from the log. It waits on the busy timeout for
/// another connection's read to end.
///
/// The delete has committed by the time this runs, so a log that cannot be
/// cleared is a warning in the server log, never a failed request: the
/// connection settings named in the module notes clear it at the next start
/// of the log over.
pub(crate) async fn clear_write_ahead_log(conn: &mut SqliteConnection) -> LogCleared {
    let answer = sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&mut *conn)
        .await;
    match answer {
        Ok((0, _, _)) => LogCleared::Cleared,
        Ok(_) => {
            tracing::warn!(
                "The write-ahead log could not be cleared after a delete for good, because another connection was still reading, so it holds the deleted text until it next starts over"
            );
            LogCleared::Busy
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "The write-ahead log could not be cleared after a delete for good, so it holds the deleted text until it next starts over"
            );
            LogCleared::Busy
        }
    }
}

#[cfg(test)]
mod tests;
