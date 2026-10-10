use super::{
    Platform, WTSEXPORTER_TROUBLESHOOTING, WtsexporterArgs, android_crypt_backup,
    extracts_ios_backup, input_search_root, names_a_full_disk, resolve_forwarded_paths,
    run_wtsexporter, scratch_write_error, wtsexporter_command, wtsexporter_file_name,
    wtsexporter_in,
};
use crate::ios_backup::DecryptedWhatsapp;
use media::testutil::write_with_mode;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn android_args(input: &Path, key: Option<&str>) -> WtsexporterArgs {
    WtsexporterArgs {
        platform: Platform::Android,
        input: input.to_path_buf(),
        work_dir: input.to_path_buf(),
        key: key.map(str::to_string),
        backup: None,
        wa: None,
        media: None,
        db: None,
        business: false,
    }
}

/// wtsexporter looks for its default files (`msgstore.db`, `wa.db`, the
/// key) in one directory. For a file input that is the directory holding the
/// file, not the directory the app happens to run in.
#[test]
fn a_file_input_is_searched_in_the_directory_that_holds_it() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("msgstore.db");
    fs::write(&file, b"db").unwrap();
    assert_eq!(input_search_root(&file).unwrap(), dir.path());
    assert_eq!(input_search_root(dir.path()).unwrap(), dir.path());
}

#[test]
fn prefers_msgstore_db_over_crypt() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("msgstore.db"), b"db").unwrap();
    fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
    assert_eq!(android_crypt_backup(dir.path()), None);
}

#[test]
fn finds_crypt15_when_msgstore_missing() {
    let dir = tempdir().unwrap();
    let crypt = dir.path().join("msgstore.db.crypt15");
    fs::write(&crypt, b"crypt").unwrap();
    assert_eq!(
        android_crypt_backup(dir.path()).as_deref(),
        Some(crypt.as_path())
    );
}

#[test]
fn prefers_crypt12_over_crypt15() {
    let dir = tempdir().unwrap();
    let crypt12 = dir.path().join("msgstore.db.crypt12");
    fs::write(&crypt12, b"c12").unwrap();
    fs::write(dir.path().join("msgstore.db.crypt15"), b"c15").unwrap();
    assert_eq!(
        android_crypt_backup(dir.path()).as_deref(),
        Some(crypt12.as_path())
    );
}

#[test]
fn ignores_crypt15_directory() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("msgstore.db.crypt15")).unwrap();
    assert_eq!(android_crypt_backup(dir.path()), None);
}

#[test]
fn drops_key_when_msgstore_db_is_present() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("msgstore.db"), b"db").unwrap();
    fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
    let paths = resolve_forwarded_paths(&android_args(dir.path(), Some("deadbeef"))).unwrap();
    assert!(paths.backup.is_none());
    assert!(paths.key.is_none());
}

#[test]
fn forwards_crypt15_and_key_when_msgstore_missing() {
    let dir = tempdir().unwrap();
    let crypt = dir.path().join("msgstore.db.crypt15");
    fs::write(&crypt, b"crypt").unwrap();
    let paths = resolve_forwarded_paths(&android_args(dir.path(), Some("deadbeef"))).unwrap();
    assert_eq!(paths.backup.as_deref(), Some(crypt.as_path()));
    assert_eq!(paths.key.as_deref(), Some("deadbeef"));
}

#[test]
fn forwards_crypt15_without_key_when_key_omitted() {
    let dir = tempdir().unwrap();
    let crypt = dir.path().join("msgstore.db.crypt15");
    fs::write(&crypt, b"crypt").unwrap();
    let paths = resolve_forwarded_paths(&android_args(dir.path(), None)).unwrap();
    assert_eq!(paths.backup.as_deref(), Some(crypt.as_path()));
    assert!(paths.key.is_none());
}

/// The arguments of the command built for `args`, as strings.
fn command_args(args: &WtsexporterArgs, out_dir: &Path, json_out: &Path) -> Vec<String> {
    let cmd = wtsexporter_command(Path::new("wtsexporter"), args, out_dir, json_out).unwrap();
    assert_eq!(cmd.get_current_dir(), Some(args.work_dir.as_path()));
    cmd.get_args()
        .map(|arg| arg.to_str().unwrap().to_string())
        .collect()
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

/// The arguments every command starts with: the platform flag (`-a` or
/// `-i`), then `--no-html`, `--no-banner`, `-o <out>` and `-j <json>`,
/// before the paths the backup forwards.
fn base_args(platform_flag: &str, out: &Path, json: &Path) -> Vec<String> {
    vec![
        platform_flag.to_string(),
        "--no-html".to_string(),
        "--no-banner".to_string(),
        "-o".to_string(),
        text(out),
        "-j".to_string(),
        text(json),
    ]
}

/// The arguments that pass an iPhone backup's files read straight from
/// disk: its database, contacts and media directory.
fn ios_files_args(db: &Path, contacts: &Path, media: &Path) -> Vec<String> {
    vec![
        "-d".to_string(),
        text(db),
        "-w".to_string(),
        text(contacts),
        "-m".to_string(),
        text(media),
    ]
}

/// An Android backup with every option found: an encrypted database,
/// a hex key (passed as a key file, never on the command line),
/// contacts, media, and the business app. The whole command line is
/// pinned, so a flag added in any form (`-c`, which moves the user's
/// media into the scratch directory, above all) fails here.
#[test]
fn an_android_command_forwards_every_found_path_and_nothing_else() {
    let dir = tempdir().unwrap();
    let crypt = dir.path().join("msgstore.db.crypt15");
    fs::write(&crypt, b"crypt").unwrap();
    let wa = dir.path().join("wa.db");
    fs::write(&wa, b"wa").unwrap();
    let media = dir.path().join("WhatsApp");
    fs::create_dir(&media).unwrap();
    let work = tempdir().unwrap();
    let out = work.path().join("out");
    let json = out.join("result.json");
    let mut args = android_args(dir.path(), Some("deadbeef"));
    args.work_dir = work.path().to_path_buf();
    args.business = true;

    let key_file = work.path().join("decryption.key");
    assert_eq!(
        command_args(&args, &out, &json),
        [
            base_args("-a", &out, &json),
            vec![
                "-k".to_string(),
                text(&key_file),
                "-b".to_string(),
                text(&crypt),
                "-w".to_string(),
                text(&wa),
                "-m".to_string(),
                text(&media),
                "--business".to_string(),
            ],
        ]
        .concat()
    );
    assert_eq!(fs::read(&key_file).unwrap(), [0xde, 0xad, 0xbe, 0xef]);
}

/// A key given as a file path is passed to wtsexporter as that path,
/// made absolute, rather than read as hex key material. A path is told
/// from hex by a slash or by the `.key` ending.
#[test]
fn a_key_file_path_is_forwarded_as_a_path() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
    let work = tempdir().unwrap();
    let out = work.path().join("out");
    let json = out.join("result.json");
    let cwd = std::env::current_dir().unwrap();

    let absolute = dir.path().join("backup.key");
    for (key, expected) in [
        (text(&absolute), absolute.clone()),
        ("x.key".to_string(), cwd.join("x.key")),
        ("keys/key".to_string(), cwd.join("keys/key")),
    ] {
        let mut args = android_args(dir.path(), Some(&key));
        args.work_dir = work.path().to_path_buf();
        let command = command_args(&args, &out, &json);
        let k = command.iter().position(|a| a == "-k").expect("a -k flag");
        assert_eq!(command[k + 1], text(&expected), "{key}");
        assert!(
            !work.path().join("decryption.key").exists(),
            "{key} was read as hex"
        );
    }
}

/// A database file chosen as the input is passed with `-d`, whatever it
/// is called.
#[test]
fn a_database_file_given_as_the_input_is_passed_as_the_database() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("phone-msgstore.db");
    fs::write(&db, b"db").unwrap();
    let mut args = android_args(&db, None);
    args.work_dir = dir.path().to_path_buf();
    let out = dir.path().join("out");
    let command = command_args(&args, &out, &out.join("result.json"));
    let d = command.iter().position(|a| a == "-d").expect("a -d flag");
    assert_eq!(command[d + 1], text(&db));
}

/// Only Android has crypt backup files, so an iOS directory that happens to
/// hold one passes no `-b`.
#[test]
fn an_ios_directory_forwards_no_android_backup() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
    let args = WtsexporterArgs {
        platform: Platform::Ios,
        ..android_args(dir.path(), None)
    };
    let paths = resolve_forwarded_paths(&args).unwrap();
    assert!(paths.backup.is_none());
}

/// An encrypted iPhone backup is never passed to wtsexporter: the
/// command names the decrypted database, contacts and media, and has
/// no `-b`.
#[test]
fn a_decrypted_iphone_backup_is_read_from_its_files_with_no_backup_flag() {
    let backup = tempdir().unwrap();
    let work = tempdir().unwrap();
    let domain = work
        .path()
        .join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared");
    fs::create_dir(&domain).unwrap();
    let db = domain.join("ChatStorage.sqlite");
    fs::write(&db, b"db").unwrap();
    let contacts = domain.join("ContactsV2.sqlite");
    fs::write(&contacts, b"contacts").unwrap();
    let out = work.path().join("out");
    let json = out.join("result.json");
    let mut args = WtsexporterArgs {
        platform: Platform::Ios,
        backup: Some(backup.path().to_path_buf()),
        work_dir: work.path().to_path_buf(),
        ..android_args(backup.path(), None)
    };
    args.read_decrypted(DecryptedWhatsapp {
        domain_dir: domain.clone(),
        database: db.clone(),
    });

    assert_eq!(
        command_args(&args, &out, &json),
        [
            base_args("-i", &out, &json),
            ios_files_args(&db, &contacts, &domain),
        ]
        .concat()
    );
}

/// An iPhone backup is passed with `-b` alone. WhatsApp's own files at
/// the backup directory's root are left over from an earlier extract there
/// and are not passed: with the shared directory but no `ChatStorage.sqlite`
/// at the root, `-m` and `-w` without `-d` make wtsexporter stop with
/// "The message database does not exist".
#[test]
fn an_iphone_backup_is_passed_alone_without_the_files_at_its_root() {
    let backup = tempdir().unwrap();
    let shared = backup
        .path()
        .join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared");
    fs::create_dir(&shared).unwrap();
    fs::write(shared.join("ContactsV2.sqlite"), b"contacts").unwrap();
    let work = tempdir().unwrap();
    let out = work.path().join("out");
    let json = out.join("result.json");
    let args = WtsexporterArgs {
        platform: Platform::Ios,
        backup: Some(backup.path().to_path_buf()),
        work_dir: work.path().to_path_buf(),
        ..android_args(backup.path(), None)
    };

    assert_eq!(
        command_args(&args, &out, &json),
        [
            base_args("-i", &out, &json),
            vec!["-b".to_string(), text(backup.path())],
        ]
        .concat()
    );
}

/// An iOS backup forwards its database, contacts and media found under
/// the input directory; with no backup file there is no `-b`, so the key
/// is dropped too.
#[test]
fn an_ios_command_forwards_the_found_database_contacts_and_media() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("ChatStorage.sqlite");
    fs::write(&db, b"db").unwrap();
    let shared = dir
        .path()
        .join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared");
    fs::create_dir(&shared).unwrap();
    let contacts = shared.join("ContactsV2.sqlite");
    fs::write(&contacts, b"contacts").unwrap();
    let out = dir.path().join("out");
    let json = out.join("result.json");
    let args = WtsexporterArgs {
        platform: Platform::Ios,
        key: Some("deadbeef".to_string()),
        ..android_args(dir.path(), None)
    };

    assert_eq!(
        command_args(&args, &out, &json),
        [
            base_args("-i", &out, &json),
            ios_files_args(&db, &contacts, &shared),
        ]
        .concat()
    );
}

/// wtsexporter extracts an iPhone backup's WhatsApp files into the work
/// directory only when it gets the backup and no media directory that
/// exists, so only then is there an extract to measure. Android never
/// extracts one.
#[test]
fn only_an_iphone_backup_without_a_media_directory_is_extracted() {
    let dir = tempdir().unwrap();
    let backup = dir.path().join("backup");
    let media = dir.path().join("WhatsApp");
    fs::create_dir_all(&backup).unwrap();
    let ios = |media: Option<&Path>| WtsexporterArgs {
        platform: Platform::Ios,
        backup: Some(backup.clone()),
        media: media.map(Path::to_path_buf),
        ..android_args(&backup, None)
    };

    assert!(extracts_ios_backup(&ios(None)).unwrap());
    assert!(
        extracts_ios_backup(&ios(Some(&media))).unwrap(),
        "a media directory that does not exist is no reason to skip"
    );
    fs::create_dir_all(&media).unwrap();
    assert!(!extracts_ios_backup(&ios(Some(&media))).unwrap());
    let no_backup = WtsexporterArgs {
        backup: None,
        ..ios(None)
    };
    assert!(!extracts_ios_backup(&no_backup).unwrap());
    assert!(!extracts_ios_backup(&android_args(&backup, None)).unwrap());
}

/// Held while a test writes a stand-in wtsexporter and runs it. A file
/// still open for writing cannot be run ("Text file busy"), and a test
/// that starts a process on another thread holds a copy of every open
/// file until that process starts; one at a time, no stand-in is being
/// written while another starts.
#[cfg(unix)]
static STAND_IN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A stand-in for wtsexporter in `dir`: it writes part of a decrypted
/// `msgstore.db` into its working directory, as the real one does before
/// its disk fills, prints `error` and exits 1. The caller holds
/// [`STAND_IN`] until it has run it.
#[cfg(unix)]
fn failing_wtsexporter(dir: &Path, error: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("wtsexporter");
    fs::write(
        &bin,
        format!("#!/bin/sh\nprintf partial > msgstore.db\necho '{error}' >&2\nexit 1\n"),
    )
    .unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

/// The Android arguments for a run whose work directory is `work`.
#[cfg(unix)]
fn android_run_args(input: &Path, work: &Path) -> WtsexporterArgs {
    WtsexporterArgs {
        work_dir: work.to_path_buf(),
        ..android_args(input, Some("deadbeef"))
    }
}

/// wtsexporter decrypts an Android backup's `msgstore.db` into the work
/// directory, under the Scratch Directory. When that disk fills, the run
/// says so in the sentence every free-space check gives, naming the
/// Scratch Directory's disk, in place of wtsexporter's raw error (#1820).
#[cfg(unix)]
#[test]
fn a_full_scratch_disk_is_reported_as_the_free_space_sentence() {
    let _one_at_a_time = STAND_IN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempdir().unwrap();
    let input = dir.path().join("backup");
    let work = dir.path().join("work");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&work).unwrap();
    let bin = failing_wtsexporter(dir.path(), "OSError: [Errno 28] No space left on device");

    let err = run_wtsexporter(
        &bin,
        &android_run_args(&input, &work),
        &work.join("result.json"),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.starts_with("Not enough space on the disk that holds the Scratch Directory"),
        "{err}"
    );
    assert!(!err.contains("wtsexporter failed"), "{err}");
    assert!(!err.contains("Errno 28"), "{err}");
}

/// A failed run's whole output goes to the log as one warning, while
/// the error is still the free-space sentence:
/// the Import Run's log keeps what wtsexporter said about a full disk
/// that may not be the Scratch Directory's (#1938).
#[cfg(unix)]
#[test]
fn a_failed_runs_output_goes_to_the_log_as_a_warning() {
    let _one_at_a_time = STAND_IN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempdir().unwrap();
    let input = dir.path().join("backup");
    let work = dir.path().join("work");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&work).unwrap();
    let bin = failing_wtsexporter(
        dir.path(),
        "Copying media\nOSError: [Errno 28] No space left on device: /tmp/x",
    );
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = {
        let lines = lines.clone();
        let warnings = warnings.clone();
        message_crate_core::LogSink::new(move |line| lines.lock().unwrap().push(line.to_string()))
            .with_warnings(move |text| warnings.lock().unwrap().push(text.to_string()))
    };

    let err = run_wtsexporter(
        &bin,
        &android_run_args(&input, &work),
        &work.join("result.json"),
        Some(&log),
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.starts_with("Not enough space on the disk that holds the Scratch Directory"),
        "{err}"
    );
    let warnings = warnings.lock().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("wtsexporter failed ("),
        "{warnings:?}"
    );
    assert!(warnings[0].contains("\nCopying media\n"), "{warnings:?}");
    assert!(
        warnings[0].contains("OSError: [Errno 28] No space left on device: /tmp/x"),
        "{warnings:?}"
    );
    assert!(lines.lock().unwrap().is_empty());
}

/// Any other wtsexporter failure is reported as wtsexporter gave it.
#[cfg(unix)]
#[test]
fn any_other_wtsexporter_failure_is_reported_as_it_is() {
    let _one_at_a_time = STAND_IN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempdir().unwrap();
    let input = dir.path().join("backup");
    let work = dir.path().join("work");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&work).unwrap();
    let bin = failing_wtsexporter(dir.path(), "ValueError: The key is incorrect");

    let err = run_wtsexporter(
        &bin,
        &android_run_args(&input, &work),
        &work.join("result.json"),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(err.starts_with("wtsexporter failed ("), "{err}");
    assert!(err.contains("ValueError: The key is incorrect"), "{err}");
}

/// The partial `msgstore.db` a full disk leaves is in the run's work
/// directory, so it goes when the run lets go of that directory, as it
/// does when wtsexporter's error ends the run.
#[cfg(unix)]
#[test]
fn the_partial_database_goes_with_the_work_directory() {
    let _one_at_a_time = STAND_IN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempdir().unwrap();
    let input = dir.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let work = message_crate_core::ScratchDir::create(
        &dir.path()
            .join("scratch")
            .join(message_crate_core::WHATSAPP_DIRECTORY),
    )
    .unwrap();
    let bin = failing_wtsexporter(dir.path(), "OSError: [Errno 28] No space left on device");

    run_wtsexporter(
        &bin,
        &android_run_args(&input, work.path()),
        &work.path().join("result.json"),
        None,
    )
    .unwrap_err();
    let partial = work.path().join("msgstore.db");
    assert!(
        partial.is_file(),
        "wtsexporter wrote into the work directory"
    );

    drop(work);
    assert!(!partial.exists(), "the partial database is deleted");
}

/// Python's error codes for a full disk name it in any system language,
/// so a German Windows' `ERROR_DISK_FULL` is still the free-space error.
#[test]
fn a_full_disk_is_found_by_its_error_code_in_any_language() {
    assert!(names_a_full_disk(
        "OSError: [WinError 112] Auf dem Datenträger ist nicht genug Speicherplatz vorhanden"
    ));
    assert!(names_a_full_disk(
        "OSError: [Errno 28] Espace insuffisant sur le périphérique"
    ));
    assert!(!names_a_full_disk("ValueError: The key is incorrect"));
}

/// A Scratch Directory disk already full before wtsexporter starts gives
/// the same free-space error as one that fills while it runs; any other
/// write error keeps its own words.
#[test]
fn a_full_disk_before_wtsexporter_starts_is_the_free_space_error() {
    let full = scratch_write_error(
        std::io::Error::from(std::io::ErrorKind::StorageFull),
        "create work/decryption.key".to_string(),
    )
    .to_string();
    assert!(
        full.starts_with("Not enough space on the disk that holds the Scratch Directory"),
        "{full}"
    );

    let denied = scratch_write_error(
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        "create work/decryption.key".to_string(),
    );
    assert_eq!(denied.to_string(), "create work/decryption.key");
}

/// wtsexporter is always the app's own copy in the Tools Directory.
#[test]
fn wtsexporter_is_found_in_the_tools_directory() {
    let tools = tempfile::tempdir().unwrap();
    let program = tools.path().join(wtsexporter_file_name());
    write_with_mode(&program, 0o755);

    assert_eq!(wtsexporter_in(Some(tools.path())).unwrap(), program);
}

/// A release file put in the Tools Directory without `chmod +x` cannot
/// start, so it is not reported as found, and the error says how to fix it.
#[cfg(unix)]
#[test]
fn a_wtsexporter_that_is_not_executable_is_refused() {
    let tools = tempfile::tempdir().unwrap();
    let program = tools.path().join(wtsexporter_file_name());
    write_with_mode(&program, 0o644);

    let err = wtsexporter_in(Some(tools.path())).expect_err("not executable");
    let message = err.to_string();
    assert!(message.contains("is not executable"), "{message}");
    assert!(message.contains("chmod +x"), "{message}");
    assert!(
        super::find_wtsexporter(Some(tools.path())).is_err(),
        "the status lookup refuses it too"
    );
}

/// A missing copy is reported as missing, naming the Tools Directory,
/// and a copy outside it is not found (#1053).
#[test]
fn wtsexporter_is_looked_for_nowhere_but_the_tools_directory() {
    let tools = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    write_with_mode(&elsewhere.path().join(wtsexporter_file_name()), 0o755);

    let err = wtsexporter_in(Some(tools.path())).expect_err("not in the Tools Directory");
    let no_dir = wtsexporter_in(None).expect_err("no Tools Directory");

    let message = err.to_string();
    assert!(message.contains("Tools Directory"), "{message}");
    // A pipx install linked there is replaced by the app's own download,
    // so the error sends the person to the guide, not to pipx.
    assert!(message.ends_with(WTSEXPORTER_TROUBLESHOOTING), "{message}");
    assert!(!message.contains("pipx"), "{message}");
    assert!(
        message.contains(&tools.path().display().to_string()),
        "{message}"
    );
    assert!(
        no_dir.to_string().contains("no Tools Directory"),
        "{no_dir}"
    );
}
