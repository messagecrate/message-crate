//! Where the desktop app keeps what is not a Message Crate's data: the
//! Export Directory ([`crate::export_directories`]), the Logs Directory and
//! the Scratch Directory, each one directory in the app-data directory. #1053
//! moves them into the Message Crate Directory.
//!
//! - The **Logs Directory** holds each Import Run's log, named for the run,
//!   and keeps it after the run's directory in the Staging Directory is
//!   deleted. Staging, Media and Upload all write into it ([`RunLog`] for the
//!   first two, the push library's own writer for the Upload). Nothing
//!   deletes a log.
//! - The **Scratch Directory** holds what a run writes that is neither its
//!   output nor kept: the attachment spool and the databases the Apple
//!   Messages Reader decrypts ([`message_crate_core::ScratchDir`]). What a
//!   killed run left there is deleted at start-up.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::export_directories::{self, EXPORT_DIRECTORY_NAME};

/// The Logs Directory's name in the app-data directory.
pub const LOGS_DIRECTORY_NAME: &str = "logs";

/// The Scratch Directory's name in the app-data directory.
pub const SCRATCH_DIRECTORY_NAME: &str = "scratch";

/// The Logs Directory in `app_data_dir`.
pub fn logs_dir_in(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(LOGS_DIRECTORY_NAME)
}

/// The Scratch Directory in `app_data_dir`.
pub fn scratch_dir_in(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SCRATCH_DIRECTORY_NAME)
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
/// Upload's own writer appends to the same file.
#[derive(Debug, Clone)]
pub struct RunLog {
    /// The log, or `None` when it could not be opened: a run goes on without
    /// it, as its lines still reach the window.
    file: Option<Arc<Mutex<File>>>,
}

impl RunLog {
    /// The log of the run whose directory is `run_dir`, in `logs_dir`,
    /// opened to add to it.
    pub fn open(logs_dir: &Path, run_dir: &Path) -> Self {
        let path = import_run_log(logs_dir, run_dir);
        let file = std::fs::create_dir_all(logs_dir)
            .and_then(|()| File::options().create(true).append(true).open(&path))
            .ok()
            .map(|file| Arc::new(Mutex::new(file)));
        Self { file }
    }

    /// Add `line` to the log. A line that cannot be written is dropped: the
    /// window has it.
    pub fn line(&self, line: &str) {
        if let Some(file) = &self.file {
            let mut file = file
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

/// What the app deletes when it starts, on a thread of its own so a large
/// leftover does not hold up the window: what killed runs left in the
/// Scratch Directory (decrypted databases, attachment payloads), and the
/// directory of every Export or Convert the app did not see to its end. A
/// run another app process has under way is kept.
pub fn sweep_at_start_up(app_data_dir: &Path) {
    message_crate_core::sweep_scratch(&scratch_dir_in(app_data_dir));
    export_directories::sweep(&app_data_dir.join(EXPORT_DIRECTORY_NAME));
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{RunLog, import_run_log, logs_dir_in, scratch_dir_in, sweep_at_start_up};

    #[test]
    fn the_start_up_sweep_clears_the_scratch_directory() {
        let app_data = tempfile::tempdir().unwrap();
        let scratch = scratch_dir_in(app_data.path());
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

        sweep_at_start_up(app_data.path());

        assert!(!decrypted.exists());
        assert!(!spool.exists());
        assert!(scratch.starts_with(app_data.path()));
    }

    #[test]
    fn staging_and_media_lines_go_into_the_run_s_log() {
        let logs = tempfile::tempdir().unwrap();
        let run = Path::new("/home/sam/message-crate/staging-whatsapp-261004-143000");
        let staging = RunLog::open(logs.path(), run);
        staging.line("Conversations: 3");
        drop(staging);
        RunLog::open(logs.path(), run).line("Converting and compressing attachments…");

        let text = std::fs::read_to_string(import_run_log(logs.path(), run)).unwrap();
        assert_eq!(
            text,
            "Conversations: 3\nConverting and compressing attachments…\n"
        );
    }

    #[test]
    fn an_import_run_s_log_is_named_for_its_run_in_the_logs_directory() {
        let logs = logs_dir_in(Path::new("/app-data"));
        assert_eq!(
            import_run_log(
                &logs,
                Path::new("/home/sam/message-crate/staging-iphone-ios-261004-143000")
            ),
            Path::new("/app-data/logs/import-iphone-ios-261004-143000.log")
        );
    }
}
