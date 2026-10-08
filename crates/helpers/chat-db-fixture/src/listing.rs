//! What a directory holds, for the tests that check what a decrypting
//! request wrote and what it left behind.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Every file and directory under `dir`, at any depth, sorted. None when
/// `dir` is not there.
///
/// Directories are listed too, so a request that leaves an empty directory
/// behind shows up. A directory that cannot be listed is passed over, so a
/// test can walk a scratch directory while a request deletes part of it.
#[must_use]
pub fn paths_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The names of the entries directly in `dir`, sorted.
///
/// # Panics
///
/// Panics when `dir` cannot be listed.
#[must_use]
pub fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("list the directory")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::{file_names, paths_under};

    /// The walker reaches files in nested directories and lists the
    /// directories themselves; the lister names only the top level.
    #[test]
    fn the_walker_goes_deep_and_the_lister_stays_on_top() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/b/file"), b"x").unwrap();
        std::fs::write(dir.path().join("top"), b"x").unwrap();

        assert_eq!(
            paths_under(dir.path()),
            vec![
                dir.path().join("a"),
                dir.path().join("a/b"),
                dir.path().join("a/b/file"),
                dir.path().join("top"),
            ]
        );
        assert_eq!(file_names(dir.path()), vec!["a", "top"]);
        assert!(paths_under(&dir.path().join("missing")).is_empty());
    }
}
