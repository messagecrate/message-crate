//! `ANALYZE` and `VACUUM`: the statements that keep the database file fast
//! and small after an import. A failure is a warning, never
//! returned.

use std::time::Instant;

use sqlx::SqliteConnection;

use crate::progress::Progress;

/// Run each statement, giving a warning instead of failing when one errors.
async fn run_sql_warn(conn: &mut SqliteConnection, statements: &[&str], progress: Progress) {
    for sql in statements {
        if let Err(err) = sqlx::query(sql).execute(&mut *conn).await {
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
