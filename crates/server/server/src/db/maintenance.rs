//! `ANALYZE`, `VACUUM` and the write-ahead log checkpoint: the statements
//! that keep the database file fast and small. A failure of `ANALYZE` or
//! `VACUUM` after an import is a warning, never returned.

use std::time::Instant;

use anyhow::Context as _;
use sqlx::SqliteConnection;

use crate::progress::Progress;

/// Run each statement, giving a warning instead of failing when one errors.
async fn run_sql_warn(
    conn: &mut SqliteConnection,
    statements: &[&'static str],
    progress: Progress,
) {
    for sql in statements {
        if let Err(err) = sqlx::query(*sql).execute(&mut *conn).await {
            progress.warn(format_args!("{sql} did not complete: {err}"));
        }
    }
}

/// Refresh planner stats on the committed tables promote writes. Errors are
/// warnings; the caller still opens the promote transaction.
pub async fn analyze_import_tables(conn: &mut SqliteConnection, progress: Progress) {
    let started = Instant::now();
    run_sql_warn(
        conn,
        &[
            "ANALYZE messages",
            "ANALYZE attachments",
            "ANALYZE tapbacks",
        ],
        progress,
    )
    .await;
    progress.say(format_args!(
        "ANALYZE of messages, attachments and tapbacks took {:.1} s",
        started.elapsed().as_secs_f64()
    ));
}

/// Reclaim the space the demo import freed: `VACUUM` rewrites the whole
/// file. Errors are warnings; `reset-demo` still succeeds.
pub async fn vacuum_import_tables(conn: &mut SqliteConnection, progress: Progress) {
    let started = Instant::now();
    run_sql_warn(conn, &["VACUUM"], progress).await;
    progress.say(format_args!(
        "VACUUM of the database took {:.1} s",
        started.elapsed().as_secs_f64()
    ));
}

/// Copy every page of the write-ahead log into the database file and
/// truncate the log (`PRAGMA wal_checkpoint(TRUNCATE)`). Returns whether it
/// finished: `false` when a reader or writer kept it from finishing.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn checkpoint_truncate(conn: &mut SqliteConnection) -> anyhow::Result<bool> {
    let (busy, _log, _checkpointed) =
        sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&mut *conn)
            .await
            .context("checkpoint the write-ahead log")?;
    Ok(busy == 0)
}
