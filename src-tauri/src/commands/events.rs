//! JSON shapes sent to the UI as Tauri events.
//!
//! The core library has a similar error type, but it cannot be sent through
//! Tauri because it is not serializable. Each struct has the same name and
//! fields as its TypeScript type in `web/src/lib/types.ts`.
//!
//! Every desktop job (an Import Run's stages, an Export, a Convert) reports on
//! the same `desktop-job:*` channels. The payloads only an Import Run sends are
//! named `Import*Event`; the error every job can send is
//! [`DesktopJobErrorEvent`].

use message_crate_core::{
    IssueSink, LogSink, ProgressEvent, ProgressSink, RunIssue, RunIssueKind, WriteStatus,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::tool_downloads::Program;

/// One log line for the UI's log panel. Payload: `String`.
pub const LOG: &str = "desktop-job:log";
/// Progress-bar numbers. Payload: [`ImportProgressEvent`].
pub const PROGRESS: &str = "desktop-job:progress";
/// One skipped or failed item for the Import Errors list, sent as the job
/// records it. Payload: an issue row.
pub const ISSUE: &str = "desktop-job:issue";
/// The Upload finished with one conversation file. Payload:
/// [`ImportFileDoneEvent`].
pub const FILE_DONE: &str = "desktop-job:file-done";
/// Staging's write queue finished with one conversation file. Payload:
/// [`ImportFileWrittenEvent`].
pub const FILE_WRITTEN: &str = "desktop-job:file-written";
/// The job finished. Payload: the summary line or JSON the screen shows.
pub const FINISHED: &str = "desktop-job:finished";
/// The job failed before it could finish. Payload: [`DesktopJobErrorEvent`].
pub const ERROR: &str = "desktop-job:error";

/// Send one event to the UI. An emit fails only when no window is left to
/// receive it; the job carries on, and the miss goes to the process log so it
/// is not silent.
pub fn emit(app: &AppHandle, event: &str, payload: impl Serialize + Clone) {
    if let Err(error) = app.emit(event, payload) {
        eprintln!("{}", undelivered_line(event, &error));
    }
}

/// The process log's line for an event no window received.
fn undelivered_line(event: &str, error: &dyn std::fmt::Display) -> String {
    format!("The desktop app could not send the {event} event to its window: {error}")
}

/// An issue sink that sends each row to the window as `desktop-job:issue` the
/// moment the job records it, and adds it to the Import Run's log, so the log
/// holds what was skipped or failed and the Logs panel's level filter finds
/// it.
pub fn run_issue_sink(app: &AppHandle, run_log: &crate::app_directories::RunLog) -> IssueSink {
    let app = app.clone();
    let run_log = run_log.clone();
    IssueSink::new(move |issue| {
        run_log.issue(&issue);
        emit(&app, ISSUE, ImportIssueEvent::from(&issue));
    })
}

/// A progress sink that sends each count to the window as
/// `desktop-job:progress`, and each conversation file the write queue finishes
/// as `desktop-job:file-written`.
pub fn progress_sink(app: &AppHandle) -> ProgressSink {
    let app = app.clone();
    ProgressSink::new(move |event| match WindowEvent::from(event) {
        WindowEvent::Progress(progress) => emit(&app, PROGRESS, progress),
        WindowEvent::FileWritten(written) => emit(&app, FILE_WRITTEN, written),
    })
}

/// What one of the exporters' progress events becomes in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    /// A count for the progress bar (`desktop-job:progress`).
    Progress(ImportProgressEvent),
    /// A conversation file the write queue finished
    /// (`desktop-job:file-written`).
    FileWritten(ImportFileWrittenEvent),
}

/// Progress numbers the UI uses to update the progress bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportProgressEvent {
    /// Current pipeline stage: `setup`, `parse`, `attachments`, `prepare`,
    /// `check`, `media`, or `upload`.
    pub step: String,
    /// Number of items finished so far.
    pub done: usize,
    /// Total items, or 0 when the total is unknown.
    pub total: usize,
    /// Bytes finished on the attachments step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_done: Option<u64>,
    /// Byte total on the attachments step (grows when a size was unknown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_total: Option<u64>,
    /// Extra step status the UI shows. On `setup` it is the step's label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The program the run waits for while it downloads (#1053). The event
    /// then has no counts, `bytes_done` is how much has arrived and
    /// `bytes_total` the file's size when the server said, and the window
    /// says "Waiting for the <program> download" with them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<Program>,
}

impl ImportProgressEvent {
    /// A count-only event for `step`, with no bytes and no status.
    fn counts(step: &str, done: usize, total: usize) -> Self {
        Self {
            step: step.into(),
            done,
            total,
            bytes_done: None,
            bytes_total: None,
            status: None,
            waiting: None,
        }
    }
}

/// The exporters' typed progress event, in the shape the UI listens for.
/// The stage name becomes `step`; byte counts ride along on `attachments`
/// and the setup label rides along as `status`. A finished conversation
/// file is not a count, and goes to its own event.
impl From<ProgressEvent> for WindowEvent {
    fn from(event: ProgressEvent) -> Self {
        let counts = ImportProgressEvent::counts;
        Self::Progress(match event {
            ProgressEvent::Setup { label, step, total } => ImportProgressEvent {
                status: Some(label),
                ..counts("setup", step, total)
            },
            ProgressEvent::Parse { done, total } => counts("parse", done, total),
            ProgressEvent::Attachments {
                done,
                total,
                bytes_done,
                bytes_total,
            } => ImportProgressEvent {
                bytes_done: Some(bytes_done),
                bytes_total: Some(bytes_total),
                ..counts("attachments", done, total)
            },
            ProgressEvent::Prepare { done, total } => counts("prepare", done, total),
            ProgressEvent::Media { done, total } => counts("media", done, total),
            ProgressEvent::FileWritten { file, status } => {
                return Self::FileWritten(ImportFileWrittenEvent { file, status });
            }
        })
    }
}

/// One row of the Import Run's issues, from an exporter's or the Media
/// stage's [`RunIssue`], or from the Upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportIssueEvent {
    /// What the row is, sent as its lowercase word.
    pub kind: RunIssueKind,
    /// The step that raised it, such as `attachments`.
    pub step: String,
    /// What was affected.
    pub item: String,
    /// Why, in one sentence.
    pub reason: String,
    /// The conversation file an Upload row, or a Staging row recorded while
    /// the write queue wrote that file, is about, which tells the window
    /// whether a resumed Upload or Staging reports the row again. Left out
    /// of the JSON for every other row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
}

impl From<&RunIssue> for ImportIssueEvent {
    fn from(issue: &RunIssue) -> Self {
        Self {
            kind: issue.kind,
            step: issue.step.clone(),
            item: issue.item.clone(),
            reason: issue.reason.clone(),
            conversation: issue.conversation.clone(),
        }
    }
}

/// The Upload finished with one conversation file. The window drops from
/// the run record the rows of an earlier stop about a conversation that is
/// now on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportFileDoneEvent {
    /// The conversation file, as the Upload's issue rows name it.
    pub file: String,
    /// `ok` (sent now), `skipped` (an earlier part of the run sent it), or
    /// `failed`.
    pub status: String,
}

/// Staging's write queue finished with one conversation file. The window
/// keeps a Staging row about a conversation apart until its file is written,
/// and drops an earlier part's row about a file this Staging wrote again
/// (#1688).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportFileWrittenEvent {
    /// The conversation file, as Staging's issue rows and the Upload name it.
    pub file: String,
    /// What the write queue did with the file: `written` or `skipped`, as
    /// `ImportFileWrittenEvent.status` names it.
    #[serde(serialize_with = "write_status")]
    pub status: WriteStatus,
}

/// A [`WriteStatus`] as the window names it.
fn write_status<S: serde::Serializer>(status: &WriteStatus, out: S) -> Result<S::Ok, S::Error> {
    out.serialize_str(match status {
        WriteStatus::Written => "written",
        WriteStatus::Skipped => "skipped",
    })
}

/// Failure details for the `desktop-job:error` event.
///
/// When `user_message` is missing, it is left out of the JSON so the
/// TypeScript type can treat it as optional.
#[derive(Debug, Clone, Serialize)]
pub struct DesktopJobErrorEvent {
    /// Full error chain, for logs and the advanced-details view.
    pub detail: String,
    /// Friendlier message for the UI, when one is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

/// A job's error, with its full chain as `detail` and no friendlier message.
impl From<anyhow::Error> for DesktopJobErrorEvent {
    fn from(err: anyhow::Error) -> Self {
        Self {
            detail: format!("{err:#}"),
            user_message: None,
        }
    }
}

/// Send `line` to the window's log and add it to the Import Run's log, as
/// [`run_log_sink`] does.
pub(crate) fn log_to_run(app: &AppHandle, run_log: &crate::app_directories::RunLog, line: String) {
    run_log_sink(app, run_log).emit(&line);
}

/// The run's log sink: each line and each warning goes into the Import
/// Run's log, at its own level, and to the window's log.
pub(crate) fn run_log_sink(app: &AppHandle, run_log: &crate::app_directories::RunLog) -> LogSink {
    let app = app.clone();
    log_sink_into(run_log.clone(), move |text| emit(&app, LOG, text))
}

/// A sink that adds each line to `run_log` as something the run did and each
/// warning as a warning, and hands both to `window`.
fn log_sink_into<W>(run_log: crate::app_directories::RunLog, window: W) -> LogSink
where
    W: Fn(String) + Clone + Send + Sync + 'static,
{
    let line_log = run_log.clone();
    let line_window = window.clone();
    LogSink::new(move |line: &str| {
        line_log.line(line);
        line_window(line.to_string());
    })
    .with_warnings(move |text: &str| {
        run_log.warn(text);
        window(text.to_string());
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The run's sink writes a line at `INFO` and a warning, such as a failed
    /// wtsexporter run's output, at `WARN`, one line per line of it (#1938).
    #[test]
    fn the_run_s_sink_logs_a_warning_at_warning_level() {
        use crate::app_directories::{RunLog, import_run_log};
        use message_crate_core::RunLogLevel;
        let logs = tempfile::tempdir().unwrap();
        let run = std::path::Path::new("/home/sam/message-crate/staging-whatsapp-261004-143000");
        let sink = log_sink_into(RunLog::open(logs.path(), run), |_| {});

        sink.emit("Reading the backup");
        sink.warn("wtsexporter failed (exit status: 1). Its output:\nOSError: [Errno 28]");

        let text = std::fs::read_to_string(import_run_log(logs.path(), run)).unwrap();
        let levels: Vec<_> = text
            .lines()
            .map(|raw| {
                message_crate_core::parse_run_log_line(0, raw)
                    .unwrap()
                    .level
            })
            .collect();
        assert_eq!(
            levels,
            [RunLogLevel::Info, RunLogLevel::Warn, RunLogLevel::Warn]
        );
    }

    /// The line for an event no window received is a sentence, with no
    /// `warning:` prefix (#1889).
    #[test]
    fn an_undelivered_event_is_a_sentence() {
        assert_eq!(
            undelivered_line(LOG, &"no window"),
            "The desktop app could not send the desktop-job:log event to its window: no window"
        );
    }

    /// The window's progress payload for one of the counting events.
    fn progress(event: ProgressEvent) -> ImportProgressEvent {
        match WindowEvent::from(event) {
            WindowEvent::Progress(progress) => progress,
            other => panic!("not a count: {other:?}"),
        }
    }

    #[test]
    fn typed_events_map_onto_the_ui_steps() {
        let setup = progress(ProgressEvent::Setup {
            label: "Deriving backup keys".into(),
            step: 1,
            total: 5,
        });
        assert_eq!(setup.step, "setup");
        assert_eq!((setup.done, setup.total), (1, 5));
        assert_eq!(setup.status.as_deref(), Some("Deriving backup keys"));
        assert_eq!(setup.bytes_done, None);

        let parse = progress(ProgressEvent::Parse {
            done: 500,
            total: 12_345,
        });
        assert_eq!(parse.step, "parse");
        assert_eq!((parse.done, parse.total), (500, 12_345));
        assert_eq!(parse.status, None);

        let attachments = progress(ProgressEvent::Attachments {
            done: 2,
            total: 3,
            bytes_done: 100,
            bytes_total: 500,
        });
        assert_eq!(attachments.step, "attachments");
        assert_eq!((attachments.done, attachments.total), (2, 3));
        assert_eq!(attachments.bytes_done, Some(100));
        assert_eq!(attachments.bytes_total, Some(500));

        let prepare = progress(ProgressEvent::Prepare { done: 2, total: 3 });
        assert_eq!(prepare.step, "prepare");
        assert_eq!((prepare.done, prepare.total), (2, 3));

        let media = progress(ProgressEvent::Media { done: 1, total: 4 });
        assert_eq!(media.step, "media");
        assert_eq!((media.done, media.total), (1, 4));
    }

    #[test]
    fn serialized_event_omits_absent_fields() {
        let json =
            serde_json::to_value(progress(ProgressEvent::Prepare { done: 0, total: 3 })).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "step": "prepare", "done": 0, "total": 3 })
        );
    }

    /// A conversation file the write queue finished reaches the window as
    /// its own event, which says whether it was written now (#1688).
    #[test]
    fn a_finished_conversation_file_is_sent_apart_from_the_counts() {
        for (written, status) in [
            (WriteStatus::Written, "written"),
            (WriteStatus::Skipped, "skipped"),
        ] {
            let event = WindowEvent::from(ProgressEvent::FileWritten {
                file: "c.jsonl".into(),
                status: written,
            });
            let WindowEvent::FileWritten(written) = event else {
                panic!("not a finished file: {event:?}");
            };
            assert_eq!(
                serde_json::to_value(written).unwrap(),
                serde_json::json!({ "file": "c.jsonl", "status": status })
            );
        }
    }

    /// A Staging row names the conversation file it was recorded while
    /// writing, so the window can tell whether a resumed Staging reports
    /// it again (#1688).
    #[test]
    fn a_staging_row_keeps_its_conversation() {
        let json = serde_json::to_value(ImportIssueEvent::from(&RunIssue {
            kind: message_crate_core::RunIssueKind::Error,
            step: "attachments".into(),
            item: "/backup/IMG_0001.MOV".into(),
            reason: "could not be decrypted: No space left on device".into(),
            conversation: Some("c.jsonl".into()),
        }))
        .unwrap();
        assert_eq!(json["conversation"], "c.jsonl");
    }

    /// An exporter's issue reaches the screen in the shape the upload's
    /// issues have, so the Import Run lists both the same way.
    #[test]
    fn a_run_issue_is_sent_as_an_import_issue() {
        let json = serde_json::to_value(ImportIssueEvent::from(&RunIssue {
            kind: message_crate_core::RunIssueKind::Error,
            step: "attachments".into(),
            item: "/backup/IMG_0001.MOV".into(),
            reason: "could not be decrypted: No space left on device".into(),
            conversation: None,
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "error",
                "step": "attachments",
                "item": "/backup/IMG_0001.MOV",
                "reason": "could not be decrypted: No space left on device",
            })
        );
    }
}
