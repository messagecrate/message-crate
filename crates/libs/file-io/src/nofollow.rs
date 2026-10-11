//! Reading a file only when it is a regular file under its own name: a
//! symlink at the path is refused, never followed. The server's asset store
//! reads its sources, stored files and MIME sidecars this way, so a symlink
//! in an export or in the store cannot make the server read a file outside it.

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::path::Path;

/// Fail unless `path` is a regular file, not a symlink.
///
/// # Errors
///
/// Returns an error when `path` cannot be inspected, is a symlink, or is not
/// a file.
pub fn ensure_regular_file(path: &Path) -> Result<()> {
    let meta =
        std::fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if meta.file_type().is_symlink() {
        bail!("refusing to follow symlink: {}", path.display());
    }
    if !meta.is_file() {
        bail!("asset source is not a file: {}", path.display());
    }
    Ok(())
}

/// Open `path` for reading without following a symlink.
///
/// Checks [`ensure_regular_file`] first, then on Unix opens with
/// `O_NOFOLLOW`, so a symlink put in place between the check and the open is
/// refused too.
///
/// # Errors
///
/// Returns an error when [`ensure_regular_file`] refuses `path` or the file
/// cannot be opened.
pub fn open_nofollow(path: &Path) -> Result<File> {
    ensure_regular_file(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .with_context(|| format!("open {}", path.display()))
    }
    #[cfg(not(unix))]
    {
        File::open(path).with_context(|| format!("open {}", path.display()))
    }
}

/// The [`crate::file_sha256`] fingerprint of a file opened through
/// [`open_nofollow`], so a symlink at `path` is refused rather than hashed.
///
/// # Errors
///
/// Returns an error when [`open_nofollow`] refuses or fails, or the file
/// cannot be read. Unlike [`crate::file_sha256`], a read error does not name
/// the file: the asset store's callers add their own context to it.
pub fn file_sha256_nofollow(path: &Path) -> Result<String> {
    let file = open_nofollow(path)?;
    Ok(crate::sha256_of_reader(file)?)
}
