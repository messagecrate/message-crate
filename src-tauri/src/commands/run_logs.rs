//! Commands for the Import Run logs in the Logs Directory: the account line
//! a new run's log starts with, and listing and reading the logs for the
//! Logs panel and Settings → Storage ([`crate::run_logs`]).

use std::path::Path;

use message_crate_core::RunLogAccount;

use super::paths::logs_dir;
use crate::app_directories::RunLog;
use crate::run_logs::{self, LinesPage, LinesQuery, Reader, RunLogEntry};

/// Arguments for [`start_import_run_log`].
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunLogArgs {
    /// The run's directory in the Staging Directory, which names its log.
    pub run_dir: String,
    /// Who runs the run, on which server.
    pub account: RunLogAccount,
}

/// Start the log of a new Import Run with the line that says which account
/// runs it, on which server. The window calls it once the server has created
/// the run, before Staging writes anything.
///
/// # Errors
///
/// Returns an error when the operating system names no app-data directory.
#[tauri::command]
pub fn start_import_run_log(app: tauri::AppHandle, args: StartRunLogArgs) -> Result<(), String> {
    RunLog::open(&logs_dir(&app)?, Path::new(args.run_dir.trim())).account(&args.account);
    Ok(())
}

/// The Import Run logs on this computer that `reader` may read: every one for
/// the owner, the signed-in account's own otherwise. The most recently
/// written first.
///
/// # Errors
///
/// Returns an error when the Logs Directory cannot be read.
#[tauri::command(async)]
pub fn list_import_run_logs(
    app: tauri::AppHandle,
    reader: Reader,
) -> Result<Vec<RunLogEntry>, String> {
    run_logs::list(&logs_dir(&app)?, &reader)
        .map_err(|error| format!("Could not list the Import Run logs: {error}"))
}

/// Arguments for [`read_import_run_log_lines`].
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadLinesArgs {
    /// Who is asking.
    pub reader: Reader,
    /// The log's name in the Logs Directory.
    pub name: String,
    /// The page asked for.
    pub query: LinesQuery,
}

/// A page of one Import Run log's lines, newest first.
///
/// # Errors
///
/// Returns an error when the log is not one `reader` may read, or cannot be
/// read.
#[tauri::command(async)]
pub fn read_import_run_log_lines(
    app: tauri::AppHandle,
    args: ReadLinesArgs,
) -> Result<LinesPage, String> {
    run_logs::read_lines(&logs_dir(&app)?, &args.reader, &args.name, &args.query)
}

/// Arguments for [`read_import_run_log`].
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadLogArgs {
    /// Who is asking.
    pub reader: Reader,
    /// The log's name in the Logs Directory.
    pub name: String,
}

/// One Import Run log whole, as it is on disk, for the window to save.
///
/// # Errors
///
/// Returns an error when the log is not one `reader` may read, or cannot be
/// read.
#[tauri::command(async)]
pub fn read_import_run_log(app: tauri::AppHandle, args: ReadLogArgs) -> Result<String, String> {
    run_logs::read_whole(&logs_dir(&app)?, &args.reader, &args.name)
}
