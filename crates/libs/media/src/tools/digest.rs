//! The SHA-256 of a program in the Tools Directory, read once per version
//! of the file (#2692).
//!
//! The desktop app counts a wtsexporter in the Tools Directory only when it
//! is the pinned release's file, and every lookup asks. Reading a file of
//! tens of megabytes at each lookup would be slow, so the digest is kept
//! beside the file's [`FileStamp`] and read again only when the file
//! changes: a copy still being written grows, and a file put in its place
//! differs in its size, times or inode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::FileStamp;

/// The digest of one file, as it was when it was read.
struct Digest {
    stamp: FileStamp,
    sha256: String,
    /// How many times the file was read, for the tests that a file is not
    /// read again while it is unchanged.
    #[cfg(any(test, feature = "testutil"))]
    reads: u32,
}

/// The digests read in this process, by path.
fn digests() -> &'static Mutex<HashMap<PathBuf, Digest>> {
    static DIGESTS: OnceLock<Mutex<HashMap<PathBuf, Digest>>> = OnceLock::new();
    DIGESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The stamp of the file at `path`, following a link. `None` when it can't
/// be read or is not a file.
fn stamp_at(path: &Path) -> Option<FileStamp> {
    let metadata = std::fs::metadata(path).ok()?;
    metadata.is_file().then(|| FileStamp::of(&metadata))
}

/// Whether the file at `path` has the SHA-256 `sha256`, as 64 lowercase hex
/// digits. The file is read only when this process has not read it as it is
/// now, so an unchanged file is read once.
///
/// A file that changes while it is read, as a copy still being written
/// does, is not remembered, and the next call reads it again. A file that
/// is missing or can't be read does not have it.
pub fn file_has_sha256(path: &Path, sha256: &str) -> bool {
    let Some(before) = stamp_at(path) else {
        return false;
    };
    {
        let digests = digests().lock().expect("digests lock");
        if let Some(digest) = digests.get(path)
            && digest.stamp == before
        {
            return digest.sha256 == sha256;
        }
    }
    // No lock is held while the file is read.
    let Ok(read) = file_io::file_sha256(path) else {
        return false;
    };
    if stamp_at(path).is_some_and(|after| after == before) {
        let mut digests = digests().lock().expect("digests lock");
        #[cfg(any(test, feature = "testutil"))]
        let reads = digests.get(path).map_or(0, |digest| digest.reads) + 1;
        digests.insert(
            path.to_path_buf(),
            Digest {
                stamp: before,
                sha256: read.clone(),
                #[cfg(any(test, feature = "testutil"))]
                reads,
            },
        );
    }
    read == sha256
}

/// How many times [`file_has_sha256`] read the file at `path` and kept
/// what it read.
#[cfg(any(test, feature = "testutil"))]
pub fn times_read(path: &Path) -> u32 {
    digests()
        .lock()
        .expect("digests lock")
        .get(path)
        .map_or(0, |digest| digest.reads)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file with the SHA-256 asked for has it, and is not read again
    /// while it is unchanged.
    #[test]
    fn an_unchanged_file_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("program");
        std::fs::write(&path, b"the pinned file").unwrap();
        let sha256 = file_io::sha256_hex(b"the pinned file");

        assert!(file_has_sha256(&path, &sha256));
        assert!(file_has_sha256(&path, &sha256));
        assert!(!file_has_sha256(&path, &file_io::sha256_hex(b"another")));

        assert_eq!(times_read(&path), 1);
    }

    /// A file that grows, as a copy still being written does, is read again,
    /// and has the SHA-256 only once it is whole.
    #[test]
    fn a_file_that_changes_is_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("program");
        let sha256 = file_io::sha256_hex(b"the pinned file");
        std::fs::write(&path, b"the pinned").unwrap();

        let partial = file_has_sha256(&path, &sha256);
        std::fs::write(&path, b"the pinned file").unwrap();
        let whole = file_has_sha256(&path, &sha256);

        assert!(!partial);
        assert!(whole);
        assert_eq!(times_read(&path), 2);
    }

    /// A missing file, or a directory, does not have any SHA-256.
    #[test]
    fn a_missing_file_does_not_have_it() {
        let dir = tempfile::tempdir().unwrap();
        let sha256 = file_io::sha256_hex(b"");

        assert!(!file_has_sha256(&dir.path().join("missing"), &sha256));
        assert!(!file_has_sha256(dir.path(), &sha256));
    }
}
