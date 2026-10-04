//! The free-space check a run makes before it writes.
//!
//! A run writes to two places: the output folder (the attachments it
//! copies) and a scratch folder under the app's cache folder (the
//! attachment spool, the databases `imessage-reader` decrypts). Most people
//! keep both on one disk, but the cache folder can sit on another, so each
//! write is checked against the disk that holds it, before it starts. A
//! disk that fills part-way through fails a run with a bare write error,
//! often after hours; this check fails it first, with the space it needs.

use std::path::Path;

use anyhow::Result;

/// Slack above the measured need, for the derivative a convert holds in
/// flight and for whatever else shares the disk.
pub const DISK_HEADROOM_SLACK: u64 = 64 * 1024 * 1024;

/// Which disk a check is about, for the sentence that names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disk {
    /// The disk that holds the output folder.
    Staging,
    /// The disk that holds the app's cache folder, where a run's scratch
    /// data goes.
    Cache,
}

/// Refuse a write of `needed` bytes into `dir` that its disk plainly cannot
/// hold, with `needed` plus [`DISK_HEADROOM_SLACK`] as the requirement.
///
/// A filesystem that cannot say how much is free never blocks a run.
///
/// # Errors
///
/// Returns the sentence that names the space needed and the space free when
/// the disk is short.
pub fn check_headroom(dir: &Path, needed: u64, disk: Disk) -> Result<()> {
    let Ok(available) = fs2::available_space(dir) else {
        return Ok(());
    };
    match headroom_shortfall(needed, available, disk) {
        Some(message) => anyhow::bail!(message),
        None => Ok(()),
    }
}

/// `None` when `available` covers `needed` plus slack; otherwise what to say.
pub(crate) fn headroom_shortfall(needed: u64, available: u64, disk: Disk) -> Option<String> {
    let required = needed.saturating_add(DISK_HEADROOM_SLACK);
    if available >= required {
        return None;
    }
    let required = media::format_bytes(required);
    let available = media::format_bytes(available);
    Some(match disk {
        Disk::Staging => format!(
            "Not enough space on the staging disk: this backup needs about {required}, \
             and {available} is free."
        ),
        Disk::Cache => format!(
            "Not enough space on the disk that holds the app's cache folder: reading this \
             backup needs about {required} more there, and {available} is free."
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headroom_shortfall_speaks_when_space_is_short() {
        assert!(headroom_shortfall(10 * 1024 * 1024 * 1024, 1024, Disk::Staging).is_some());
        assert_eq!(
            headroom_shortfall(1024, 10 * 1024 * 1024 * 1024, Disk::Staging),
            None
        );
        let msg = headroom_shortfall(2 * 1024 * 1024 * 1024, 1024, Disk::Staging).unwrap();
        assert!(msg.contains("free"), "{msg}");
        assert!(msg.contains("GB"), "{msg}");
    }

    /// A short cache disk is named as the cache folder's, not the staging
    /// disk, so the person knows which disk to clear.
    #[test]
    fn a_short_cache_disk_is_named_as_the_cache_folder_s() {
        let msg = headroom_shortfall(2 * 1024 * 1024 * 1024, 1024, Disk::Cache).unwrap();
        assert!(
            msg.starts_with("Not enough space on the disk that holds the app's cache folder"),
            "{msg}"
        );
    }

    /// The check reads the disk that holds the folder it is given.
    #[test]
    fn check_headroom_passes_a_disk_with_room_and_refuses_one_without() {
        let dir = tempfile::tempdir().unwrap();
        check_headroom(dir.path(), 0, Disk::Cache).unwrap();
        let err = check_headroom(dir.path(), u64::MAX / 2, Disk::Cache).unwrap_err();
        assert!(err.to_string().contains("cache folder"), "{err}");
    }
}
