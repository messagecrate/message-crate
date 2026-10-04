//! The regular files directly in a chat folder, which both the row lookup
//! ([`crate::attachments`]) and the files no row names
//! ([`crate::unnamed_files`]) read.

use anyhow::{Context, Result};
use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};

/// The regular files directly in `folder`, in the order the folder lists
/// them.
///
/// Symbolic links are skipped, because following one can reach a file
/// outside the export.
///
/// # Errors
///
/// Returns an error, with `folder` named, when `folder` cannot be read or
/// one of its entries cannot be read (see [`regular_files_of`]).
pub(crate) fn regular_files(folder: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(folder).with_context(|| format!("read {}", folder.display()))?;
    regular_files_of(folder, entries)
}

/// The regular files among `entries`, the entries of `folder`.
///
/// An entry that cannot be read, or whose type cannot be read, fails the
/// listing. Skipping it would leave a row whose file was in that entry with
/// no file, or drop a Live Photo video, and the conversion would give no
/// reason. Reading the rest of the export's folders fails the same way.
///
/// # Errors
///
/// Returns an error for the first entry that cannot be read.
fn regular_files_of(
    folder: &Path,
    entries: impl IntoIterator<Item = io::Result<DirEntry>>,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("read an entry of {}", folder.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("read the type of {}", entry.path().display()))?;
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        files.push(entry.path());
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry the listing cannot read fails it with the folder named,
    /// rather than leaving the entry's file out of the conversion (#1563).
    #[test]
    fn an_entry_that_cannot_be_read_fails_the_listing_and_names_the_folder() {
        let folder = Path::new("/imazing/Messages/Bob");
        let entries = vec![Err(io::Error::other("stale file handle"))];

        let error = regular_files_of(folder, entries).unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("/imazing/Messages/Bob"), "{message}");
        assert!(message.contains("stale file handle"), "{message}");
    }

    /// Regular files are listed; a subfolder and a symbolic link are not.
    #[test]
    fn only_regular_files_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.jpg"), b"a").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("a.jpg"), dir.path().join("link.jpg")).unwrap();

        assert_eq!(
            regular_files(dir.path()).unwrap(),
            vec![dir.path().join("a.jpg")]
        );
    }
}
