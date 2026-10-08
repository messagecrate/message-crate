//! The lines of an Import Run's log, as the desktop app writes them into the
//! Logs Directory and reads them back.
//!
//! Each line starts with its time in UTC (RFC 3339, microseconds) and its
//! level, then the text, the way the server's own log does
//! (`docs/architecture/server-log.md`), so one viewer reads both and the
//! level filter means the same thing in each. Text that holds a line break is
//! written as one line per part, each with the time and the level, so every
//! line in the file is whole on its own.
//!
//! The first line of a log the desktop app starts says which account ran the
//! run, on which server, as [`RunLogAccount`]: the listing reads it to show
//! an account only its own runs' logs.

use std::fmt;

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

impl RunLogLevel {
    /// The level as the line writes it, the way `tracing` names it.
    fn word(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
        }
    }

    /// The level a line names, or `None` for a word that is not one.
    fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "ERROR" => Self::Error,
            "WARN" => Self::Warn,
            "INFO" => Self::Info,
            _ => return None,
        })
    }
}

impl fmt::Display for RunLogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// `text` at `level` as the lines of a run log, each ending in a line break,
/// stamped with the time now. Empty when `text` holds nothing but white
/// space, so a blank spacer line is not written.
pub fn format_run_log(level: RunLogLevel, text: &str) -> String {
    let time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    format_run_log_at(&time, level, text)
}

/// [`format_run_log`] at a fixed `time`, for the tests.
fn format_run_log_at(time: &str, level: RunLogLevel, text: &str) -> String {
    let mut out = String::new();
    for part in text
        .lines()
        .map(str::trim_end)
        .filter(|p| !p.trim().is_empty())
    {
        out.push_str(&format!("{time} {:>5} {part}\n", level.word()));
    }
    out
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
    let (time, rest) = raw.split_once(' ')?;
    chrono::DateTime::parse_from_rfc3339(time).ok()?;
    let rest = rest.trim_start();
    let (level, text) = rest.split_once(' ').unwrap_or((rest, ""));
    Some(RunLogLine {
        id: offset,
        time: time.to_string(),
        level: RunLogLevel::parse(level)?,
        text: text.to_string(),
    })
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

    const TIME: &str = "2026-10-08T12:00:00.123456Z";

    #[test]
    fn a_line_carries_its_time_and_level_and_reads_back() {
        let text = format_run_log_at(TIME, RunLogLevel::Warn, "Did not upload a.jpg");
        assert_eq!(text, format!("{TIME}  WARN Did not upload a.jpg\n"));
        let line = parse_run_log_line(9, text.trim_end()).unwrap();
        assert_eq!(line.id, 9);
        assert_eq!(line.time, TIME);
        assert_eq!(line.level, RunLogLevel::Warn);
        assert_eq!(line.text, "Did not upload a.jpg");
    }

    #[test]
    fn text_over_several_lines_writes_each_with_its_time_and_level() {
        let text = format_run_log_at(TIME, RunLogLevel::Info, "Summary\n\n  12 messages\n");
        assert_eq!(
            text,
            format!("{TIME}  INFO Summary\n{TIME}  INFO   12 messages\n")
        );
        assert_eq!(format_run_log_at(TIME, RunLogLevel::Info, "  "), "");
    }

    #[test]
    fn a_line_with_no_time_or_level_is_not_a_line() {
        assert_eq!(parse_run_log_line(0, "Conversations: 3"), None);
        assert_eq!(parse_run_log_line(0, &format!("{TIME} DEBUG x")), None);
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
