//! Where the desktop app keeps what is not a Message Crate's data: the
//! Export Directory ([`crate::export_directories`]), the Logs Directory and
//! the Scratch Directory, each one directory in the app-data directory. #1053
//! moves them into the Message Crate Directory.
//!
//! - The **Logs Directory** holds each Import Run's log, named for the run,
//!   and keeps it after the run's directory in the Staging Directory is
//!   deleted. Nothing deletes a log.
//! - The **Scratch Directory** holds what a run writes that is neither its
//!   output nor kept: the attachment spool and the databases the Apple
//!   Messages Reader decrypts ([`message_crate_core::ScratchDir`]). What a
//!   killed run left there is deleted at start-up.

use std::path::{Path, PathBuf};

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

    use super::{import_run_log, logs_dir_in, scratch_dir_in, sweep_at_start_up};

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
