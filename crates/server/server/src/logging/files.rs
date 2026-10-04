//! The server's log files: `server-<n>.log` in the `logs` directory, `<n>`
//! counting up from 1. The newest takes every line; when a line would carry
//! it past [`LogLimits::file_bytes`], the next number starts, and the oldest
//! files past [`LogLimits::files`] are deleted. A file is never renamed, so
//! a file's number names it for as long as it exists, which is what lets a
//! line's id (`read.rs`) stay valid while the log grows.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// How large the server's log files grow and how many are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogLimits {
    /// A file is closed and the next one started before a line would carry
    /// it past this many bytes. One line longer than this is a file of its
    /// own.
    pub file_bytes: u64,
    /// The most files kept, the newest among them. The oldest is deleted
    /// when one more starts.
    pub files: usize,
}

/// The server's log limits: at most 5 files of 50 MB (50,000,000 bytes),
/// 250 MB in all (`docs/architecture/server-log.md`, "Trimmed by size only").
pub const SERVER_LOG_LIMITS: LogLimits = LogLimits {
    file_bytes: 50_000_000,
    files: 5,
};

/// The server's log files, open for writing. Cloning shares the files.
#[derive(Clone)]
pub struct LogFiles {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    limits: LogLimits,
    newest: Mutex<Newest>,
}

/// The file every line goes to.
struct Newest {
    number: u64,
    file: File,
    bytes: u64,
    /// Its last line was cut short, so the next line starts a new file
    /// rather than run on from the middle of it.
    cut_short: bool,
}

impl LogFiles {
    /// Open the log in `dir`, making the directory when it is missing. Lines
    /// go on the end of the newest file while it has room, and to a new one
    /// when it has none. Files past `limits.files` are deleted now, so a
    /// lowered limit holds from the start.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be made or read, or the
    /// newest file cannot be opened.
    pub fn open(dir: &Path, limits: LogLimits) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let number = file_numbers(dir)?.last().copied().unwrap_or(1);
        let path = file_path(dir, number);
        let cut_short = ends_mid_line(&path)?;
        let file = open_append(&path)?;
        let bytes = file.metadata()?.len();
        let files = Self {
            inner: Arc::new(Inner {
                dir: dir.to_path_buf(),
                limits,
                newest: Mutex::new(Newest {
                    number,
                    file,
                    bytes,
                    cut_short,
                }),
            }),
        };
        files.trim()?;
        Ok(files)
    }

    /// The directory the files are in.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// Write one formatted event as one line. A line break inside it, from a
    /// message or an error chain that holds one, is written as the two
    /// characters `\n`, so every line of the file is one event and starts
    /// with its time and level.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written or the next one
    /// cannot be started.
    pub fn write_event(&self, event: &[u8]) -> io::Result<()> {
        let line = one_line(event);
        if line.len() <= 1 {
            return Ok(());
        }
        let mut newest = self
            .inner
            .newest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let len = line.len() as u64;
        if newest.cut_short
            || (newest.bytes > 0 && newest.bytes + len > self.inner.limits.file_bytes)
        {
            let number = newest.number + 1;
            newest.file = open_append(&file_path(&self.inner.dir, number))?;
            newest.number = number;
            newest.bytes = 0;
            newest.cut_short = false;
            self.trim()?;
        }
        if let Err(error) = newest.file.write_all(&line) {
            // Part of the line may be on disk, so the file no longer ends
            // where a line does.
            newest.cut_short = true;
            return Err(error);
        }
        newest.bytes += len;
        Ok(())
    }

    /// Delete the oldest files past the limit.
    fn trim(&self) -> io::Result<()> {
        let numbers = file_numbers(&self.inner.dir)?;
        let excess = numbers.len().saturating_sub(self.inner.limits.files);
        for number in &numbers[..excess] {
            match fs::remove_file(file_path(&self.inner.dir, *number)) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        Ok(())
    }
}

/// The path of file `number` in `dir`.
pub(crate) fn file_path(dir: &Path, number: u64) -> PathBuf {
    dir.join(file_name(number))
}

/// The name of file `number`: `server-000001.log`. The zeros keep a
/// directory listing in order up to a million files.
pub(crate) fn file_name(number: u64) -> String {
    format!("server-{number:06}.log")
}

/// The numbers of the log files in `dir`, oldest first. Anything else in
/// the directory is left out. A missing directory has none.
pub(crate) fn file_numbers(dir: &Path) -> io::Result<Vec<u64>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut numbers = Vec::new();
    for entry in entries {
        let name = entry?.file_name();
        if let Some(number) = name
            .to_str()
            .and_then(|name| name.strip_prefix("server-"))
            .and_then(|name| name.strip_suffix(".log"))
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse::<u64>().ok())
            .filter(|number| *number > 0)
        {
            numbers.push(number);
        }
    }
    numbers.sort_unstable();
    Ok(numbers)
}

/// Whether the file at `path` holds bytes after its last line break: a line
/// cut short by a full disk or a server that stopped mid-write. A missing or
/// empty file does not.
fn ends_mid_line(path: &Path) -> io::Result<bool> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// `event` as one line ending in `\n`: trailing line breaks dropped, and each
/// line break inside written as `\n`.
fn one_line(event: &[u8]) -> Vec<u8> {
    let trimmed = event
        .iter()
        .rposition(|b| *b != b'\n' && *b != b'\r')
        .map_or(&event[..0], |last| &event[..=last]);
    let mut line = Vec::with_capacity(trimmed.len() + 1);
    let mut bytes = trimmed.iter().peekable();
    while let Some(&b) = bytes.next() {
        match b {
            b'\r' if bytes.peek() == Some(&&b'\n') => {}
            b'\n' | b'\r' => line.extend_from_slice(b"\\n"),
            _ => line.push(b),
        }
    }
    line.push(b'\n');
    line
}
