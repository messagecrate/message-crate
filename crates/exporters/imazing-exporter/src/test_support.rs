//! Helpers shared by this crate's tests.

use std::fs;
use std::path::Path;

/// Run `f` with the permissions of `directory` set to `mode`, then set them back
/// to `0o755` before returning, so the temporary directory can still be removed.
///
/// `None`, with a line on stderr, when `directory` can still be listed under
/// `mode`: a user such as root cannot exercise the failure, so the test has
/// nothing to check.
#[cfg(unix)]
pub(crate) fn with_directory_mode<T>(
    directory: &Path,
    mode: u32,
    f: impl FnOnce() -> T,
) -> Option<T> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(directory, fs::Permissions::from_mode(mode))
        .expect("set the directory's mode");
    let result = if fs::read_dir(directory).is_ok() {
        eprintln!(
            "skipped: {} can still be listed with mode {mode:o}",
            directory.display()
        );
        None
    } else {
        Some(f())
    };
    fs::set_permissions(directory, fs::Permissions::from_mode(0o755))
        .expect("restore the directory's mode");
    result
}
