//! The free-space check a run makes before it writes.
//!
//! A run writes to two places: the output directory (the attachments it
//! copies) and a scratch directory under the Scratch Directory (the
//! attachment spool, the databases `imessage-reader` decrypts). Most people
//! keep both on one disk, but the Scratch Directory can sit on another, so each
//! write is checked against the disk that holds it, before it starts. A
//! disk that fills part-way through fails a run with a bare write error,
//! often after hours; this check fails it first, with the space it needs.
//!
//! The Import form asks earlier, before any run starts, and with no size in
//! hand: [`low_disk_space`] names each of the two disks with less than
//! [`LOW_DISK_SPACE_BYTES`] free, so the person can free space first. It
//! never refuses anything.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
/// hold, with `needed` plus `DISK_HEADROOM_SLACK` as the requirement.
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

/// The error for a write into the Scratch Directory that found its disk
/// full, where no check could measure what it needed first. It opens as
/// [`check_headroom`]'s sentence for that disk does, so the person reads the
/// same words whichever way the disk ran short.
pub fn scratch_disk_full() -> anyhow::Error {
    anyhow::anyhow!(
        "{}: there was no room left to finish reading this backup. Free some space \
         on that disk and run the import again.",
        not_enough_space(Disk::Scratch)
    )
}

/// Below this much free space, the Import form says how little a disk has
/// left (#2010): 10 GB, in the decimal units [`media::format_bytes`] writes.
pub const LOW_DISK_SPACE_BYTES: u64 = 10_000_000_000;

/// One directory's free space, as [`low_disk_space`] read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskReading {
    /// Which directory this is.
    disk: Disk,
    /// The directory, as the person set it or the app made it.
    dir: PathBuf,
    /// The disk the directory is on, when that can be told, so two
    /// directories on one disk are read as one.
    disk_id: Option<String>,
    /// The directory the disk is mounted at, which names it on screen:
    /// `/mnt/data` on Unix, `C:\\` on Windows.
    mount: Option<PathBuf>,
    /// The bytes free on that disk.
    available: u64,
}

/// What the Import form says about the disks that hold the Staging
/// Directory and the Scratch Directory: one sentence for each disk with
/// less than [`LOW_DISK_SPACE_BYTES`] free, and nothing for one with more.
///
/// Two directories on one disk get one sentence naming both. A directory
/// that does not exist yet is read on the disk of its nearest parent that
/// does, where it will be made. A filesystem that cannot say how much is
/// free says nothing, as [`check_headroom`] does.
pub fn low_disk_space(staging: &Path, scratch: &Path) -> Vec<String> {
    let readings = [(Disk::Staging, staging), (Disk::Scratch, scratch)]
        .into_iter()
        .filter_map(|(disk, dir)| read_disk(disk, dir))
        .collect();
    low_disk_space_lines(readings)
}

/// The free space of the disk that holds `dir`, or `None` when the
/// filesystem cannot say.
fn read_disk(disk: Disk, dir: &Path) -> Option<DiskReading> {
    let existing = dir.ancestors().find(|ancestor| ancestor.is_dir())?;
    let available = fs2::available_space(existing).ok()?;
    let (disk_id, mount) = match disk_of(existing) {
        Some((id, mount)) => (Some(id), Some(mount)),
        None => (None, None),
    };
    Some(DiskReading {
        disk,
        dir: dir.to_path_buf(),
        disk_id,
        mount,
        available,
    })
}

/// The disk `dir` is on, and the directory it is mounted at: the device
/// number on Unix, and the highest parent of `dir` on that device.
#[cfg(unix)]
fn disk_of(dir: &Path) -> Option<(String, PathBuf)> {
    use std::os::unix::fs::MetadataExt;
    let device = |path: &Path| std::fs::metadata(path).ok().map(|meta| meta.dev());
    let dev = device(dir)?;
    let mount = dir
        .ancestors()
        .take_while(|ancestor| device(ancestor) == Some(dev))
        .last()?;
    Some((dev.to_string(), mount.to_path_buf()))
}

/// The disk `dir` is on, and the directory it is mounted at, on Windows: the
/// highest parent of `dir` whose disk has the same size and free space.
/// Windows tells the volume of a path through no API the standard library
/// has, and a volume mounted in a directory of `C:` shares its drive letter,
/// so the drive letter alone would read two disks as one. Two volumes with
/// the same size and the same free space to the byte are not met in
/// practice.
#[cfg(windows)]
fn disk_of(dir: &Path) -> Option<(String, PathBuf)> {
    let resolved = std::fs::canonicalize(dir).ok()?;
    let space = |path: &Path| {
        Some((
            fs2::total_space(path).ok()?,
            fs2::available_space(path).ok()?,
        ))
    };
    let own = space(&resolved)?;
    let mount = resolved
        .ancestors()
        .take_while(|ancestor| space(ancestor) == Some(own))
        .last()?;
    Some((mount.to_string_lossy().to_lowercase(), mount.to_path_buf()))
}

/// No disk can be told apart elsewhere, so each directory is its own.
#[cfg(not(any(unix, windows)))]
fn disk_of(_dir: &Path) -> Option<(String, PathBuf)> {
    None
}

/// The sentences for `readings`: the ones on one disk joined, then each
/// disk below [`LOW_DISK_SPACE_BYTES`] said once.
pub(crate) fn low_disk_space_lines(readings: Vec<DiskReading>) -> Vec<String> {
    let mut disks: Vec<Vec<DiskReading>> = Vec::new();
    for reading in readings {
        let shared = disks.iter_mut().find(|disk| {
            reading.disk_id.is_some()
                && disk.first().map(|first| &first.disk_id) == Some(&reading.disk_id)
        });
        match shared {
            Some(disk) => disk.push(reading),
            None => disks.push(vec![reading]),
        }
    }
    disks
        .into_iter()
        .filter_map(|disk| {
            let available = disk.iter().map(|reading| reading.available).min()?;
            if available >= LOW_DISK_SPACE_BYTES {
                return None;
            }
            let held = disk
                .iter()
                .map(|reading| {
                    format!(
                        "the {} ({})",
                        directory_name(reading.disk),
                        reading.dir.display()
                    )
                })
                .collect::<Vec<_>>()
                .join(" and ");
            let free = media::format_bytes(available);
            let on = match disk.first().and_then(|reading| reading.mount.as_ref()) {
                Some(mount) => format!("on {}, the disk that holds", mount.display()),
                None => "on the disk that holds".to_string(),
            };
            Some(format!(
                "{free} is free {on} {held}. A large backup can need more than that."
            ))
        })
        .collect()
}

/// The directory a [`Disk`] holds, by its name on screen.
fn directory_name(disk: Disk) -> &'static str {
    match disk {
        Disk::Staging => "Staging Directory",
        Disk::Scratch => "Scratch Directory",
    }
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

    /// A Scratch Directory disk that filled part-way is named the way a
    /// check names it.
    #[test]
    fn a_full_scratch_disk_is_named_as_a_short_one_is() {
        let full = scratch_disk_full().to_string();
        let short = headroom_shortfall(2 * 1024 * 1024 * 1024, 1024, Disk::Scratch).unwrap();
        let opening = |text: &str| text.split(':').next().unwrap().to_string();
        assert_eq!(opening(&full), opening(&short), "{full}");
    }

    /// A reading of `available` bytes free for `dir`, on the disk
    /// `disk_id` mounted at `/home` when it has one.
    fn reading(disk: Disk, dir: &str, disk_id: Option<&str>, available: u64) -> DiskReading {
        DiskReading {
            disk,
            dir: PathBuf::from(dir),
            disk_id: disk_id.map(str::to_string),
            mount: disk_id.map(|_| PathBuf::from("/home")),
            available,
        }
    }

    /// The form speaks about a disk below 10 GB free and not one at 10 GB.
    #[test]
    fn a_disk_below_ten_gb_free_is_named_and_one_at_ten_gb_is_not() {
        let short = low_disk_space_lines(vec![reading(
            Disk::Staging,
            "/home/a/message-crate",
            Some("1"),
            LOW_DISK_SPACE_BYTES - 1,
        )]);
        assert_eq!(short.len(), 1, "{short:?}");
        assert!(
            short[0].contains("the Staging Directory (/home/a/message-crate)"),
            "{short:?}"
        );
        let enough = low_disk_space_lines(vec![reading(
            Disk::Staging,
            "/home/a/message-crate",
            Some("1"),
            LOW_DISK_SPACE_BYTES,
        )]);
        assert!(enough.is_empty(), "{enough:?}");
    }

    /// The Staging and Scratch Directories on one disk get one sentence that
    /// names both, not the same free space twice.
    #[test]
    fn two_directories_on_one_disk_get_one_sentence() {
        let lines = low_disk_space_lines(vec![
            reading(
                Disk::Staging,
                "/home/a/message-crate",
                Some("1"),
                4_200_000_000,
            ),
            reading(
                Disk::Scratch,
                "/home/a/.local/share/mc/scratch",
                Some("1"),
                4_200_000_000,
            ),
        ]);
        assert_eq!(
            lines,
            vec![
                "4.2 GB is free on /home, the disk that holds the Staging Directory \
                 (/home/a/message-crate) and the Scratch Directory \
                 (/home/a/.local/share/mc/scratch). A large backup can need more than that."
                    .to_string()
            ]
        );
    }

    /// Directories on two disks are each read on their own: only the short
    /// one is named. Two disks that cannot be told apart are kept apart.
    #[test]
    fn directories_on_two_disks_are_named_apart() {
        let lines = low_disk_space_lines(vec![
            reading(
                Disk::Staging,
                "/mnt/big/staging",
                Some("1"),
                500_000_000_000,
            ),
            reading(Disk::Scratch, "/home/a/scratch", Some("2"), 2_000_000_000),
        ]);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("the Scratch Directory (/home/a/scratch)"),
            "{lines:?}"
        );
        assert!(!lines[0].contains("Staging"), "{lines:?}");

        let unknown = low_disk_space_lines(vec![
            reading(Disk::Staging, "/a", None, 1),
            reading(Disk::Scratch, "/b", None, 1),
        ]);
        assert_eq!(unknown.len(), 2, "{unknown:?}");
        assert!(
            unknown[0].starts_with("1 B is free on the disk that holds the Staging Directory"),
            "a disk with no mount point is not named: {unknown:?}"
        );
    }

    /// Two directories in one temporary directory are on one disk, and one
    /// not made yet is read where it will be made.
    #[test]
    fn a_directory_not_made_yet_is_read_on_its_parent_s_disk() {
        let dir = tempfile::tempdir().unwrap();
        let made = dir.path().join("scratch");
        std::fs::create_dir(&made).unwrap();
        let not_made = dir.path().join("message-crate").join("staging");

        let staging = read_disk(Disk::Staging, &not_made).unwrap();
        let scratch = read_disk(Disk::Scratch, &made).unwrap();

        assert_eq!(staging.dir, not_made);
        assert!(staging.disk_id.is_some());
        assert_eq!(staging.disk_id, scratch.disk_id);
        let mount = staging.mount.unwrap();
        assert!(dir.path().starts_with(&mount), "{mount:?}");
        assert_eq!(scratch.mount, Some(mount));
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
