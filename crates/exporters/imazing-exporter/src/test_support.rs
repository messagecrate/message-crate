//! Helpers shared by this crate's tests.

use std::fs;
use std::path::Path;

/// Run `f` with the permissions of `folder` set to `mode`, then set them back
/// to `0o755` before returning, so the temporary folder can still be removed.
///
/// `None`, with a line on stderr, when `folder` can still be listed under
/// `mode`: a user such as root cannot exercise the failure, so the test has
/// nothing to check.
#[cfg(unix)]
pub(crate) fn with_folder_mode<T>(folder: &Path, mode: u32, f: impl FnOnce() -> T) -> Option<T> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(folder, fs::Permissions::from_mode(mode)).expect("set the folder's mode");
    let result = if fs::read_dir(folder).is_ok() {
        eprintln!(
            "skipped: {} can still be listed with mode {mode:o}",
            folder.display()
        );
        None
    } else {
        Some(f())
    };
    fs::set_permissions(folder, fs::Permissions::from_mode(0o755))
        .expect("restore the folder's mode");
    result
}
