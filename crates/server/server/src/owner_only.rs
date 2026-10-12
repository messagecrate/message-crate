//! The Data Directory and the database, kept to the user the server runs as
//! (#2097).
//!
//! The Data Directory holds every account's attachments, the server's log,
//! the upload parts under `.incoming/` and the import bodies, so the server
//! makes it `0700` and takes group and other access off one that is already
//! there: nothing inside it is then readable by another user of the computer,
//! whatever the umask gave the files in it. The database may live outside it
//! (`[paths] db`, `--db`), so the database file, its `-wal`, `-shm` and
//! `-journal` files, and the copy kept before a schema rebuild are made
//! `0600` too. The database's directory is made `0700` when the server makes
//! it, and an existing one is left as it is, since it may be any directory
//! the owner named.
//!
//! Windows has no permission bits: there the directories and files take the
//! access list of the directory they are made in ([`file_io::owner_only`]).

use std::path::Path;

use anyhow::{Context, Result};

/// Make the Data Directory `data_dir`, new directories above it included,
/// readable by the server's user alone, and take group and other access off
/// it when it is already there.
///
/// A directory another user owns cannot be changed. That is warned of, and
/// the server goes on with the directory as its owner left it.
///
/// # Errors
///
/// Returns an error, naming the directory, when it cannot be made.
pub(crate) fn keep_data_dir_to_owner(data_dir: &Path) -> Result<()> {
    file_io::create_dir_all_owner_only(data_dir)
        .with_context(|| format!("create data directory {}", data_dir.display()))?;
    restrict_or_warn(data_dir);
    Ok(())
}

/// Make the database file at `db` readable by the server's user alone before
/// SQLite opens it: a missing file is created empty with mode `0600`, and an
/// existing one, with its `-wal`, `-shm` and `-journal` files, loses group
/// and other access. SQLite gives the `-wal`, `-shm` and `-journal` files it
/// creates later the database file's mode, so they are `0600` too.
///
/// # Errors
///
/// Returns an error, naming the file, when a missing file cannot be created.
pub(crate) fn keep_database_to_owner(db: &Path) -> Result<()> {
    match file_io::owner_only(std::fs::OpenOptions::new().write(true).create_new(true)).open(db) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).with_context(|| format!("create database {}", db.display()));
        }
    }
    restrict_or_warn(db);
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        let path = Path::new(&name);
        if path.exists() {
            restrict_or_warn(path);
        }
    }
    Ok(())
}

/// Take group and other access off `path`, or warn when it cannot be done.
fn restrict_or_warn(path: &Path) {
    match file_io::restrict_to_owner(path) {
        Ok(true) => tracing::info!(
            "{} was readable by other users of this computer, so the server made it readable by its own user alone",
            path.display()
        ),
        Ok(false) => {}
        Err(error) => tracing::warn!(
            %error,
            "{} may be readable by other users of this computer, and the server could not make it readable by its own user alone",
            path.display()
        ),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    use super::{keep_data_dir_to_owner, keep_database_to_owner};

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_new_data_directory_and_the_ones_above_it_are_the_owner_s_alone() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("srv").join("data");
        keep_data_dir_to_owner(&data_dir).unwrap();
        assert_eq!(mode(&root.path().join("srv")), 0o700);
        assert_eq!(mode(&data_dir), 0o700);
    }

    #[test]
    fn a_data_directory_others_could_read_is_tightened() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        fs::create_dir(&data_dir).unwrap();
        fs::set_permissions(&data_dir, Permissions::from_mode(0o755)).unwrap();
        keep_data_dir_to_owner(&data_dir).unwrap();
        assert_eq!(mode(&data_dir), 0o700);
    }

    #[test]
    fn a_missing_database_file_is_made_the_owner_s_alone() {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join("messagecrate.db");
        keep_database_to_owner(&db).unwrap();
        assert_eq!(mode(&db), 0o600);
        assert_eq!(fs::metadata(&db).unwrap().len(), 0);
    }

    #[test]
    fn an_existing_database_and_its_sidecars_lose_other_users_access() {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join("messagecrate.db");
        let files = [
            "messagecrate.db",
            "messagecrate.db-wal",
            "messagecrate.db-shm",
        ]
        .map(|name| root.path().join(name));
        for file in &files {
            fs::write(file, b"x").unwrap();
            fs::set_permissions(file, Permissions::from_mode(0o644)).unwrap();
        }
        keep_database_to_owner(&db).unwrap();
        for file in &files {
            assert_eq!(mode(file), 0o600, "{}", file.display());
            assert_eq!(fs::read(file).unwrap(), b"x");
        }
    }
}
