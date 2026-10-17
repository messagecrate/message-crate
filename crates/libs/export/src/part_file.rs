//! Writing a fetched or copied Asset through a temporary file beside its path.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// The temporary file for the Asset whose SHA-256 is `sha256`, on its way to
/// `dest`: `.<sha256>.part` in `dest`'s directory.
///
/// The name comes from the Asset, not from `dest`, for three reasons. Two
/// Assets whose paths differ only in extension (`menu.pdf` and `menu.jpg`)
/// are two fingerprints, so the workers that fetch them at once never share a
/// file. The name is 70 bytes whatever `dest` is called, so an Asset whose
/// own name is near the 255-byte limit of a file name still has room for
/// one. And a later Export Run into the same directory fetches the Asset to
/// the same name again, so a file a crash left behind is written over and
/// renamed away rather than kept for good. One Export Run fetches each
/// fingerprint once, and copies it to its other paths only after every
/// fetch has finished, one at a time, so nothing else writes this name while
/// a write to it runs. That holds because of `run.rs`: `note_asset_refs`
/// keys the Assets by their lowercased fingerprint, `fetch_assets_parallel`
/// makes one job per key, and `place_other_paths` runs after it returns. The
/// directory is `dest`'s, so the rename never crosses a file system.
fn part_path(dest: &Path, sha256: &str) -> PathBuf {
    dest.with_file_name(format!(".{sha256}.part"))
}

/// Write the Asset whose SHA-256 is `sha256` to `dest` through its temporary
/// file ([`part_path`]), with [`file_io::write_atomic_via`]: `write` fills
/// the temporary file, which is synced and renamed onto `dest` only when
/// `write` succeeds, and removed when it fails. A synced rename matters,
/// because the export journal records the Asset next, and a later Export Run
/// skips an Asset the journal names whose file exists.
///
/// # Errors
///
/// Returns the error `write` returns, unchanged, or an error when the
/// directory or the temporary file cannot be created, or the sync or the
/// rename fails.
pub(crate) fn write_asset(
    dest: &Path,
    sha256: &str,
    write: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<()> {
    file_io::write_atomic_via(&part_path(dest, sha256), dest, write)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_asset_with_a_name_of_250_bytes_is_written() {
        // A name of the whole file name plus a suffix ran past the 255-byte
        // limit of ext4, APFS and NTFS, and every retry failed the same way.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join(format!("{}.pdf", "m".repeat(246)));

        write_asset(&dest, &"ab".repeat(32), |out| Ok(out.write_all(b"menu")?)).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"menu");
    }

    #[test]
    fn a_file_a_crash_left_is_written_over_by_the_next_fetch() {
        // A random name was never reused, so every interrupted Export left
        // its partial file behind for good.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("menu.pdf");
        let sha256 = "ab".repeat(32);
        std::fs::write(part_path(&dest, &sha256), b"half of a longer m").unwrap();

        write_asset(&dest, &sha256, |out| Ok(out.write_all(b"menu")?)).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"menu");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["menu.pdf"]);
    }
}
