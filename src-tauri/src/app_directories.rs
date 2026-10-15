//! The Message Crate Directory, `{home}/message-crate`: the one directory the
//! desktop app keeps on a computer, made every time the app starts (#1053).
//!
//! ```text
//! ~/message-crate/
//!   data/      the Data Directory of the Message Crate the app starts
//!   staging/   the default Staging Directory
//!   exports/   the Export Directory (crate::export_directories)
//!   logs/      the Logs Directory
//!   scratch/   the Scratch Directory
//!   tools/     the Tools Directory: ffmpeg, ffprobe, wtsexporter
//! ```
//!
//! A dev build (`cargo tauri dev`) gives its server `data-dev` in place of
//! `data`, so it never opens the installed app's database: a branch with
//! another Schema Fingerprint would rebuild that database empty. Everything
//! else is shared.
//!
//! The Message Crate Directory is readable by the user alone, and so are the
//! directories in it that hold messages (`data`, `staging`, `exports`,
//! `logs`, `scratch`), each made or tightened at start
//! ([`file_io::restrict_to_owner`], #2097). The Staging Directory setting can
//! name another directory; that one is the person's and keeps its mode.
//!
//! - The **Logs Directory** holds each Import Run's log, named for the run,
//!   and keeps it after the run's directory in the Staging Directory is
//!   deleted. Staging, Media and Upload all write into it ([`RunLog`] for the
//!   first two, the import library's own writer for the Upload). Nothing
//!   deletes a log.
//! - The **Scratch Directory** holds what a run writes that is neither its
//!   output nor kept: the attachment spool and the databases the Apple
//!   Messages Reader decrypts ([`message_crate_core::ScratchDir`]). What a
//!   killed run left there is deleted at start-up.
//!
//! Nothing is moved here from the operating system's app-data directory,
//! where the app kept these before: an app with a database there starts a
//! new Message Crate here.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use message_crate_core::{RunIssue, RunLogAccount, RunLogLevel, format_run_log};

use crate::export_directories::{self, EXPORT_DIRECTORY_NAME};

/// The Message Crate Directory's name in the home directory.
pub const MESSAGE_CRATE_DIRECTORY_NAME: &str = "message-crate";

/// The Data Directory's name in the Message Crate Directory.
pub const DATA_DIRECTORY_NAME: &str = "data";

/// The Data Directory's name in the Message Crate Directory for a dev build.
pub const DEV_DATA_DIRECTORY_NAME: &str = "data-dev";

/// The default Staging Directory's name in the Message Crate Directory.
pub const STAGING_DIRECTORY_NAME: &str = "staging";

/// The Logs Directory's name in the Message Crate Directory.
pub const LOGS_DIRECTORY_NAME: &str = "logs";

/// The Scratch Directory's name in the Message Crate Directory.
pub const SCRATCH_DIRECTORY_NAME: &str = "scratch";

/// The Tools Directory's name in the Message Crate Directory.
pub const TOOLS_DIRECTORY_NAME: &str = "tools";

/// The Message Crate Directory and the directories in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageCrateDirectory {
    /// `{home}/message-crate`.
    root: PathBuf,
    /// Whether this is a dev build, whose server keeps its data in
    /// [`DEV_DATA_DIRECTORY_NAME`].
    dev: bool,
}

impl MessageCrateDirectory {
    /// The Message Crate Directory under `home`, for a dev build when `dev`.
    pub fn in_home(home: &Path, dev: bool) -> Self {
        Self {
            root: home.join(MESSAGE_CRATE_DIRECTORY_NAME),
            dev,
        }
    }

    /// The Message Crate Directory itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The Data Directory of the Message Crate the app starts: `data`, or
    /// `data-dev` for a dev build.
    pub fn data(&self) -> PathBuf {
        self.root.join(if self.dev {
            DEV_DATA_DIRECTORY_NAME
        } else {
            DATA_DIRECTORY_NAME
        })
    }

    /// The default Staging Directory.
    pub fn staging(&self) -> PathBuf {
        self.root.join(STAGING_DIRECTORY_NAME)
    }

    /// The Export Directory.
    pub fn exports(&self) -> PathBuf {
        self.root.join(EXPORT_DIRECTORY_NAME)
    }

    /// The Logs Directory.
    pub fn logs(&self) -> PathBuf {
        self.root.join(LOGS_DIRECTORY_NAME)
    }

    /// The Scratch Directory.
    pub fn scratch(&self) -> PathBuf {
        self.root.join(SCRATCH_DIRECTORY_NAME)
    }

    /// The Tools Directory, where the app keeps its own copies of ffmpeg,
    /// ffprobe and wtsexporter. ffmpeg and ffprobe are looked for on `PATH`
    /// first and here second; wtsexporter only here.
    pub fn tools(&self) -> PathBuf {
        self.root.join(TOOLS_DIRECTORY_NAME)
    }

    /// The Message Crate Directory and each directory in it that holds
    /// messages: every directory but the Tools Directory.
    pub fn private_directories(&self) -> [PathBuf; 6] {
        [
            self.root.clone(),
            self.data(),
            self.staging(),
            self.exports(),
            self.logs(),
            self.scratch(),
        ]
    }

    /// Make the Message Crate Directory and every directory in it, so each
    /// is there before anything is put in it, and a program put in the Tools
    /// Directory by hand has a place to go. The Message Crate Directory and
    /// the directories that hold messages are made, or tightened to be,
    /// readable by the user alone; the Tools Directory is left to the umask
    /// inside it.
    ///
    /// # Errors
    ///
    /// Returns the first directory that could not be made or tightened.
    /// Everything that could be made is made.
    pub fn make(&self) -> io::Result<()> {
        let mut first_error = None;
        for dir in &self.private_directories() {
            let made = file_io::create_dir_all_owner_only(dir)
                .and_then(|()| file_io::restrict_to_owner(dir).map(|_| ()))
                .map_err(|error| with_path(dir, error));
            if let Err(error) = made {
                first_error.get_or_insert(error);
            }
        }
        let tools = self.tools();
        if let Err(error) = std::fs::create_dir_all(&tools) {
            first_error.get_or_insert(with_path(&tools, error));
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// `error` with the directory it is about in its text.
fn with_path(dir: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", dir.display()))
}

/// The log of the Import Run whose directory in the Staging Directory is
/// `run_dir`: `import-<source>-<started>.log` in `logs_dir`, named after the
/// run's directory (`staging-<source>-<started>`), so a resumed run writes on
/// into the same log.
pub fn import_run_log(logs_dir: &Path, run_dir: &Path) -> PathBuf {
    let name = run_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let run = name.strip_prefix("staging-").unwrap_or(&name);
    logs_dir.join(format!("import-{run}.log"))
}

/// An Import Run's log, open for Staging or Media to add lines to. The
/// Upload's own writer appends to the same file. Every line carries its time
/// and level ([`message_crate_core::format_run_log`]).
#[derive(Debug, Clone)]
pub struct RunLog {
    /// The log, or `None` when it could not be opened: a run goes on without
    /// it, as its lines still reach the window.
    file: Option<Arc<Mutex<File>>>,
}

impl RunLog {
    /// The log of the run whose directory is `run_dir`, in `logs_dir`,
    /// opened to add to it. The Logs Directory is made, or kept, readable by
    /// the user alone ([`file_io::restrict_to_owner`]), because a run's log
    /// names the phone numbers and email addresses in its file names (#2097).
    pub fn open(logs_dir: &Path, run_dir: &Path) -> Self {
        let path = import_run_log(logs_dir, run_dir);
        let mut not_tightened = None;
        let file = file_io::create_dir_all_owner_only(logs_dir)
            .and_then(|()| {
                // A Logs Directory that cannot be tightened still takes the
                // log, which says so below.
                not_tightened = file_io::restrict_to_owner(logs_dir).err();
                File::options().create(true).append(true).open(&path)
            })
            .ok()
            .map(|file| Arc::new(Mutex::new(file)));
        let log = Self { file };
        if let Some(error) = not_tightened {
            log.warn(&format!(
                "Could not make {} readable by this user alone: {error}",
                logs_dir.display()
            ));
        }
        log
    }

    /// Add the error that stopped the stage to the log, as the window shows
    /// it.
    pub fn error(&self, error: &anyhow::Error) {
        self.write(RunLogLevel::Error, &format!("{error:#}"));
    }

    /// Say which account runs the run, on which server: the line the Logs
    /// panel reads to show an account only its own runs' logs.
    pub fn account(&self, account: &RunLogAccount) {
        self.write(RunLogLevel::Info, &account.line_text());
    }

    /// Add `line` to the log, as something the run did.
    pub fn line(&self, line: &str) {
        self.write(RunLogLevel::Info, line);
    }

    /// Add `text` to the log as a warning, one line per line of it: what the
    /// run went on without, or the output of a step that failed, such as a
    /// failed wtsexporter run's (#1938).
    pub fn warn(&self, text: &str) {
        self.write(RunLogLevel::Warn, text);
    }

    /// Add a row of the Import Run's record to the log, at the level its
    /// kind gives it: a skip as a warning, an error as an error, and a note
    /// as something the run did.
    pub fn issue(&self, issue: &RunIssue) {
        self.write(
            issue.kind.into(),
            &format!("{}: {}", issue.item, issue.reason),
        );
    }

    /// Add `text` at `level`. A line that cannot be written is dropped: the
    /// window has it.
    fn write(&self, level: RunLogLevel, text: &str) {
        if let Some(file) = &self.file {
            let mut file = file
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = file.write_all(format_run_log(level, text).as_bytes());
            let _ = file.flush();
        }
    }
}

/// What the app deletes when it starts, on a thread of its own so a large
/// leftover does not hold up the window: what killed runs left in the
/// Scratch Directory (decrypted databases, attachment payloads), and the
/// directory of every Export or Convert the app did not see to its end. A
/// run another app process has under way is kept.
pub fn sweep_at_start_up(directory: &MessageCrateDirectory) {
    message_crate_core::sweep_scratch(&directory.scratch());
    export_directories::sweep(&directory.exports());
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use message_crate_core::RunLogLevel;
    use message_crate_core::testutil::run_log_lines;

    use super::{MessageCrateDirectory, RunLog, import_run_log, sweep_at_start_up};

    /// The layout #1053 decided: every directory the app keeps is in
    /// `{home}/message-crate`, and a dev build's server has a Data Directory
    /// of its own beside the installed app's.
    #[test]
    fn every_directory_is_in_the_message_crate_directory() {
        let home = Path::new("/home/sam");
        let installed = MessageCrateDirectory::in_home(home, false);
        let root = Path::new("/home/sam/message-crate");
        assert_eq!(installed.root(), root);
        assert_eq!(installed.data(), root.join("data"));
        assert_eq!(installed.staging(), root.join("staging"));
        assert_eq!(installed.exports(), root.join("exports"));
        assert_eq!(installed.logs(), root.join("logs"));
        assert_eq!(installed.scratch(), root.join("scratch"));
        assert_eq!(installed.tools(), root.join("tools"));

        let dev = MessageCrateDirectory::in_home(home, true);
        assert_eq!(dev.data(), root.join("data-dev"));
        assert_eq!(dev.staging(), installed.staging());
    }

    #[test]
    fn the_app_makes_every_directory_at_start() {
        let home = tempfile::tempdir().unwrap();
        let directory = MessageCrateDirectory::in_home(home.path(), false);

        directory.make().unwrap();

        for dir in directory
            .private_directories()
            .into_iter()
            .chain([directory.tools()])
        {
            assert!(dir.is_dir(), "{} was not made", dir.display());
        }
    }

    /// The Message Crate Directory holds every account's messages, so it is
    /// the user's alone, and so is each directory in it that holds them: a
    /// new one is made so, and one other users could read is tightened
    /// (#2097, #2969).
    #[cfg(unix)]
    #[test]
    fn the_message_crate_directory_is_the_user_s_alone() {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let home = tempfile::tempdir().unwrap();
        let directory = MessageCrateDirectory::in_home(home.path(), false);
        let private = directory.private_directories();

        directory.make().unwrap();
        for dir in &private {
            assert_eq!(mode(dir), 0o700, "{} was made open", dir.display());
        }

        for dir in &private {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        directory.make().unwrap();
        for dir in &private {
            assert_eq!(mode(dir), 0o700, "{} was not tightened", dir.display());
        }
    }

    #[test]
    fn the_start_up_sweep_clears_the_scratch_directory() {
        let home = tempfile::tempdir().unwrap();
        let directory = MessageCrateDirectory::in_home(home.path(), false);
        let scratch = directory.scratch();
        // What a killed Apple Messages import and a killed SMS import left.
        let decrypted = scratch
            .join(message_crate_core::IMESSAGE_READER_DIRECTORY)
            .join("request-1");
        let spool = scratch
            .join(message_crate_core::ATTACHMENT_SPOOL_DIRECTORY)
            .join("request-2");
        for left in [&decrypted, &spool] {
            std::fs::create_dir_all(left).unwrap();
            std::fs::write(left.join("sms.db"), "decrypted").unwrap();
        }

        sweep_at_start_up(&directory);

        assert!(!decrypted.exists());
        assert!(!spool.exists());
    }

    #[test]
    fn staging_and_media_lines_go_into_the_run_s_log() {
        let logs = tempfile::tempdir().unwrap();
        let run = Path::new("/home/sam/message-crate/staging/staging-whatsapp-261004-143000");
        let staging = RunLog::open(logs.path(), run);
        staging.line("Conversations: 3");
        drop(staging);
        RunLog::open(logs.path(), run).line("Converting and compressing attachments…");

        assert_eq!(
            run_log_lines(&import_run_log(logs.path(), run)),
            [
                (RunLogLevel::Info, "Conversations: 3".to_string()),
                (
                    RunLogLevel::Info,
                    "Converting and compressing attachments…".to_string()
                ),
            ]
        );
    }

    /// A warning that holds several lines, such as a failed wtsexporter
    /// run's output, is one warning line per line of it, so the Logs panel's
    /// level filter finds each (#1938).
    #[test]
    fn a_warning_is_one_warning_line_per_line_of_it() {
        let logs = tempfile::tempdir().unwrap();
        let run = Path::new("/home/sam/message-crate/staging/staging-whatsapp-261004-143000");
        RunLog::open(logs.path(), run)
            .warn("wtsexporter failed (exit status: 1). Its output:\nOSError: [Errno 28] No space left on device");

        assert_eq!(
            run_log_lines(&import_run_log(logs.path(), run)),
            [
                (
                    RunLogLevel::Warn,
                    "wtsexporter failed (exit status: 1). Its output:".to_string()
                ),
                (
                    RunLogLevel::Warn,
                    "OSError: [Errno 28] No space left on device".to_string()
                ),
            ]
        );
    }

    #[test]
    fn an_import_run_s_log_is_named_for_its_run_in_the_logs_directory() {
        let logs = MessageCrateDirectory::in_home(Path::new("/home/sam"), false).logs();
        assert_eq!(
            import_run_log(
                &logs,
                Path::new("/home/sam/message-crate/staging/staging-iphone-ios-261004-143000")
            ),
            Path::new("/home/sam/message-crate/logs/import-iphone-ios-261004-143000.log")
        );
    }

    /// A run's log names the phone numbers and email addresses in its file
    /// names, so the Logs Directory is readable by the user alone: a new one
    /// is made so, and one other users could read is tightened (#2097).
    #[cfg(unix)]
    #[test]
    fn the_logs_directory_is_the_user_s_alone() {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let home = tempfile::tempdir().unwrap();
        let logs = MessageCrateDirectory::in_home(home.path(), false).logs();
        let run = Path::new("/home/sam/message-crate/staging/staging-iphone-ios-261004-143000");

        RunLog::open(&logs, run).line("made");
        assert_eq!(mode(&logs), 0o700);

        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o755)).unwrap();
        RunLog::open(&logs, run).line("tightened");
        assert_eq!(mode(&logs), 0o700);
    }
}
