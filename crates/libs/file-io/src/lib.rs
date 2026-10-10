//! File helpers that know nothing about conversations: the atomic, synced
//! write that conversation files and journals go through ([`write_atomic`]),
//! the same write through a temporary file the caller names, which staged
//! attachments and exported Assets use ([`write_atomic_via`]), the synced
//! rename for a file another program wrote ([`rename_into_place`]), and the
//! streamed SHA-256 of a file ([`file_sha256`]).
//!
//! They live in a leaf crate so attachment staging, media conversion, the
//! journal, the export and import crates and the desktop app share one copy
//! without depending on the conversation model in `message-ir`.

use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::Path;

mod durable;

pub use durable::{rename_into_place, write_atomic, write_atomic_via};

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
    let mut file = std::fs::File::open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("open {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!("read {}: {e}", path.display()),
                ));
            }
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests;
