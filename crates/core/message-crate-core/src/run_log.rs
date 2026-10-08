//! The lines of an Import Run's log, as the desktop app writes them into the
//! Logs Directory and reads them back.
//!
//! Each line is written and read in the format of the server's own log
//! (`docs/architecture/server-log.md`), defined once in
//! `message-crate-log-lines`, so one viewer reads both and the level filter
//! means the same thing in each. A run log uses three of its levels.
//!
//! The first line of a log the desktop app starts says which account ran the
//! run, on which server, as [`RunLogAccount`]: the listing reads it to show
//! an account only its own runs' logs.

use std::fmt;

use message_crate_log_lines::{LogLevel, ParsedLine, format_lines, parse_line, time_now};

/// How severe a line of an Import Run's log is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RunLogLevel {
    /// Something stopped: a stage that failed, a conversation not sent.
    Error,
    /// Something left out that the run went on without: an attachment not
    /// uploaded, a session the server stopped accepting.
    Warn,
    /// What the run did.
    Info,
}

impl From<RunLogLevel> for LogLevel {
    fn from(level: RunLogLevel) -> Self {
        match level {
            RunLogLevel::Error => Self::Error,
            RunLogLevel::Warn => Self::Warn,
            RunLogLevel::Info => Self::Info,
        }
    }
}

impl RunLogLevel {
    /// The run log's level for a line at `level`, or `None` for `DEBUG` and
    /// `TRACE`, which a run log never writes.
    fn from_log_level(level: LogLevel) -> Option<Self> {
        match level {
            LogLevel::Error => Some(Self::Error),
            LogLevel::Warn => Some(Self::Warn),
            LogLevel::Info => Some(Self::Info),
            LogLevel::Debug | LogLevel::Trace => None,
        }
    }
}

impl fmt::Display for RunLogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(LogLevel::from(*self).word())
    }
}

/// `text` at `level` as the lines of a run log, each ending in a line break,
/// stamped with the time now ([`message_crate_log_lines::format_lines`]).
/// Empty when `text` holds nothing but white space, so a blank spacer line is
/// not written.
pub fn format_run_log(level: RunLogLevel, text: &str) -> String {
    format_lines(&time_now(), level.into(), text)
}

/// One line of a run log, read back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RunLogLine {
    /// Where the line starts in the file, in bytes. A larger id is a newer
    /// line, and the lines older than one are read from its id.
    pub id: u64,
    /// When the line was written, in UTC, as RFC 3339 with microseconds.
    pub time: String,
    /// How severe the line is.
    pub level: RunLogLevel,
    /// The line after its time and level.
    pub text: String,
}

/// `raw`, found at byte `offset`, as a line, or `None` when it does not
/// start with an RFC 3339 time and a level.
pub fn parse_run_log_line(offset: u64, raw: &str) -> Option<RunLogLine> {
    RunLogLine::from_parsed(offset, &parse_line(raw)?)
}

impl RunLogLine {
    /// `line`, found at byte `offset`, as a run log's line, or `None` when it
    /// is at a level a run log never writes.
    pub fn from_parsed(offset: u64, line: &ParsedLine<'_>) -> Option<Self> {
        Some(Self {
            id: offset,
            time: line.time.to_string(),
            level: RunLogLevel::from_log_level(line.level)?,
            text: line.text.to_string(),
        })
    }
}

/// Which account ran an Import Run, on which server: the first line of the
/// run's log.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogAccount {
    /// The Import Run's id on that server.
    pub import_run_id: i64,
    /// The account that ran it, by its id on that server.
    pub account_id: i64,
    /// The address of the server the run imported into.
    pub server: String,
}

/// The words before the run's id in the account line.
const ACCOUNT_LINE_START: &str = "Import Run ";

impl RunLogAccount {
    /// The account line's text: `Import Run 42 by account 7 on
    /// http://127.0.0.1:8080`.
    pub fn line_text(&self) -> String {
        format!(
            "{ACCOUNT_LINE_START}{} by account {} on {}",
            self.import_run_id, self.account_id, self.server
        )
    }

    /// The account a line's text names, or `None` when the text is not an
    /// account line.
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_prefix(ACCOUNT_LINE_START)?;
        let (import_run_id, rest) = rest.split_once(" by account ")?;
        let (account_id, server) = rest.split_once(" on ")?;
        Some(Self {
            import_run_id: import_run_id.parse().ok()?,
            account_id: account_id.parse().ok()?,
            server: server.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_reads_back_at_its_level() {
        let text = format_run_log(RunLogLevel::Warn, "Did not upload a.jpg");
        let line = parse_run_log_line(9, text.trim_end()).unwrap();
        assert_eq!(line.id, 9);
        assert_eq!(line.level, RunLogLevel::Warn);
        assert_eq!(line.text, "Did not upload a.jpg");
    }

    #[test]
    fn a_line_at_a_level_a_run_log_never_writes_is_not_a_run_log_line() {
        let debug = format_lines("2026-10-08T12:00:00.123456Z", LogLevel::Debug, "x");
        assert_eq!(parse_run_log_line(0, debug.trim_end()), None);
        assert_eq!(parse_run_log_line(0, "Conversations: 3"), None);
    }

    #[test]
    fn the_account_line_reads_back() {
        let account = RunLogAccount {
            import_run_id: 42,
            account_id: 7,
            server: "http://127.0.0.1:8080".into(),
        };
        let text = account.line_text();
        assert_eq!(text, "Import Run 42 by account 7 on http://127.0.0.1:8080");
        assert_eq!(RunLogAccount::parse(&text), Some(account));
        assert_eq!(RunLogAccount::parse("Import Run 42 completed"), None);
    }
}
