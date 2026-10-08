//! The identities, export and backup domain requests against an iPhone
//! backup, through the real `imessage-reader` process, as the desktop app
//! makes them.
//!
//! `chat_db_fixture::ios_backup` writes the backup from the fixture
//! `chat.db`, once as it is and once encrypted with a password in Apple's
//! format. The program decrypts the encrypted one with `crabapple`, on its
//! side of the process boundary
//! (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use chat_db_fixture::{
    OWNER, OWNER_EMAIL, PHOTO_BYTES,
    ios_backup::{
        BACKUP_PASSWORD, BackupFile, DECRYPTED_MANIFEST_NAME, Encryption, HOME_DOMAIN,
        MEDIA_DOMAIN, MESSAGES_DB_PATH, PHOTO_PATH, SystemTempTurn, stored_path, system_temp_turn,
        write_backup, write_messages_backup, write_messages_backup_named,
    },
    listing::{file_names, paths_under},
};
use imessage_reader_protocol::{
    AttachmentFile, Event, ExportRequest, IOS_BACKUP_PASSWORD_INCORRECT, IdentitiesRequest,
    Platform, Request, Source,
};
use ios_backup::{
    DecryptedDomain, Helper, backup_identities, decrypt_ios_backup_domain,
    reader_build::build_imessage_reader,
};

/// A backup in a directory that lives as long as the value.
fn backup(encryption: Encryption<'_>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_messages_backup(dir.path(), encryption);
    dir
}

/// What the request left under the scratch root, less the root's lock file.
fn left_in(scratch_root: &Path) -> Vec<PathBuf> {
    paths_under(scratch_root)
        .into_iter()
        .filter(|path| path != &scratch_root.join(".lock"))
        .collect()
}

/// The reader's decrypted copies in the system's temporary directory, where
/// it wrote them before #1386 (the Messages and Contacts databases) and #788
/// (`Manifest.db`), each with its modification time, seen while this test
/// holds the turn at that directory.
///
/// `imessage-reader`'s own tests open encrypted backups in process and
/// write [`DECRYPTED_MANIFEST_NAME`] there, so the turn keeps them out until
/// the test ends. A copy a killed test left keeps its name, so a copy the
/// reader writes again shows as a newer modification time.
struct SystemTemp {
    _turn: SystemTempTurn,
    before: BTreeMap<String, Option<SystemTime>>,
}

impl SystemTemp {
    fn snapshot() -> Self {
        let turn = system_temp_turn();
        Self {
            _turn: turn,
            before: decrypted_in_system_temp(),
        }
    }

    /// Assert that no decrypted copy written since the snapshot is still
    /// there.
    fn assert_untouched(self) {
        let written: Vec<_> = decrypted_in_system_temp()
            .into_iter()
            .filter(|(name, modified)| self.before.get(name) != Some(modified))
            .collect();
        assert!(written.is_empty(), "{written:?}");
    }
}

/// The `crabapple-` files in the system's temporary directory, each with
/// its modification time.
fn decrypted_in_system_temp() -> BTreeMap<String, Option<SystemTime>> {
    fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.metadata().and_then(|m| m.modified()).ok(),
            )
        })
        .filter(|(name, _)| name.starts_with("crabapple-"))
        .collect()
}

/// The encrypted backup gives up the addresses its device sent from with
/// the right password. The databases it decrypted are gone with the
/// request's scratch directory, and no decrypted copy was left beside the
/// backup or in the system's temporary directory.
///
/// `crabapple` deletes its `Manifest.db` copy when the reader drops the
/// backup, so a copy written to the system's temporary directory and
/// deleted again is not seen here. The test that fails when the copy goes
/// there is
/// `an_identities_request_decrypts_into_its_scratch_directory_while_it_runs`.
#[test]
fn an_encrypted_backup_answers_the_identities_request_and_leaves_nothing() {
    build_imessage_reader();
    let backup = backup(Encryption::Password(BACKUP_PASSWORD));
    let scratch_root = tempfile::tempdir().unwrap();
    let backup_before = paths_under(backup.path());
    let system_temp = SystemTemp::snapshot();

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
    assert_eq!(paths_under(backup.path()), backup_before);
    system_temp.assert_untouched();
}

/// While an identities request runs, everything the reader decrypted is in
/// the scratch directory the request names: the backup's `Manifest.db`, the
/// Messages database and the Contacts database (#1386, #788). Once the
/// reader has answered, it has deleted all three.
///
/// The backup's device name is longer than a pipe holds. The reader logs it
/// after decrypting both databases and before deleting the Contacts copy,
/// so it stops on that line until this side reads its output, and the test
/// lists the scratch directory in between.
#[test]
fn an_identities_request_decrypts_into_its_scratch_directory_while_it_runs() {
    build_imessage_reader();
    let backup = tempfile::tempdir().unwrap();
    write_messages_backup_named(
        backup.path(),
        Encryption::Password(BACKUP_PASSWORD),
        &"x".repeat(1 << 20),
    );
    let scratch = tempfile::tempdir().unwrap();

    let mut helper = Helper::spawn(
        &identities_request(backup.path(), Some(BACKUP_PASSWORD), scratch.path()),
        None,
        None,
    )
    .unwrap();
    let names = wait_for_file(scratch.path(), "crabapple-contacts-");

    assert_eq!(names.len(), 3, "{names:?}");
    assert_eq!(names[0], DECRYPTED_MANIFEST_NAME);
    assert!(names[1].starts_with("crabapple-contacts-"), "{names:?}");
    assert!(names[2].starts_with("crabapple-sms-"), "{names:?}");
    let answer = loop {
        match helper.next_event().unwrap() {
            Event::Identities { values } => break values,
            Event::Source { .. } => {}
            other => panic!("expected the identities, got {other:?}"),
        }
    };
    helper.finish().unwrap();
    assert!(answer.contains(&OWNER.to_string()), "{answer:?}");
    assert_eq!(file_names(scratch.path()), Vec::<String>::new());
}

/// The names in `dir` once one starts with `prefix`. Panics after a minute
/// without one.
fn wait_for_file(dir: &Path, prefix: &str) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let names = file_names(dir);
        if names.iter().any(|name| name.starts_with(prefix)) {
            return names;
        }
        assert!(
            Instant::now() < deadline,
            "no {prefix} file in {}: {names:?}",
            dir.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
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

    assert_eq!(err.to_string(), IOS_BACKUP_PASSWORD_INCORRECT);
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

/// The WhatsApp import decrypts WhatsApp's domain of an encrypted backup
/// into its work directory through the reader. While the reader waits for
/// the go, the backup's decrypted `Manifest.db` is in that work directory,
/// not the system's temporary directory (#788). Once the domain is written,
/// the reader has deleted it and only the domain is left.
#[test]
fn a_backup_domain_request_decrypts_into_the_directory_it_names() {
    const WHATSAPP: &str = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";
    const CHAT_STORAGE: &[u8] = b"made-up ChatStorage.sqlite bytes";
    build_imessage_reader();
    let backup = tempfile::tempdir().unwrap();
    write_backup(
        backup.path(),
        &[BackupFile {
            domain: WHATSAPP,
            relative_path: "ChatStorage.sqlite",
            bytes: Some(CHAT_STORAGE),
        }],
        Encryption::Password(BACKUP_PASSWORD),
    );
    let work = tempfile::tempdir().unwrap();
    let mut seen_before_the_go = Vec::new();

    let written = decrypt_ios_backup_domain(
        backup.path(),
        BACKUP_PASSWORD,
        WHATSAPP,
        work.path(),
        |_| {
            seen_before_the_go = file_names(work.path());
            Ok(())
        },
        None,
        None,
    )
    .unwrap();

    assert_eq!(
        seen_before_the_go,
        vec![DECRYPTED_MANIFEST_NAME.to_string()]
    );
    assert_eq!(
        written,
        DecryptedDomain {
            files: 1,
            failures: 0
        }
    );
    assert_eq!(file_names(work.path()), vec![WHATSAPP.to_string()]);
    assert_eq!(
        fs::read(work.path().join(WHATSAPP).join("ChatStorage.sqlite")).unwrap(),
        CHAT_STORAGE
    );
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
