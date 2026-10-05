//! The Export Directory and the directory each Export or Convert gets in it.
//!
//! Every Export and every Convert gets a directory of its own in the Export
//! Directory, named for what it is, when it started and the format it writes:
//! `export-2026-10-04-1430-mbox`. It is where the result lands unless the
//! person chose another destination, and it is where an Export keeps its
//! in-between files while it runs: the JSON Lines it pulls from the server
//! before converting them ([`PULLED`]), and the converted files while they are
//! written ([`CONVERTING`]), since a conversion may not write into a
//! directory that holds its input. [`ExportDirectories::finish`] deletes the
//! in-between files and moves the result up, so a finished directory holds
//! only the result. A directory left with nothing in it, because the result
//! went to another destination, is deleted.
//!
//! The window names a directory by its path, so every command checks the
//! path is one of these directories: directly in the Export Directory, with
//! a name this module gives. Nothing else is ever moved or deleted here.

use std::path::{Path, PathBuf};

/// The Export Directory's name in the app-data directory. #1053 moves it into
/// the Message Crate Directory.
pub const EXPORT_DIRECTORY_NAME: &str = "exports";

/// The directory inside an Export's directory that holds the JSON Lines it
/// pulled, while it converts them.
pub const PULLED: &str = ".pulled";

/// The directory inside an Export's directory that the conversion writes
/// into, before [`ExportDirectories::finish`] moves its files up.
pub const CONVERTING: &str = ".converting";

/// What an Export or Convert's directory is for, the first part of its name.
const KINDS: [&str; 2] = ["export", "convert"];

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

/// The Export Directory, in the app-data directory.
#[derive(Debug)]
pub struct ExportDirectories {
    /// The Export Directory itself.
    root: PathBuf,
}

impl ExportDirectories {
    /// The Export Directory in `app_data_dir`.
    pub fn in_app_data(app_data_dir: &Path) -> Self {
        Self {
            root: app_data_dir.join(EXPORT_DIRECTORY_NAME),
        }
    }

    /// The Export Directory, which may not exist yet.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Make the directory of a new Export or Convert: `<kind>-<started>-<format>`,
    /// with `-2`, `-3`… added when one started in the same minute.
    ///
    /// # Errors
    ///
    /// Returns an error when `kind` is not `export` or `convert`, `format` is
    /// not lowercase letters, digits and dashes, or the directory cannot be
    /// made.
    pub fn create(&self, kind: &str, format: &str, started: &str) -> Result<ExportDir, String> {
        if !KINDS.contains(&kind) {
            return Err(format!("{kind:?} is not an Export or a Convert"));
        }
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
        let base = format!("{kind}-{started}-{format}");
        let mut dir = self.root.join(&base);
        let mut n = 1;
        loop {
            match std::fs::create_dir(&dir) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    n += 1;
                    dir = self.root.join(format!("{base}-{n}"));
                }
                Err(error) => return Err(format!("Could not make {}: {error}", dir.display())),
            }
        }
        let dir = dir
            .canonicalize()
            .map_err(|error| format!("Could not resolve {}: {error}", dir.display()))?;
        Ok(ExportDir {
            pulled: dir.join(PULLED).display().to_string(),
            converting: dir.join(CONVERTING).display().to_string(),
            dir: dir.display().to_string(),
        })
    }

    /// `dir`, resolved, when it is an Export's or Convert's directory directly
    /// in the Export Directory.
    ///
    /// # Errors
    ///
    /// Returns an error when `dir` is not absolute, is not on disk, or is not
    /// such a directory.
    fn own(&self, dir: &str) -> Result<PathBuf, String> {
        let path = PathBuf::from(dir.trim());
        if !path.is_absolute() {
            return Err("Path must be absolute".to_string());
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("Could not find {}: {error}", path.display()))?;
        let root = self
            .root
            .canonicalize()
            .map_err(|error| format!("Could not find {}: {error}", self.root.display()))?;
        let named = canonical
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                KINDS
                    .iter()
                    .any(|kind| name.starts_with(&format!("{kind}-")))
            });
        if canonical.parent() != Some(root.as_path()) || !named || !canonical.is_dir() {
            return Err(format!(
                "{} is not an Export's or a Convert's directory",
                canonical.display()
            ));
        }
        Ok(canonical)
    }

    /// Finish the directory `dir` after its Export or Convert succeeded:
    /// delete the JSON Lines it pulled and the pull's journal, move the
    /// converted files up out of [`CONVERTING`], and delete the directory
    /// when nothing is left in it because the result went elsewhere. Returns
    /// the directory when the result is in it.
    ///
    /// # Errors
    ///
    /// Returns an error when `dir` is not an Export's or Convert's directory,
    /// or a file cannot be deleted or moved.
    pub fn finish(&self, dir: &str) -> Result<Option<PathBuf>, String> {
        let dir = self.own(dir)?;
        let fail = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
        let pulled = dir.join(PULLED);
        if pulled.exists() {
            std::fs::remove_dir_all(&pulled).map_err(|e| fail(&pulled, e))?;
        }
        // The journal lets a later pull into the same directory skip what it
        // downloaded. Nothing pulls into this directory again.
        let journal = dir.join(message_crate_pull::PULL_JOURNAL_NAME);
        if journal.exists() {
            std::fs::remove_file(&journal).map_err(|e| fail(&journal, e))?;
        }
        let converting = dir.join(CONVERTING);
        if converting.exists() {
            for entry in std::fs::read_dir(&converting).map_err(|e| fail(&converting, e))? {
                let entry = entry.map_err(|e| fail(&converting, e))?;
                let to = dir.join(entry.file_name());
                std::fs::rename(entry.path(), &to).map_err(|e| fail(&to, e))?;
            }
            std::fs::remove_dir(&converting).map_err(|e| fail(&converting, e))?;
        }
        let empty = std::fs::read_dir(&dir)
            .map_err(|e| fail(&dir, e))?
            .next()
            .is_none();
        if empty {
            std::fs::remove_dir(&dir).map_err(|e| fail(&dir, e))?;
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
        std::fs::remove_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))
    }

    /// `path`, resolved, when it is the Export Directory or inside it, for
    /// opening.
    pub fn openable(&self, path: &Path) -> Option<PathBuf> {
        if !path.is_absolute() {
            return None;
        }
        let root = self.root.canonicalize().ok()?;
        let canonical = path.canonicalize().ok()?;
        canonical.starts_with(&root).then_some(canonical)
    }
}

/// The local date and time as `YYYY-MM-DD-HHMM`, for an export's directory.
pub fn started_now() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H%M").to_string()
}

#[cfg(test)]
mod tests;
