//! Where the built program keeps the file `crabapple` decrypts an encrypted
//! backup's `Manifest.db` to.
//!
//! `crabapple` writes it into the process's temporary directory, so `main`
//! points that directory at the one the request names (#788). This test
//! runs the program this package builds, so `cargo test -p imessage-reader`,
//! which is what a mutation test of `main.rs` runs, sees a change there. The
//! identities, export and backup domain requests are tested through the app's
//! side in `crates/libs/ios-backup/tests/encrypted_backup.rs`.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use chat_db_fixture::{
    ios_backup::{BACKUP_PASSWORD, BackupFile, DECRYPTED_MANIFEST_NAME, Encryption, write_backup},
    listing::file_names,
};
use imessage_reader_protocol::{BackupDomainRequest, Event, Request};

/// While the program waits for the go on a backup domain request, the
/// backup's decrypted `Manifest.db` is in the directory the request names.
/// Told nothing more, the program ends and deletes it.
#[test]
fn the_decrypted_manifest_is_kept_in_the_directory_the_request_names() {
    const DOMAIN: &str = "AppDomainGroup-group.example.shared";
    let backup = tempfile::tempdir().unwrap();
    write_backup(
        backup.path(),
        &[BackupFile {
            domain: DOMAIN,
            relative_path: "file",
            bytes: Some(b"made-up bytes"),
        }],
        Encryption::Password(BACKUP_PASSWORD),
    );
    let out_dir = tempfile::tempdir().unwrap();
    let request = Request::BackupDomain(BackupDomainRequest {
        backup_path: backup.path().to_path_buf(),
        backup_password: BACKUP_PASSWORD.to_string(),
        domain: DOMAIN.to_string(),
        out_dir: out_dir.path().to_path_buf(),
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_imessage-reader"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{}", serde_json::to_string(&request).unwrap()).unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    loop {
        let line = stdout.next().expect("the program ended early").unwrap();
        match serde_json::from_str::<Event>(&line).unwrap() {
            Event::BackupDomainSize { .. } => break,
            Event::Error { message } => panic!("{message}"),
            _ => {}
        }
    }

    let waiting = file_names(out_dir.path());
    drop(stdin);
    assert!(child.wait().unwrap().success());
    assert_eq!(waiting, vec![DECRYPTED_MANIFEST_NAME.to_string()]);
    assert_eq!(file_names(out_dir.path()), Vec::<String>::new());
}
