//! Listing and reading the Import Run logs in the Logs Directory, for the
//! Logs panel on Owner Home and the run's row in Settings → Storage.
//!
//! Who reads which log (decided on #1665): the owner reads every run log on
//! this computer, whoever ran the import; an account reads the logs of the
//! runs it ran on the Message Crate it is signed in to. A log names the
//! account that ran it, and the Message Crate's id, in its first line
//! ([`RunLogAccount`]); a log that names none, such as one a run left before
//! that line was written, is the owner's alone.
//!
//! This is a filter, not a guard. The window says who is asking
//! ([`Reader`]) and the commands take its word, and a run log is a plain
//! file that any program run by this computer's user can open. It decides
//! which logs each screen offers, not who can read a file.
//!
//! The lines are read the way the server's log is read
//! (`docs/architecture/server-log.md`), with the same reader
//! (`message-crate-log-lines`): newest first, walking back from the id of the
//! last line held a chunk at a time, filtered by level and text. A line's id
//! is its byte offset in the file, which never moves, because a log is only
//! ever appended to. A line still being written, with no line break yet, is
//! left out until it is whole.

use std::fs;
use std::io::{self, Read as _, Seek as _, SeekFrom};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use message_crate_core::{RunLogAccount, RunLogLevel, RunLogLine, parse_run_log_line};
use message_crate_log_lines::{LineFilter, lines_backward, parse_line};
use serde::{Deserialize, Serialize};

/// How many lines at the start of a log are read for its account line.
const ACCOUNT_LINE_SEARCH: usize = 8;

/// The most bytes read for the account line.
const ACCOUNT_LINE_BYTES: u64 = 16 * 1024;

/// The most lines on one page.
const MAX_LIMIT: usize = 500;

/// Who is asking for the logs: the signed-in account, on which Message
/// Crate, and whether it is the owner.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reader {
    /// The id of the Message Crate the window is signed in to, as
    /// `GET /v1/server` answers it.
    pub message_crate_id: String,
    /// The signed-in account.
    pub account_id: i64,
    /// Whether that account is the owner, who reads every log.
    pub owner: bool,
}

impl Reader {
    /// Whether this reader may read a log whose account line is `account`.
    fn reads(&self, account: Option<&RunLogAccount>) -> bool {
        self.owner
            || account.is_some_and(|account| {
                account.account_id == self.account_id && self.signed_in_to(account)
            })
    }

    /// Whether the run `account` names imported into the Message Crate this
    /// reader is signed in to. The address is not compared: the desktop
    /// app's own server and a Docker one both answer at
    /// `http://127.0.0.1:8080`, and one address can be written several ways.
    fn signed_in_to(&self, account: &RunLogAccount) -> bool {
        account.message_crate_id == self.message_crate_id
    }
}

/// One Import Run log on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogEntry {
    /// The file's name in the Logs Directory, such as
    /// `import-iphone-ios-261004-143000.log`. A download is named the same.
    pub name: String,
    /// Who ran the run, on which Message Crate, or `None` when the log does
    /// not say.
    pub account: Option<RunLogAccount>,
    /// Whether the run imported into the Message Crate the reader is signed
    /// in to, so the window need not compare Message Crates itself.
    pub this_message_crate: bool,
    /// Whether the log's first lines have a time and a level. A log written
    /// before its lines carried them has none the viewer can read, though it
    /// still downloads.
    pub has_lines: bool,
    /// The file's size.
    pub bytes: u64,
    /// When the last line was written, in UTC (RFC 3339).
    pub modified_at: String,
}

/// What a page of a run log's lines asks for.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinesQuery {
    /// Only lines at this level or more severe; every line when `None`.
    #[serde(default)]
    pub level: Option<RunLogLevel>,
    /// Only lines whose text holds this, ignoring case.
    #[serde(default)]
    pub text: Option<String>,
    /// Only lines older than the line with this id.
    #[serde(default)]
    pub after: Option<u64>,
    /// The most lines on the page, at most [`MAX_LIMIT`].
    pub limit: usize,
}

/// A page of a run log's lines, newest first, in the shape the server's
/// `GET /v1/server/log-lines` answers, so one viewer reads both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LinesPage {
    /// The lines, newest first.
    pub items: Vec<RunLogLine>,
    /// The most lines this page could hold.
    pub limit: usize,
    /// Whether older lines match too.
    pub has_more: bool,
}

/// The run logs in `logs_dir` that `reader` may read, the most recently
/// written first. A Logs Directory that does not exist yet holds none.
///
/// # Errors
///
/// Returns an error when the directory cannot be read. A log in it that
/// cannot be read is left out.
pub fn list(logs_dir: &Path, reader: &Reader) -> io::Result<Vec<RunLogEntry>> {
    let entries = match fs::read_dir(logs_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut logs = Vec::new();
    // An entry that cannot be read, or a log deleted since the directory was
    // listed, is left out: one bad file does not empty the listing.
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_run_log_name(&name) || !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(Head { account, has_lines }) = head_of(&entry.path()) else {
            continue;
        };
        if !reader.reads(account.as_ref()) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let modified: chrono::DateTime<chrono::Utc> = modified.into();
        logs.push(RunLogEntry {
            name,
            this_message_crate: account
                .as_ref()
                .is_some_and(|account| reader.signed_in_to(account)),
            account,
            has_lines,
            bytes: metadata.len(),
            modified_at: modified.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        });
    }
    logs.sort_by(|a, b| {
        b.modified_at
            .cmp(&a.modified_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    Ok(logs)
}

/// The lines `query` asks for from the log `name` in `logs_dir`, newest
/// first. A line that does not start with a time and a level is left out.
///
/// # Errors
///
/// Returns an error when `name` is not a run log `reader` may read, or the
/// log cannot be read.
pub fn read_lines(
    logs_dir: &Path,
    reader: &Reader,
    name: &str,
    query: &LinesQuery,
) -> Result<LinesPage, String> {
    let path = readable_log(logs_dir, reader, name)?;
    let could_not_read = |error: io::Error| format!("Could not read {name}: {error}");
    let mut file = fs::File::open(&path).map_err(could_not_read)?;
    let len = file.metadata().map_err(could_not_read)?.len();
    let end = query.after.map_or(len, |after| after.min(len));
    let limit = query.limit.clamp(1, MAX_LIMIT);
    let filter = LineFilter::new(query.level.map(Into::into), query.text.as_deref());
    let mut items: Vec<RunLogLine> = Vec::new();
    lines_backward(&mut file, end, |offset, raw| {
        let Some(line) = parse_line(raw)
            .filter(|line| filter.matches(line))
            .and_then(|line| RunLogLine::from_parsed(offset, &line))
        else {
            return ControlFlow::Continue(());
        };
        items.push(line);
        if items.len() > limit {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .map_err(could_not_read)?;
    let has_more = items.len() > limit;
    items.truncate(limit);
    Ok(LinesPage {
        items,
        limit,
        has_more,
    })
}

/// The whole of the log `name` in `logs_dir`, byte for byte as it is on
/// disk, for a download.
///
/// # Errors
///
/// Returns an error when `name` is not a run log `reader` may read, or the
/// log cannot be read.
pub fn read_whole(logs_dir: &Path, reader: &Reader, name: &str) -> Result<Vec<u8>, String> {
    let path = readable_log(logs_dir, reader, name)?;
    fs::read(&path).map_err(|error| format!("Could not read {name}: {error}"))
}

/// The path of the log `name` in `logs_dir`, when it is a run log there that
/// `reader` may read.
fn readable_log(logs_dir: &Path, reader: &Reader, name: &str) -> Result<PathBuf, String> {
    if !is_run_log_name(name) {
        return Err(format!("{name} is not an Import Run log"));
    }
    let path = logs_dir.join(name);
    // Not `is_file`, which follows a symbolic link: a link named like a run
    // log would read whatever it points at.
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
        return Err(format!(
            "No Import Run log named {name} is on this computer"
        ));
    }
    let account = head_of(&path)
        .map_err(|error| format!("Could not read {name}: {error}"))?
        .account;
    if !reader.reads(account.as_ref()) {
        return Err(format!("{name} is the log of another account's Import Run"));
    }
    Ok(path)
}

/// Whether `name` is the name of a run log in the Logs Directory: one file
/// name, `import-….log`, with no directory in it.
fn is_run_log_name(name: &str) -> bool {
    name.starts_with("import-")
        && name.ends_with(".log")
        && !name.contains(['/', '\\'])
        && name != "."
        && name != ".."
}

/// What a log's first and last lines say: the account the first name, if
/// they name one, and whether any of them has a time and a level.
struct Head {
    account: Option<RunLogAccount>,
    has_lines: bool,
}

/// The [`Head`] of the log at `path`. A run started before its lines carried
/// a time and a level, and resumed after, has untimed lines first and timed
/// ones after, so the last lines are read too.
///
/// # Errors
///
/// Returns an error when the log cannot be read.
fn head_of(path: &Path) -> io::Result<Head> {
    let mut file = fs::File::open(path)?;
    let mut head = Vec::new();
    (&mut file)
        .take(ACCOUNT_LINE_BYTES)
        .read_to_end(&mut head)?;
    let head = String::from_utf8_lossy(&head);
    let lines: Vec<_> = head
        .lines()
        .take(ACCOUNT_LINE_SEARCH)
        .filter_map(|raw| parse_run_log_line(0, raw))
        .collect();
    let has_lines = !lines.is_empty() || tail_has_lines(&mut file)?;
    Ok(Head {
        account: lines
            .iter()
            .find_map(|line| RunLogAccount::parse(&line.text)),
        has_lines,
    })
}

/// Whether one of the whole lines in the last [`ACCOUNT_LINE_BYTES`] of
/// `file` has a time and a level.
fn tail_has_lines(file: &mut fs::File) -> io::Result<bool> {
    let len = file.metadata()?.len();
    let start = len.saturating_sub(ACCOUNT_LINE_BYTES);
    let mut tail = Vec::new();
    file.seek(SeekFrom::Start(start))?;
    file.take(ACCOUNT_LINE_BYTES).read_to_end(&mut tail)?;
    // A window that starts inside the file starts inside a line.
    let whole = if start == 0 {
        &tail[..]
    } else {
        tail.iter()
            .position(|b| *b == b'\n')
            .map_or(&[][..], |newline| &tail[newline + 1..])
    };
    Ok(String::from_utf8_lossy(whole)
        .lines()
        .any(|raw| parse_run_log_line(0, raw).is_some()))
}

#[cfg(test)]
mod tests;
