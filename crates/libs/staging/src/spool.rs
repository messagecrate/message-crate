//! Attachment payloads written to disk as an exporter parses them.
//!
//! A backup that carries its attachments inside the file (SMS Backup &
//! Restore's base64 parts, GO SMS Pro's PDUs, SMS Backup+'s EML parts) hands
//! the exporter each payload as bytes. Parse finishes before anything is
//! staged, so an exporter that kept those bytes would hold every attachment
//! of the backup in memory at once. The spool writes each payload to a file
//! named by its SHA-256 the moment it is parsed, and the write tail reads it
//! back one file at a time.
//!
//! The spool is scratch data, not output: unencrypted attachments no screen
//! names. So it lives in a [`ScratchDir`] under the Scratch Directory,
//! never in the output directory. The directory goes when the spool is dropped,
//! whichever way the run ends, and what a killed run left is deleted by the
//! next spool and by the sweep when the app starts
//! (`message_crate_core::sweep_scratch`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use message_crate_core::{ATTACHMENT_SPOOL_DIRECTORY, ScratchDir};
use message_ir::IrAttachment;
use sha2::{Digest, Sha256};

use crate::headroom::{Disk, check_headroom};
use crate::write_queue::AttachmentSource;

/// Content-addressed store of attachment payloads for one run.
#[derive(Debug)]
pub struct AttachmentSpool {
    /// The spool's scratch root, under the Scratch Directory.
    root: PathBuf,
    /// The directory the spooled payloads are copied into once the backup is
    /// read, when the spool is told; see [`AttachmentSpool::new`].
    copy_dir: Option<PathBuf>,
    state: Mutex<SpoolState>,
}

/// What the spool has written, behind one lock.
#[derive(Debug, Default)]
struct SpoolState {
    /// This run's directory, made when the first payload arrives. Dropping it
    /// deletes the directory.
    directory: Option<ScratchDir>,
    /// Size of each spooled payload by its SHA-256, for the progress totals.
    sizes: HashMap<String, u64>,
    /// The sum of `sizes`.
    spooled_bytes: u64,
}

impl SpoolState {
    /// The spooled file for `digest`, when one was spooled.
    fn path(&self, digest: &str) -> Option<PathBuf> {
        let directory = self.directory.as_ref()?;
        self.sizes
            .contains_key(digest)
            .then(|| directory.path().join(digest))
    }
}

impl AttachmentSpool {
    /// A spool whose directory goes under `scratch_dir`, the Scratch Directory.
    /// Nothing is made on disk until the first payload arrives.
    pub fn new(scratch_dir: &Path) -> Self {
        Self {
            root: scratch_dir.join(ATTACHMENT_SPOOL_DIRECTORY),
            copy_dir: None,
            state: Mutex::new(SpoolState::default()),
        }
    }

    /// Name `copy_dir`, the directory the payloads are copied into once the
    /// backup is read. Each payload then also checks that the disk holding
    /// it still has room for the copy of everything spooled so far, so a
    /// run that will not fit stops while the backup is read, not after.
    /// When the Scratch Directory is on that same disk, the spool has already
    /// taken its share of what is free.
    #[must_use]
    pub fn with_copy_dir(mut self, copy_dir: &Path) -> Self {
        self.copy_dir = Some(copy_dir.to_path_buf());
        self
    }

    /// Write `bytes` to the spool and return their lowercase hex SHA-256.
    /// A payload already spooled is not written again.
    ///
    /// The disk that holds the spool is checked for room before each
    /// payload, and the disk that holds the copy for the copy of every
    /// payload spooled so far, so a disk that fills while the backup is read
    /// fails the run with the space it needs rather than a bare write
    /// error.
    ///
    /// # Errors
    ///
    /// Returns an error when the spool's directory cannot be made, its disk
    /// has no room for the payload, or the payload file cannot be written.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let digest = hex::encode(Sha256::digest(bytes));
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.sizes.contains_key(&digest) {
            return Ok(digest);
        }
        let directory = match &mut state.directory {
            Some(directory) => directory.path().to_path_buf(),
            empty => empty
                .insert(ScratchDir::create(&self.root)?)
                .path()
                .to_path_buf(),
        };
        check_headroom(&directory, bytes.len() as u64, Disk::Scratch)?;
        if let Some(copy_dir) = &self.copy_dir {
            let copied = state.spooled_bytes.saturating_add(bytes.len() as u64);
            check_headroom(copy_dir, copied, Disk::Staging)?;
        }
        let path = directory.join(&digest);
        // Written under a temporary name first, so a file under a digest is
        // always the whole payload.
        let tmp = directory.join(format!("{digest}.tmp"));
        fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("rename {}", path.display()))?;
        state.sizes.insert(digest.clone(), bytes.len() as u64);
        state.spooled_bytes = state.spooled_bytes.saturating_add(bytes.len() as u64);
        Ok(digest)
    }

    /// The spooled file holding the payload with SHA-256 `digest`, when one
    /// was spooled.
    pub fn path(&self, digest: &str) -> Option<PathBuf> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.path(digest)
    }

    /// Size of the spooled payload with SHA-256 `digest`.
    pub fn size(&self, digest: &str) -> Option<u64> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.sizes.get(digest).copied()
    }

    /// Where `att`'s bytes come from when its digest was spooled: the
    /// spooled file, with the payload's size for the progress totals.
    /// `None` for an attachment the spool does not hold.
    pub fn source(&self, att: &IrAttachment) -> Option<(AttachmentSource, Option<u64>)> {
        let digest = att.digest_sha256.as_deref()?;
        let path = self.path(digest)?;
        let size = att.size_bytes.or_else(|| self.size(digest));
        Some((AttachmentSource::Path(path), size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_crate_core::testutil::names_in;

    #[test]
    fn a_payload_is_on_disk_under_its_digest_until_the_spool_is_dropped() {
        let cache = tempfile::tempdir().unwrap();
        let spool = AttachmentSpool::new(cache.path());
        let digest = spool.put(b"hello").unwrap();
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(spool.put(b"hello").unwrap(), digest, "spooled once");
        let path = spool.path(&digest).unwrap();
        assert!(path.starts_with(cache.path().join(ATTACHMENT_SPOOL_DIRECTORY)));
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        assert_eq!(spool.size(&digest), Some(5));
        assert!(spool.path("0000").is_none());
        drop(spool);
        assert!(names_in(&cache.path().join(ATTACHMENT_SPOOL_DIRECTORY)).is_empty());
    }

    /// A spool that is never written to makes nothing on disk.
    #[test]
    fn an_unused_spool_makes_no_directory() {
        let cache = tempfile::tempdir().unwrap();
        drop(AttachmentSpool::new(cache.path()));
        assert!(names_in(cache.path()).is_empty());
    }

    /// What a killed run's spool left is deleted when the next spool
    /// writes, and a running run's spool is kept.
    #[test]
    fn a_new_spool_deletes_a_killed_run_s_and_keeps_a_running_one() {
        let cache = tempfile::tempdir().unwrap();
        let running = AttachmentSpool::new(cache.path());
        let kept = running.put(b"kept").unwrap();
        let killed = cache
            .path()
            .join(ATTACHMENT_SPOOL_DIRECTORY)
            .join("request-killed");
        fs::create_dir_all(&killed).unwrap();
        fs::write(killed.join(".lock"), b"").unwrap();
        fs::write(killed.join("stale"), b"x").unwrap();

        let spool = AttachmentSpool::new(cache.path());
        spool.put(b"new").unwrap();
        assert!(!killed.exists());
        assert!(running.path(&kept).unwrap().exists());
    }

    #[test]
    fn a_spooled_attachment_is_read_from_its_file() {
        let cache = tempfile::tempdir().unwrap();
        let spool = AttachmentSpool::new(cache.path());
        let digest = spool.put(b"photo").unwrap();
        let mut att = IrAttachment {
            path: None,
            original_name: Some("photo.jpg".into()),
            mime_type: Some("image/jpeg".into()),
            digest_sha256: Some(digest.clone()),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        };
        let (source, size) = spool.source(&att).unwrap();
        assert!(matches!(source, AttachmentSource::Path(p) if p == spool.path(&digest).unwrap()));
        assert_eq!(size, Some(5));
        att.digest_sha256 = Some("0000".into());
        assert!(spool.source(&att).is_none(), "not spooled");
    }
}
