//! The Export Directory and the directory each Export or Convert gets in it.
//!
//! Every Export, and every Convert with no output directory chosen, gets a
//! directory of its own in the Export Directory, named for what it is, when
//! it started and the format it writes: `export-2026-10-04-1430-mbox`. It is
//! where the result lands unless the person chose another destination, and it
//! is where an Export keeps its in-between files while it runs: the JSON Lines
//! it pulls from the server before converting them ([`PULLED`]), and the
//! converted files while they are written ([`CONVERTING`]), since a conversion
//! may not write into a directory that holds its input.
//! [`ExportDirectories::finish`] moves the result up and deletes the
//! in-between files, so a finished directory holds only the result. A
//! directory left with nothing in it, because the result went to another
//! destination, is deleted.
//!
//! While a run goes, its directory has a marker beside it,
//! `<name>.running`, which this process holds locked. Finishing or
//! discarding the directory removes the marker. A marker nobody holds is what
//! a run the app did not see to its end left behind (the app quit or
//! crashed), and [`sweep`] deletes that run's directory at
//! the next start, with the copy of the messages it held. A marker another
//! app process holds is left alone.
//!
//! The window names a directory by its path, so every command checks the
//! path is one of these directories: directly in the Export Directory, with
//! a name this module gives. Nothing else is ever moved or deleted here.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The Export Directory's name in the app-data directory. #1053 moves it into
/// the Message Crate Directory.
pub const EXPORT_DIRECTORY_NAME: &str = "exports";

/// The directory inside an Export's directory that holds the JSON Lines it
/// pulled, while it converts them.
pub const PULLED: &str = ".pulled";

/// The directory inside an Export's directory that the conversion writes
/// into, before [`ExportDirectories::finish`] moves its files up.
pub const CONVERTING: &str = ".converting";

/// The ending of the marker beside a directory whose run has not ended.
const RUNNING: &str = ".running";

/// What a directory in the Export Directory is for, the first part of its
/// name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportKind {
    /// An Export from the Message Crate.
    Export,
    /// A Convert in Settings.
    Convert,
}

impl ExportKind {
    /// Every kind, for recognising a name.
    const ALL: [Self; 2] = [Self::Export, Self::Convert];

    /// The first part of the directory's name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Convert => "convert",
        }
    }

    /// Whether `name` is a name this module gives a directory.
    fn is_given_name(name: &str) -> bool {
        Self::ALL.iter().any(|kind| {
            name.strip_prefix(kind.as_str())
                .is_some_and(|rest| rest.starts_with('-'))
        })
    }
}

/// One Export's or Convert's directory, and where its in-between files go.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDir {
    /// The directory, where the result lands unless another destination is
    /// chosen.
    pub dir: String,
    /// Where an Export pulls its JSON Lines to before converting them.
    pub pulled: String,
    /// Where an Export's conversion writes when the result lands in `dir`.
    pub converting: String,
}

/// The Export Directory, in the app-data directory, and the markers this
/// process holds for the runs it has under way.
#[derive(Debug)]
pub struct ExportDirectories {
    /// The Export Directory itself.
    root: PathBuf,
    /// The locked marker of each run under way, by its directory.
    running: Mutex<HashMap<PathBuf, File>>,
}

impl ExportDirectories {
    /// The Export Directory in `app_data_dir`.
    pub fn in_app_data(app_data_dir: &Path) -> Self {
        Self {
            root: app_data_dir.join(EXPORT_DIRECTORY_NAME),
            running: Mutex::new(HashMap::new()),
        }
    }

    /// The Export Directory, which may not exist yet.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Make the directory of a new Export or Convert:
    /// `<kind>-<started>-<format>`, with `-2`, `-3`… added when one started in
    /// the same minute. `chosen` is the destination the person chose, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when `format` is not lowercase letters, digits and
    /// dashes, `chosen` is the Export Directory or holds it (the run works
    /// inside it, and a conversion may not write over its own input), or the
    /// directory cannot be made.
    pub fn create(
        &self,
        kind: ExportKind,
        format: &str,
        started: &str,
        chosen: Option<&str>,
    ) -> Result<ExportDir, String> {
        let valid_format = !format.is_empty()
            && format.len() <= 40
            && format
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid_format {
            return Err(format!("{format:?} cannot name an export's directory"));
        }
        std::fs::create_dir_all(&self.root)
            .map_err(|error| format!("Could not make {}: {error}", self.root.display()))?;
        let root = self.canonical_root()?;
        if let Some(chosen) = chosen.map(str::trim).filter(|chosen| !chosen.is_empty())
            && let Ok(chosen) = Path::new(chosen).canonicalize()
            && root.starts_with(&chosen)
        {
            return Err(format!(
                "{} holds the Export Directory, where the {} works. Choose a directory \
                 inside the Export Directory or elsewhere, or leave it empty.",
                chosen.display(),
                kind.as_str()
            ));
        }
        let base = format!("{}-{started}-{format}", kind.as_str());
        let mut n = 1;
        let (dir, marker) = loop {
            let name = if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            n += 1;
            // The marker comes first, so a directory never exists without one
            // while its run goes.
            let marker_path = root.join(format!("{name}{RUNNING}"));
            let marker = match File::options()
                .write(true)
                .create_new(true)
                .open(&marker_path)
            {
                Ok(marker) => marker,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("Could not make {}: {error}", marker_path.display()));
                }
            };
            marker
                .lock()
                .map_err(|error| format!("Could not lock {}: {error}", marker_path.display()))?;
            // A sweep never removes a marker with no directory beside it, so
            // the marker is still this one.
            let dir = root.join(&name);
            match std::fs::create_dir(&dir) {
                Ok(()) => break (dir, marker),
                Err(error) => {
                    drop(marker);
                    let _ = std::fs::remove_file(&marker_path);
                    if error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(format!("Could not make {}: {error}", dir.display()));
                    }
                }
            }
        };
        self.running_markers().insert(dir.clone(), marker);
        Ok(ExportDir {
            pulled: dir.join(PULLED).display().to_string(),
            converting: dir.join(CONVERTING).display().to_string(),
            dir: dir.display().to_string(),
        })
    }

    /// The markers this process holds, even after a thread panicked while
    /// holding them: the map stays usable.
    fn running_markers(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, File>> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The Export Directory, resolved.
    fn canonical_root(&self) -> Result<PathBuf, String> {
        self.root
            .canonicalize()
            .map_err(|error| format!("Could not resolve {}: {error}", self.root.display()))
    }

    /// `path`, resolved, with the Export Directory resolved, when both are on
    /// disk and `path` is absolute.
    fn resolve(&self, path: &Path) -> Option<(PathBuf, PathBuf)> {
        if !path.is_absolute() {
            return None;
        }
        Some((self.canonical_root().ok()?, path.canonicalize().ok()?))
    }

    /// `dir`, resolved, when it is an Export's or Convert's directory directly
    /// in the Export Directory.
    ///
    /// # Errors
    ///
    /// Returns an error when `dir` is not absolute, is not on disk, or is not
    /// such a directory.
    fn own(&self, dir: &str) -> Result<PathBuf, String> {
        let not_ours = || format!("{} is not an Export's or a Convert's directory", dir.trim());
        let (root, dir) = self.resolve(Path::new(dir.trim())).ok_or_else(not_ours)?;
        let named = dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(ExportKind::is_given_name);
        if dir.parent() != Some(root.as_path()) || !named || !dir.is_dir() {
            return Err(not_ours());
        }
        Ok(dir)
    }

    /// Drop this process's hold on `dir`'s marker, removing the marker first
    /// when `remove` says so. The marker goes before the lock does, so a
    /// sweep that was waiting for the lock finds no marker and leaves the
    /// directory alone. A marker kept but no longer held has the next start's
    /// sweep delete the directory.
    fn release(&self, dir: &Path, remove: bool) {
        let held = self.running_markers().remove(dir);
        if remove {
            let _ = std::fs::remove_file(marker_of(dir));
        }
        drop(held);
    }

    /// Finish the directory `dir` after its Export or Convert succeeded: move
    /// the converted files up out of [`CONVERTING`], delete the JSON Lines it
    /// pulled and the pull's journal, and delete the directory when nothing is
    /// left in it because the result went elsewhere. Returns the directory
    /// when the result is in it.
    ///
    /// # Errors
    ///
    /// Returns an error when `dir` is not an Export's or Convert's directory,
    /// or a file cannot be moved or deleted. The error says where the result
    /// is.
    pub fn finish(&self, dir: &str) -> Result<Option<PathBuf>, String> {
        let dir = self.own(dir)?;
        // The result is written, so nothing sweeps this directory any more,
        // even when what follows fails part-way.
        self.release(&dir, true);
        let converting = dir.join(CONVERTING);
        if converting.exists() {
            let stuck = |error: std::io::Error| {
                format!(
                    "The result is in {}, partly moved into {}: {error}",
                    converting.display(),
                    dir.display()
                )
            };
            for entry in std::fs::read_dir(&converting).map_err(stuck)? {
                let entry = entry.map_err(stuck)?;
                std::fs::rename(entry.path(), dir.join(entry.file_name())).map_err(stuck)?;
            }
            std::fs::remove_dir(&converting).map_err(stuck)?;
        }
        let left_over = |path: &Path, error: std::io::Error| {
            format!(
                "The result is in place, but {} could not be deleted: {error}",
                path.display()
            )
        };
        let pulled = dir.join(PULLED);
        if pulled.exists() {
            std::fs::remove_dir_all(&pulled).map_err(|e| left_over(&pulled, e))?;
        }
        // The journal lets a later pull into the same directory skip what it
        // downloaded. Nothing pulls into this directory again.
        let journal = dir.join(message_crate_pull::PULL_JOURNAL_NAME);
        if journal.exists() {
            std::fs::remove_file(&journal).map_err(|e| left_over(&journal, e))?;
        }
        let empty = std::fs::read_dir(&dir)
            .map_err(|e| left_over(&dir, e))?
            .next()
            .is_none();
        if empty {
            std::fs::remove_dir(&dir).map_err(|e| left_over(&dir, e))?;
            return Ok(None);
        }
        Ok(Some(dir))
    }

    /// Delete the directory `dir` and everything in it, after its Export or
    /// Convert failed or was cancelled. A directory already gone is not an
    /// error.
    ///
    /// # Errors
    ///
    /// Returns an error when `dir` is not an Export's or Convert's directory,
    /// or cannot be deleted.
    pub fn discard(&self, dir: &str) -> Result<(), String> {
        if !Path::new(dir.trim()).exists() {
            return Ok(());
        }
        let dir = self.own(dir)?;
        let removed = std::fs::remove_dir_all(&dir);
        // A directory that could not be deleted keeps its marker, so the next
        // start's sweep deletes it.
        self.release(&dir, removed.is_ok());
        removed.map_err(|error| format!("{}: {error}", dir.display()))
    }

    /// `path`, resolved, when it is the Export Directory or inside it, for
    /// opening.
    pub fn openable(&self, path: &Path) -> Option<PathBuf> {
        let (root, path) = self.resolve(path)?;
        path.starts_with(&root).then_some(path)
    }
}

/// The marker beside the directory `dir`.
fn marker_of(dir: &Path) -> PathBuf {
    let mut marker = dir.as_os_str().to_owned();
    marker.push(RUNNING);
    PathBuf::from(marker)
}

/// Delete the directories in the Export Directory `root` whose marker no
/// process holds: the runs whose app quit or crashed before they ended, with
/// the copy of the messages they may hold. Called once at start-up; a
/// directory another app process is writing is kept, since it holds its
/// marker.
///
/// A marker with no directory beside it belongs to a run being made, and is
/// left alone. A marker its run removed while this sweep waited for the lock
/// is no longer the file at its path, and is left alone too, so a run that
/// just finished keeps its result.
pub fn sweep(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let marker_path = entry.path();
        let Some(name) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(RUNNING))
            .map(str::to_owned)
        else {
            continue;
        };
        if !ExportKind::is_given_name(&name) {
            continue;
        }
        let Ok(marker) = File::options().write(true).open(&marker_path) else {
            continue;
        };
        if marker.try_lock().is_err() || !still_at(&marker, &marker_path) {
            continue;
        }
        let dir = root.join(&name);
        let is_dir = std::fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir());
        if is_dir && std::fs::remove_dir_all(&dir).is_ok() {
            let _ = std::fs::remove_file(&marker_path);
        }
        drop(marker);
    }
}

/// Whether `file` is still the file at `path`.
#[cfg(unix)]
fn still_at(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(open), Ok(now)) => open.dev() == now.dev() && open.ino() == now.ino(),
        _ => false,
    }
}

/// Whether `file` is still the file at `path`. Windows does not remove a
/// file another handle has open until that handle closes, so the path being
/// there is enough.
#[cfg(not(unix))]
fn still_at(_file: &File, path: &Path) -> bool {
    path.exists()
}

/// The local date and time as `YYYY-MM-DD-HHMM`, for an export's directory.
pub fn started_now() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H%M").to_string()
}

#[cfg(test)]
mod tests;
