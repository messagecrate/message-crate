//! The temporary file a fetched or copied Asset is written to before it is
//! renamed onto its path.

use std::path::Path;

use tempfile::NamedTempFile;

/// Create an empty file beside `dest` under a name no other file has, for
/// bytes that are renamed onto `dest` once they are whole.
///
/// The name is `.<file name of dest>.<random>.part`, made with `O_EXCL`, so
/// two Assets whose paths differ only in extension (`menu.pdf` and
/// `menu.jpg`) never write one file while several workers fetch at once, and
/// an Asset whose own path ends in `.part` is never written over. The file is
/// in `dest`'s directory, so the rename never crosses a file system. A
/// dropped [`NamedTempFile`] removes its file.
///
/// # Errors
///
/// Returns an error when the file cannot be created in `dest`'s directory.
pub(crate) fn part_file_beside(dest: &Path) -> std::io::Result<NamedTempFile> {
    let directory = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut prefix = std::ffi::OsString::from(".");
    if let Some(name) = dest.file_name() {
        prefix.push(name);
        prefix.push(".");
    }
    let mut builder = tempfile::Builder::new();
    builder.prefix(&prefix).suffix(".part");
    // tempfile makes its file readable by its owner alone, and the rename
    // keeps that mode. The Asset gets the mode any new file gets instead,
    // 0o666 less the umask, as `File::create` gives.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    builder.tempfile_in(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_paths_that_differ_only_in_extension_get_two_files() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = part_file_beside(&dir.path().join("menu.pdf")).unwrap();
        let jpg = part_file_beside(&dir.path().join("menu.jpg")).unwrap();

        assert_ne!(pdf.path(), jpg.path());
        assert_eq!(pdf.path().parent(), Some(dir.path()));
        let name = pdf.path().file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with(".menu.pdf.") && name.ends_with(".part"),
            "{name}"
        );
    }
}
