//! File helpers that know nothing about conversations: the atomic, synced
//! write that conversation files and journals go through ([`write_atomic`]),
//! the same write through a temporary file the caller names, which staged
//! attachments and exported Assets use ([`write_atomic_via`]), the synced
//! rename for a file another program wrote ([`rename_into_place`]), the
//! streamed SHA-256 of a file ([`file_sha256`]), the same fingerprint of
//! bytes already in memory ([`sha256_hex`]), and the open and hash that refuse
//! a symlink rather than follow it, which the server's asset store reads
//! through ([`open_nofollow`], [`file_sha256_nofollow`]), and the
//! directories and files only their owner can read, which hold every
//! account's messages ([`create_dir_all_owner_only`],
//! [`create_dir_owner_only`], [`restrict_to_owner`], [`owner_only`]).
//!
//! [`sha256_hex`] lives here, not with its callers, so it stays beside
//! [`file_sha256`] and the tests pin the two to one fingerprint format,
//! whether the bytes are a file, an attachment in memory, or a session or API
//! token.
//!
//! They live in a leaf crate so attachment staging, media conversion, the
//! journal, the export and import crates, the server and the desktop app share
//! one copy without depending on the conversation model in `ir`.

use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::Path;

mod durable;
mod nofollow;
mod owner_only;

pub use durable::{rename_into_place, write_atomic, write_atomic_via};
pub use nofollow::{ensure_regular_file, file_sha256_nofollow, open_nofollow};
pub use owner_only::{
    create_dir_all_owner_only, create_dir_owner_only, owner_only, restrict_to_owner,
};

/// Stream a file through SHA-256 in 64 KB chunks (no full read into memory).
///
/// Returns 64 lowercase hex digits, the same fingerprint format
/// `digest_sha256` fields carry.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or read; the error message
/// names the file.
pub fn file_sha256(path: &Path) -> io::Result<String> {
    let file = std::fs::File::open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("open {}: {e}", path.display())))?;
    sha256_of_reader(file)
        .map_err(|e| io::Error::new(e.kind(), format!("read {}: {e}", path.display())))
}

/// Stream `reader` through SHA-256 in 64 KB chunks, retrying an interrupted
/// read, and return 64 lowercase hex digits. A read error is returned
/// unchanged: [`file_sha256`] names the file in it, and
/// [`file_sha256_nofollow`] passes it on bare, as the asset store's hash
/// always has.
fn sha256_of_reader(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

/// SHA-256 of `bytes`, as 64 lowercase hex digits: the same fingerprint
/// [`file_sha256`] gives for a file holding them.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests;
