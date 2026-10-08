//! Exporting an iPhone backup through the real `imessage-reader` process, as
//! the desktop app does.
//!
//! `chat_db_fixture::ios_backup` writes the backup from the fixture
//! `chat.db`, once as it is and once encrypted with a password in Apple's
//! format. The reader decrypts the encrypted one into a directory of the
//! run's own under the Scratch Directory, which the run deletes when it ends,
//! whichever way it ends (#1386).

mod common;

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use chat_db_fixture::{
    PHOTO_BYTES,
    ios_backup::{BACKUP_PASSWORD, Encryption, write_messages_backup},
    listing::{file_names, paths_under},
};
use common::{config, helper_binary};
use imessage_reader_protocol::IOS_BACKUP_PASSWORD_INCORRECT;
use message_crate_core::{
    AppleConfig, ApplePlatform, ExporterConfig, IMESSAGE_READER_DIRECTORY, LogSink, ProgressSink,
    SourceConfig,
};

/// A backup and the run's output, in one directory that lives as long as
/// the value.
struct Run {
    dir: tempfile::TempDir,
    /// The names of every file seen under the Scratch Directory while the
    /// run reported progress or logged a line.
    seen_in_scratch: Arc<Mutex<BTreeSet<String>>>,
}

impl Run {
    fn new(encryption: Encryption<'_>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("backup")).unwrap();
        write_messages_backup(&dir.path().join("backup"), encryption);
        Self {
            dir,
            seen_in_scratch: Arc::default(),
        }
    }

    fn backup(&self) -> PathBuf {
        self.dir.path().join("backup")
    }

    fn output(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    /// The export of the backup with `password`. Each log line and progress
    /// count records what is under the Scratch Directory at that moment, and
    /// `cancel_when` is set once a decrypted Messages database is there.
    fn config(
        &self,
        password: Option<&str>,
        cancel_when: Option<Arc<AtomicBool>>,
    ) -> ExporterConfig {
        let base = config(&self.backup(), &self.output(), cancel_when.clone());
        let scratch = base.scratch_dir.clone();
        let seen = Arc::clone(&self.seen_in_scratch);
        let look = move || {
            let names = file_names_under(&scratch);
            if let Some(cancel) = &cancel_when
                && names.iter().any(|name| name.starts_with("crabapple-sms-"))
            {
                cancel.store(true, Ordering::Relaxed);
            }
            seen.lock().unwrap().extend(names);
        };
        let on_log = look.clone();
        ExporterConfig {
            log: Some(LogSink::new(move |_| on_log())),
            progress: Some(ProgressSink::unpaced(move |_| look())),
            source: SourceConfig::Apple(AppleConfig {
                platform: Some(ApplePlatform::Ios),
                backup_password: password.map(str::to_string),
                ..AppleConfig::default()
            }),
            ..base
        }
    }

    /// The run's directory under the Scratch Directory, which holds a
    /// directory per request while one runs and its lock file always.
    fn reader_scratch_root(&self) -> PathBuf {
        self.output()
            .with_extension("cache")
            .join(IMESSAGE_READER_DIRECTORY)
    }

    /// Assert that the run's scratch directory is gone: only the lock file
    /// of the directory it was made in is left.
    fn assert_scratch_is_gone(&self) {
        assert_eq!(
            file_names(&self.reader_scratch_root()),
            vec![".lock".to_string()]
        );
    }

    fn seen_in_scratch(&self) -> BTreeSet<String> {
        self.seen_in_scratch.lock().unwrap().clone()
    }
}

/// The names of the files under `dir`, at any depth; none when it is not
/// there.
fn file_names_under(dir: &Path) -> Vec<String> {
    files_under(dir)
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// Every file under `dir`, at any depth, without the directories.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    paths_under(dir)
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
}

/// Every conversation file of the export, read into one string.
fn exported_text(output: &Path) -> String {
    files_under(output)
        .iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .map(|path| fs::read_to_string(path).unwrap())
        .collect()
}

/// The photo the export staged under `attachments/`.
fn staged_photo(output: &Path) -> Vec<u8> {
    let staged = files_under(&output.join("attachments"));
    assert_eq!(staged.len(), 1, "{staged:?}");
    fs::read(&staged[0]).unwrap()
}

/// The encrypted backup exports with its password: the messages come out,
/// the photo is staged with its original bytes, the decrypted databases were
/// in the run's scratch directory while it ran, and that directory is gone
/// afterwards. Nothing decrypted is left beside the backup or the export.
#[test]
fn an_encrypted_backup_exports_through_the_scratch_directory() {
    helper_binary();
    let run = Run::new(Encryption::Password(BACKUP_PASSWORD));
    let backup_before = files_under(&run.backup());

    imessage_ir_exporter::run(&run.config(Some(BACKUP_PASSWORD), None)).unwrap();

    let all = exported_text(&run.output());
    assert!(all.contains("\"Saturday works\""), "{all}");
    assert!(all.contains("\"Weekend plans\""), "{all}");
    assert_eq!(staged_photo(&run.output()), PHOTO_BYTES);

    let seen = run.seen_in_scratch();
    assert!(
        seen.iter().any(|name| name.starts_with("crabapple-sms-")),
        "{seen:?}"
    );
    assert!(seen.contains("crabapple-Manifest.db"), "{seen:?}");
    run.assert_scratch_is_gone();
    assert_eq!(files_under(&run.backup()), backup_before);
    let decrypted_in_output: Vec<_> = file_names_under(&run.output())
        .into_iter()
        .filter(|name| name.starts_with("crabapple-") || name.ends_with(".attachment"))
        .collect();
    assert!(decrypted_in_output.is_empty(), "{decrypted_in_output:?}");
}

/// The backup that is not encrypted exports without a password, reading
/// the photo where it is, and nothing is decrypted.
#[test]
fn an_unencrypted_backup_exports_without_a_password() {
    helper_binary();
    let run = Run::new(Encryption::None);

    imessage_ir_exporter::run(&run.config(None, None)).unwrap();

    assert!(exported_text(&run.output()).contains("\"Saturday works\""));
    assert_eq!(staged_photo(&run.output()), PHOTO_BYTES);
    assert!(
        !run.seen_in_scratch()
            .iter()
            .any(|name| name.starts_with("crabapple-")),
        "{:?}",
        run.seen_in_scratch()
    );
    run.assert_scratch_is_gone();
}

/// A wrong password is refused with the reader's own message, and the
/// failed run's scratch directory is gone.
#[test]
fn a_wrong_password_is_refused_in_the_readers_words() {
    helper_binary();
    let run = Run::new(Encryption::Password(BACKUP_PASSWORD));

    let err = imessage_ir_exporter::run(&run.config(Some("not-the-password"), None)).unwrap_err();

    assert_eq!(err.to_string(), IOS_BACKUP_PASSWORD_INCORRECT);
    run.assert_scratch_is_gone();
}

/// A run that fails after the reader has decrypted the Messages database,
/// here because it is cancelled the moment the database is seen in the
/// scratch directory, deletes that directory all the same (#1386).
#[test]
fn a_run_that_fails_after_decrypting_leaves_no_scratch_directory() {
    helper_binary();
    let run = Run::new(Encryption::Password(BACKUP_PASSWORD));
    let cancel = Arc::new(AtomicBool::new(false));

    let err =
        imessage_ir_exporter::run(&run.config(Some(BACKUP_PASSWORD), Some(Arc::clone(&cancel))))
            .unwrap_err();

    assert!(
        cancel.load(Ordering::Relaxed),
        "the database was never seen"
    );
    assert_eq!(err.to_string(), "cancelled");
    run.assert_scratch_is_gone();
}
