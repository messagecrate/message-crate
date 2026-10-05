//! Remove leftover files from a previous export in the same directory.

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Sentinel file written into export directories so `clean_previous_ir_output` can
/// distinguish a real export directory from a person's own directory that was
/// pointed at by mistake. It also lists, one per line, the files a merged
/// archive wrote into the directory ([`record_archive_files`]), which the next
/// clean removes.
pub const EXPORT_SENTINEL: &str = ".message-crate-export";

/// Whether `output_dir` holds the sentinel, which only an export writes.
pub(crate) fn has_export_sentinel(output_dir: &Path) -> bool {
    output_dir.join(EXPORT_SENTINEL).is_file()
}

/// Refuse `output_dir` unless it holds the sentinel. Every path in this crate
/// that removes files from an output directory reaches the sentinel check
/// before it removes anything: the mail clean and the placeholders call
/// this, [`clean_previous_ir_output`] marks a directory without one that is
/// empty apart from operating-system files and refuses any other, and
/// [`FormatSink::finish`] runs only on a sink
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
/// archive files listed. Outside tests, [`mark_export_directory`] calls it after
/// checking the directory is empty, and [`clean_previous_ir_output`] calls it
/// once it has removed the files the list named.
fn write_export_sentinel(output_dir: &Path) -> Result<()> {
    let path = output_dir.join(EXPORT_SENTINEL);
    fs::write(&path, "").with_context(|| format!("write {}", path.display()))
}

/// Files an operating system leaves in a directory the person opened, such as
/// Finder's `.DS_Store`. A directory that holds only these looks empty to the
/// person, so it counts as empty.
const OPERATING_SYSTEM_FILES: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

/// Mark `output_dir` as a directory an export wrote, unless it already is one.
/// An existing sentinel is left as it is, so the archive files it lists are
/// still removed by the next clean.
///
/// A directory without the sentinel file `.message-crate-export` is marked only
/// when it is empty, ignoring the files an operating system leaves behind.
/// Any other directory belongs to the person who chose it, whatever its files
/// are named, so it is refused and nothing in it is touched.
///
/// # Errors
///
/// Returns an error when the directory cannot be read, the sentinel cannot be
/// written, or the directory has no sentinel and is not empty.
pub fn mark_export_directory(output_dir: &Path) -> Result<()> {
    if has_export_sentinel(output_dir) {
        return Ok(());
    }
    for entry in read_dir(output_dir)? {
        let name = entry?.file_name();
        if !OPERATING_SYSTEM_FILES.contains(&name.to_str().unwrap_or("")) {
            bail!(
                "{} is not empty and Message Crate did not write it. Refusing to write into it. \
                 Choose an empty directory or one an earlier export wrote.",
                output_dir.display()
            );
        }
    }
    write_export_sentinel(output_dir)
}

/// Clean a directory an earlier export marked, or mark an empty one.
///
/// A directory that holds the sentinel loses the files a merged archive recorded
/// in it ([`record_archive_files`]) and its previous CSV, JSON, JSON Lines,
/// meta, temps, staged attachments, and mail archives, and keeps every other
/// file. The crate that owns an archive names its files, and this crate knows
/// none. A directory without the sentinel goes through [`mark_export_directory`],
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
        return mark_export_directory(output_dir);
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
/// not a plain file name in the directory is passed over, so a damaged sentinel
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
mod tests;
