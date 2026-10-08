//! Reading the server's log back: its lines newest first, a page at a time,
//! and the list of its files.
//!
//! A line's id is where the line starts: its file's number times 2³², plus
//! its byte offset in that file. Files are never renamed and only ever grow,
//! so an id names the same line until its file is deleted, and a larger id
//! is a newer line. A page of older lines is read from the id of the last
//! line the client holds, so lines written in between do not move the page.

use std::fs::File;
use std::io;
use std::ops::ControlFlow;
use std::path::Path;

pub use message_crate_log_lines::LogLevel;
use message_crate_log_lines::{LineFilter, ParsedLine, lines_backward, parse_line};
use serde::{Deserialize, Serialize};

use super::files::{file_numbers, file_path};

/// A line's file number is the part of its id above this many bits.
const OFFSET_BITS: u32 = 32;

/// One line of the server's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LogLine {
    /// Where the line is in the log. A larger id is a newer line. Send it as
    /// `after` to read the lines older than this one.
    pub id: i64,
    /// When the line was written, in UTC, as RFC 3339 with microseconds.
    pub time: String,
    /// How severe the line is.
    pub level: LogLevel,
    /// The line after its time and level: the request or work it belongs to,
    /// and what it says. A line break inside it is written as `\n`.
    pub text: String,
}

/// One file of the server's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LogFile {
    /// The file's number. A larger number is a newer file, and the largest is
    /// the one the server is writing to.
    pub id: i64,
    /// The file's name, as a download is named: `server-000001.log`.
    pub name: String,
    /// The file's size when it was read.
    pub bytes: u64,
    /// When the last line was written to it, in UTC (RFC 3339).
    pub modified_at: String,
}

/// What a page of lines asks for.
#[derive(Debug, Clone, Default)]
pub struct LogLinesQuery {
    /// Only lines older than the line with this id; the newest lines when
    /// `None`.
    pub after: Option<i64>,
    /// Only lines at this level or more severe.
    pub level: Option<LogLevel>,
    /// Only lines whose text, after the time and the level, holds this,
    /// ignoring case.
    pub text: Option<String>,
    /// The most lines on the page.
    pub limit: usize,
}

/// The lines `query` asks for from the log in `dir`, newest first, and
/// whether older lines match too. A line that does not start with a time and
/// a level is left out. Only a write cut short leaves one, and the
/// downloaded file still has it.
///
/// # Errors
///
/// Returns an error when the directory or a file in it cannot be read.
pub fn read_lines(dir: &Path, query: &LogLinesQuery) -> io::Result<(Vec<LogLine>, bool)> {
    let filter = LineFilter::new(query.level, query.text.as_deref());
    let start = query.after.map(|id| {
        let id = id.max(0).cast_unsigned();
        (id >> OFFSET_BITS, id & ((1 << OFFSET_BITS) - 1))
    });
    let mut items: Vec<LogLine> = Vec::new();
    for number in file_numbers(dir)?.into_iter().rev() {
        if start.is_some_and(|(file, _)| number > file) {
            continue;
        }
        let mut file = match File::open(file_path(dir, number)) {
            Ok(file) => file,
            // Deleted by the writer since the directory was listed: older
            // still, and gone.
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        };
        let len = file.metadata()?.len();
        let end = match start {
            Some((file, offset)) if file == number => offset.min(len),
            _ => len,
        };
        lines_backward(&mut file, end, |offset, raw| {
            let Some(parsed) = parse_line(raw).filter(|line| filter.matches(line)) else {
                return ControlFlow::Continue(());
            };
            let Some(line) = log_line(number, offset, &parsed) else {
                return ControlFlow::Continue(());
            };
            items.push(line);
            if items.len() > query.limit {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })?;
        if items.len() > query.limit {
            break;
        }
    }
    let more = items.len() > query.limit;
    items.truncate(query.limit);
    Ok((items, more))
}

/// The log files in `dir`, newest first.
///
/// # Errors
///
/// Returns an error when the directory cannot be read.
pub fn list_files(dir: &Path) -> io::Result<Vec<LogFile>> {
    let mut files = Vec::new();
    for number in file_numbers(dir)?.into_iter().rev() {
        let metadata = match std::fs::metadata(file_path(dir, number)) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let modified: chrono::DateTime<chrono::Utc> = metadata.modified()?.into();
        files.push(LogFile {
            id: i64::try_from(number).unwrap_or(i64::MAX),
            name: super::files::file_name(number),
            bytes: metadata.len(),
            modified_at: modified.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        });
    }
    Ok(files)
}

/// `parsed`, found at `offset` in file `number`, as the line the API
/// answers, or `None` when the id does not fit.
fn log_line(number: u64, offset: u64, parsed: &ParsedLine<'_>) -> Option<LogLine> {
    Some(LogLine {
        id: i64::try_from((number << OFFSET_BITS) | offset).ok()?,
        time: parsed.time.to_string(),
        level: parsed.level,
        text: parsed.text.to_string(),
    })
}
