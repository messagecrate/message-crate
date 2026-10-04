//! The scratch folders a run writes into, under the desktop app's cache
//! folder.
//!
//! Two things a run writes are not output: the attachment spool (each
//! attachment payload an SMS Backup & Restore, GO SMS Pro or SMS Backup+
//! file carries, written to disk as it is parsed) and the databases
//! `imessage-reader` decrypts out of an encrypted iPhone backup. Both are
//! plain copies of personal data that no screen names, so they live in
//! folders under the app's cache folder ([`IMESSAGE_READER_FOLDER`],
//! [`ATTACHMENT_SPOOL_FOLDER`]), never in the output folder the person
//! chose. The app deletes a request's folder when the request ends, and
//! [`sweep_scratch`] at app start, and every new request, delete what a
//! killed one left.
//!
//! Two requests can run at once (the Import form can ask for a second
//! backup's identities while an Import Run decrypts the first), so the
//! clean-up must tell a dead request's folder from a live one. A live
//! request holds an exclusive lock on the `.lock` file in its folder for as
//! long as it runs. The operating system drops that lock when the process
//! ends, however it ends, so a folder whose lock can be taken belongs to
//! nobody. Cleaning up and making a new folder happen under a lock on the
//! root's own `.lock` file, so no request is between making its folder and
//! locking it while another cleans up.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

/// The folder under the app's cache folder that `imessage-reader` decrypts
/// an encrypted iPhone backup's databases and attachments into.
pub const IMESSAGE_READER_FOLDER: &str = "imessage-reader";

/// The folder under the app's cache folder that holds each run's attachment
/// spool.
pub const ATTACHMENT_SPOOL_FOLDER: &str = "attachment-spool";

/// Every scratch folder [`sweep_scratch`] cleans.
const SCRATCH_FOLDERS: [&str; 2] = [IMESSAGE_READER_FOLDER, ATTACHMENT_SPOOL_FOLDER];

/// The lock file, in the root and in each request's folder.
const LOCK: &str = ".lock";

/// Delete what killed runs left in every scratch folder under `cache_dir`,
/// keeping the folders of requests still running. The desktop app calls it
/// when it starts, so a killed run's data does not wait for the next run of
/// the same kind.
///
/// A scratch folder that cannot be read or locked is left as it is: the
/// next request in it cleans it, and a failed sweep must not stop the app.
pub fn sweep_scratch(cache_dir: &Path) {
    for folder in SCRATCH_FOLDERS {
        let root = cache_dir.join(folder);
        if !root.is_dir() {
            continue;
        }
        let Ok(root_lock) = File::create(root.join(LOCK)) else {
            continue;
        };
        if root_lock.lock().is_ok() {
            remove_leftovers(&root);
        }
    }
}

/// One request's scratch folder. Dropping it deletes the folder and
/// everything in it.
#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
    /// Held for the life of the request. `None` once [`Drop`] has let go
    /// of it, because Windows cannot delete a file that is still open.
    lock: Option<File>,
}

impl ScratchDir {
    /// Delete what earlier requests left under `root`, then make and lock a
    /// new folder there for this request.
    ///
    /// # Errors
    ///
    /// Returns an error when `root` is not a full path, `root` or the new
    /// folder cannot be made, or a lock file cannot be made or locked.
    pub fn create(root: &Path) -> Result<Self> {
        // A relative root would land wherever the process happens to run.
        if !root.is_absolute() {
            bail!("the scratch folder {} is not a full path", root.display());
        }
        fs::create_dir_all(root)
            .with_context(|| format!("make the scratch folder {}", root.display()))?;
        restrict_to_owner(root)?;
        let root_lock = File::create(root.join(LOCK))
            .with_context(|| format!("make the lock file in {}", root.display()))?;
        root_lock
            .lock()
            .with_context(|| format!("lock {}", root.display()))?;

        remove_leftovers(root);

        let path = tempfile::Builder::new()
            .prefix("request-")
            .tempdir_in(root)
            .with_context(|| format!("make a request folder in {}", root.display()))?
            .keep();
        // From here the folder is deleted on drop, and on an error below.
        let mut scratch = Self { path, lock: None };
        restrict_to_owner(&scratch.path)?;
        let lock = File::create(scratch.path.join(LOCK))
            .and_then(|lock| lock.lock().map(|()| lock))
            .with_context(|| format!("lock {}", scratch.path.display()))?;
        scratch.lock = Some(lock);
        Ok(scratch)
    }

    /// The folder this request writes into.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        drop(self.lock.take());
        // A folder that will not go now is removed by the next request, or
        // by the sweep when the app next starts.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Delete every entry of `root` except its lock file and the folders of
/// requests still running. The caller holds the root's lock.
fn remove_leftovers(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == LOCK {
            continue;
        }
        if !path.is_dir() {
            // Nothing writes loose files here; whatever this is, it is not
            // a running request's.
            let _ = fs::remove_file(&path);
            continue;
        }
        if belongs_to_a_running_request(&path) {
            continue;
        }
        let _ = fs::remove_dir_all(&path);
    }
}

/// Whether another request holds the lock in `folder`. A folder without a
/// lock file belongs to a request killed before it took one: requests make
/// the folder and its lock under the root's lock, which the caller holds.
fn belongs_to_a_running_request(folder: &Path) -> bool {
    let Ok(lock) = File::open(folder.join(LOCK)) else {
        return false;
    };
    lock.try_lock().is_err()
}

/// Make `folder` readable by its owner only, because it holds decrypted
/// message data or attachment payloads.
#[cfg(unix)]
fn restrict_to_owner(folder: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(folder, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restrict {} to its owner", folder.display()))
}

/// Off Unix the folder keeps the permissions of the app's cache folder it
/// sits in.
#[cfg(not(unix))]
fn restrict_to_owner(_folder: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::testutil::names_in;

    use super::{ATTACHMENT_SPOOL_FOLDER, IMESSAGE_READER_FOLDER, LOCK, ScratchDir, sweep_scratch};

    /// A request's folder sits under the root and is gone, with what the
    /// reader decrypted into it, once the request ends.
    #[test]
    fn a_request_folder_is_deleted_when_the_request_ends() {
        let root = tempfile::tempdir().unwrap();
        let scratch = ScratchDir::create(root.path()).unwrap();
        assert_eq!(scratch.path().parent(), Some(root.path()));
        fs::write(scratch.path().join("crabapple-sms-x.db"), b"decrypted").unwrap();

        drop(scratch);
        assert!(
            names_in(root.path()).is_empty(),
            "{:?}",
            names_in(root.path())
        );
    }

    /// What a killed request left is deleted when the next request starts:
    /// its folder (whose lock nobody holds), a folder it made before it
    /// took a lock, and a loose decrypted database.
    #[test]
    fn a_killed_request_s_leftovers_are_deleted_at_the_next_request() {
        let root = tempfile::tempdir().unwrap();
        let dead = root.path().join("request-dead");
        fs::create_dir(&dead).unwrap();
        fs::write(dead.join(LOCK), b"").unwrap();
        fs::write(dead.join("crabapple-sms-x.db"), b"decrypted").unwrap();
        let unlocked = root.path().join("request-unlocked");
        fs::create_dir(&unlocked).unwrap();
        fs::write(unlocked.join("crabapple-contacts-x.db"), b"decrypted").unwrap();
        fs::write(root.path().join("crabapple-sms-y.db"), b"decrypted").unwrap();

        let scratch = ScratchDir::create(root.path()).unwrap();
        let own = scratch.path().file_name().unwrap().to_string_lossy();
        assert_eq!(names_in(root.path()), vec![own.into_owned()]);
    }

    /// A request still running keeps its folder while another starts.
    #[test]
    fn a_running_request_s_folder_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let first = ScratchDir::create(root.path()).unwrap();
        fs::write(first.path().join("crabapple-sms-x.db"), b"decrypted").unwrap();

        let second = ScratchDir::create(root.path()).unwrap();
        assert!(first.path().join("crabapple-sms-x.db").exists());
        assert_ne!(first.path(), second.path());
    }

    /// The root holds decrypted message data, so on Unix only its owner
    /// may open it, whatever it was made with.
    #[cfg(unix)]
    #[test]
    fn the_root_is_restricted_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("imessage-reader");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();

        let scratch = ScratchDir::create(&root).unwrap();
        for folder in [root.as_path(), scratch.path()] {
            let mode = fs::metadata(folder).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}: {mode:o}", folder.display());
        }
    }

    /// A folder a killed request left: its lock file, which nobody holds,
    /// and the data it wrote.
    fn killed_request(root: &std::path::Path, name: &str, file: &str) {
        let folder = root.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(LOCK), b"").unwrap();
        fs::write(folder.join(file), b"plain").unwrap();
    }

    /// When the app starts, the sweep deletes what killed runs left in the
    /// attachment spool's folder and the reader's folder, and keeps the
    /// folder a running job holds, in either (#1421, #1402).
    #[test]
    fn the_start_up_sweep_deletes_a_killed_run_s_scratch_and_keeps_a_running_one() {
        let cache = tempfile::tempdir().unwrap();
        let spool_root = cache.path().join(ATTACHMENT_SPOOL_FOLDER);
        let reader_root = cache.path().join(IMESSAGE_READER_FOLDER);
        killed_request(&spool_root, "request-killed", "2cf24dba");
        killed_request(&reader_root, "request-killed", "crabapple-sms-x.db");
        let running_spool = ScratchDir::create(&spool_root).unwrap();
        fs::write(running_spool.path().join("e3b0c442"), b"payload").unwrap();
        let running_reader = ScratchDir::create(&reader_root).unwrap();
        // Made after the running requests, so their own clean-up did not
        // see it: only the sweep can.
        killed_request(&spool_root, "request-later", "9f86d081");
        fs::write(cache.path().join("unrelated"), b"kept").unwrap();

        sweep_scratch(cache.path());

        let own = |scratch: &ScratchDir| {
            vec![
                scratch
                    .path()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ]
        };
        assert_eq!(names_in(&spool_root), own(&running_spool));
        assert_eq!(names_in(&reader_root), own(&running_reader));
        assert!(running_spool.path().join("e3b0c442").exists());
        assert!(
            cache.path().join("unrelated").exists(),
            "only scratch folders are swept"
        );
    }

    /// A cache folder with no scratch folders yet is swept without making
    /// any.
    #[test]
    fn the_sweep_of_an_empty_cache_folder_makes_nothing() {
        let cache = tempfile::tempdir().unwrap();
        sweep_scratch(cache.path());
        assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
    }

    /// A relative root would put decrypted data wherever the process runs,
    /// so it is refused.
    #[test]
    fn a_relative_root_is_refused() {
        let err = ScratchDir::create(std::path::Path::new("attachment-spool")).unwrap_err();
        assert!(err.to_string().contains("not a full path"), "{err}");
    }
}
