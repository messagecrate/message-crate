//! Known answers for the hashes the rest of the product keys on.
//!
//! `file_sha256` is behind every digest check and asset key in import, staging
//! and transcode, and `sha256_hex` gives the same fingerprint for bytes already
//! in memory. A change to either silently re-keys everything already in a
//! database, so both are pinned to values computed outside Rust (Python's
//! `hashlib`). The message id's known answers are in `message-ir`'s
//! `identity.rs`.

#[cfg(unix)]
use crate::open_nofollow;
use crate::{file_sha256, file_sha256_nofollow, sha256_hex};

/// A file in the temp directory, removed when dropped.
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn with(name: &str, bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!("file-io-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn file_sha256_of_an_empty_file() {
    let file = TempFile::with("empty", b"");
    assert_eq!(
        file_sha256(&file.0).unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn file_sha256_of_a_file_that_spans_several_reads() {
    // 200,000 bytes is three full 64 KiB reads and a partial fourth.
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let file = TempFile::with("large", &bytes);
    assert_eq!(
        file_sha256(&file.0).unwrap(),
        "e24bc62381f1224fbbb74688663f8f9743b9680b193edd666835e97b06e730eb"
    );
}

#[test]
fn file_sha256_names_a_missing_file() {
    let path = std::env::temp_dir().join("file-io-no-such-file");
    let err = file_sha256(&path).unwrap_err();
    assert!(err.to_string().contains("file-io-no-such-file"), "{err}");
}

#[test]
fn sha256_hex_of_bytes_in_memory() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha256_hex_matches_file_sha256_of_the_same_bytes() {
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let file = TempFile::with("same-bytes", &bytes);
    assert_eq!(sha256_hex(&bytes), file_sha256(&file.0).unwrap());
}

#[test]
fn file_sha256_nofollow_matches_file_sha256_of_a_regular_file() {
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let file = TempFile::with("nofollow-regular", &bytes);
    assert_eq!(
        file_sha256_nofollow(&file.0).unwrap(),
        file_sha256(&file.0).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn file_sha256_nofollow_refuses_a_symlink_to_a_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::write(&target, b"abc").unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let refused = format!("refusing to follow symlink: {}", link.display());
    assert_eq!(
        file_sha256_nofollow(&link).unwrap_err().to_string(),
        refused
    );
    assert_eq!(open_nofollow(&link).unwrap_err().to_string(), refused);
}

#[test]
fn file_sha256_nofollow_refuses_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let err = file_sha256_nofollow(dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("asset source is not a file: {}", dir.path().display())
    );
}

#[test]
fn file_sha256_nofollow_names_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing");
    let err = file_sha256_nofollow(&path).unwrap_err();
    assert_eq!(err.to_string(), format!("stat {}", path.display()));
    assert!(
        err.chain()
            .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
            .any(|io| io.kind() == std::io::ErrorKind::NotFound)
    );
}
