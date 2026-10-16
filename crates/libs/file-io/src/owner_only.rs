//! Directories and files that only the user who owns them can read: the
//! Data Directory, the database and its `-wal` and `-shm` files, and the
//! Logs Directories, because they hold every account's messages (#2097), and
//! each Import Run's directory in the Staging Directory, because it holds the
//! conversations the run staged (#2969).
//!
//! The rule is Unix permission bits: a directory is `0700` and a file `0600`
//! less whatever the owner's own bits already leave out. A directory no other
//! user can enter keeps everything inside it from them whatever the inner
//! files' modes are, so callers protect the directory that holds the data
//! and leave the files in it to the umask.
//!
//! Windows has no permission bits. There a new directory or file takes the
//! access list of the directory it is made in, which under the user's own
//! profile (the Message Crate Directory) is that user's alone; these functions
//! make the directory and change nothing else.

use std::fs::OpenOptions;
use std::io;
use std::path::Path;

/// Make `dir` and every missing directory above it, each new one readable by
/// its owner alone (`0700`). A directory that is already there keeps its
/// mode: [`restrict_to_owner`] tightens one.
///
/// # Errors
///
/// Returns the error `create_dir_all` would: a directory that cannot be made,
/// or a file where a directory should be.
pub fn create_dir_all_owner_only(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Make the one directory `dir`, readable by its owner alone (`0700`). The
/// directory above it must be there.
///
/// # Errors
///
/// Returns the error `create_dir` would, `AlreadyExists` among them when
/// `dir` is already there.
pub fn create_dir_owner_only(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new().mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(dir)
    }
}

/// Take every permission for the group and for other users off `path`, a
/// directory or a file, keeping the owner's own. Answers whether the mode
/// changed: `false` when nothing but the owner had any access.
///
/// A symlink is followed, as `chmod` follows one. Off Unix nothing is
/// changed and the answer is `false`.
///
/// # Errors
///
/// Returns the error when `path` cannot be read, or its mode cannot be
/// changed, as when another user owns it (`PermissionDenied`).
pub fn restrict_to_owner(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(path)?.permissions().mode();
        if mode & 0o077 == 0 {
            return Ok(false);
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & !0o077))?;
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path).map(|_| false)
    }
}

/// Set `options` to create a new file readable by its owner alone (`0600`).
/// A file that is already there keeps its mode. Off Unix `options` is left as
/// it is.
pub fn owner_only(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600)
    }
    #[cfg(not(unix))]
    {
        options
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs::{self, OpenOptions, Permissions};
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    use super::{create_dir_all_owner_only, create_dir_owner_only, owner_only, restrict_to_owner};

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn every_directory_it_makes_is_its_owner_s_alone() {
        let root = tempfile::tempdir().unwrap();
        let inner = root.path().join("data").join("logs");
        create_dir_all_owner_only(&inner).unwrap();
        assert_eq!(mode(&root.path().join("data")), 0o700);
        assert_eq!(mode(&inner), 0o700);
    }

    #[test]
    fn the_one_directory_it_makes_is_its_owner_s_alone_and_must_be_new() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("staging-run");
        create_dir_owner_only(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(
            create_dir_owner_only(&dir).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn a_directory_already_there_keeps_its_mode_until_it_is_restricted() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("data");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, Permissions::from_mode(0o755)).unwrap();
        create_dir_all_owner_only(&dir).unwrap();
        assert_eq!(mode(&dir), 0o755);
        assert!(restrict_to_owner(&dir).unwrap());
        assert_eq!(mode(&dir), 0o700);
        assert!(!restrict_to_owner(&dir).unwrap());
    }

    #[test]
    fn restricting_keeps_the_owner_s_own_bits() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("messagecrate.db");
        fs::write(&file, b"").unwrap();
        fs::set_permissions(&file, Permissions::from_mode(0o444)).unwrap();
        assert!(restrict_to_owner(&file).unwrap());
        assert_eq!(mode(&file), 0o400);
    }

    #[test]
    fn a_file_it_creates_is_its_owner_s_alone() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("copy.partial");
        owner_only(OpenOptions::new().write(true).create_new(true))
            .open(&file)
            .unwrap();
        assert_eq!(mode(&file), 0o600);
    }
}
