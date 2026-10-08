//! The identities and export requests against an iPhone backup, through the
//! real `imessage-reader` process, as the desktop app makes them.
//!
//! `chat_db_fixture::ios_backup` writes the backup from the fixture
//! `chat.db`, once as it is and once encrypted with a password in Apple's
//! format. The program decrypts the encrypted one with `crabapple`, on its
//! side of the process boundary
//! (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use chat_db_fixture::{
    OWNER, OWNER_EMAIL, PHOTO_BYTES,
    ios_backup::{
        BACKUP_PASSWORD, Encryption, HOME_DOMAIN, MEDIA_DOMAIN, MESSAGES_DB_PATH, PHOTO_PATH,
        stored_path, write_messages_backup,
    },
};
use imessage_reader_protocol::{
    AttachmentFile, Event, ExportRequest, IdentitiesRequest, Platform, Request, Source,
};
use ios_backup::{Helper, backup_identities, reader_build::build_imessage_reader};

/// What the reader says when the password does not open the backup
/// (`IOS_BACKUP_PASSWORD_INCORRECT` in `imessage-reader`).
const PASSWORD_INCORRECT: &str = "The iOS backup password was incorrect.";

/// A backup in a directory that lives as long as the value.
fn backup(encryption: Encryption<'_>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_messages_backup(dir.path(), encryption);
    dir
}

/// Every path under `dir`, sorted.
fn listing(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            out.push(path);
        }
    }
    out.sort();
    out
}

/// What the request left under the scratch root, less the root's lock file.
fn left_in(scratch_root: &Path) -> Vec<PathBuf> {
    listing(scratch_root)
        .into_iter()
        .filter(|path| path != &scratch_root.join(".lock"))
        .collect()
}

/// The reader's decrypted copies in the system's temporary directory, where
/// it wrote them before #1386.
fn decrypted_in_system_temp() -> BTreeSet<String> {
    fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("crabapple-"))
        .collect()
}

/// The encrypted backup gives up the addresses its device sent from with
/// the right password. The databases it decrypted are gone with the
/// request's scratch directory, and nothing was written beside the backup or
/// in the system's temporary directory.
#[test]
fn an_encrypted_backup_answers_the_identities_request_and_leaves_nothing() {
    build_imessage_reader();
    let backup = backup(Encryption::Password(BACKUP_PASSWORD));
    let scratch_root = tempfile::tempdir().unwrap();
    let backup_before = listing(backup.path());
    let temp_before = decrypted_in_system_temp();

    let mut identities = backup_identities(
        backup.path(),
        true,
        Some(BACKUP_PASSWORD),
        scratch_root.path(),
    )
    .unwrap();

    identities.sort();
    assert_eq!(identities, vec![OWNER.to_string(), OWNER_EMAIL.to_string()]);
    assert_eq!(left_in(scratch_root.path()), Vec::<PathBuf>::new());
    assert_eq!(listing(backup.path()), backup_before);
    let new_in_temp: Vec<_> = decrypted_in_system_temp()
        .difference(&temp_before)
        .cloned()
        .collect();
    assert!(new_in_temp.is_empty(), "{new_in_temp:?}");
}

/// The backup that is not encrypted answers without a password.
#[test]
fn an_unencrypted_backup_answers_without_a_password() {
    build_imessage_reader();
    let backup = backup(Encryption::None);
    let scratch_root = tempfile::tempdir().unwrap();

    let mut identities = backup_identities(backup.path(), true, None, scratch_root.path()).unwrap();

    identities.sort();
    assert_eq!(identities, vec![OWNER.to_string(), OWNER_EMAIL.to_string()]);
    assert_eq!(left_in(scratch_root.path()), Vec::<PathBuf>::new());
}

/// A wrong password is refused with the reader's own message, and the
/// failed request's scratch directory is gone too.
#[test]
fn a_wrong_password_is_refused_in_the_readers_words() {
    build_imessage_reader();
    let backup = backup(Encryption::Password(BACKUP_PASSWORD));
    let scratch_root = tempfile::tempdir().unwrap();

    let err = backup_identities(
        backup.path(),
        true,
        Some("not-the-password"),
        scratch_root.path(),
    )
    .unwrap_err();

    assert_eq!(err.to_string(), PASSWORD_INCORRECT);
    assert_eq!(left_in(scratch_root.path()), Vec::<PathBuf>::new());
}

/// A request that fails part-way through decrypting, here on a Messages
/// database whose last block was cut off, leaves no scratch directory
/// behind (#1386).
#[test]
fn a_request_that_fails_while_decrypting_leaves_no_scratch_directory() {
    build_imessage_reader();
    let backup = backup(Encryption::Password(BACKUP_PASSWORD));
    let messages = stored_path(backup.path(), HOME_DOMAIN, MESSAGES_DB_PATH);
    let mut ciphertext = fs::read(&messages).unwrap();
    ciphertext.truncate(ciphertext.len() - 8);
    fs::write(&messages, ciphertext).unwrap();
    let scratch_root = tempfile::tempdir().unwrap();

    backup_identities(
        backup.path(),
        true,
        Some(BACKUP_PASSWORD),
        scratch_root.path(),
    )
    .unwrap_err();

    assert_eq!(left_in(scratch_root.path()), Vec::<PathBuf>::new());
}

/// The reader's first answer says whether the backup is encrypted: true for
/// the encrypted backup and false for the other, which is how the app knows
/// to ask the reader for every attachment of an encrypted one.
#[test]
fn the_reader_says_which_backup_is_encrypted() {
    build_imessage_reader();
    for (encryption, password, expected) in [
        (
            Encryption::Password(BACKUP_PASSWORD),
            Some(BACKUP_PASSWORD),
            true,
        ),
        (Encryption::None, None, false),
    ] {
        let backup = backup(encryption);
        let scratch = tempfile::tempdir().unwrap();
        let mut helper = Helper::spawn(
            &identities_request(backup.path(), password, scratch.path()),
            None,
            None,
        )
        .unwrap();
        let Event::Source { encrypted, .. } = helper.next_event().unwrap() else {
            panic!("the reader's first answer is not its source line");
        };
        assert_eq!(encrypted, expected, "{encryption:?}");
        assert!(matches!(
            helper.next_event().unwrap(),
            Event::Identities { .. }
        ));
        helper.finish().unwrap();
    }
}

/// The reader writes the decrypted databases into the scratch directory the
/// request names: with one it may not write to, the request fails.
#[cfg(unix)]
#[test]
fn the_reader_decrypts_into_the_scratch_directory_the_request_names() {
    use std::os::unix::fs::PermissionsExt;

    build_imessage_reader();
    let backup = backup(Encryption::Password(BACKUP_PASSWORD));
    let scratch = tempfile::tempdir().unwrap();
    fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o500)).unwrap();
    if fs::File::create(scratch.path().join("probe")).is_ok() {
        eprintln!("skipped: a read-only directory can still be written to by this user");
        return;
    }

    let mut helper = Helper::spawn(
        &identities_request(backup.path(), Some(BACKUP_PASSWORD), scratch.path()),
        None,
        None,
    )
    .unwrap();
    let err = helper.next_event().unwrap_err();
    fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        err.to_string().to_lowercase().contains("permission denied"),
        "{err:#}"
    );
}

/// An export of the encrypted backup, made the way the exporter makes it.
/// While the reader is open, what it decrypted is in the request's scratch
/// directory, the backup's `Manifest.db` included, and the messages came out
/// of it. The photo decrypts to its original bytes there. Once the reader
/// exits, only that decrypted photo is left, which the exporter reads and
/// deletes.
#[test]
fn an_export_decrypts_into_its_scratch_directory_and_the_photo_comes_out_whole() {
    build_imessage_reader();
    let backup = backup(Encryption::Password(BACKUP_PASSWORD));
    let scratch = tempfile::tempdir().unwrap();
    let mut helper = Helper::spawn(
        &Request::Export(ExportRequest {
            source: ios_source(backup.path(), Some(BACKUP_PASSWORD)),
            attachment_root: None,
            contacts_path: None,
            use_caller_id: true,
            scratch_dir: scratch.path().to_path_buf(),
        }),
        None,
        None,
    )
    .unwrap();

    let mut texts = Vec::new();
    loop {
        match helper.next_event().unwrap() {
            Event::Message(message) => texts.push(message.text),
            Event::ExportDone { .. } => break,
            _ => {}
        }
    }
    assert!(texts.iter().any(|t| t == "Saturday works"), "{texts:?}");
    let names = file_names(scratch.path());
    assert_eq!(names.len(), 2, "{names:?}");
    assert_eq!(names[0], "crabapple-Manifest.db");
    assert!(names[1].starts_with("crabapple-sms-"), "{names:?}");

    let photo = stored_path(backup.path(), MEDIA_DOMAIN, PHOTO_PATH);
    let AttachmentFile::Ready { path } = helper.decrypt_attachment(&photo).unwrap() else {
        panic!("the photo was not decrypted");
    };
    assert_eq!(path.parent(), Some(scratch.path()));
    assert_eq!(fs::read(&path).unwrap(), PHOTO_BYTES);
    helper.finish().unwrap();

    assert_eq!(
        file_names(scratch.path()),
        vec![path.file_name().unwrap().to_string_lossy().into_owned()]
    );
}

/// The names of the entries in `dir`, sorted.
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// An iPhone backup as a source, with `password`.
fn ios_source(backup: &Path, password: Option<&str>) -> Source {
    Source {
        db_path: backup.to_path_buf(),
        platform: Platform::Ios,
        backup_password: password.map(str::to_string),
    }
}

/// The identities request the app sends for an iPhone backup.
fn identities_request(backup: &Path, password: Option<&str>, scratch: &Path) -> Request {
    Request::Identities(IdentitiesRequest {
        source: ios_source(backup, password),
        scratch_dir: scratch.to_path_buf(),
    })
}
