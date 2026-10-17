//! Where an import batch's body is written while it is read:
//! `data_dir/.import-batches/`, one directory per batch, removed when the
//! batch is done.
//!
//! The body lives under the Data Directory, on the disk the owner sized for
//! Message Crate, and not in the operating system's temporary directory,
//! which is often small or held in memory and is not the `/app/data` volume
//! of the Docker image (#2123). A server stopped part-way through a batch
//! cannot remove that batch's directory, so `serve` removes every one left
//! there before it listens: no batch is being read then.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// The directory under the Data Directory that holds the body of every
/// batch being read.
const BATCH_BODIES: &str = ".import-batches";

/// `data_dir/.import-batches`.
pub(crate) fn root(data_dir: &Path) -> PathBuf {
    data_dir.join(BATCH_BODIES)
}

/// A new directory for one batch's body under [`root`], removed when the
/// returned [`TempDir`] is dropped.
///
/// # Errors
///
/// Returns an error when the directory cannot be made.
pub(crate) fn new_dir(data_dir: &Path) -> std::io::Result<TempDir> {
    let root = root(data_dir);
    std::fs::create_dir_all(&root)?;
    tempfile::Builder::new().prefix("batch-").tempdir_in(&root)
}

/// Remove every batch body a stopped server left under [`root`]. Run once
/// as `serve` starts, before it listens, so no batch is being read. A
/// directory that cannot be removed is logged and left.
pub(crate) fn sweep_at_start(data_dir: &Path) {
    let root = root(data_dir);
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            tracing::warn!(
                path = %root.display(),
                %error,
                "The import batch bodies a stopped server left could not be listed"
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let removed = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(error) = removed {
            tracing::warn!(
                path = %path.display(),
                %error,
                "An import batch body a stopped server left could not be removed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{new_dir, root, sweep_at_start};

    /// A batch's body goes under the Data Directory, never the operating
    /// system's temporary directory (#2123).
    #[test]
    fn a_batch_body_directory_is_under_the_data_directory() {
        let data_dir = tempfile::tempdir().unwrap();
        let dir = new_dir(data_dir.path()).unwrap();
        assert_eq!(dir.path().parent(), Some(root(data_dir.path()).as_path()));
    }

    /// What a server stopped mid-batch left is removed as the next one
    /// starts, so nothing builds up where nothing else looks.
    #[test]
    fn the_start_sweep_removes_what_a_stopped_server_left() {
        let data_dir = tempfile::tempdir().unwrap();
        let left = new_dir(data_dir.path()).unwrap().keep();
        std::fs::write(left.join("_import.jsonl"), "{}\n").unwrap();
        std::fs::write(root(data_dir.path()).join("stray"), "x").unwrap();

        sweep_at_start(data_dir.path());

        let remaining = std::fs::read_dir(root(data_dir.path())).unwrap().count();
        assert_eq!(remaining, 0);
    }

    /// A Data Directory that has never held a batch has nothing to sweep.
    #[test]
    fn the_start_sweep_of_a_data_directory_with_no_batches_does_nothing() {
        let data_dir = tempfile::tempdir().unwrap();
        sweep_at_start(data_dir.path());
        assert!(!root(data_dir.path()).exists());
    }
}
