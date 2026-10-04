//! Reading the server's log back: its lines newest first, a page at a time,
//! and the list of its files.
//!
//! A line's id is where the line starts: its file's number times 2³², plus
//! its byte offset in that file. Files are never renamed and only ever grow,
//! so an id names the same line until its file is deleted, and a larger id
//! is a newer line. A page of older lines is read from the id of the last
//! line the client holds, so lines written in between do not move the page.

use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom};
use std::ops::ControlFlow;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::files::{file_numbers, file_path};

/// A line's file number is the part of its id above this many bits.
const OFFSET_BITS: u32 = 32;

/// How many bytes are read at a time, walking a file backwards.
const CHUNK_BYTES: u64 = 64 * 1024;

/// How severe a line is, as `tracing` writes it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema,
)]
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
    fn parse(word: &str) -> Option<Self> {
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
    let needle = query
        .text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_lowercase);
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
            let Some(line) = parse_line(number, offset, raw) else {
                return ControlFlow::Continue(());
            };
            if query.level.is_some_and(|level| line.level > level) {
                return ControlFlow::Continue(());
            }
            if let Some(needle) = &needle
                && !line.text.to_lowercase().contains(needle.as_str())
            {
                return ControlFlow::Continue(());
            }
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

/// `raw`, found at `offset` in file `number`, as a line, or `None` when it
/// does not start with an RFC 3339 time and a level.
fn parse_line(number: u64, offset: u64, raw: &str) -> Option<LogLine> {
    let (time, rest) = raw.split_once(' ')?;
    chrono::DateTime::parse_from_rfc3339(time).ok()?;
    let rest = rest.trim_start();
    let (level, text) = rest.split_once(' ').unwrap_or((rest, ""));
    Some(LogLine {
        id: i64::try_from((number << OFFSET_BITS) | offset).ok()?,
        time: time.to_string(),
        level: LogLevel::parse(level)?,
        text: text.to_string(),
    })
}

/// Call `each` with every whole line that ends before byte `end` of `file`,
/// the last first, with the offset it starts at. Bytes after the last line
/// break before `end` are a line still being written, or the middle of one
/// when `end` is not where a line starts, and are skipped.
fn lines_backward(
    file: &mut File,
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
