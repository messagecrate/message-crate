//! `export` command — export messages from a Message Crate server.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use message_crate_core::count_of;
use message_crate_export::{
    DEFAULT_ASSET_FETCH_WORKERS, DEFAULT_PAGE_LIMIT, ExportConfig, ExportQueryList, ProgressEvent,
    run as run_export,
};

use super::events;
use super::jobs::{spawn_job, start_job};
use crate::state::{AppState, JobName};

/// User-facing parameters for the `export` command.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportArgs {
    /// Base URL of the server, for example `http://127.0.0.1:8080`.
    pub base_url: String,
    /// The logged-in Session's token, sent as the bearer token. Never an API
    /// Token, and never a password.
    pub token: String,
    /// Directory the exported conversation files are written into.
    pub out_dir: String,
    /// Search query selecting what to export. Blank exports everything.
    pub query: String,
    /// The list the query is for, `conversations` or `messages`: every
    /// message of the conversations the query shows, or the messages it
    /// matches.
    pub list: ExportQueryList,
    /// When true, skip attachments and export messages only.
    pub skip_attachments: bool,
}

/// Ask this process to export conversations from a server.
///
/// Returns as soon as the background thread starts. Log lines and the final
/// summary use the same `desktop-job:log` / `desktop-job:finished` /
/// `desktop-job:error` events as the `extract` command.
///
/// # Errors
///
/// Returns an error if another job is running, or if another thread panicked
/// while holding the shared state lock. Failures during the Export are
/// sent as `desktop-job:error`.
#[tauri::command(async)]
pub fn export(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    args: ExportArgs,
) -> Result<(), String> {
    let job = start_job(&state, JobName::Export)?;
    let cancel = job.cancel_flag();

    let app_handle = app.clone();
    spawn_job(app, job, move || {
        let cfg = ExportConfig {
            out_dir: PathBuf::from(&args.out_dir),
            base_url: args.base_url,
            token: args.token,
            query: args.query,
            list: args.list,
            skip_attachments: args.skip_attachments,
            page_limit: DEFAULT_PAGE_LIMIT,
            cancel,
            asset_fetch_workers: DEFAULT_ASSET_FETCH_WORKERS,
        };

        let mut progress = |event: ProgressEvent| match event {
            ProgressEvent::Log(line) => {
                events::emit(&app_handle, events::LOG, line);
            }
            ProgressEvent::Auth { .. } => {}
            ProgressEvent::Page {
                messages,
                total_so_far,
            } => {
                events::emit(
                    &app_handle,
                    events::LOG,
                    fetched_line(messages, total_so_far),
                );
            }
            ProgressEvent::Done(_) => {}
        };

        let report = run_export(&cfg, Some(&mut progress))?;
        Ok(finished_line(report.messages, report.conversations))
    });

    Ok(())
}

/// The log line for one page of messages fetched, `total` being every
/// message fetched so far.
fn fetched_line(messages: usize, total: u64) -> String {
    format!(
        "Fetched {} ({total} so far)",
        count_of(messages as u64, "message", "messages")
    )
}

/// The line that ends an Export from a server.
fn finished_line(messages: u64, conversations: u64) -> String {
    format!(
        "Export complete: {} in {}",
        count_of(messages, "message", "messages"),
        count_of(conversations, "conversation", "conversations")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each count is singular for one and plural otherwise, never `(s)`
    /// (#1815).
    #[test]
    fn the_lines_count_one_and_many() {
        assert_eq!(fetched_line(1, 1), "Fetched 1 message (1 so far)");
        assert_eq!(
            fetched_line(500, 1200),
            "Fetched 500 messages (1200 so far)"
        );
        assert_eq!(
            finished_line(1, 1),
            "Export complete: 1 message in 1 conversation"
        );
        assert_eq!(
            finished_line(3, 2),
            "Export complete: 3 messages in 2 conversations"
        );
    }
}
