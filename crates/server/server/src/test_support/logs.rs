//! The server's log, written and read by a test.

use crate::server::AppState;

/// The server's log in `state`'s Data Directory, in files of 200 bytes, so
/// a few lines cross from one file into the next.
pub fn small_log_files(state: &AppState) -> crate::logging::LogFiles {
    crate::logging::LogFiles::open(
        &crate::logging::log_dir(&state.cfg.paths.data_dir),
        crate::logging::LogLimits {
            file_bytes: 200,
            files: 50,
        },
    )
    .unwrap()
}

/// Write line `n` at `level` saying `text` and ending `n=<n>`, as the
/// server's subscriber writes an event.
pub fn write_log_line(files: &crate::logging::LogFiles, n: usize, level: &str, text: &str) {
    let line = log_event(n, level, &format!("{text} n={n}"));
    files.write_event(line.as_bytes()).unwrap();
}

/// An event at second `second` of a fixed minute, at `level`, saying
/// `text`, as `tracing_subscriber`'s `fmt` layer formats one for the
/// server's log: the time, the level padded to five, the text, a line break.
pub fn log_event(second: usize, level: &str, text: &str) -> String {
    format!(
        "2026-10-04T12:00:{:02}.000000Z {level:>5} {text}\n",
        second % 60
    )
}

/// What `run` returns, and the lines it left in a log of its own, oldest
/// first. `run` is awaited with a subscriber at `filter` as this thread's
/// default, so on a test's one-thread runtime the tasks it starts write
/// through it too.
pub async fn lines_written_by<F: Future>(
    filter: &str,
    run: F,
) -> (F::Output, Vec<crate::logging::LogLine>) {
    use crate::logging::{LogFiles, LogLinesQuery, SERVER_LOG_LIMITS, read_lines};

    let dir = tempfile::TempDir::new().unwrap();
    let log_dir = dir.path().join("logs");
    let files = LogFiles::open(&log_dir, SERVER_LOG_LIMITS).unwrap();
    let output = {
        let _subscriber =
            tracing::subscriber::set_default(crate::logging::subscriber_for(files, filter));
        run.await
    };
    let mut lines = read_lines(
        &log_dir,
        &LogLinesQuery {
            limit: 1000,
            ..LogLinesQuery::default()
        },
    )
    .unwrap()
    .0;
    lines.reverse();
    (output, lines)
}
