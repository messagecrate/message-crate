//! The placeholder files an obfuscated export ships in place of its real
//! media.

use crate::clean::require_export_directory;
use anyhow::{Context, Result};
use obfuscate::PLACEHOLDER_FILES;
use std::fs;
use std::path::Path;

/// Delete everything under `output_dir/attachments/`, in subdirectories
/// too, and write the three shared placeholder files there
/// ([`PLACEHOLDER_FILES`]).
///
/// `output_dir` must hold the sentinel `.message-crate-export`: a directory
/// without it belongs to the person who chose it, so it is refused and
/// nothing in it is removed.
///
/// Each entry's type is read without following a symlink. A directory is
/// emptied one entry at a time and then removed, so a failure names the
/// file that stayed. A symlink is removed, never followed, so a link to a
/// directory outside the export leaves that directory alone, and a link with a
/// placeholder's name cannot carry the placeholder out of the export. The
/// placeholders are written afresh each time.
///
/// # Errors
///
/// Returns an error when the directory has no sentinel, `attachments/`
/// cannot be created or read, an entry
/// cannot be removed, or a placeholder cannot be written. Each names its
/// path.
pub(crate) fn materialize_placeholders(output_dir: &Path) -> Result<()> {
    require_export_directory(output_dir)?;
    let dir = output_dir.join("attachments");
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    // Anything left behind is content the obfuscated export exists to leave
    // out, so a failed delete fails the pass.
    remove_directory_contents(&dir)?;
    for (rel, bytes) in PLACEHOLDER_FILES {
        let path = output_dir.join(rel);
        fs::write(&path, bytes).with_context(|| format!("could not write {}", path.display()))?;
    }
    Ok(())
}

/// Remove every entry of `dir`, which stays, reading each type without
/// following a symlink.
fn remove_directory_contents(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("could not read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("could not read {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("could not read {}", path.display()))?;
        if file_type.is_dir() {
            remove_directory_contents(&path)?;
            fs::remove_dir(&path)
        } else if file_type.is_symlink() {
            remove_symlink(&path, file_type)
        } else {
            fs::remove_file(&path)
        }
        .with_context(|| {
            format!(
                "could not remove {} from the obfuscated export",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Remove the symlink at `path` itself. Windows removes a link to a directory
/// (or a junction) as a directory, and any other link as a file.
#[cfg(windows)]
fn remove_symlink(path: &Path, file_type: fs::FileType) -> std::io::Result<()> {
    use std::os::windows::fs::FileTypeExt;
    if file_type.is_symlink_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

/// Remove the symlink at `path` itself. Unix removes every link as a file.
#[cfg(not(windows))]
fn remove_symlink(path: &Path, _file_type: fs::FileType) -> std::io::Result<()> {
    fs::remove_file(path)
}

#[cfg(test)]
mod tests;
