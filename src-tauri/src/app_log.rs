//! The desktop app's own log: `desktop-app.log` in the Logs Directory, for
//! what the app does and fails at outside an Import Run, such as an event no
//! window received or a download whose record could not be written (#2185).
//!
//! A desktop launches the app with standard error thrown away, so a line
//! written only there is lost. Every line goes here instead, in the line
//! format of the server's log and the Import Run logs
//! (`message-crate-log-lines`), so the Logs panel reads it with the same
//! viewer, as "Desktop app". Rules: `docs/architecture/import-run-logs.md`,
//! "The desktop app's log".
//!
//! The log is opened when its first line is written, so a computer where
//! the app never had anything to say has no file, and the Logs panel lists
//! none. A line written before [`init`] names the Logs Directory, or one that
//! cannot be written there, goes to standard error, which is the one place
//! left for it.
//!
//! A line names only paths inside Message Crate's own directories, and no
//! phone's name. Why: the log is listed for the owner of any Message Crate
//! the app signs in to, and a backup's path or a phone's name can name the
//! person whose phone it is.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use message_crate_core::{RunLogLevel, format_run_log};

use crate::app_directories::{not_tightened_line, open_in_logs_directory};

/// The desktop app's log's name in the Logs Directory.
pub const DESKTOP_APP_LOG: &str = "desktop-app.log";

/// The log every line of the app goes to, once [`init`] names it.
static APP_LOG: OnceLock<AppLog> = OnceLock::new();

/// Name the Logs Directory the app's log is in. Called once, when the app
/// starts; a later call changes nothing.
pub fn init(logs_dir: &Path) {
    let _ = APP_LOG.set(AppLog::in_logs_dir(logs_dir));
}

/// Add `text` to the app's log as a warning: something the app went on
/// without.
pub fn warn(text: &str) {
    write(RunLogLevel::Warn, text);
}

/// Add `text` at `level` to the app's log, or to standard error before
/// [`init`].
fn write(level: RunLogLevel, text: &str) {
    match APP_LOG.get() {
        Some(log) => log.write(level, text),
        None => eprint!("{}", format_run_log(level, text)),
    }
}

/// The desktop app's log in one Logs Directory.
#[derive(Debug)]
pub struct AppLog {
    /// The Logs Directory.
    logs_dir: PathBuf,
    /// The log, open to add to, once a line has been written.
    file: Mutex<Option<File>>,
}

impl AppLog {
    /// The app's log in `logs_dir`. Nothing is opened yet.
    pub fn in_logs_dir(logs_dir: &Path) -> Self {
        Self {
            logs_dir: logs_dir.to_path_buf(),
            file: Mutex::new(None),
        }
    }

    /// The log's path.
    pub fn path(&self) -> PathBuf {
        self.logs_dir.join(DESKTOP_APP_LOG)
    }

    /// Add `text` at `level`, opening the log first if no line has been
    /// written yet ([`open_in_logs_directory`]), with a warning before it
    /// when the Logs Directory could not be made readable by the user alone.
    /// A line that cannot be written goes to standard error.
    pub fn write(&self, level: RunLogLevel, text: &str) {
        let mut lines = String::new();
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if file.is_none()
            && let Ok((opened, not_tightened)) =
                open_in_logs_directory(&self.logs_dir, &self.path())
        {
            if let Some(error) = not_tightened {
                lines.push_str(&format_run_log(
                    RunLogLevel::Warn,
                    &not_tightened_line(&self.logs_dir, &error),
                ));
            }
            *file = Some(opened);
        }
        lines.push_str(&format_run_log(level, text));
        let written = file
            .as_mut()
            .is_some_and(|file| file.write_all(lines.as_bytes()).is_ok() && file.flush().is_ok());
        if !written {
            eprint!("{lines}");
        }
    }
}

#[cfg(test)]
mod tests {
    use message_crate_core::RunLogLevel;
    use message_crate_core::testutil::run_log_lines;

    use super::{AppLog, DESKTOP_APP_LOG};

    /// Each line is written in the shared line format, as a warning, into
    /// `desktop-app.log` in the Logs Directory, which is made for it.
    #[test]
    fn a_line_goes_into_desktop_app_log_in_the_logs_directory() {
        let home = tempfile::tempdir().unwrap();
        let logs = home.path().join("message-crate").join("logs");
        let log = AppLog::in_logs_dir(&logs);

        log.write(
            RunLogLevel::Warn,
            "The server's processes will not end with it",
        );
        log.write(
            RunLogLevel::Warn,
            "The desktop app could not send the desktop-job:log event to its window",
        );

        assert_eq!(log.path(), logs.join(DESKTOP_APP_LOG));
        assert_eq!(
            run_log_lines(&log.path()),
            [
                (
                    RunLogLevel::Warn,
                    "The server's processes will not end with it".to_string()
                ),
                (
                    RunLogLevel::Warn,
                    "The desktop app could not send the desktop-job:log event to its window"
                        .to_string()
                ),
            ]
        );
    }

    /// Nothing is written until a line is: an app with nothing to say leaves
    /// no empty log for the Logs panel to list.
    #[test]
    fn no_file_is_made_before_the_first_line() {
        let logs = tempfile::tempdir().unwrap();
        let log = AppLog::in_logs_dir(logs.path());

        assert!(!log.path().exists());
    }

    /// A second start of the app writes on into the same log.
    #[test]
    fn a_later_start_adds_to_the_same_log() {
        let logs = tempfile::tempdir().unwrap();
        AppLog::in_logs_dir(logs.path()).write(RunLogLevel::Warn, "first");
        let log = AppLog::in_logs_dir(logs.path());
        log.write(RunLogLevel::Warn, "second");

        assert_eq!(
            run_log_lines(&log.path()),
            [
                (RunLogLevel::Warn, "first".to_string()),
                (RunLogLevel::Warn, "second".to_string()),
            ]
        );
    }

    /// The app's log is in the Logs Directory with the run logs, which name
    /// phone numbers, so the directory is the user's alone (#2097).
    #[cfg(unix)]
    #[test]
    fn the_logs_directory_stays_the_user_s_alone() {
        use std::os::unix::fs::PermissionsExt as _;

        let home = tempfile::tempdir().unwrap();
        let logs = home.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o755)).unwrap();

        AppLog::in_logs_dir(&logs).write(RunLogLevel::Warn, "tightened");

        let mode = std::fs::metadata(&logs).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
