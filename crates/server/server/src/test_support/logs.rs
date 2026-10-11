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
