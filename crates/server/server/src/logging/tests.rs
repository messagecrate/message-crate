//! The server's log files: rotation and trimming, one event per line, and
//! reading the lines back newest first.

use std::path::Path;

use tempfile::TempDir;

use super::files::{file_name, file_numbers};
use super::{LogFiles, LogLevel, LogLimits, LogLinesQuery, SERVER_LOG_LIMITS, read_lines};

/// A line as `tracing_subscriber`'s `fmt` layer writes it, `n` naming it.
fn event(level: &str, n: usize) -> String {
    format!(
        "2026-10-04T12:00:{:02}.000000Z {level:>5} line {n}\n",
        n % 60
    )
}

/// The size of each file in `dir`, oldest first.
fn sizes(dir: &Path) -> Vec<(u64, u64)> {
    file_numbers(dir)
        .unwrap()
        .into_iter()
        .map(|n| {
            let len = std::fs::metadata(dir.join(file_name(n))).unwrap().len();
            (n, len)
        })
        .collect()
}

/// Writing past 50 MB (50,000,000 bytes) starts a new file, and the first
/// stays at or under 50 MB: no line is split between two files.
#[test]
fn writing_past_fifty_megabytes_starts_a_new_file() {
    let tmp = TempDir::new().unwrap();
    let files = LogFiles::open(tmp.path(), SERVER_LOG_LIMITS).unwrap();
    // A line of 1,000,000 bytes, 50 of which fill the first file exactly.
    let mut line = event("INFO", 0).into_bytes();
    line.pop();
    line.resize(1_000_000 - 1, b'x');
    line.push(b'\n');
    for _ in 0..50 {
        files.write_event(&line).unwrap();
    }
    assert_eq!(sizes(tmp.path()), [(1, 50_000_000)]);

    files.write_event(event("INFO", 1).as_bytes()).unwrap();

    let after = sizes(tmp.path());
    assert_eq!(after.len(), 2, "{after:?}");
    assert_eq!(after[0], (1, 50_000_000));
    assert_eq!(after[1].0, 2);
}

/// However much is written, no more than the limit's files are kept: the
/// oldest is deleted when a new one starts, and the numbers keep counting.
#[test]
fn rotation_never_keeps_more_than_the_limits_files() {
    let tmp = TempDir::new().unwrap();
    // The server's own count of files, in files small enough to fill fast.
    let limits = LogLimits {
        file_bytes: 200,
        files: SERVER_LOG_LIMITS.files,
    };
    let files = LogFiles::open(tmp.path(), limits).unwrap();
    for n in 0..400 {
        files.write_event(event("INFO", n).as_bytes()).unwrap();
        let kept = file_numbers(tmp.path()).unwrap();
        assert!(kept.len() <= 5, "after line {n}: {kept:?}");
    }
    let kept = sizes(tmp.path());
    assert_eq!(kept.len(), 5);
    assert!(kept.iter().all(|(_, len)| *len <= 200), "{kept:?}");
    let numbers: Vec<u64> = kept.iter().map(|(n, _)| *n).collect();
    let newest = numbers[4];
    assert!(newest > 5, "rotated past the first five: {numbers:?}");
    assert_eq!(numbers, (newest - 4..=newest).collect::<Vec<_>>());
}

/// A restarted server writes on the end of the newest file, and a log with
/// more files than the limit is trimmed when it is opened.
#[test]
fn opening_appends_to_the_newest_file_and_trims_to_the_limit() {
    let tmp = TempDir::new().unwrap();
    for n in 1..=7 {
        std::fs::write(tmp.path().join(file_name(n)), event("INFO", 0)).unwrap();
    }
    std::fs::write(tmp.path().join("notes.txt"), "not a log file").unwrap();

    let files = LogFiles::open(tmp.path(), SERVER_LOG_LIMITS).unwrap();
    files.write_event(event("WARN", 1).as_bytes()).unwrap();

    assert_eq!(file_numbers(tmp.path()).unwrap(), [3, 4, 5, 6, 7]);
    let newest = std::fs::read_to_string(tmp.path().join(file_name(7))).unwrap();
    assert_eq!(newest, format!("{}{}", event("INFO", 0), event("WARN", 1)));
    assert!(tmp.path().join("notes.txt").exists());
}

/// An event whose message holds a line break is still one line, so every
/// line of a file starts with its time and level.
#[test]
fn a_line_break_inside_an_event_is_written_as_backslash_n() {
    let tmp = TempDir::new().unwrap();
    let files = LogFiles::open(tmp.path(), SERVER_LOG_LIMITS).unwrap();

    files
        .write_event(b"2026-10-04T12:00:00.000000Z ERROR failed: one\ntwo\r\nthree\n")
        .unwrap();

    let text = std::fs::read_to_string(tmp.path().join(file_name(1))).unwrap();
    assert_eq!(
        text,
        "2026-10-04T12:00:00.000000Z ERROR failed: one\\ntwo\\nthree\n"
    );
}

/// What the subscriber writes is what the reader reads back: the level and
/// the text of the event, newest first.
#[test]
fn the_subscriber_writes_lines_the_reader_reads_back() {
    let tmp = TempDir::new().unwrap();
    let files = LogFiles::open(tmp.path(), SERVER_LOG_LIMITS).unwrap();
    tracing::subscriber::with_default(super::subscriber_for(files, "info"), || {
        tracing::warn!(import_id = 7, "the import could not finish");
        tracing::info!("listening");
        tracing::debug!("left out at info");
    });

    let (lines, more) = read_lines(tmp.path(), &query(10)).unwrap();

    assert!(!more);
    let read: Vec<(LogLevel, &str)> = lines.iter().map(|l| (l.level, l.text.as_str())).collect();
    assert_eq!(
        read,
        [
            (LogLevel::Info, "listening"),
            (LogLevel::Warn, "the import could not finish import_id=7"),
        ]
    );
    assert!(
        chrono::DateTime::parse_from_rfc3339(&lines[0].time).is_ok(),
        "{}",
        lines[0].time
    );
}

fn query(limit: usize) -> LogLinesQuery {
    LogLinesQuery {
        limit,
        ..LogLinesQuery::default()
    }
}

/// A file whose last line was cut short, by a full disk or a server that
/// stopped mid-write, is not written on: the next event starts a new file,
/// so it is a line of its own and is read back.
#[test]
fn a_file_cut_short_mid_line_is_not_written_on() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join(file_name(1)),
        format!("{}2026-10-04T12:00:09.000000Z  INFO half", event("INFO", 0)),
    )
    .unwrap();

    let files = LogFiles::open(tmp.path(), SERVER_LOG_LIMITS).unwrap();
    files.write_event(event("WARN", 1).as_bytes()).unwrap();

    let (lines, _) = read_lines(tmp.path(), &query(10)).unwrap();
    assert_eq!(numbers(&lines), [1, 0]);
    assert_eq!(file_numbers(tmp.path()).unwrap(), [1, 2]);
}

/// Lines `0..count` written across files of 100 bytes, so a page crosses
/// from one file into the next.
fn written(count: usize, level: impl Fn(usize) -> &'static str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let files = LogFiles::open(
        tmp.path(),
        LogLimits {
            file_bytes: 100,
            files: 50,
        },
    )
    .unwrap();
    for n in 0..count {
        files.write_event(event(level(n), n).as_bytes()).unwrap();
    }
    tmp
}

/// The numbers the lines' texts carry, in the order they were read.
fn numbers(lines: &[super::LogLine]) -> Vec<usize> {
    lines
        .iter()
        .map(|l| l.text.strip_prefix("line ").unwrap().parse().unwrap())
        .collect()
}

/// Newest first, a page at a time: each page starts after the last line of
/// the one before, across files, until no older line is left.
#[test]
fn pages_run_newest_first_across_files_from_the_last_lines_id() {
    let tmp = written(10, |_| "INFO");
    assert!(file_numbers(tmp.path()).unwrap().len() > 3);

    let (first, more) = read_lines(tmp.path(), &query(4)).unwrap();
    assert_eq!(numbers(&first), [9, 8, 7, 6]);
    assert!(more);
    let (second, more) = read_lines(
        tmp.path(),
        &LogLinesQuery {
            after: Some(first[3].id),
            ..query(4)
        },
    )
    .unwrap();
    assert_eq!(numbers(&second), [5, 4, 3, 2]);
    assert!(more);
    let (last, more) = read_lines(
        tmp.path(),
        &LogLinesQuery {
            after: Some(second[3].id),
            ..query(4)
        },
    )
    .unwrap();
    assert_eq!(numbers(&last), [1, 0]);
    assert!(!more);
    let ids: Vec<i64> = first
        .iter()
        .chain(&second)
        .chain(&last)
        .map(|l| l.id)
        .collect();
    assert!(ids.windows(2).all(|w| w[0] > w[1]), "{ids:?}");
}

/// Lines written after the first page do not move the next one: the cursor
/// names a line, not a count from the newest.
#[test]
fn lines_written_between_pages_do_not_move_the_next_page() {
    let tmp = TempDir::new().unwrap();
    let files = LogFiles::open(tmp.path(), SERVER_LOG_LIMITS).unwrap();
    for n in 0..6 {
        files.write_event(event("INFO", n).as_bytes()).unwrap();
    }
    let (first, _) = read_lines(tmp.path(), &query(3)).unwrap();
    for n in 6..9 {
        files.write_event(event("INFO", n).as_bytes()).unwrap();
    }

    let (second, more) = read_lines(
        tmp.path(),
        &LogLinesQuery {
            after: Some(first[2].id),
            ..query(3)
        },
    )
    .unwrap();

    assert_eq!(numbers(&second), [2, 1, 0]);
    assert!(!more);
}

/// A level asks for that level and every more severe one.
#[test]
fn a_level_keeps_that_level_and_the_more_severe() {
    let levels = ["ERROR", "WARN", "INFO", "DEBUG"];
    let tmp = written(8, |n| levels[n % 4]);

    let read = |level| {
        let (lines, _) = read_lines(
            tmp.path(),
            &LogLinesQuery {
                level: Some(level),
                ..query(100)
            },
        )
        .unwrap();
        numbers(&lines)
    };

    assert_eq!(read(LogLevel::Error), [4, 0]);
    assert_eq!(read(LogLevel::Warn), [5, 4, 1, 0]);
    assert_eq!(read(LogLevel::Trace), [7, 6, 5, 4, 3, 2, 1, 0]);
}

/// The text filter matches anywhere in a line's text, ignoring case, but not
/// its time or level, and a page of matches reads past lines that do not
/// match.
#[test]
fn text_matches_anywhere_in_a_lines_text_ignoring_case() {
    let tmp = TempDir::new().unwrap();
    let files = LogFiles::open(tmp.path(), SERVER_LOG_LIMITS).unwrap();
    for (n, text) in ["Import finished", "request done", "import started", "other"]
        .iter()
        .enumerate()
    {
        let line = format!("2026-10-04T12:00:0{n}.000000Z  INFO {text}\n");
        files.write_event(line.as_bytes()).unwrap();
    }

    let (lines, more) = read_lines(
        tmp.path(),
        &LogLinesQuery {
            text: Some("IMPORT".to_string()),
            ..query(1)
        },
    )
    .unwrap();
    assert_eq!(lines[0].text, "import started");
    assert!(more);
    let (lines, more) = read_lines(
        tmp.path(),
        &LogLinesQuery {
            text: Some("IMPORT".to_string()),
            after: Some(lines[0].id),
            ..query(1)
        },
    )
    .unwrap();
    assert_eq!(lines[0].text, "Import finished");
    assert!(!more);

    // The time and the level are not text: `info` is not in any of these
    // lines' text, though every line is at `INFO`.
    for text in ["info", "2026", ":"] {
        let (lines, _) = read_lines(
            tmp.path(),
            &LogLinesQuery {
                text: Some(text.to_string()),
                ..query(10)
            },
        )
        .unwrap();
        assert!(lines.is_empty(), "{text}: {lines:?}");
    }
}

/// A line still being written, with no line break yet, is not read; nor is
/// a line that does not start with a time and a level.
#[test]
fn an_unfinished_line_and_a_line_with_no_time_are_left_out() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join(file_name(1)),
        format!(
            "not a log line\n{}{}2026-10-04T12:00:09.000000Z  INFO half",
            event("INFO", 0),
            event("WARN", 1)
        ),
    )
    .unwrap();

    let (lines, more) = read_lines(tmp.path(), &query(10)).unwrap();

    assert_eq!(numbers(&lines), [1, 0]);
    assert!(!more);
}

/// A log longer than one read of the file is read whole, and a cursor in
/// the middle of a line starts at the whole line before it.
#[test]
fn a_file_longer_than_one_read_is_read_whole() {
    let tmp = TempDir::new().unwrap();
    let files = LogFiles::open(tmp.path(), SERVER_LOG_LIMITS).unwrap();
    for n in 0..5000 {
        files.write_event(event("INFO", n).as_bytes()).unwrap();
    }

    let (lines, more) = read_lines(tmp.path(), &query(500)).unwrap();
    assert_eq!(numbers(&lines), (4500..5000).rev().collect::<Vec<_>>());
    assert!(more);
    let mut seen = 0;
    let mut after = None;
    loop {
        let (page, more) = read_lines(
            tmp.path(),
            &LogLinesQuery {
                after,
                ..query(500)
            },
        )
        .unwrap();
        seen += page.len();
        after = page.last().map(|l| l.id);
        if !more {
            break;
        }
    }
    assert_eq!(seen, 5000);

    let (lines, _) = read_lines(
        tmp.path(),
        &LogLinesQuery {
            after: Some(lines[0].id + 3),
            ..query(1)
        },
    )
    .unwrap();
    assert_eq!(numbers(&lines), [4998]);
}

/// A log that does not exist yet has no lines and no files.
#[test]
fn a_missing_log_has_no_lines() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("logs");

    assert_eq!(
        read_lines(&missing, &query(10)).unwrap(),
        (Vec::new(), false)
    );
    assert!(super::list_files(&missing).unwrap().is_empty());
}
