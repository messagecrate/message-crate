//! Remove leftover files from a previous export in the same directory.

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Sentinel file written into export directories so `clean_previous_ir_output` can
/// distinguish a real export directory from a person's own folder that was
/// pointed at by mistake. It also lists, one per line, the files a merged
/// archive wrote into the folder ([`record_archive_files`]), which the next
/// clean removes.
pub const EXPORT_SENTINEL: &str = ".message-crate-export";

/// Whether `output_dir` holds the sentinel, which only an export writes.
pub(crate) fn has_export_sentinel(output_dir: &Path) -> bool {
    output_dir.join(EXPORT_SENTINEL).is_file()
}

/// Refuse `output_dir` unless it holds the sentinel. Every path in this crate
/// that removes files from an output directory reaches the sentinel check
/// before it removes anything: the mail clean and the placeholders call
/// this, [`clean_previous_ir_output`] marks an empty directory or refuses
/// any other without one, and [`FormatSink::finish`] runs only on a sink
/// [`FormatSink::open`] checked. So none of them can remove a person's own
/// files, whoever calls it.
///
/// [`FormatSink::finish`]: crate::FormatSink::finish
/// [`FormatSink::open`]: crate::FormatSink::open
///
/// # Errors
///
/// Returns an error when the directory has no sentinel.
pub(crate) fn require_export_directory(output_dir: &Path) -> Result<()> {
    if has_export_sentinel(output_dir) {
        return Ok(());
    }
    bail!(
        "{} has no {EXPORT_SENTINEL} file, so Message Crate did not write it. \
         Refusing to remove anything in it.",
        output_dir.display()
    )
}

/// Write an empty sentinel marking `output_dir` as an export target, with no
/// archive files listed. Outside tests, [`mark_export_folder`] calls it after
/// checking the folder is empty, and [`clean_previous_ir_output`] calls it
/// once it has removed the files the list named.
fn write_export_sentinel(output_dir: &Path) -> Result<()> {
    let path = output_dir.join(EXPORT_SENTINEL);
    fs::write(&path, "").with_context(|| format!("write {}", path.display()))
}

/// Files an operating system leaves in a folder the person opened, such as
/// Finder's `.DS_Store`. A folder that holds only these looks empty to the
/// person, so it counts as empty.
const OPERATING_SYSTEM_FILES: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

/// Mark `output_dir` as a folder an export wrote, unless it already is one.
/// An existing sentinel is left as it is, so the archive files it lists are
/// still removed by the next clean.
///
/// A folder without the sentinel file `.message-crate-export` is marked only
/// when it is empty, ignoring the files an operating system leaves behind.
/// Any other folder belongs to the person who chose it, whatever its files
/// are named, so it is refused and nothing in it is touched.
///
/// # Errors
///
/// Returns an error when the directory cannot be read, the sentinel cannot be
/// written, or the directory has no sentinel and is not empty.
pub fn mark_export_folder(output_dir: &Path) -> Result<()> {
    if has_export_sentinel(output_dir) {
        return Ok(());
    }
    for entry in read_dir(output_dir)? {
        let name = entry?.file_name();
        if !OPERATING_SYSTEM_FILES.contains(&name.to_str().unwrap_or("")) {
            bail!(
                "{} is not empty and Message Crate did not write it. Refusing to write into it. \
                 Choose an empty folder or one an earlier export wrote.",
                output_dir.display()
            );
        }
    }
    write_export_sentinel(output_dir)
}

/// Clean a folder an earlier export marked, or mark an empty one.
///
/// A folder that holds the sentinel loses the files a merged archive recorded
/// in it ([`record_archive_files`]) and its previous CSV, JSON, JSON Lines,
/// meta, temps, staged attachments, and mail archives, and keeps every other
/// file. The crate that owns an archive names its files, and this crate knows
/// none. A folder without the sentinel goes through [`mark_export_folder`],
/// so nothing is removed from it.
///
/// # Errors
///
/// Returns an error when the directory cannot be read, a file cannot be
/// removed, or the directory has no sentinel and is not empty.
pub fn clean_previous_ir_output(output_dir: &Path) -> Result<()> {
    if !output_dir.is_dir() {
        return Ok(());
    }
    if !has_export_sentinel(output_dir) {
        return mark_export_folder(output_dir);
    }
    for name in recorded_archive_files(output_dir)? {
        let path = output_dir.join(name);
        if path.is_file() {
            remove_previous(&path)?;
        }
    }
    for entry in read_dir(output_dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !path.is_file() {
            continue;
        }
        if is_export_artifact(name) {
            remove_previous(&path)?;
        }
    }
    // Drop staged attachments from previous runs. Files named by a SHA-256
    // fingerprint of their bytes would otherwise pile up when a new run does
    // not reuse them. Media transforms also reprocess every file under
    // attachments/, so leftover files can fail a later run. Callers copy the
    // attachments they need after this function.
    let attachments = output_dir.join("attachments");
    if attachments.is_dir() {
        fs::remove_dir_all(&attachments)
            .with_context(|| format!("remove previous {}", attachments.display()))?;
    } else if attachments.is_file() {
        remove_previous(&attachments)?;
    }
    clean_previous_mail_output(output_dir)?;
    // The archive files the sentinel listed are gone, so it starts with no list.
    write_export_sentinel(output_dir)
}

/// Remove the mail archives an earlier export left in `output_dir`: `.mbox`
/// files, and directories that hold an `.eml` file. Leaves `attachments/`
/// alone. Reached only through [`clean_previous_ir_output`], and refuses a
/// directory without the sentinel itself as well.
///
/// # Errors
///
/// Returns an error when the directory has no sentinel, a directory cannot
/// be read, or a file cannot be removed.
fn clean_previous_mail_output(output_dir: &Path) -> Result<()> {
    require_export_directory(output_dir)?;
    for entry in read_dir(output_dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mbox"))
        {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
            continue;
        }
        if path.is_dir() && name != "attachments" {
            let entries = read_dir(&path)?;
            if holds_eml(&path, entries.map(|entry| entry.map(|e| e.path())))? {
                fs::remove_dir_all(&path).with_context(|| format!("remove {}", path.display()))?;
            }
        }
    }
    Ok(())
}

/// Whether `entries`, the paths of `dir`, include an `.eml` file.
///
/// An entry that cannot be read fails the check, with `dir` named. Skipping
/// it could make a directory of an earlier export read as holding no `.eml`,
/// and that directory would then stay beside the new export. The server's
/// `import` command and the Upload fail the same way.
///
/// # Errors
///
/// Returns an error for the first entry that cannot be read before an `.eml`
/// is found.
fn holds_eml(
    dir: &Path,
    entries: impl IntoIterator<Item = std::io::Result<PathBuf>>,
) -> Result<bool> {
    for entry in entries {
        let path = entry.with_context(|| format!("read an entry of {}", dir.display()))?;
        if path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("eml"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_dir(dir: &Path) -> Result<fs::ReadDir> {
    fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))
}

/// Remove one file a previous export left.
fn remove_previous(path: &Path) -> Result<()> {
    fs::remove_file(path).with_context(|| format!("remove previous {}", path.display()))
}

/// Record in the sentinel of `output_dir` the names of the files a merged
/// archive is about to write there, so the next fresh export removes them.
/// Recording before the write covers the partial files of a run that stops.
///
/// # Errors
///
/// Returns an error when the sentinel cannot be written.
pub(crate) fn record_archive_files(output_dir: &Path, names: &[String]) -> Result<()> {
    let path = output_dir.join(EXPORT_SENTINEL);
    let mut sentinel = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    for name in names {
        writeln!(sentinel, "{name}").with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

/// The file names recorded in the sentinel of `output_dir`. A line that is
/// not a plain file name in the folder is passed over, so a damaged sentinel
/// cannot remove anything outside it.
fn recorded_archive_files(output_dir: &Path) -> Result<Vec<String>> {
    let path = output_dir.join(EXPORT_SENTINEL);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|name| {
            !name.is_empty()
                && !name.contains(['/', '\\'])
                && *name != "."
                && *name != ".."
                && *name != EXPORT_SENTINEL
        })
        .map(String::from)
        .collect())
}

/// Returns true when `name` matches a known export artifact pattern.
/// `.json` and `.json.tmp` cover the `.meta.json` sidecars as well.
fn is_export_artifact(name: &str) -> bool {
    name.ends_with(".csv")
        || name.ends_with(".csv.tmp")
        || name.ends_with(".json")
        || name.ends_with(".json.tmp")
        || name.ends_with(".jsonl")
        || name.ends_with(".jsonl.tmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn refuses_a_folder_of_the_persons_own_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("notes.txt"), "mine").unwrap();

        let err = clean_previous_ir_output(tmp.path()).unwrap_err();

        assert!(
            err.to_string().contains("Refusing to write into it"),
            "{err}"
        );
        assert_eq!(names(tmp.path()), ["notes.txt"]);
    }

    #[test]
    fn removes_only_export_files_from_a_marked_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_export_sentinel(dir).unwrap();
        for name in [
            "a.jsonl",
            "b.csv",
            "c.json",
            "c.meta.json",
            "d.jsonl.tmp",
            "notes.txt",
        ] {
            fs::write(dir.join(name), "x").unwrap();
        }
        fs::create_dir(dir.join("attachments")).unwrap();
        fs::write(dir.join("attachments").join("a.jpg"), "x").unwrap();
        // A folder named like an export file is not an export file.
        fs::create_dir(dir.join("kept.json")).unwrap();

        clean_previous_ir_output(dir).unwrap();

        assert_eq!(names(dir), [EXPORT_SENTINEL, "kept.json", "notes.txt"]);
    }

    #[test]
    fn cleans_a_marked_folder_that_holds_no_export_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_export_sentinel(tmp.path()).unwrap();
        fs::write(tmp.path().join("notes.txt"), "mine").unwrap();

        clean_previous_ir_output(tmp.path()).unwrap();

        assert_eq!(names(tmp.path()), [EXPORT_SENTINEL, "notes.txt"]);
    }

    #[test]
    fn refuses_an_unmarked_folder_that_holds_export_like_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("budget.csv"), "mine").unwrap();
        fs::write(tmp.path().join("settings.json"), "{}").unwrap();
        fs::write(tmp.path().join("notes.txt"), "mine").unwrap();
        fs::create_dir_all(tmp.path().join("attachments")).unwrap();
        fs::write(tmp.path().join("attachments/holiday.jpg"), "mine").unwrap();

        let err = clean_previous_ir_output(tmp.path()).unwrap_err();

        assert!(
            err.to_string().contains(&tmp.path().display().to_string()),
            "{err}"
        );
        assert_eq!(
            names(tmp.path()),
            ["attachments", "budget.csv", "notes.txt", "settings.json"]
        );
        assert!(tmp.path().join("attachments/holiday.jpg").exists());
    }

    /// Every exporter cleans through here, so the files a merged archive
    /// recorded go whatever the next run writes, and an XML file nothing
    /// recorded, such as a person's own backup, stays.
    #[test]
    fn removes_the_files_an_archive_recorded_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("export");
        fs::create_dir(&dir).unwrap();
        let dir = dir.as_path();
        write_export_sentinel(dir).unwrap();
        let recorded = ["archive.xml", "archive.xml.tmp"].map(String::from);
        record_archive_files(dir, &recorded).unwrap();
        // A damaged list cannot reach outside the folder.
        record_archive_files(dir, &["../outside.xml".to_string()]).unwrap();
        let outside = tmp.path().join("outside.xml");
        fs::write(&outside, "mine").unwrap();
        for name in ["archive.xml", "archive.xml.tmp", "sms-20261001.xml"] {
            fs::write(dir.join(name), "x").unwrap();
        }

        clean_previous_ir_output(dir).unwrap();

        assert_eq!(names(dir), [EXPORT_SENTINEL, "sms-20261001.xml"]);
        assert!(outside.is_file(), "a file outside the folder is kept");
        assert_eq!(fs::read_to_string(dir.join(EXPORT_SENTINEL)).unwrap(), "");
    }

    /// Pull and WhatsApp mark a folder without cleaning it, so marking it
    /// again keeps the archive files an earlier export listed for the next
    /// clean.
    #[test]
    fn marking_a_folder_again_keeps_the_archive_files_it_lists() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_export_sentinel(dir).unwrap();
        record_archive_files(dir, &["archive.xml".to_string()]).unwrap();
        fs::write(dir.join("archive.xml"), "x").unwrap();

        mark_export_folder(dir).unwrap();
        clean_previous_ir_output(dir).unwrap();

        assert_eq!(names(dir), [EXPORT_SENTINEL]);
    }

    #[test]
    fn marks_an_empty_folder() {
        let tmp = tempfile::tempdir().unwrap();

        clean_previous_ir_output(tmp.path()).unwrap();

        assert_eq!(names(tmp.path()), [EXPORT_SENTINEL]);
    }

    #[test]
    fn export_artifacts_are_recognised_by_name() {
        for name in [
            "a.csv",
            "a.csv.tmp",
            "a.meta.json",
            "a.meta.json.tmp",
            "a.json",
            "a.json.tmp",
            "a.jsonl",
            "a.jsonl.tmp",
        ] {
            assert!(is_export_artifact(name), "{name} is an export file");
        }
        for name in ["notes.txt", "photo.jpg", "other.xml", "a.csv.bak", ""] {
            assert!(!is_export_artifact(name), "{name} is not an export file");
        }
    }

    #[test]
    fn marks_a_folder_that_holds_only_operating_system_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".DS_Store"), "finder").unwrap();

        clean_previous_ir_output(tmp.path()).unwrap();

        assert_eq!(names(tmp.path()), [".DS_Store", EXPORT_SENTINEL]);
    }

    /// The clean of earlier mail archives is reached only through
    /// [`clean_previous_ir_output`], and refuses a directory without the
    /// sentinel itself as well, so `.mbox` files and directories of `.eml`
    /// files a person keeps there stay (#1531).
    #[test]
    fn the_mail_clean_refuses_a_directory_without_the_sentinel_and_removes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("+15555550101.mbox"), "From x\n").unwrap();
        fs::create_dir(dir.join("+15555550102")).unwrap();
        fs::write(dir.join("+15555550102/0001.eml"), "Subject: x\n").unwrap();

        assert!(clean_previous_mail_output(dir).is_err());

        assert_eq!(names(dir), ["+15555550101.mbox", "+15555550102"]);
        assert!(dir.join("+15555550102/0001.eml").is_file());
    }

    #[test]
    fn the_mail_clean_removes_only_mail_archives() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_export_sentinel(dir).unwrap();
        fs::write(dir.join("+15555550101.mbox"), "From x\n").unwrap();
        fs::write(dir.join("Old.MBOX"), "From x\n").unwrap();
        fs::create_dir(dir.join("+15555550102")).unwrap();
        fs::write(dir.join("+15555550102/0001.eml"), "Subject: x\n").unwrap();
        // An email kept as an attachment is not a previous export.
        fs::create_dir(dir.join("attachments")).unwrap();
        fs::write(dir.join("attachments/forwarded.eml"), "Subject: x\n").unwrap();
        fs::create_dir(dir.join("photos")).unwrap();
        fs::write(dir.join("photos/a.jpg"), "jpg").unwrap();
        fs::write(dir.join("notes.txt"), "mine").unwrap();

        clean_previous_mail_output(dir).unwrap();

        assert_eq!(
            names(dir),
            [EXPORT_SENTINEL, "attachments", "notes.txt", "photos"]
        );
        assert!(dir.join("attachments/forwarded.eml").is_file());
        assert!(dir.join("photos/a.jpg").is_file());
    }

    /// An entry of a subdirectory that cannot be read fails the clean-up with
    /// the directory named, rather than reading the directory as holding no
    /// `.eml` and leaving an earlier export's directory beside the new one
    /// (#1563).
    #[test]
    fn an_entry_that_cannot_be_read_fails_the_eml_check_and_names_the_directory() {
        let dir = Path::new("/exports/+15555550102");
        let entries = vec![
            Ok(dir.join("notes.txt")),
            Err(std::io::Error::other("stale file handle")),
            Ok(dir.join("0001.eml")),
        ];

        let error = holds_eml(dir, entries).unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("/exports/+15555550102"), "{message}");
        assert!(message.contains("stale file handle"), "{message}");
    }

    /// Run `f` with the permissions of `dir` set to `mode`, then set them
    /// back to `0o755` before returning, so the temporary directory can still
    /// be removed.
    ///
    /// `None`, with a line on stderr, when `dir` can still be listed under
    /// `mode`: a user such as root cannot exercise the failure, so the test
    /// has nothing to check.
    #[cfg(unix)]
    fn with_directory_mode<T>(dir: &Path, mode: u32, f: impl FnOnce() -> T) -> Option<T> {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(dir, fs::Permissions::from_mode(mode))
            .expect("set the directory's mode");
        let result = if fs::read_dir(dir).is_ok() {
            eprintln!(
                "skipped: {} can still be listed with mode {mode:o}",
                dir.display()
            );
            None
        } else {
            Some(f())
        };
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755))
            .expect("restore the directory's mode");
        result
    }

    /// A subdirectory the mail clean cannot list fails it with the
    /// subdirectory named.
    #[cfg(unix)]
    #[test]
    fn a_subdirectory_that_cannot_be_read_fails_the_mail_clean_and_names_it() {
        let tmp = tempfile::tempdir().unwrap();
        write_export_sentinel(tmp.path()).unwrap();
        let sub = tmp.path().join("+15555550102");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("0001.eml"), "Subject: x\n").unwrap();
        let Some(result) =
            with_directory_mode(&sub, 0o000, || clean_previous_mail_output(tmp.path()))
        else {
            return;
        };

        let message = format!("{:#}", result.unwrap_err());
        assert!(message.contains("+15555550102"), "{message}");
        assert!(sub.join("0001.eml").is_file());
    }
}
