//! The line format of Message Crate's logs, defined once for the server,
//! which reads its own log back (`docs/architecture/server-log.md`), and the
//! desktop app, which writes and reads each Import Run's log
//! (`docs/architecture/import-run-logs.md`).
//!
//! A line is its time in UTC (RFC 3339, microseconds), its level padded to
//! five characters, and its text, separated by spaces:
//!
//! ```text
//! 2026-10-08T12:00:00.123456Z  WARN Did not upload a.jpg
//! ```
//!
//! That is what `tracing-subscriber`'s default format writes for the server
//! with ANSI colours off, so the server's log and a run log read the same way:
//! [`parse_line`] reads both, [`LineFilter`] filters both, and
//! [`lines_backward`] walks both from their newest line. The desktop app
//! writes its lines with [`format_lines`].

use std::io::{self, Read, Seek, SeekFrom};
use std::ops::ControlFlow;

/// How many bytes are read at a time, walking a file backwards.
const CHUNK_BYTES: u64 = 64 * 1024;

/// How severe a line is, as `tracing` writes it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Something failed: a `500 Internal Server Error`, or work the server
    /// had to give up.
    Error,
    /// Work the server could not complete but did not fail over.
    Warn,
    /// One line per HTTP response, and the server's own milestones.
    Info,
    /// Detail written only when `RUST_LOG` asks for it.
    Debug,
    /// The finest detail, written only when `RUST_LOG` asks for it.
    Trace,
}

impl LogLevel {
    /// The level as `tracing` writes it at the start of a line.
    pub fn word(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }

    /// The level a line names, or `None` for a word that is not one.
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "ERROR" => Self::Error,
            "WARN" => Self::Warn,
            "INFO" => Self::Info,
            "DEBUG" => Self::Debug,
            "TRACE" => Self::Trace,
            _ => return None,
        })
    }
}

/// The time now, as a line writes it: UTC, RFC 3339, microseconds.
pub fn time_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// `text` at `level` as lines, each stamped with `time` and ending in a line
/// break. Text that holds a line break is written as one line per part, each
/// with the time and the level, so every line is whole on its own. Empty when
/// `text` holds nothing but white space, so a blank spacer line is not
/// written.
pub fn format_lines(time: &str, level: LogLevel, text: &str) -> String {
    let mut out = String::new();
    for part in text
        .lines()
        .map(str::trim_end)
        .filter(|part| !part.trim().is_empty())
    {
        out.push_str(&format!("{time} {:>5} {part}\n", level.word()));
    }
    out
}

/// One line read back, borrowed from the text it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedLine<'a> {
    /// When the line was written, in UTC, as RFC 3339.
    pub time: &'a str,
    /// How severe the line is.
    pub level: LogLevel,
    /// The line after its time and level.
    pub text: &'a str,
}

/// `raw`, one line without its line break, read back, or `None` when it does
/// not start with an RFC 3339 time and a level.
pub fn parse_line(raw: &str) -> Option<ParsedLine<'_>> {
    let (time, rest) = raw.split_once(' ')?;
    chrono::DateTime::parse_from_rfc3339(time).ok()?;
    let rest = rest.trim_start();
    let (level, text) = rest.split_once(' ').unwrap_or((rest, ""));
    Some(ParsedLine {
        time,
        level: LogLevel::parse(level)?,
        text,
    })
}

/// Which lines a reader asks for: those at a level or more severe, and those
/// whose text holds a search, ignoring case.
#[derive(Debug, Clone, Default)]
pub struct LineFilter {
    level: Option<LogLevel>,
    needle: Option<String>,
}

impl LineFilter {
    /// Lines at `level` or more severe (every line when `None`) whose text
    /// holds `text`, ignoring case and the white space around it (every line
    /// when `None` or blank).
    pub fn new(level: Option<LogLevel>, text: Option<&str>) -> Self {
        Self {
            level,
            needle: text
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_lowercase),
        }
    }

    /// Whether `line` is one this filter asks for.
    pub fn matches(&self, line: &ParsedLine<'_>) -> bool {
        if self.level.is_some_and(|level| line.level > level) {
            return false;
        }
        self.needle
            .as_deref()
            .is_none_or(|needle| line.text.to_lowercase().contains(needle))
    }
}

/// Call `each` with every whole line that ends before byte `end` of `file`,
/// the last first, with the offset it starts at and the line without its
/// line break. Bytes after the last line break before `end` are a line still
/// being written, or the middle of one when `end` is not where a line starts,
/// and are skipped. The file is read a chunk at a time from `end`, so a page
/// near the end of a large file reads only that far.
///
/// # Errors
///
/// Returns an error when the file cannot be read.
pub fn lines_backward<F: Read + Seek>(
    file: &mut F,
    end: u64,
    mut each: impl FnMut(u64, &str) -> ControlFlow<()>,
) -> io::Result<()> {
    let mut emit = |offset: u64, bytes: &[u8]| {
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        if bytes.is_empty() {
            return ControlFlow::Continue(());
        }
        each(offset, &String::from_utf8_lossy(bytes))
    };
    // The bytes of the line that runs on past the chunk being read, whose
    // start is in an earlier chunk.
    let mut carry: Vec<u8> = Vec::new();
    let mut whole = false;
    let mut pos = end;
    while pos > 0 {
        let start = pos.saturating_sub(CHUNK_BYTES);
        let mut chunk = vec![0; usize::try_from(pos - start).unwrap_or(0)];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut chunk)?;
        chunk.extend_from_slice(&carry);
        let mut line_end = chunk.len();
        while let Some(newline) = chunk[..line_end].iter().rposition(|b| *b == b'\n') {
            if whole && emit(start + newline as u64 + 1, &chunk[newline + 1..line_end]).is_break() {
                return Ok(());
            }
            whole = true;
            line_end = newline;
        }
        carry = chunk[..line_end].to_vec();
        pos = start;
    }
    if whole {
        let _ = emit(0, &carry);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const TIME: &str = "2026-10-08T12:00:00.123456Z";

    #[test]
    fn a_line_carries_its_time_and_level_and_reads_back() {
        let text = format_lines(TIME, LogLevel::Warn, "Did not upload a.jpg");
        assert_eq!(text, format!("{TIME}  WARN Did not upload a.jpg\n"));
        let line = parse_line(text.trim_end()).unwrap();
        assert_eq!(line.time, TIME);
        assert_eq!(line.level, LogLevel::Warn);
        assert_eq!(line.text, "Did not upload a.jpg");
    }

    #[test]
    fn text_over_several_lines_writes_each_with_its_time_and_level() {
        let text = format_lines(TIME, LogLevel::Info, "Summary\n\n  12 messages\n");
        assert_eq!(
            text,
            format!("{TIME}  INFO Summary\n{TIME}  INFO   12 messages\n")
        );
        assert_eq!(format_lines(TIME, LogLevel::Info, "  "), "");
    }

    #[test]
    fn a_line_with_no_time_or_level_is_not_a_line() {
        assert_eq!(parse_line("Conversations: 3"), None);
        assert_eq!(parse_line(&format!("{TIME} NOTICE x")), None);
    }

    #[test]
    fn the_filter_keeps_lines_at_the_level_or_more_severe_holding_the_search() {
        let line = |level, text| ParsedLine {
            time: TIME,
            level,
            text,
        };
        let warnings = LineFilter::new(Some(LogLevel::Warn), Some("  UPLOAD "));
        assert!(warnings.matches(&line(LogLevel::Error, "could not upload")));
        assert!(!warnings.matches(&line(LogLevel::Info, "upload done")));
        assert!(!warnings.matches(&line(LogLevel::Warn, "skipped")));
        assert!(LineFilter::new(None, Some(" ")).matches(&line(LogLevel::Trace, "x")));
    }

    #[test]
    fn lines_come_back_last_first_across_chunks_without_a_line_cut_short() {
        let long = "x".repeat(usize::try_from(CHUNK_BYTES).unwrap());
        let text = format!("one\n{long}\nthree\nfour, still being writ");
        let mut seen = Vec::new();
        lines_backward(
            &mut Cursor::new(text.as_bytes()),
            text.len() as u64,
            |offset, line| {
                seen.push((offset, line.len()));
                ControlFlow::Continue(())
            },
        )
        .unwrap();
        let three = text.find("three").unwrap() as u64;
        assert_eq!(seen, [(three, 5), (4, long.len()), (0, 3)]);
    }
}
