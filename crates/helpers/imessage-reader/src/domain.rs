//! Decrypt every file of one domain of an encrypted iPhone backup into a
//! directory the app owns.
//!
//! WhatsApp keeps its data in an app-group domain of the backup. The program
//! that reads WhatsApp (wtsexporter) asks for a backup password on a
//! terminal and takes it no other way, so the app has this helper write the
//! domain out in the clear first and points wtsexporter at the result. The
//! layout is the one wtsexporter extracts for itself:
//! `<out_dir>/<domain>/<path inside the domain>`.

use std::{
    fs::{File, create_dir_all},
    io::{BufWriter, Write, copy},
    path::{Component, Path, PathBuf},
};

use crabapple::Backup;
use imessage_reader_protocol::{BackupDomainRequest, Event, Platform, Progress, Source};
use rusqlite::Connection;

use crate::{
    backup::{decrypt_backup, restrict_permissions},
    error::{RuntimeError, UNENCRYPTED_BACKUP_CLEAR_PASSWORD},
    log::{emit, emit_log},
    options::ReaderOptions,
};

/// How many progress events a domain's files are reported in, at most.
const PROGRESS_EVENTS: u64 = 200;

/// `Manifest.db` flag for a regular file. Directories and symbolic links have
/// rows too, and no bytes.
const FLAG_FILE: i64 = 1;

/// One regular file of a domain, as `Manifest.db` lists it.
pub(crate) struct DomainFile {
    /// The backup's id for the file.
    file_id: String,
    /// The file's path inside the domain.
    relative_path: String,
    /// The size the manifest records, which is the size decrypting writes.
    size: u64,
}

/// What one run wrote.
pub(crate) struct Written {
    /// Files decrypted into the directory.
    pub files: u64,
    /// Files the manifest lists that could not be decrypted.
    pub failures: u64,
}

/// Open the encrypted backup `request` names.
///
/// # Errors
///
/// Returns an error when the directory is not an iPhone backup, the backup is
/// not encrypted, or the password is wrong.
pub(crate) fn open(request: &BackupDomainRequest) -> Result<Backup, RuntimeError> {
    // Opening the backup writes nothing; the directory the app named is the
    // only one this request may write to, so it is the scratch directory too.
    let options = ReaderOptions::from_source(
        Source {
            db_path: request.backup_path.clone(),
            platform: Platform::Ios,
            backup_password: Some(request.backup_password.clone()),
        },
        request.out_dir.clone(),
    );
    decrypt_backup(&options)?
        .ok_or_else(|| RuntimeError::InvalidOptions(UNENCRYPTED_BACKUP_CLEAR_PASSWORD.to_string()))
}

/// Every regular file of `request.domain`, with the size the manifest
/// records for it, so the app can check its disk before anything is
/// written. A file whose metadata cannot be read counts for nothing here;
/// decrypting it fails the same way and is counted then.
///
/// # Errors
///
/// Returns an error when the manifest cannot be read.
pub(crate) fn list_domain(
    backup: &Backup,
    request: &BackupDomainRequest,
) -> Result<Vec<DomainFile>, RuntimeError> {
    let manifest = Connection::open(backup.manifest_db_path())?;
    Ok(domain_files(&manifest, &request.domain)?
        .into_iter()
        .map(|(file_id, relative_path)| {
            let size = backup
                .get_file(&file_id)
                .map_or(0, |entry| entry.metadata.size);
            DomainFile {
                file_id,
                relative_path,
                size,
            }
        })
        .collect())
}

/// The bytes decrypting `files` writes.
pub(crate) fn total_bytes(files: &[DomainFile]) -> u64 {
    files
        .iter()
        .fold(0, |total, file| total.saturating_add(file.size))
}

/// Decrypt `files`, the domain's files as [`list_domain`] listed them, into
/// `request.out_dir`.
///
/// A file that cannot be decrypted is reported on the log and counted; the
/// rest are still written. A backup routinely lists a file whose bytes it
/// does not hold, and one missing photo must not cost the conversations.
///
/// # Errors
///
/// Returns an error when the domain is not a plain directory name.
pub(crate) fn decrypt_domain(
    backup: &Backup,
    request: &BackupDomainRequest,
    files: &[DomainFile],
) -> Result<Written, RuntimeError> {
    let total = files.len() as u64;
    let every = (total / PROGRESS_EVENTS).max(1);
    emit_log(format!("Decrypting {total} files from the backup..."));

    let mut written = Written {
        files: 0,
        failures: 0,
    };
    for (index, file) in files.iter().enumerate() {
        let relative_path = &file.relative_path;
        let Some(target) = target_path(&request.out_dir, &request.domain, relative_path) else {
            return Err(RuntimeError::InvalidOptions(format!(
                "the backup names a file outside its own directory: {relative_path}"
            )));
        };
        match decrypt_to(backup, &file.file_id, &target) {
            Ok(()) => written.files += 1,
            Err(why) => {
                written.failures += 1;
                emit_log(format!(
                    "Backup file {relative_path} could not be decrypted: {why}"
                ));
            }
        }
        let done = index as u64 + 1;
        if done.is_multiple_of(every) || done == total {
            emit(&Event::Progress(Progress::Setup {
                label: "Decrypting files".to_string(),
                step: done,
                total,
            }));
        }
    }
    Ok(written)
}

/// The regular files `Manifest.db` lists under `domain`, as file id and
/// path inside the domain, in path order.
fn domain_files(
    manifest: &Connection,
    domain: &str,
) -> Result<Vec<(String, String)>, RuntimeError> {
    let mut statement = manifest.prepare(
        "SELECT fileID, relativePath FROM Files \
         WHERE domain = ?1 AND flags = ?2 ORDER BY relativePath",
    )?;
    let rows = statement.query_map((domain, FLAG_FILE), |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Where a file of `domain` is written, or `None` when the domain or the
/// path would leave `out_dir`: the manifest comes from the backup, and a
/// path with `..` or a root in it is not one to follow.
fn target_path(out_dir: &Path, domain: &str, relative_path: &str) -> Option<PathBuf> {
    let plain = |path: &Path| {
        path.components()
            .all(|part| matches!(part, Component::Normal(_)))
    };
    let domain = Path::new(domain);
    let relative = Path::new(relative_path);
    if domain.components().count() != 1 || !plain(domain) || !plain(relative) {
        return None;
    }
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(out_dir.join(domain).join(relative))
}

/// Decrypt the backup entry `file_id` into a new file at `target`, readable
/// by its owner only.
fn decrypt_to(backup: &Backup, file_id: &str, target: &Path) -> Result<(), RuntimeError> {
    let entry = backup.get_file(file_id)?;
    if let Some(parent) = target.parent() {
        create_dir_all(parent)?;
    }
    let file = File::create(target)?;
    restrict_permissions(&file)?;
    // An empty file has no ciphertext to decrypt.
    if entry.metadata.size == 0 {
        return Ok(());
    }
    let mut plain = backup.decrypt_entry_stream(&entry)?;
    let mut writer = BufWriter::new(file);
    copy(&mut plain, &mut writer)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decrypt_domain, domain_files, list_domain, open, target_path, total_bytes};
    use crate::{error::IOS_BACKUP_PASSWORD_INCORRECT, test_support::one_open_backup_at_a_time};
    use chat_db_fixture::ios_backup::{BackupFile, Encryption, write_backup};
    use imessage_reader_protocol::BackupDomainRequest;
    use rusqlite::Connection;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    const DOMAIN: &str = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";

    /// Only the asked domain's regular files are listed: another domain's
    /// file, and a directory row of the same domain, are left out.
    #[test]
    fn only_the_regular_files_of_the_domain_are_listed() {
        let manifest = Connection::open_in_memory().unwrap();
        manifest
            .execute_batch(&format!(
                "CREATE TABLE Files (fileID TEXT, domain TEXT, relativePath TEXT, flags INTEGER);
                 INSERT INTO Files VALUES
                     ('aa', '{DOMAIN}', 'Message/Media/photo.jpg', 1),
                     ('bb', '{DOMAIN}', 'ChatStorage.sqlite', 1),
                     ('cc', '{DOMAIN}', 'Message', 2),
                     ('dd', 'HomeDomain', 'Library/SMS/sms.db', 1);"
            ))
            .unwrap();
        assert_eq!(
            domain_files(&manifest, DOMAIN).unwrap(),
            [
                ("bb".to_string(), "ChatStorage.sqlite".to_string()),
                ("aa".to_string(), "Message/Media/photo.jpg".to_string()),
            ]
        );
    }

    /// A file lands under the domain's own directory, and a manifest path that
    /// would climb out of it, or name a root, is refused.
    #[test]
    fn a_path_that_leaves_the_directory_is_refused() {
        let out = Path::new("/scratch");
        assert_eq!(
            target_path(out, DOMAIN, "Message/Media/photo.jpg"),
            Some(PathBuf::from(format!(
                "/scratch/{DOMAIN}/Message/Media/photo.jpg"
            )))
        );
        for bad in ["../outside", "Message/../../outside", "/etc/passwd", ""] {
            assert_eq!(target_path(out, DOMAIN, bad), None, "{bad:?}");
        }
        for bad_domain in ["..", "a/b", "/abs", ""] {
            assert_eq!(target_path(out, bad_domain, "file"), None, "{bad_domain:?}");
        }
    }

    /// One file of the made-up backup.
    fn file<'a>(
        domain: &'a str,
        relative_path: &'a str,
        bytes: Option<&'a [u8]>,
    ) -> BackupFile<'a> {
        BackupFile {
            domain,
            relative_path,
            bytes,
        }
    }

    fn request(backup: &Path, password: &str, out: &Path) -> BackupDomainRequest {
        BackupDomainRequest {
            backup_path: backup.to_path_buf(),
            backup_password: password.to_string(),
            domain: DOMAIN.to_string(),
            out_dir: out.to_path_buf(),
        }
    }

    /// The whole path against a backup encrypted the way an iPhone's is:
    /// the password opens it, the domain's files come out as their plain
    /// bytes under the domain's directory, another domain's file stays in the
    /// backup, and a file the manifest lists without bytes is counted and
    /// skipped rather than ending the run.
    #[test]
    fn an_encrypted_backup_gives_up_the_domain_and_nothing_else() {
        let _one_open = one_open_backup_at_a_time();
        let backup = tempfile::tempdir().unwrap();
        let photo = vec![7u8; 100_000];
        write_backup(
            backup.path(),
            &[
                file(DOMAIN, "ChatStorage.sqlite", Some(b"the database")),
                file(DOMAIN, "Message/Media/photo.jpg", Some(&photo)),
                file(DOMAIN, "Message/Media/empty.txt", Some(b"")),
                file(DOMAIN, "Message/Media/gone.jpg", None),
                file("HomeDomain", "Library/SMS/sms.db", Some(b"messages")),
            ],
            Encryption::Password("secret"),
        );
        let out = tempfile::tempdir().unwrap();
        let request = request(backup.path(), "secret", out.path());

        let opened = open(&request).unwrap();
        let files = list_domain(&opened, &request).unwrap();
        // Before anything is written, the domain's four files are measured
        // by the sizes the manifest records: the one with no bytes in the
        // backup still counts the size it is listed with.
        assert_eq!(files.len(), 4);
        assert_eq!(total_bytes(&files), 12 + 100_000 + 1);
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
        let written = decrypt_domain(&opened, &request, &files).unwrap();

        assert_eq!((written.files, written.failures), (3, 1));
        let domain = out.path().join(DOMAIN);
        assert_eq!(
            fs::read(domain.join("ChatStorage.sqlite")).unwrap(),
            b"the database"
        );
        assert_eq!(
            fs::read(domain.join("Message/Media/photo.jpg")).unwrap(),
            photo
        );
        assert_eq!(
            fs::read(domain.join("Message/Media/empty.txt")).unwrap(),
            b""
        );
        assert!(!out.path().join("HomeDomain").exists());
    }

    #[test]
    fn a_wrong_password_is_refused_in_the_apps_words() {
        let _one_open = one_open_backup_at_a_time();
        let backup = tempfile::tempdir().unwrap();
        write_backup(backup.path(), &[], Encryption::Password("secret"));
        let out = tempfile::tempdir().unwrap();
        let Err(err) = open(&request(backup.path(), "wrong", out.path())) else {
            panic!("a wrong password opened the backup");
        };
        assert_eq!(err.to_string(), IOS_BACKUP_PASSWORD_INCORRECT);
    }
}
