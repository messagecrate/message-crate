//! An iPhone backup that is not encrypted, small and made up, for the tests
//! here and in `whatsapp-exporter`.

use std::{fs, path::Path};

use rusqlite::Connection;

/// One file of a made-up backup: its domain, its path inside the domain,
/// and its size in bytes. `None` lists the file in the manifest and leaves
/// it out of the backup.
pub type ManifestFile<'a> = (&'a str, &'a str, Option<usize>);

/// Write a plain `Manifest.db` listing `files` as regular files into `dir`,
/// and each file the backup holds where an iPhone keeps it
/// (`<first two characters of its id>/<id>`), filled with zeros.
///
/// # Panics
///
/// Panics when the database or a file cannot be written.
pub fn write_unencrypted_manifest(dir: &Path, files: &[ManifestFile<'_>]) {
    let manifest = Connection::open(dir.join("Manifest.db")).unwrap();
    manifest
        .execute_batch(
            "CREATE TABLE Files (fileID TEXT, domain TEXT, relativePath TEXT, flags INTEGER);",
        )
        .unwrap();
    for (index, (domain, path, size)) in files.iter().enumerate() {
        let id = format!("{index:040x}");
        manifest
            .execute(
                "INSERT INTO Files VALUES (?1, ?2, ?3, 1)",
                (&id, domain, path),
            )
            .unwrap();
        if let Some(size) = size {
            let stored = dir.join(&id[..2]);
            fs::create_dir_all(&stored).unwrap();
            fs::write(stored.join(&id), vec![0u8; *size]).unwrap();
        }
    }
}
