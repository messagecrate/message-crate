//! JSON shapes sent to the UI as Tauri events.
//!
//! The core library has a similar error type, but it cannot be sent through
//! Tauri because it is not serializable. These structs match the TypeScript
//! types in `web/src/lib/types.ts`.

use message_crate_core::{IssueSink, ProgressEvent, ProgressSink, RunIssue};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// One log line for the UI's log panel. Payload: `String`.
pub const LOG: &str = "extract:log";
/// Progress-bar numbers. Payload: [`ExtractProgressEvent`].
pub const PROGRESS: &str = "extract:progress";
/// One skipped or failed item for the Import Errors list, sent as the job
/// records it. Payload: an issue row.
pub const ISSUE: &str = "extract:issue";
/// The Upload finished with one conversation file. Payload:
/// [`ExtractFileDoneEvent`].
pub const FILE_DONE: &str = "extract:file-done";
/// Staging's write queue finished with one conversation file. Payload:
/// [`ExtractFileWrittenEvent`].
pub const FILE_WRITTEN: &str = "extract:file-written";
/// The job finished. Payload: the summary line or JSON the screen shows.
pub const FINISHED: &str = "extract:finished";
/// The job failed before it could finish. Payload: [`ExtractErrorEvent`].
pub const ERROR: &str = "extract:error";

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

/// An issue sink that sends each row to the window as `extract:issue` the
/// moment the job records it.
pub fn issue_sink(app: &AppHandle) -> IssueSink {
    let app = app.clone();
    IssueSink::new(move |issue| emit(&app, ISSUE, ExtractIssueEvent::from(&issue)))
}

/// A progress sink that sends each count to the window as
/// `extract:progress`, and each conversation file the write queue finishes
/// as `extract:file-written`.
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
    /// A count for the progress bar (`extract:progress`).
    Progress(ExtractProgressEvent),
    /// A conversation file the write queue finished (`extract:file-written`).
    FileWritten(ExtractFileWrittenEvent),
}

/// Progress numbers the UI uses to update the progress bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractProgressEvent {
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
}

impl ExtractProgressEvent {
    /// A count-only event for `step`, with no bytes and no status.
    fn counts(step: &str, done: usize, total: usize) -> Self {
        Self {
            step: step.into(),
            done,
            total,
            bytes_done: None,
            bytes_total: None,
            status: None,
        }
    }
}

/// The exporters' typed progress event, in the shape the UI listens for.
/// The stage name becomes `step`; byte counts ride along on `attachments`
/// and the setup label rides along as `status`. A finished conversation
/// file is not a count, and goes to its own event.
impl From<ProgressEvent> for WindowEvent {
    fn from(event: ProgressEvent) -> Self {
        let counts = ExtractProgressEvent::counts;
        Self::Progress(match event {
            ProgressEvent::Setup { label, step, total } => ExtractProgressEvent {
                status: Some(label),
                ..counts("setup", step, total)
            },
            ProgressEvent::Parse { done, total } => counts("parse", done, total),
            ProgressEvent::Attachments {
                done,
                total,
                bytes_done,
                bytes_total,
            } => ExtractProgressEvent {
                bytes_done: Some(bytes_done),
                bytes_total: Some(bytes_total),
                ..counts("attachments", done, total)
            },
            ProgressEvent::Prepare { done, total } => counts("prepare", done, total),
            ProgressEvent::Media { done, total } => counts("media", done, total),
            ProgressEvent::FileDone { file, skipped } => {
                return Self::FileWritten(ExtractFileWrittenEvent {
                    file,
                    status: if skipped { "skipped" } else { "written" }.into(),
                });
            }
        })
    }
}

/// One row of the Import Run's issues, from an exporter's or the Media
/// stage's [`RunIssue`], or from the Upload. Matches `ImportIssueEvent` in
/// `web/src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractIssueEvent {
    /// `skip` when the item was left out, `error` when it failed.
    pub kind: String,
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

impl From<&RunIssue> for ExtractIssueEvent {
    fn from(issue: &RunIssue) -> Self {
        Self {
            kind: issue.kind.clone(),
            step: issue.step.clone(),
            item: issue.item.clone(),
            reason: issue.reason.clone(),
            conversation: issue.conversation.clone(),
        }
    }
}

/// The Upload finished with one conversation file. Matches
/// `ImportFileDoneEvent` in `web/src/lib/types.ts`. The window drops from
/// the run record the rows of an earlier stop about a conversation that is
/// now on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractFileDoneEvent {
    /// The conversation file, as the Upload's issue rows name it.
    pub file: String,
    /// `ok` (sent now), `skipped` (an earlier part of the run sent it), or
    /// `failed`.
    pub status: String,
}

/// Staging's write queue finished with one conversation file. Matches
/// `ImportFileWrittenEvent` in `web/src/lib/types.ts`. The window keeps a
/// Staging row about a conversation apart until its file is written, and
/// drops an earlier part's row about a file this Staging wrote again
/// (#1688).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractFileWrittenEvent {
    /// The conversation file, as Staging's issue rows and the Upload name it.
    pub file: String,
    /// `written` (written now) or `skipped` (an earlier part of the run had
    /// written it to the end, and it was not read again).
    pub status: String,
}

/// Failure details for the `extract:error` event.
///
/// When `user_message` is missing, it is left out of the JSON so the
/// TypeScript type can treat it as optional.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractErrorEvent {
    /// Full error chain, for logs and the advanced-details view.
    pub detail: String,
    /// Friendlier message for the UI, when one is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

/// A job's error, with its full chain as `detail` and no friendlier message.
impl From<anyhow::Error> for ExtractErrorEvent {
    fn from(err: anyhow::Error) -> Self {
        Self {
            detail: format!("{err:#}"),
            user_message: None,
        }
    }
}

/// Send `line` to the window's log and add it to the Import Run's log.
pub(crate) fn log_to_run(app: &AppHandle, run_log: &crate::app_directories::RunLog, line: String) {
    run_log.line(&line);
    emit(app, LOG, line);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line for an event no window received is a sentence, with no
    /// `warning:` prefix (#1889).
    #[test]
    fn an_undelivered_event_is_a_sentence() {
        assert_eq!(
            undelivered_line(LOG, &"no window"),
            "The desktop app could not send the extract:log event to its window: no window"
        );
    }

    /// The window's progress payload for one of the counting events.
    fn progress(event: ProgressEvent) -> ExtractProgressEvent {
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
        for (skipped, status) in [(false, "written"), (true, "skipped")] {
            let event = WindowEvent::from(ProgressEvent::FileDone {
                file: "c.jsonl".into(),
                skipped,
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
        let json = serde_json::to_value(ExtractIssueEvent::from(&RunIssue {
            kind: "error".into(),
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
        let json = serde_json::to_value(ExtractIssueEvent::from(&RunIssue {
            kind: "error".into(),
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
