//! What an iPhone backup directory says about itself before anything is opened.

use std::{fs::File, path::Path};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

/// `Manifest.db` flag for a regular file. Directories and symbolic links have
/// rows too, and no bytes.
const FLAG_FILE: i64 = 1;

/// Whether `backup_root/Manifest.plist` is marked encrypted.
///
/// Returns `None` when the file is missing or cannot be parsed. That is
/// intentional: Import then leaves the password optional and the converter
/// still fails after start if the backup turns out to be encrypted.
pub fn ios_backup_encrypted_flag(backup_root: &Path) -> Option<bool> {
    let path = backup_root.join("Manifest.plist");
    let file = File::open(path).ok()?;
    let value = plist::Value::from_reader(file).ok()?;
    let dict = value.as_dictionary()?;
    match dict.get("IsEncrypted") {
        Some(plist::Value::Boolean(flag)) => Some(*flag),
        Some(_) => None,
        None => Some(false),
    }
}

/// Every regular file `Manifest.db` lists under `domain` in the iPhone
/// backup at `backup_root`, which is not encrypted, as its path inside the
/// domain and its size where the backup keeps it
/// (`<first two characters of its id>/<id>`), in path order. A file the
/// manifest lists and the backup does not hold has size 0.
///
/// This is what a program that extracts the domain writes, measured before
/// it starts, so the caller can check its disk for room.
///
/// # Errors
///
/// Returns an error when `Manifest.db` cannot be opened or read, as it
/// cannot for an encrypted backup, whose manifest only `imessage-reader`
/// can open.
pub fn ios_backup_domain_files(backup_root: &Path, domain: &str) -> Result<Vec<(String, u64)>> {
    let path = backup_root.join("Manifest.db");
    let manifest = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", path.display()))?;
    let mut statement = manifest
        .prepare(
            "SELECT fileID, relativePath FROM Files \
             WHERE domain = ?1 AND flags = ?2 ORDER BY relativePath",
        )
        .with_context(|| format!("read {}", path.display()))?;
    let rows = statement
        .query_map((domain, FLAG_FILE), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .with_context(|| format!("read {}", path.display()))?;
    Ok(rows
        .into_iter()
        .map(|(file_id, relative_path)| {
            let stored = backup_root
                .join(file_id.get(..2).unwrap_or_default())
                .join(&file_id);
            let size = std::fs::metadata(stored).map_or(0, |meta| meta.len());
            (relative_path, size)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{ios_backup_domain_files, ios_backup_encrypted_flag};
    use std::fs;

    fn write_plist(dir: &std::path::Path, is_encrypted: &str) {
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>IsEncrypted</key>
  <{is_encrypted}/>
</dict>
</plist>
"#
        );
        fs::write(dir.join("Manifest.plist"), body).unwrap();
    }

    #[test]
    fn encrypted_flag_none_when_manifest_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ios_backup_encrypted_flag(dir.path()), None);
    }

    #[test]
    fn encrypted_flag_none_when_manifest_is_garbage() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Manifest.plist"), b"not a plist").unwrap();
        assert_eq!(ios_backup_encrypted_flag(dir.path()), None);
    }

    #[test]
    fn encrypted_flag_reads_is_encrypted_boolean() {
        let encrypted = tempfile::tempdir().unwrap();
        write_plist(encrypted.path(), "true");
        assert_eq!(ios_backup_encrypted_flag(encrypted.path()), Some(true));

        let plain = tempfile::tempdir().unwrap();
        write_plist(plain.path(), "false");
        assert_eq!(ios_backup_encrypted_flag(plain.path()), Some(false));
    }

    /// A domain's regular files are listed with the size the backup keeps
    /// each at, in path order: another domain's file and a directory row are
    /// left out, and a file the backup lists without holding it is size 0.
    #[test]
    fn a_domain_s_files_are_listed_with_their_sizes_on_disk() {
        let backup = tempfile::tempdir().unwrap();
        let manifest = rusqlite::Connection::open(backup.path().join("Manifest.db")).unwrap();
        manifest
            .execute_batch(
                "CREATE TABLE Files (fileID TEXT, domain TEXT, relativePath TEXT, flags INTEGER);
                 INSERT INTO Files VALUES
                     ('aa11', 'Shared', 'Message/Media/photo.jpg', 1),
                     ('bb22', 'Shared', 'ChatStorage.sqlite', 1),
                     ('cc33', 'Shared', 'Message', 2),
                     ('dd44', 'Shared', 'Message/Media/gone.jpg', 1),
                     ('ee55', 'HomeDomain', 'Library/SMS/sms.db', 1);",
            )
            .unwrap();
        drop(manifest);
        for (id, bytes) in [("aa11", 700), ("bb22", 30), ("ee55", 9)] {
            let directory = backup.path().join(&id[..2]);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(id), vec![0u8; bytes]).unwrap();
        }

        assert_eq!(
            ios_backup_domain_files(backup.path(), "Shared").unwrap(),
            [
                ("ChatStorage.sqlite".to_string(), 30),
                ("Message/Media/gone.jpg".to_string(), 0),
                ("Message/Media/photo.jpg".to_string(), 700),
            ]
        );
    }

    #[test]
    fn a_backup_without_a_manifest_database_is_refused() {
        let backup = tempfile::tempdir().unwrap();
        assert!(ios_backup_domain_files(backup.path(), "Shared").is_err());
    }
}
