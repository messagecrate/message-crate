//! `upload` command: the Upload of an Import Run, which sends a run directory to a Message Crate server.

use message_crate_push::ImportMode;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use message_crate_push::{ProgressEvent, PushConfig, run as run_push};

use super::events;
use super::events::ExtractProgressEvent;
use super::jobs::{spawn_job, start_job};
use super::paths::logs_dir;
use crate::app_directories::import_run_log;
use crate::state::AppState;

/// Convert a report count to the `usize` the progress event uses.
fn as_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Progress bar update and finished JSON payload after an Upload completes.
fn finished_upload_events(
    report: &message_crate_push::PushReport,
) -> (ExtractProgressEvent, serde_json::Value) {
    let progress = ExtractProgressEvent {
        step: "upload".into(),
        done: as_usize(report.conversations_total),
        total: as_usize(report.conversations_total),
        bytes_done: None,
        bytes_total: None,
        status: None,
    };
    let summary = serde_json::json!({
        "summary": format!(
            "Upload complete: {} new, {} deduped, {} failed of {} attempted; {}/{} conversations ok; {} assets uploaded",
            report.messages_inserted,
            report.messages_deduped,
            report.messages_failed,
            report.messages_attempted,
            report.conversations_ok,
            report.conversations_total,
            report.assets_uploaded
        ),
        "ok": report.ok,
        "cancelled": report.cancelled,
        "session_refused": report.session_refused,
        "messages_attempted": report.messages_attempted,
        "messages_inserted": report.messages_inserted,
        "messages_deduped": report.messages_deduped,
        "messages_failed": report.messages_failed,
        "assets_uploaded": report.assets_uploaded,
        "assets_bytes": report.assets_bytes,
        "conversations_ok": report.conversations_ok,
        "conversations_total": report.conversations_total,
        "conversations_failed": report.conversations_failed,
        "conversations_skipped": report.conversations_skipped,
        "conversations_cancelled": report.conversations_cancelled,
        "results": report.results,
    });
    (progress, summary)
}

/// User-facing parameters for the `upload` command.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadArgs {
    /// Base URL of the server, for example `http://127.0.0.1:8080`.
    pub base_url: String,
    /// Account name.
    pub username: String,
    /// The logged-in Session's token, sent as the bearer token. Never an API
    /// Token, and never a password.
    pub token: String,
    /// Directory of conversation files to upload.
    pub input_dir: String,
    /// Import mode. `append` adds to existing data (safe to re-run);
    /// `replace` deletes existing messages for this source, then imports.
    pub mode: ImportMode,
    /// When true, import messages without uploading attachments.
    pub skip_attachments: bool,
    /// When true, trust export metadata: skip re-hashing attachments when
    /// size_bytes matches the file size on disk. Without this flag every
    /// attachment is re-hashed.
    pub trust_export: bool,
    /// Import id of an earlier import to resume, when set.
    pub import_id: Option<i64>,
}

/// Ask this process to upload extracted conversations to a server.
///
/// Returns as soon as the background thread starts. Upload progress uses the
/// same `extract:*` events as Extract so the UI can reuse one progress view.
///
/// # Errors
///
/// Returns an error if another job is running, or if another thread panicked
/// while holding the shared state lock. Failures during the upload are sent
/// as `extract:error`.
#[tauri::command(async)]
pub fn upload(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    args: UploadArgs,
) -> Result<(), String> {
    let logs = logs_dir(&app)?;
    let job = start_job(&state, "The Upload")?;
    let cancel = job.cancel_flag();
    let app_handle = app.clone();
    spawn_job(app, job, move || {
        let mut cfg = upload_config(args, &logs)?;
        cfg.cancel = Some(cancel);
        let mut progress = |event: ProgressEvent| forward_upload_event(&app_handle, event);
        let report = run_push(&cfg, Some(&mut progress))?;
        Ok(finished_upload_events(&report).1.to_string())
    });
    Ok(())
}

/// The Upload settings the desktop app uses. They differ from the
/// `message_crate_push::DEFAULT_*` constants because desktop imports are many
/// small files over a local network; each number says why.
///
/// The attachment size limit comes from the media settings Staging recorded
/// in the directory ([`message_staging::read_media_settings`]): the number the
/// Staging Review forecast against, read from the one place that holds it
/// after Staging.
///
/// The run's log goes to the Logs Directory, `logs_dir`, named for the run
/// ([`import_run_log`]), so it outlives the run's directory, which is
/// deleted when the run ends.
///
/// # Errors
///
/// Returns an error when the directory holds no readable media settings,
/// because its Staging never finished.
fn upload_config(args: UploadArgs, logs_dir: &Path) -> anyhow::Result<PushConfig> {
    let input = PathBuf::from(&args.input_dir);
    let recorded = message_staging::read_media_settings(&input)?;
    let log_path = Some(import_run_log(logs_dir, &input));
    Ok(PushConfig {
        input,
        base_url: args.base_url,
        username: args.username,
        token: args.token,
        mode: args.mode,
        // A resumed Upload skips what the journal in the run directory
        // already recorded as sent.
        force: false,
        skip_attachments: args.skip_attachments,
        trust_export: args.trust_export,
        verify_digests: false,
        max_retries: 3,
        // Pack until message_crate_push::MAX_IMPORT_BODY_BYTES (64 MiB); do not stop at a message count.
        batch_size: message_crate_push::NO_MESSAGE_COUNT_LIMIT,
        // Above DEFAULT_ASSET_UPLOAD_WORKERS (8): desktop imports are often many small files.
        asset_upload_workers: 16,
        // Above DEFAULT_PREPARE_AHEAD (3): hide more hashing behind in-flight imports.
        prepare_ahead: 8,
        // Above DEFAULT_PREPARE_WORKERS (2): more of the prepare-ahead queue runs at once.
        prepare_workers: 4,
        // Below message_crate_push::MAX_PROXY_BODY_BYTES (90 MiB):
        // desktop uploads switch to multipart sooner so a large attachment
        // moves in small parts instead of one long PUT.
        asset_multipart_threshold: 5 * 1024 * 1024,
        // Per-file attachment cap, the server's own as the run recorded it.
        // JSONL import batches use MAX_IMPORT_BODY_BYTES.
        asset_max_bytes: recorded.asset_max_bytes,
        report_path: None,
        log_path,
        // The journal stays in the run directory, beside the files it tracks.
        journal_path: None,
        cancel: None,
        import_id: args.import_id,
    })
}

/// Relay one Upload progress event to the window as `extract:*` events.
fn forward_upload_event(app: &tauri::AppHandle, event: ProgressEvent) {
    match event {
        ProgressEvent::Log(line) => {
            events::emit(app, events::LOG, line);
        }
        ProgressEvent::Auth { .. } => {}
        ProgressEvent::FileStart { index, total, file } => {
            events::emit(app, events::LOG, format!("Starting: {file}"));
            events::emit(
                app,
                events::PROGRESS,
                ExtractProgressEvent {
                    step: "upload".into(),
                    done: index.saturating_sub(1),
                    total,
                    bytes_done: None,
                    bytes_total: None,
                    status: None,
                },
            );
        }
        ProgressEvent::FileDone { file, status } => {
            events::emit(app, events::LOG, format!("Done: {file} ({status})"));
            events::emit(
                app,
                events::FILE_DONE,
                events::ExtractFileDoneEvent { file, status },
            );
        }
        ProgressEvent::Issue {
            kind,
            step,
            item,
            reason,
            conversation,
        } => {
            events::emit(
                app,
                events::ISSUE,
                events::ExtractIssueEvent {
                    kind,
                    step,
                    item,
                    reason,
                    conversation: Some(conversation),
                },
            );
        }
        ProgressEvent::Finished(report) => {
            // The finished event goes out once the job has ended (`spawn_job`).
            let (progress, _summary) = finished_upload_events(&report);
            events::emit(app, events::PROGRESS, progress);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use message_crate_push::{FileResult, PushReport};
    use message_ir::{
        ConversationMeta, ConversationStats, ExportMeta, IrConversationType, IrDirection,
        IrMessage, IrMessageKind, IrParticipant, IrService, SCHEMA_VERSION,
    };
    use serde_json::json;

    /// A run directory with the media settings Staging records, its
    /// attachment size limit `asset_max_bytes`.
    fn staged_directory(asset_max_bytes: u64) -> tempfile::TempDir {
        let run_dir = tempfile::tempdir().unwrap();
        message_staging::write_media_settings(
            run_dir.path(),
            &message_staging::TranscodeOptions {
                mode: media::MediaMode::Clone,
                compress: media::CompressOptions::default(),
                asset_max_bytes,
            },
        )
        .unwrap();
        run_dir
    }

    /// Upload holds a file to the limit Staging recorded in the directory, the
    /// number the Staging Review forecast against.
    #[test]
    fn upload_uses_the_attachment_size_limit_the_directory_recorded() {
        let run_dir = staged_directory(123_456_789);
        let args: UploadArgs = serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:8080",
            "username": "",
            "token": "token",
            "inputDir": run_dir.path(),
            "mode": "append",
            "skipAttachments": false,
            "trustExport": true,
            "importId": 7,
        }))
        .unwrap();
        assert_eq!(
            upload_config(args, Path::new("/logs"))
                .unwrap()
                .asset_max_bytes,
            123_456_789
        );
    }

    /// The limit has no default in the desktop app: a directory whose Staging
    /// never recorded one is refused, because the app has no number of its
    /// own to fall back on.
    #[test]
    fn upload_from_a_directory_with_no_media_settings_is_refused() {
        let run_dir = tempfile::tempdir().unwrap();
        let args: UploadArgs = serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:8080",
            "username": "",
            "token": "token",
            "inputDir": run_dir.path(),
            "mode": "append",
            "skipAttachments": false,
            "trustExport": true,
            "importId": 7,
        }))
        .unwrap();
        let err = upload_config(args, Path::new("/logs")).unwrap_err();
        assert!(format!("{err:#}").contains("no media settings"), "{err:#}");
    }

    /// A server that answers the session check. [`take_batches`] has it
    /// take import 7's batches.
    fn upload_server() -> MockServer {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v1/session");
            then.status(200).json_body(json!({
                "account_id": 1,
                "username": "alice",
            }));
        });
        server
    }

    /// Have `server` take every batch for import 7, one message each.
    fn take_batches(server: &MockServer) -> httpmock::Mock<'_> {
        server.mock(|when, then| {
            when.method(POST).path("/v1/imports/7/batches");
            then.status(200).json_body(json!({
                "messages": 1,
                "messages_appended": 1,
                "conversations": 1
            }));
        })
    }

    /// A run directory holding one staged conversation of one message.
    fn staged_conversation() -> tempfile::TempDir {
        let run_dir = staged_directory(512 * 1024 * 1024);
        let header = json!({
            "schema_version": SCHEMA_VERSION,
            "export": ExportMeta {
                source: "sms-backup-restore".into(),
                tool: "SMS Backup & Restore".into(),
                tool_version: "10.26.003".into(),
                owner_identity: Some("+15555550100".into()),
                owner_display_name: Some("Me".into()),
            },
            "conversation": ConversationMeta {
                chat_identifier: "+15555550101".into(),
                conversation_type: IrConversationType::Individual,
                group_title: None,
                participants: vec![IrParticipant {
                    identity: Some("+15555550101".into()),
                    display_name: Some("Sam".into()),
                    identity_type: None,
                }],
                stats: ConversationStats::default(),
            },
        });
        let message = json!(IrMessage {
            guid: "guid-1".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Sms,
            sender_identity: Some("+15555550101".into()),
            sender_display_name: Some("Sam".into()),
            owner_identity: None,
            subject: None,
            text: "hello there".into(),
            attachments: vec![],
            reactions: vec![],
            deletion: None,
            edits: Vec::new(),
            imessage: None,
            source: None,
        });
        std::fs::write(
            run_dir.path().join("sam.jsonl"),
            format!("{header}\n{message}\n"),
        )
        .unwrap();

        run_dir
    }

    /// What the Import screen sends for an Upload of `run_dir` to `server`,
    /// first time and resumed, with the run's log in `logs`.
    fn upload_to(
        server: &MockServer,
        run_dir: &Path,
        logs: &Path,
    ) -> message_crate_push::PushReport {
        let args: UploadArgs = serde_json::from_value(json!({
            "baseUrl": server.base_url(),
            "username": "",
            "token": "mc_test",
            "inputDir": run_dir,
            "mode": "append",
            "skipAttachments": false,
            "trustExport": true,
            "importId": 7,
        }))
        .unwrap();
        run_push(&upload_config(args, logs).unwrap(), None).unwrap()
    }

    /// A resumed Upload runs over the run directory the interrupted Upload
    /// left, journal included. Sending the journaled messages again would
    /// only make the server count each one as a Duplicate.
    #[test]
    fn a_resumed_upload_does_not_send_what_the_journal_recorded() {
        let server = upload_server();
        let batches = take_batches(&server);
        let run_dir = staged_conversation();
        let logs = tempfile::tempdir().unwrap();

        let first = upload_to(&server, run_dir.path(), logs.path());
        assert!(first.ok, "{:?}", first.results);
        assert_eq!(batches.calls(), 1);

        let resumed = upload_to(&server, run_dir.path(), logs.path());
        assert!(resumed.ok, "{:?}", resumed.results);
        assert_eq!(resumed.messages_attempted, 0);
        assert_eq!(batches.calls(), 1, "the resumed Upload sends no batch");
    }

    /// The run's log is in the Logs Directory, so deleting the run's
    /// directory when the run ends leaves it, kept for good.
    #[test]
    fn an_import_run_s_log_survives_its_run_directory_being_deleted() {
        let server = upload_server();
        take_batches(&server);
        let run_dir = staged_conversation();
        let logs = tempfile::tempdir().unwrap();
        let log = import_run_log(logs.path(), run_dir.path());

        let report = upload_to(&server, run_dir.path(), logs.path());
        assert!(report.ok, "{:?}", report.results);
        assert!(
            !run_dir.path().join(message_crate_push::LOG_NAME).exists(),
            "no log in the run's directory"
        );
        run_dir.close().unwrap();

        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("sam.jsonl"), "{text}");
    }

    #[test]
    fn finished_upload_event_reports_complete_upload_and_totals() {
        let report = PushReport {
            ok: true,
            cancelled: false,
            session_refused: false,
            account: 1,
            username: "user".into(),
            mode: ImportMode::Append,
            started_at: "2026-08-11T00:00:00Z".into(),
            finished_at: "2026-08-11T00:00:01Z".into(),
            elapsed_ms: 1_000,
            conversations_total: 3,
            conversations_ok: 2,
            conversations_failed: 0,
            conversations_skipped: 1,
            conversations_cancelled: 0,
            messages_attempted: 45,
            messages_inserted: 42,
            messages_deduped: 2,
            messages_failed: 1,
            assets_uploaded: 4,
            assets_skipped: 1,
            assets_bytes: 12_345,
            results: vec![FileResult {
                file: "failed.jsonl".into(),
                status: "failed".into(),
                error: Some("attachment exceeds limit".into()),
                messages: 0,
                attachments: 0,
                profile: None,
            }],
        };

        let (progress, summary) = finished_upload_events(&report);

        assert_eq!(progress.step, "upload");
        assert_eq!(progress.done, 3);
        assert_eq!(progress.total, 3);
        assert!(summary.get("messages").is_none());
        assert_eq!(summary["messages_attempted"], 45);
        assert_eq!(summary["messages_inserted"], 42);
        assert_eq!(summary["messages_deduped"], 2);
        assert_eq!(summary["messages_failed"], 1);
        assert_eq!(summary["conversations_failed"], 0);
        assert_eq!(summary["conversations_skipped"], 1);
        assert_eq!(summary["conversations_cancelled"], 0);
        assert_eq!(summary["results"][0]["error"], "attachment exceeds limit");
        assert_eq!(
            summary["summary"],
            "Upload complete: 42 new, 2 deduped, 1 failed of 45 attempted; 2/3 conversations ok; 4 assets uploaded"
        );
        assert_eq!(summary["assets_bytes"], 12_345);
        assert_eq!(summary["conversations_ok"], 2);
        assert_eq!(summary["conversations_total"], 3);
        assert_eq!(summary["cancelled"], false);
        // The window ends the Session when the Upload says the server refused it.
        assert_eq!(summary["session_refused"], false);
    }
}
