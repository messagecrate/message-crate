//! The free-space check a run makes before it writes.
//!
//! A run writes to two places: the output directory (the attachments it
//! copies) and a scratch directory under the Scratch Directory (the
//! attachment spool, the databases `imessage-reader` decrypts). Most people
//! keep both on one disk, but the Scratch Directory can sit on another, so each
//! write is checked against the disk that holds it, before it starts. A
//! disk that fills part-way through fails a run with a bare write error,
//! often after hours; this check fails it first, with the space it needs.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use message_crate_core::OutputFormat;

/// Slack above the measured need, for the derivative a convert holds in
/// flight and for whatever else shares the disk.
pub(crate) const DISK_HEADROOM_SLACK: u64 = 64 * 1024 * 1024;

/// Which disk a check is about, for the sentence that names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disk {
    /// The disk that holds the output directory.
    Staging,
    /// The disk that holds the Scratch Directory, where a run's scratch
    /// data goes.
    Scratch,
}

/// The bytes a copy of `attachments` writes: each one's size, given with
/// its SHA-256 when it is known. Staging writes one file per SHA-256, so an
/// attachment that appears in many messages is counted once; one with no
/// digest yet is counted every time, which can only over-ask.
pub fn bytes_to_copy<'a>(attachments: impl IntoIterator<Item = (Option<&'a str>, u64)>) -> u64 {
    let mut seen = HashSet::new();
    attachments
        .into_iter()
        .filter(|(digest, _)| digest.is_none_or(|digest| seen.insert(digest)))
        .fold(0, |total, (_, size)| total.saturating_add(size))
}

/// The bytes a file holds when `attachments` are embedded in it, as a mail
/// archive or a merged archive does: every message carries its own copy,
/// base64-encoded, so each occurrence counts, at four thirds of its size.
pub fn bytes_embedded<'a>(attachments: impl IntoIterator<Item = (Option<&'a str>, u64)>) -> u64 {
    let raw = attachments
        .into_iter()
        .fold(0u64, |total, (_, size)| total.saturating_add(size));
    raw.saturating_add(raw / 3)
}

/// The bytes a run writing `format` puts in the output for `attachments`:
/// the staged copy under `attachments/`, plus, for a format that embeds
/// them (EML, MBOX, and the merged archives), every embedded copy. The
/// staged copy of an embedding format is deleted only after the archive is
/// written, so both are on the disk at once.
pub fn bytes_to_write(format: OutputFormat, attachments: &[(Option<&str>, u64)]) -> u64 {
    let staged = bytes_to_copy(attachments.iter().copied());
    if embeds_attachments(format) {
        staged.saturating_add(bytes_embedded(attachments.iter().copied()))
    } else {
        staged
    }
}

/// Whether `format` writes attachment bytes into its files.
pub fn embeds_attachments(format: OutputFormat) -> bool {
    matches!(
        format,
        OutputFormat::Eml | OutputFormat::Mbox | OutputFormat::Xml | OutputFormat::SmsBackupPlus
    )
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
    let short = not_enough_space(disk);
    Some(match disk {
        Disk::Staging => {
            format!("{short}: this backup needs about {required}, and {available} is free.")
        }
        Disk::Scratch => format!(
            "{short}: reading this backup needs about {required} more there, and \
             {available} is free."
        ),
    })
}

/// The error for a write that ran out of room part-way on `disk`, where no
/// check could measure the need first: wtsexporter decrypting an Android
/// WhatsApp backup, whose decrypted size nothing knows until it is written.
/// It opens as [`check_headroom`]'s sentence does, so the person reads the
/// same words whichever way the disk ran short.
pub fn disk_full(disk: Disk) -> anyhow::Error {
    anyhow::anyhow!(
        "{}: reading this backup filled it before it finished. Free some space there \
         and run the import again.",
        not_enough_space(disk)
    )
}

/// The opening of every sentence about a short disk, naming which one.
fn not_enough_space(disk: Disk) -> &'static str {
    match disk {
        Disk::Staging => "Not enough space on the staging disk",
        Disk::Scratch => "Not enough space on the disk that holds the Scratch Directory",
    }
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

    /// One payload in many messages is written once, so it is counted once.
    #[test]
    fn bytes_to_copy_counts_each_digest_once() {
        let items = [
            (Some("a"), 5),
            (Some("a"), 5),
            (Some("b"), 7),
            (None, 3),
            (None, 3),
        ];
        assert_eq!(bytes_to_copy(items), 5 + 7 + 3 + 3);
    }

    /// A format that embeds its attachments writes every occurrence, base64,
    /// beside the staged copy; one that keeps files writes each digest once.
    #[test]
    fn bytes_to_write_counts_every_embedded_copy() {
        let items = [(Some("a"), 30), (Some("a"), 30)];
        assert_eq!(bytes_to_write(OutputFormat::Csv, &items), 30);
        assert_eq!(bytes_to_write(OutputFormat::Mbox, &items), 30 + 80);
        assert_eq!(bytes_to_write(OutputFormat::Xml, &items), 30 + 80);
    }

    /// A short scratch disk is named as the Scratch Directory's, not the staging
    /// disk, so the person knows which disk to clear.
    #[test]
    fn a_short_scratch_disk_is_named_as_the_scratch_directory_s() {
        let msg = headroom_shortfall(2 * 1024 * 1024 * 1024, 1024, Disk::Scratch).unwrap();
        assert!(
            msg.starts_with("Not enough space on the disk that holds the Scratch Directory"),
            "{msg}"
        );
    }

    /// A disk that filled part-way is named the way a check names it.
    #[test]
    fn a_full_disk_is_named_as_a_short_one_is() {
        for disk in [Disk::Staging, Disk::Scratch] {
            let full = disk_full(disk).to_string();
            let short = headroom_shortfall(2 * 1024 * 1024 * 1024, 1024, disk).unwrap();
            let opening = |text: &str| text.split(':').next().unwrap().to_string();
            assert_eq!(opening(&full), opening(&short), "{full}");
        }
    }

    /// The check reads the disk that holds the directory it is given.
    #[test]
    fn check_headroom_passes_a_disk_with_room_and_refuses_one_without() {
        let dir = tempfile::tempdir().unwrap();
        check_headroom(dir.path(), 0, Disk::Scratch).unwrap();
        let err = check_headroom(dir.path(), u64::MAX / 2, Disk::Scratch).unwrap_err();
        assert!(err.to_string().contains("Scratch Directory"), "{err}");
    }
}
