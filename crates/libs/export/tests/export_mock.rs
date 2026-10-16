//! Mock server tests for one Export: login, the Export Run it records, two
//! pages of messages, Asset fetches, the journal a second run reads, and
//! the progress a caller sees.
//!
//! The mock answers the five routes `run` calls — `GET /v1/session`,
//! `POST /v1/exports`, `GET /v1/exports/{id}/messages`,
//! `POST /v1/exports/{id}/complete` (with `completed` or `failed`) or
//! `/cancel`, and `GET /v1/assets/{sha256}`
//! — with the JSON the server serializes (`message-crate-api-types`,
//! `docs/src/assets/openapi.json`). Every request derives from
//! `ExportConfig::base_url`, so the mock's address is the only seam.

use message_crate_core::CancelFlag;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use httpmock::prelude::*;
use message_crate_export::{
    AssetFailure, ExportConfig, ExportJournalState, ExportQueryList, ExportReport, ProgressEvent,
    journal, run,
};
use message_ir::{Deletion, EarlierVersion, Reaction};
use message_ir_format::{EXPORT_SENTINEL, read_conversation_jsonl};
use serde_json::{Value, json};
use tempfile::tempdir;

/// SHA-256 of [`MENU_BYTES`], the menu attachment's fingerprint.
const MENU_SHA: &str = "9170c75b5d7058e5075b811f12e88ef3e7b0bd9018ff8b52bead945b6b1219f7";
/// SHA-256 of [`PHOTO_BYTES`], the fingerprint of the photo attachment, which
/// the server sends without a path.
const PHOTO_SHA: &str = "be04c407026cf352d54a051993ef2fec8153cc554f8ff7c697b225c87a6e5e03";
/// 13 bytes.
const MENU_BYTES: &[u8] = b"%PDF-1.4 menu";
/// 9 bytes.
const PHOTO_BYTES: &[u8] = b"PNG photo";
/// The file an Export Run of `+15555550101` from `sms-backup-restore` writes.
const CONVERSATION_FILE: &str = "+15555550101__sms-backup-restore.jsonl";
/// The id the mock server gives every run it records.
const EXPORT_ID: i64 = 7;

/// The menu attachment as the server serializes it, at `path`.
fn menu_attachment(path: Value) -> Value {
    json!({
        "path": path,
        "original_name": "menu.pdf",
        "mime_type": "application/pdf",
        "sha256": MENU_SHA,
        "is_sticker": false, "shown_as_is": false
    })
}

/// The photo attachment: no `path`, so the Export Run files it under its fingerprint.
fn photo_attachment() -> Value {
    json!({
        "path": null,
        "original_name": "photo.png",
        "mime_type": "image/png",
        "sha256": PHOTO_SHA,
        "is_sticker": false, "shown_as_is": false
    })
}

/// One exported message as `GET /v1/exports/{id}/messages` serializes it: a
/// one-to-one SMS conversation with Sam, `service` on the message rather than
/// on the conversation, received at the holder's own `+15555550100`.
fn message(
    id: i64,
    source: &str,
    guid: &str,
    timestamp: &str,
    text: &str,
    attachments: Value,
) -> Value {
    json!({
        "id": id,
        "source": source,
        "service": "sms",
        "guid": guid,
        "timestamp": timestamp,
        "time_precision": "milliseconds",
        "sort_order": id,
        "is_from_me": false,
        "sender": "+15555550101",
        "owner": "+15555550100",
        "subject": null,
        "text": text,
        "is_announcement": false,
        "reply_to": null,
        "reply_count": 0,
        "conversation": {
            "id": 9,
            "chat_identifier": "+15555550101",
            "conversation_type": "one-to-one",
            "is_group": false,
            "group_title": null,
            "participants": [
                { "name": "Sam", "identity": "+15555550101", "service": "sms", "contact_id": 3 }
            ]
        },
        "attachments": attachments,
        "tapbacks": [],
        "earlier_versions": [],
        "matched_earlier_version": false
    })
}

/// The run the mock server records for `scope`, in `status`.
fn export_run(scope: Value, status: &str) -> Value {
    json!({
        "id": EXPORT_ID,
        "scope": scope,
        "tool": "message-crate-export",
        "status": status,
        "started_at": "2026-09-08T12:00:00Z",
        "finished_at": if status == "running" { Value::Null } else { json!("2026-09-08T12:00:09Z") },
        "message_count": 3,
        "conversation_count": 1,
        "attachment_count": 2,
        "total_bytes": 22,
        "messages_delivered": if status == "running" { 0 } else { 3 }
    })
}

/// The login: the token resolves to account `1`, username `alice`.
fn mock_auth(server: &MockServer) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200)
            .json_body(json!({ "account_id": 1, "username": "alice" }));
    })
}

/// `POST /v1/exports` for exactly `scope`, recorded as `message-crate-export`'s run.
fn mock_create<'a>(server: &'a MockServer, scope: Value) -> httpmock::Mock<'a> {
    let body = json!({ "scope": scope.clone(), "tool": "message-crate-export" });
    server.mock(move |when, then| {
        when.method(POST)
            .path("/v1/exports")
            .json_body(body.clone());
        then.status(201)
            .header("location", format!("/v1/exports/{EXPORT_ID}"))
            .json_body(export_run(scope.clone(), "running"));
    })
}

/// `POST /v1/exports/{id}/complete` with `status`, answering the run closed
/// that way.
fn mock_close_as<'a>(server: &'a MockServer, status: &'static str) -> httpmock::Mock<'a> {
    server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/exports/{EXPORT_ID}/complete"))
            .json_body(json!({ "status": status }));
        then.status(200)
            .json_body(export_run(json!({ "kind": "everything" }), status));
    })
}

/// `POST /v1/exports/{id}/complete` with `completed`.
fn mock_complete(server: &MockServer) -> httpmock::Mock<'_> {
    mock_close_as(server, "completed")
}

/// `POST /v1/exports/{id}/complete` with `failed`.
fn mock_fail(server: &MockServer) -> httpmock::Mock<'_> {
    mock_close_as(server, "failed")
}

/// `POST /v1/exports/{id}/cancel`, answering the closed run.
fn mock_cancel(server: &MockServer) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/exports/{EXPORT_ID}/cancel"));
        then.status(200)
            .json_body(export_run(json!({ "kind": "everything" }), "canceled"));
    })
}

/// The run bookends every test below needs: the everything-scope create and
/// its complete.
fn mock_run(server: &MockServer) -> (httpmock::Mock<'_>, httpmock::Mock<'_>) {
    (
        mock_create(server, json!({ "kind": "everything" })),
        mock_complete(server),
    )
}

/// Two pages of `GET /v1/exports/{id}/messages` at two messages a page:
/// messages 1 and 2 with `total` 3, then message 3. The menu is on messages
/// 1 and 3, so its second mention must not be fetched twice.
fn mock_pages<'a>(
    server: &'a MockServer,
    source: &str,
) -> (httpmock::Mock<'a>, httpmock::Mock<'a>) {
    let path = format!("/v1/exports/{EXPORT_ID}/messages");
    let first = server.mock(|when, then| {
        when.method(GET)
            .path(&path)
            .query_param("limit", "2")
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [
                message(
                    1, source, "guid-1", "2015-03-12T18:05:01Z", "dinner at seven?",
                    json!([menu_attachment(json!("attachments/menu.pdf"))])
                ),
                message(
                    2, source, "guid-2", "2015-03-12T18:05:02Z", "here is the photo",
                    json!([photo_attachment()])
                )
            ],
            "total": 3,
            "limit": 2,
            "offset": 0
        }));
    });
    let second = server.mock(|when, then| {
        when.method(GET)
            .path(&path)
            .query_param("limit", "2")
            .query_param("offset", "2");
        then.status(200).json_body(json!({
            "items": [
                message(
                    3, source, "guid-3", "2015-03-12T18:05:03Z", "the menu again",
                    json!([menu_attachment(json!("attachments/menu.pdf"))])
                )
            ],
            "total": 3,
            "limit": 2,
            "offset": 2
        }));
    });
    (first, second)
}

/// `GET /v1/assets/{sha256}`, answering `bytes`. The fingerprint alone names
/// the Asset and the token names the account, and the server refuses a
/// parameter a route does not declare, so a fetch that still sent
/// `source=` or `account=` would not match.
fn mock_asset<'a>(server: &'a MockServer, sha256: &str, bytes: &[u8]) -> httpmock::Mock<'a> {
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/assets/{sha256}"))
            .query_param_missing("source")
            .query_param_missing("account");
        then.status(200)
            .header("content-type", "application/octet-stream")
            .body(bytes);
    })
}

/// The temporary files a fetch left in `dir`: every name ending in `.part`.
fn part_files_in(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".part"))
        .collect()
}

/// The Export Date every test run is given.
const EXPORTED_AT_UNIX_MS: i64 = 1_791_000_000_000;

/// An Export Run of every message into `out_dir`: two messages a page, one fetch
/// worker so the counts in the log are fixed, written at
/// [`EXPORTED_AT_UNIX_MS`].
fn config(out_dir: &Path, base_url: String) -> ExportConfig {
    ExportConfig {
        out_dir: out_dir.to_path_buf(),
        base_url,
        token: "mc_test".into(),
        query: String::new(),
        list: ExportQueryList::Messages,
        skip_attachments: false,
        page_limit: 2,
        cancel: CancelFlag::default(),
        asset_fetch_workers: 1,
        exported_at_unix_ms: EXPORTED_AT_UNIX_MS,
    }
}

/// The journal a run against `server` wrote in `out_dir` for `alice`. The
/// run writes every line itself, so the test fails on any line
/// `journal::load` could not read.
fn load_journal(out_dir: &Path, server: &MockServer) -> ExportJournalState {
    let mut unreadable = Vec::new();
    let state = journal::load(
        &journal::journal_path(out_dir),
        &journal::ServerTarget::new(server.base_url(), "alice"),
        &mut |sentence| unreadable.push(sentence),
    )
    .unwrap();
    assert!(unreadable.is_empty(), "{unreadable:#?}");
    state
}

/// The report `run` returns for the three-message fixture.
fn report_for(out_dir: &Path, fetched: u64, kept: u64) -> ExportReport {
    ExportReport {
        account: 1,
        export_id: EXPORT_ID,
        query: String::new(),
        conversations: 1,
        messages: 3,
        assets_fetched: fetched,
        assets_kept: kept,
        assets_failed: Vec::new(),
        refused_attachment_paths: Vec::new(),
        out_dir: out_dir.display().to_string(),
    }
}

#[test]
fn an_export_records_one_run_and_writes_the_conversation_and_every_asset_once_across_two_pages() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let (first, second) = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    let report = run(&config(&out, server.base_url()), None).unwrap();

    assert_eq!(report, report_for(&out, 2, 0));
    create.assert();
    first.assert();
    second.assert();
    menu.assert();
    photo.assert();
    complete.assert();
    assert!(out.join(EXPORT_SENTINEL).is_file());
    assert_eq!(
        fs::read(out.join("attachments/menu.pdf")).unwrap(),
        MENU_BYTES
    );
    assert_eq!(
        fs::read(out.join("attachments").join(PHOTO_SHA)).unwrap(),
        PHOTO_BYTES
    );
    assert_eq!(
        part_files_in(&out.join("attachments")),
        Vec::<String>::new(),
        "each temporary file is renamed into place"
    );
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    assert_eq!(doc.export.source, "sms-backup-restore");
    assert_eq!(doc.export.exported_at_unix_ms, Some(EXPORTED_AT_UNIX_MS));
    assert_eq!(doc.conversation.chat_identifier, "+15555550101");
    assert_eq!(
        doc.conversation.participants[0].display_name.as_deref(),
        Some("Sam")
    );
    assert_eq!(doc.conversation.stats.message_count, 3);
    assert_eq!(doc.conversation.stats.attachment_count, 3);
    assert_eq!(
        doc.messages
            .iter()
            .map(|m| m.guid.as_str())
            .collect::<Vec<_>>(),
        ["guid-1", "guid-2", "guid-3"]
    );
    assert_eq!(doc.messages[0].text, "dinner at seven?");
    // The address the server holds each message at comes out on the message,
    // and on the header since every message shares it (#1098).
    assert_eq!(
        doc.messages
            .iter()
            .map(|m| m.owner_identity.as_deref())
            .collect::<Vec<_>>(),
        [Some("+15555550100"); 3]
    );
    assert_eq!(doc.export.owner_identity.as_deref(), Some("+15555550100"));
    assert_eq!(
        doc.messages[0].attachments[0].digest_sha256.as_deref(),
        Some(MENU_SHA)
    );
    assert_eq!(
        doc.messages[1].attachments[0].path.as_deref(),
        Some(format!("attachments/{PHOTO_SHA}").as_str())
    );
}

/// An attachment whose path would leave the Export's directory and that has
/// no SHA-256 has nowhere else to go: the log says it is not written, the
/// conversation file names no path for it, and nothing is fetched for it.
#[test]
fn an_attachment_path_that_leaves_the_directory_with_no_sha256_is_not_written() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (_create, complete) = mock_run(&server);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let climbing = "../escape.pdf";
    let _page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [message(
                1, "sms-backup-restore", "guid-1", "2015-03-12T18:05:01Z", "the menu",
                json!([{
                    "path": climbing,
                    "original_name": "escape.pdf",
                    "mime_type": "application/pdf",
                    "is_sticker": false, "shown_as_is": false
                }])
            )],
            "total": 1,
            "limit": 1,
            "offset": 0
        }));
    });
    let assets = server.mock(|when, then| {
        when.method(GET).path_includes("/v1/assets/");
        then.status(500);
    });
    let mut events = Vec::new();
    let mut on_progress = |event: ProgressEvent| events.push(event);

    let report = run(&config(&out, server.base_url()), Some(&mut on_progress)).unwrap();

    complete.assert();
    assert_eq!(assets.calls(), 0, "nothing names a file to fetch");
    assert!(!dir.path().join("escape.pdf").exists());
    assert_eq!(report.refused_attachment_paths, [climbing.to_string()]);
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    assert_eq!(
        doc.messages[0].attachments[0].path, None,
        "the conversation file names no path outside the Export's directory"
    );
    let line = format!(
        "Attachment path {climbing} would leave the Export's directory, and the \
         attachment has no SHA-256 to name another path by, so it is not written"
    );
    assert!(
        events.contains(&ProgressEvent::Log(line)),
        "the log says the file is not written: {events:?}"
    );
}

/// The server can hold an attachment path that climbs out of a directory or
/// names an absolute one, because an import that reuses a stored fingerprint
/// never read the file at that path. Joined onto the output directory, such a
/// path would write the fetched Asset anywhere on disk. Each one is refused: the
/// file lands at `attachments/{sha256}`, the conversation file names that
/// path, and the report and the log name the refused path.
#[test]
fn an_attachment_path_that_leaves_the_output_directory_is_written_under_its_fingerprint_instead() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (_create, complete) = mock_run(&server);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let climbing = "../escape.pdf";
    let absolute = dir.path().join("absolute.png").display().to_string();
    let page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [
                message(
                    1, "sms-backup-restore", "guid-1", "2015-03-12T18:05:01Z", "the menu",
                    json!([menu_attachment(json!(climbing))])
                ),
                message(
                    2, "sms-backup-restore", "guid-2", "2015-03-12T18:05:02Z", "the photo",
                    json!([{
                        "path": absolute,
                        "original_name": "photo.png",
                        "mime_type": "image/png",
                        "sha256": PHOTO_SHA,
                        "is_sticker": false, "shown_as_is": false
                    }])
                )
            ],
            "total": 2,
            "limit": 2,
            "offset": 0
        }));
    });
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let mut events = Vec::new();
    let mut on_progress = |event: ProgressEvent| events.push(event);

    let report = run(&config(&out, server.base_url()), Some(&mut on_progress)).unwrap();

    page.assert();
    complete.assert();
    assert!(!dir.path().join("escape.pdf").exists());
    assert!(!dir.path().join("absolute.png").exists());
    assert_eq!(
        fs::read(out.join("attachments").join(MENU_SHA)).unwrap(),
        MENU_BYTES
    );
    assert_eq!(
        fs::read(out.join("attachments").join(PHOTO_SHA)).unwrap(),
        PHOTO_BYTES
    );
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    let written: Vec<Option<&str>> = doc
        .messages
        .iter()
        .map(|m| m.attachments[0].path.as_deref())
        .collect();
    assert_eq!(
        written,
        [
            Some(format!("attachments/{MENU_SHA}").as_str()),
            Some(format!("attachments/{PHOTO_SHA}").as_str()),
        ]
    );
    let mut refused = vec![climbing.to_string(), absolute.clone()];
    refused.sort();
    assert_eq!(report.refused_attachment_paths, refused);
    for (path, sha) in [(climbing, MENU_SHA), (absolute.as_str(), PHOTO_SHA)] {
        let line = format!(
            "Attachment path {path} would leave the Export's directory, \
             so the file is written at attachments/{sha} instead"
        );
        assert!(
            events.contains(&ProgressEvent::Log(line.clone())),
            "the log names {path}: {events:?}"
        );
    }
}

#[test]
fn the_journal_lists_every_asset_and_marks_the_run_finished() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    run(&config(&out, server.base_url()), None).unwrap();

    let state = load_journal(&out, &server);
    assert_eq!(
        state.assets,
        HashSet::from([MENU_SHA.to_string(), PHOTO_SHA.to_string()])
    );
    assert!(state.export_complete);
}

#[test]
fn a_second_run_over_the_same_directory_fetches_nothing_it_already_has() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, server.base_url());
    run(&cfg, None).unwrap();

    let mut lines = Vec::new();
    let report = {
        let mut progress = |event| {
            if let ProgressEvent::Log(line) = event {
                lines.push(line);
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert_eq!(report, report_for(&out, 0, 2));
    assert_eq!(menu.calls(), 1);
    assert_eq!(photo.calls(), 1);
    // Every Export is its own Export Run, whether or not it fetches anything.
    assert_eq!(create.calls(), 2);
    assert_eq!(complete.calls(), 2);
    assert_eq!(
        lines,
        [
            "Authenticated as alice (1)".to_string(),
            "Exporting every message".to_string(),
            "The previous Export Run finished. Checking for new messages…".to_string(),
            "Export Run 7 holds 3 messages in 1 conversation, with 2 attachments (22 B)"
                .to_string(),
            format!("Wrote 1 conversation and 3 messages to {}", out.display()),
        ]
    );
}

/// A line of the export-state file the Export cannot read is named in the
/// Export's log, by its line in the file, and costs no fetch: the Assets it
/// might have recorded are on disk and are kept (#1910).
#[test]
fn an_unreadable_journal_line_is_a_sentence_in_the_exports_log() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, server.base_url());
    run(&cfg, None).unwrap();
    let path = journal::journal_path(&out);
    let mut text = fs::read_to_string(&path).unwrap();
    let line = text.lines().count() + 1;
    text.push_str("{not json\n");
    fs::write(&path, text).unwrap();

    let mut lines = Vec::new();
    let report = {
        let mut progress = |event| {
            if let ProgressEvent::Log(line) = event {
                lines.push(line);
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert_eq!(report, report_for(&out, 0, 2));
    assert_eq!(menu.calls(), 1);
    assert_eq!(photo.calls(), 1);
    let prefix = format!(
        "Line {line} of {}, the record of fetched Assets, could not be read (",
        path.display()
    );
    let named: Vec<&String> = lines.iter().filter(|l| l.starts_with(&prefix)).collect();
    assert_eq!(named.len(), 1, "{lines:#?}");
    assert!(
        named[0].ends_with(
            "), so the Export skips it. An Asset that line recorded is not fetched \
             again while its file is in the directory."
        ),
        "{}",
        named[0]
    );
}

#[test]
fn a_file_the_journal_lists_but_the_disk_lost_is_fetched_again() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, server.base_url());
    run(&cfg, None).unwrap();
    fs::remove_file(out.join("attachments/menu.pdf")).unwrap();

    let report = run(&cfg, None).unwrap();

    assert_eq!(report, report_for(&out, 1, 1));
    assert_eq!(menu.calls(), 2);
    assert_eq!(photo.calls(), 1);
    assert_eq!(
        fs::read(out.join("attachments/menu.pdf")).unwrap(),
        MENU_BYTES
    );
}

/// A run that stopped after writing its Assets but before recording them in
/// the export-state file leaves files the next run keeps without fetching. The
/// "Fetching" line counts each of them as already on disk, as the fetch does,
/// so it agrees with the "Fetched" line after it, and the "Fetched" line
/// counts only the bytes this run fetched.
#[test]
fn a_file_on_disk_the_journal_does_not_list_is_kept_and_counted_as_on_disk() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, server.base_url());
    run(&cfg, None).unwrap();
    fs::remove_file(journal::journal_path(&out)).unwrap();
    fs::remove_file(out.join("attachments/menu.pdf")).unwrap();

    let mut lines = Vec::new();
    let report = {
        let mut progress = |event| {
            if let ProgressEvent::Log(line) = event {
                lines.push(line);
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert_eq!(report, report_for(&out, 1, 1));
    assert_eq!(menu.calls(), 2);
    assert_eq!(photo.calls(), 1);
    let asset_lines: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("Fetching ") || line.starts_with("Fetched "))
        .collect();
    assert_eq!(
        asset_lines,
        [
            "Fetching 1 Asset with 1 worker (1 already on disk)…",
            "Fetched 1 Asset (13 B) and kept 1 already on disk",
        ]
    );
}

/// With every Asset already on disk, a run logs no "Fetching" or "Fetched"
/// line whether or not the export-state file lists the Assets, because what is
/// on disk, not the export-state file, decides what is fetched.
#[test]
fn every_file_on_disk_logs_no_fetch_lines_whether_or_not_the_journal_lists_it() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, server.base_url());
    run(&cfg, None).unwrap();
    fs::remove_file(journal::journal_path(&out)).unwrap();

    let mut lines = Vec::new();
    let report = {
        let mut progress = |event| {
            if let ProgressEvent::Log(line) = event {
                lines.push(line);
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert_eq!(report, report_for(&out, 0, 2));
    assert_eq!(menu.calls(), 1);
    assert_eq!(photo.calls(), 1);
    assert!(
        !lines
            .iter()
            .any(|line| line.starts_with("Fetching ") || line.starts_with("Fetched ")),
        "{lines:#?}"
    );
    let state = load_journal(&out, &server);
    assert!(state.assets.contains(MENU_SHA) && state.assets.contains(PHOTO_SHA));
}

#[test]
fn a_cancel_requested_before_the_run_records_nothing_on_the_server() {
    let server = MockServer::start();
    let auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let cancel = mock_cancel(&server);
    let (first, _second) = mock_pages(&server, "sms-backup-restore");
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = ExportConfig {
        cancel: Arc::new(AtomicBool::new(true)),
        ..config(&out, server.base_url())
    };

    let error = run(&cfg, None).unwrap_err();

    assert_eq!(error.to_string(), "canceled");
    assert_eq!(auth.calls(), 1);
    assert_eq!(
        create.calls(),
        0,
        "a run the caller gave up on is never recorded"
    );
    assert_eq!(first.calls(), 0);
    assert_eq!(complete.calls(), 0);
    assert_eq!(cancel.calls(), 0);
    assert!(!out.join(CONVERSATION_FILE).exists());
    let state = load_journal(&out, &server);
    assert!(!state.export_complete);
}

#[test]
fn a_source_name_with_spaces_and_brackets_becomes_a_file_safe_suffix() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "whatsapp (phone 2)");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    run(&config(&out, server.base_url()), None).unwrap();

    let doc = read_conversation_jsonl(&out.join("+15555550101__whatsapp__phone_2_.jsonl")).unwrap();
    assert_eq!(doc.export.source, "whatsapp (phone 2)");
    assert_eq!(doc.conversation.stats.message_count, 3);
}

#[test]
fn two_groups_with_one_title_are_both_written() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    // One message in each of two "Family" groups from the same source.
    let group_message = |id: i64, conversation_id: i64, chat: &str, text: &str| {
        let mut message = message(
            id,
            "imessage",
            &format!("guid-{id}"),
            "2015-03-12T18:05:01Z",
            text,
            json!([]),
        );
        message["conversation"]["id"] = json!(conversation_id);
        message["conversation"]["chat_identifier"] = json!(chat);
        message["conversation"]["conversation_type"] = json!("group");
        message["conversation"]["is_group"] = json!(true);
        message["conversation"]["group_title"] = json!("Family");
        message
    };
    let _page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [
                group_message(1, 9, "chat-one", "first family"),
                group_message(2, 10, "chat-two", "second family")
            ],
            "total": 2,
            "limit": 2,
            "offset": 0
        }));
    });
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    run(&config(&out, server.base_url()), None).unwrap();

    let mut chats: Vec<String> = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            // The Export Run's own journal is a hidden JSONL file beside them.
            let name = e.file_name().to_string_lossy().to_string();
            name.ends_with(".jsonl") && !name.starts_with('.')
        })
        .map(|e| {
            read_conversation_jsonl(&e.path())
                .unwrap()
                .conversation
                .chat_identifier
        })
        .collect();
    chats.sort();
    assert_eq!(
        chats,
        ["chat-one", "chat-two"],
        "neither group's file replaces the other's"
    );
}

#[test]
fn skipping_attachments_writes_messages_without_files_or_fetches() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = ExportConfig {
        skip_attachments: true,
        ..config(&out, server.base_url())
    };

    let report = run(&cfg, None).unwrap();

    assert_eq!(report, report_for(&out, 0, 0));
    assert_eq!(menu.calls(), 0);
    assert_eq!(photo.calls(), 0);
    assert!(!out.join("attachments").exists());
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    assert_eq!(doc.conversation.stats.attachment_count, 0);
    assert!(doc.messages[0].attachments.is_empty());
}

#[test]
fn a_query_becomes_the_runs_query_scope_and_progress_narrates_the_run() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let create = mock_create(
        &server,
        json!({ "kind": "query", "list": "messages", "q": "from:sam" }),
    );
    let complete = mock_complete(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = ExportConfig {
        query: " from:sam ".into(),
        ..config(&out, server.base_url())
    };

    let mut events = Vec::new();
    let report = {
        let mut progress = |event| events.push(event);
        run(&cfg, Some(&mut progress)).unwrap()
    };

    create.assert();
    complete.assert();
    assert_eq!(
        events,
        vec![
            ProgressEvent::Auth {
                account_id: 1,
                username: "alice".into(),
            },
            ProgressEvent::Log("Authenticated as alice (1)".into()),
            ProgressEvent::Log("Exporting the messages that match: from:sam".into()),
            ProgressEvent::Log(
                "Export Run 7 holds 3 messages in 1 conversation, with 2 attachments (22 B)".into()
            ),
            ProgressEvent::Page {
                messages: 2,
                total_so_far: 2,
            },
            ProgressEvent::Page {
                messages: 1,
                total_so_far: 3,
            },
            ProgressEvent::Log("Fetching 2 Assets with 1 worker (0 already on disk)…".into()),
            ProgressEvent::Log("Fetched 2 Assets (22 B) and kept 0 already on disk".into()),
            ProgressEvent::Log(format!(
                "Wrote 1 conversation and 3 messages to {}",
                out.display()
            )),
            ProgressEvent::Done(report),
        ]
    );
}

/// A query from the Conversations list must reach the server as one: sent
/// for the Messages list it would be refused or hand over fewer messages.
#[test]
fn a_query_for_the_conversations_list_names_that_list_in_the_scope() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let create = mock_create(
        &server,
        json!({ "kind": "query", "list": "conversations", "q": "messages:>100" }),
    );
    let complete = mock_complete(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = ExportConfig {
        query: "messages:>100".into(),
        list: ExportQueryList::Conversations,
        skip_attachments: true,
        ..config(&out, server.base_url())
    };

    let mut events = Vec::new();
    {
        let mut progress = |event| events.push(event);
        run(&cfg, Some(&mut progress)).unwrap();
    }

    create.assert();
    complete.assert();
    assert!(events.contains(&ProgressEvent::Log(
        "Exporting every message of the conversations that match: messages:>100".into()
    )));
}

/// One Asset the server does not have no longer throws the Export away
/// (#2070): the conversation file and every other Asset are written, the
/// report and the log name the missing one, the journal does not list it, and
/// the run is closed as completed.
#[test]
fn an_asset_the_server_does_not_have_is_named_in_the_report_and_every_conversation_is_written() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let fail = mock_fail(&server);
    let cancel = mock_cancel(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = server.mock(|when, then| {
        when.method(GET).path(format!("/v1/assets/{MENU_SHA}"));
        then.status(404).json_body(json!({
            "type": "https://messagecrate.app/docs/developer/reference/errors/not-found",
            "title": "Not found",
            "status": 404,
            "detail": "asset not found"
        }));
    });
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let mut events = Vec::new();
    let mut on_progress = |event: ProgressEvent| events.push(event);

    let report = run(&config(&out, server.base_url()), Some(&mut on_progress)).unwrap();

    let reason = format!("Asset {MENU_SHA} fetch failed (HTTP 404 Not Found): asset not found");
    assert_eq!(
        report,
        ExportReport {
            assets_failed: vec![AssetFailure {
                sha256: MENU_SHA.into(),
                path: "attachments/menu.pdf".into(),
                reason: reason.clone(),
            }],
            ..report_for(&out, 1, 0)
        }
    );
    assert_eq!(menu.calls(), 1, "a 404 Not Found is not retried");
    photo.assert();
    create.assert();
    complete.assert();
    assert_eq!(fail.calls(), 0);
    assert_eq!(cancel.calls(), 0);
    assert!(!out.join("attachments/menu.pdf").exists());
    assert_eq!(
        fs::read(out.join("attachments").join(PHOTO_SHA)).unwrap(),
        PHOTO_BYTES
    );
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    assert_eq!(doc.messages.len(), 3, "every message is written");
    let attachments: Vec<_> = doc
        .messages
        .iter()
        .flat_map(|msg| msg.attachments.iter())
        .collect();
    let menus: Vec<_> = attachments
        .iter()
        .filter(|att| att.digest_sha256.as_deref() == Some(MENU_SHA))
        .collect();
    assert!(!menus.is_empty(), "the menu is in the conversation file");
    for menu in menus {
        assert_eq!(
            menu.missing_reason.as_deref(),
            Some(message_ir::missing_reason::NOT_FETCHED),
            "the conversation file says the Export could not fetch the menu"
        );
    }
    for photo in attachments
        .iter()
        .filter(|att| att.digest_sha256.as_deref() == Some(PHOTO_SHA))
    {
        assert_eq!(photo.missing_reason, None, "the photo was fetched");
    }
    for line in [
        format!("{reason}, so attachments/menu.pdf is not written"),
        "1 Asset could not be fetched, so its file is missing from the Export. \
         The lines above name each one"
            .to_string(),
    ] {
        assert!(
            events.contains(&ProgressEvent::Log(line.clone())),
            "{line}: {events:?}"
        );
    }
    let state = load_journal(&out, &server);
    assert!(
        !state.assets.contains(MENU_SHA),
        "the menu is not journaled"
    );
    assert!(state.assets.contains(PHOTO_SHA));
}

/// An access proxy whose session has expired answers the asset request with
/// its login page and `200 OK`. Those bytes are not the photo, so the photo
/// is reported as not fetched, and nothing at the photo's path or in the
/// journal lets a later run skip it.
#[test]
fn bytes_whose_sha256_is_not_the_one_asked_for_are_not_kept() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(
        &server,
        PHOTO_SHA,
        b"<html><body>Sign in to continue</body></html>",
    );
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    let report = run(&config(&out, server.base_url()), None).unwrap();

    let [failure] = report.assets_failed.as_slice() else {
        panic!("one Asset fails: {report:?}");
    };
    assert_eq!(failure.sha256, PHOTO_SHA);
    assert!(
        failure.reason.starts_with(&format!(
            "the server's answer to Asset {PHOTO_SHA} fetch is bytes whose SHA-256 is "
        )),
        "{}",
        failure.reason
    );
    assert_eq!(
        photo.calls(),
        1,
        "the same answer would come back, so it is not retried"
    );
    create.assert();
    complete.assert();
    let photo_path = out.join("attachments").join(PHOTO_SHA);
    assert!(
        !photo_path.exists(),
        "the login page is not kept as the photo"
    );
    assert_eq!(
        part_files_in(&out.join("attachments")),
        Vec::<String>::new(),
        "the temporary file is removed"
    );
    let state = load_journal(&out, &server);
    assert!(
        !state.assets.contains(PHOTO_SHA),
        "the photo is not journaled"
    );
}

/// A page the server cannot hand over ends the run with an error, and the
/// run is recorded as failed, never as canceled: nobody canceled it
/// (#2069).
#[test]
fn a_page_that_cannot_be_fetched_closes_the_run_as_failed() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let fail = mock_fail(&server);
    let cancel = mock_cancel(&server);
    let page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"));
        then.status(404).json_body(json!({
            "type": "https://messagecrate.app/docs/developer/reference/errors/not-found",
            "title": "Not found",
            "status": 404,
            "detail": "export 7 not found for this account"
        }));
    });
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    let error = run(&config(&out, server.base_url()), None).unwrap_err();

    assert_eq!(
        error.to_string(),
        format!(
            "Export Run {EXPORT_ID} page failed (HTTP 404 Not Found): \
             export 7 not found for this account"
        )
    );
    assert_eq!(page.calls(), 1, "a 404 Not Found is not retried");
    create.assert();
    fail.assert();
    assert_eq!(complete.calls(), 0);
    assert_eq!(cancel.calls(), 0);
    assert!(!out.join(CONVERSATION_FILE).exists());
    assert!(!load_journal(&out, &server).export_complete);
}

/// A page of a run the server closed (another Export of the account started,
/// or no page was read for an hour) ends the Export with the sentence that
/// says it was canceled, not quietly, and is not retried. Nothing tries to
/// close the run again: the server already did (#2080).
#[test]
fn a_page_of_a_run_the_server_closed_says_the_export_was_canceled() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let fail = mock_fail(&server);
    let cancel = mock_cancel(&server);
    let page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"));
        then.status(409).json_body(json!({
            "type": "https://messagecrate.app/docs/developer/reference/errors/state-conflict",
            "title": "State conflict",
            "status": 409,
            "detail": "export 7 is not running (status=canceled)"
        }));
    });
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    let error = run(&config(&out, server.base_url()), None).unwrap_err();

    assert_eq!(
        error.to_string(),
        message_crate_export::RUN_CANCELED_ON_SERVER
    );
    assert!(message_crate_export::is_run_canceled_on_server(&error));
    assert_eq!(page.calls(), 1, "a 409 Conflict is not retried");
    create.assert();
    assert_eq!(fail.calls(), 0);
    assert_eq!(complete.calls(), 0);
    assert_eq!(cancel.calls(), 0);
    assert!(!out.join(CONVERSATION_FILE).exists());
    assert!(!load_journal(&out, &server).export_complete);
}

/// A session the server refuses while the Assets are fetched (it expired, or
/// was ended from another window) refuses every other fetch too. The run
/// ends with the sentence that says to log in again, closed as failed,
/// rather than finishing with every Asset listed as missing.
#[test]
fn a_session_refused_while_fetching_assets_fails_the_run() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let fail = mock_fail(&server);
    let cancel = mock_cancel(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let _assets = server.mock(|when, then| {
        when.method(GET).path_prefix("/v1/assets/");
        then.status(401).body("unauthorized");
    });
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    let message = format!(
        "{:#}",
        run(&config(&out, server.base_url()), None).unwrap_err()
    );

    assert!(message.contains("Log in again"), "{message}");
    create.assert();
    fail.assert();
    assert_eq!(complete.calls(), 0);
    assert_eq!(cancel.calls(), 0);
    assert!(!out.join(CONVERSATION_FILE).exists());
}

/// A cancel that arrives while the pages are read closes the run as
/// canceled, the one way a run is recorded as canceled.
#[test]
fn a_cancel_during_the_run_closes_it_as_canceled() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let fail = mock_fail(&server);
    let cancel = mock_cancel(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, server.base_url());
    let flag = cfg.cancel.clone();
    let mut on_progress = |event: ProgressEvent| {
        // The first page has arrived; the person presses Cancel.
        if matches!(event, ProgressEvent::Page { .. }) {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    };

    let error = run(&cfg, Some(&mut on_progress)).unwrap_err();

    assert_eq!(error.to_string(), "canceled");
    create.assert();
    cancel.assert();
    assert_eq!(fail.calls(), 0);
    assert_eq!(complete.calls(), 0);
}

#[test]
fn a_scope_the_server_refuses_fails_the_run_with_the_servers_sentence() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let create = server.mock(|when, then| {
        when.method(POST).path("/v1/exports");
        then.status(400)
            .header("content-type", "application/problem+json")
            .json_body(json!({
                "type": "https://messagecrate.app/docs/developer/reference/errors/search-query-invalid",
                "title": "Search query invalid",
                "status": 400,
                "detail": "wibble: is not a word the Messages list has"
            }));
    });
    let (first, _second) = mock_pages(&server, "sms-backup-restore");
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = ExportConfig {
        query: "wibble:yes".into(),
        ..config(&out, server.base_url())
    };

    let error = run(&cfg, None).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Export Run start failed (HTTP 400 Bad Request): wibble: is not a word the Messages list has"
    );
    assert_eq!(create.calls(), 1, "a 400 is not retried");
    assert_eq!(first.calls(), 0);
}

/// The files are written before the run is closed, so a completion the server
/// refuses ends the Export as a success and leaves one line in the log that
/// names the Export Run once.
#[test]
fn a_refused_completion_is_a_warning_that_names_the_run_once() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _create = mock_create(&server, json!({ "kind": "everything" }));
    let complete = server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/exports/{EXPORT_ID}/complete"));
        then.status(409)
            .header("content-type", "application/problem+json")
            .json_body(json!({
                "type": "https://messagecrate.app/docs/developer/reference/errors/conflict",
                "title": "Conflict",
                "status": 409,
                "detail": "the run is already closed"
            }));
    });
    let _pages = mock_pages(&server, "sms-backup-restore");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    let mut events = Vec::new();
    {
        let mut progress = |event| events.push(event);
        run(&config(&out, server.base_url()), Some(&mut progress)).unwrap();
    }

    assert_eq!(complete.calls(), 1, "a 409 Conflict is not retried");
    assert!(
        events.contains(&ProgressEvent::Log(format!(
            "Export Run {EXPORT_ID} completion failed (HTTP 409 Conflict): \
             the run is already closed. The Export wrote its files all the same"
        ))),
        "{events:?}"
    );
}

#[test]
fn a_blank_token_or_output_directory_is_refused_before_login() {
    let dir = tempdir().unwrap();
    let base_url = "http://127.0.0.1:1".to_string();
    let blank_token = ExportConfig {
        token: "  ".into(),
        ..config(dir.path(), base_url.clone())
    };
    let blank_out_dir = ExportConfig {
        out_dir: Path::new("").to_path_buf(),
        ..config(dir.path(), base_url)
    };

    assert_eq!(
        run(&blank_token, None).unwrap_err().to_string(),
        "session token is required"
    );
    assert_eq!(
        run(&blank_out_dir, None).unwrap_err().to_string(),
        "output directory is required"
    );
}

/// The desktop app sends a session token, so a session the server refuses
/// (expired, or revoked from another window) says to log in again and names
/// no API key (#1400).
#[test]
fn a_refused_session_says_to_log_in_again() {
    let server = MockServer::start();
    let _session = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(401).body("unauthorized");
    });
    let dir = tempdir().unwrap();

    let message = format!(
        "{:#}",
        run(&config(dir.path(), server.base_url()), None).unwrap_err()
    );

    assert!(message.contains("Log in again"), "{message}");
    assert!(!message.contains("API key"), "{message}");
}

/// Staging names a file by date and fingerprint, so one menu sent on two days
/// has two paths on the server. The menu is fetched once, and every path a
/// message names exists after the Export Run holding the menu's bytes.
#[test]
fn every_path_a_message_names_exists_after_an_export() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (_create, _complete) = mock_run(&server);
    let _page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("limit", "2")
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [
                message(
                    1, "sms-backup-restore", "guid-1", "2015-03-12T18:05:01Z", "menu",
                    json!([menu_attachment(json!("attachments/20150312_180501-0a0a0a0a0a0a0a0a.pdf"))])
                ),
                message(
                    2, "sms-backup-restore", "guid-2", "2016-04-01T10:00:00Z", "menu again",
                    json!([menu_attachment(json!("attachments/20160401_100000-0a0a0a0a0a0a0a0a.pdf"))])
                )
            ],
            "total": 2,
            "limit": 2,
            "offset": 0
        }));
    });
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    run(&config(&out, server.base_url()), None).unwrap();

    menu.assert_calls(1);
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    for msg in &doc.messages {
        let rel = msg.attachments[0].path.as_deref().unwrap();
        assert_eq!(
            fs::read(out.join(rel)).ok().as_deref(),
            Some(MENU_BYTES),
            "message {} names {rel}, which the Export Run never wrote",
            msg.guid
        );
    }
}

/// Export keeps the reactions the server stored: a message whose stored
/// reactions are a friend's heart and the owner's emoji, as the export route
/// serializes them, is written with both in its `reactions`, each under the
/// person who reacted, in the shape an import reads back.
#[test]
fn an_exported_message_keeps_its_reactions_under_each_reactor() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let mut reacted = message(
        1,
        "sms-backup-restore",
        "guid-1",
        "2015-03-12T18:05:01Z",
        "Pizza?",
        json!([]),
    );
    reacted["tapbacks"] = json!([
        { "part_index": 0, "kind": "loved", "is_from_me": false, "sender": "+15555550107" },
        { "part_index": 0, "kind": "emoji", "emoji": "🔥", "is_from_me": true }
    ]);
    let _page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [reacted],
            "total": 1,
            "limit": 2,
            "offset": 0
        }));
    });
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    run(&config(&out, server.base_url()), None).unwrap();

    let written = fs::read_to_string(out.join(CONVERSATION_FILE)).unwrap();
    let line: Value = serde_json::from_str(written.lines().nth(1).unwrap()).unwrap();
    assert_eq!(
        line["reactions"],
        json!([
            { "part_index": 0, "kind": "loved", "is_from_me": false,
              "reactor_identity": "+15555550107" },
            { "part_index": 0, "kind": "emoji", "emoji": "🔥", "is_from_me": true }
        ]),
        "{line}"
    );
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    assert_eq!(
        doc.messages[0].reactions,
        [
            Reaction {
                part_index: 0,
                kind: "loved".into(),
                emoji: None,
                is_from_me: false,
                reactor_identity: Some("+15555550107".into()),
                reactor_display_name: None,
            },
            Reaction {
                part_index: 0,
                kind: "emoji".into(),
                emoji: Some("🔥".into()),
                is_from_me: true,
                reactor_identity: None,
                reactor_display_name: None,
            },
        ],
        "the file reads back as the same reactions"
    );
}

/// Export keeps each message's mark: a message the server returns as
/// `deleted_in_source_app`, one returned as `unsent`, and one with no mark
/// are written with `deletion` set to the same mark, and left out for the
/// last, in the shape an import reads back.
#[test]
fn an_exported_message_keeps_its_deletion_mark() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let mut deleted = message(
        1,
        "sms-backup-restore",
        "guid-16",
        "2020-01-06T12:14:00Z",
        "Delete me",
        json!([]),
    );
    deleted["deletion"] = json!("deleted_in_source_app");
    let mut unsent = message(
        2,
        "sms-backup-restore",
        "guid-17",
        "2020-01-06T12:15:00Z",
        "",
        json!([]),
    );
    unsent["deletion"] = json!("unsent");
    let mut kept = message(
        3,
        "sms-backup-restore",
        "guid-18",
        "2020-01-06T12:16:00Z",
        "Still here",
        json!([]),
    );
    kept["deletion"] = Value::Null;
    let _first = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [deleted, unsent],
            "total": 3,
            "limit": 2,
            "offset": 0
        }));
    });
    let _second = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "2");
        then.status(200).json_body(json!({
            "items": [kept],
            "total": 3,
            "limit": 2,
            "offset": 2
        }));
    });
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    run(&config(&out, server.base_url()), None).unwrap();

    let written = fs::read_to_string(out.join(CONVERSATION_FILE)).unwrap();
    let lines: Vec<Value> = written
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines[0]["deletion"], "deleted_in_source_app", "{written}");
    assert_eq!(lines[1]["deletion"], "unsent", "{written}");
    assert!(lines[2].get("deletion").is_none(), "{written}");
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    let marks: Vec<Option<Deletion>> = doc.messages.iter().map(|m| m.deletion).collect();
    assert_eq!(
        marks,
        [
            Some(Deletion::DeletedInSourceApp),
            Some(Deletion::Unsent),
            None
        ],
        "the file reads back as the same marks"
    );
}

/// Export keeps an edited message's earlier versions: each one the server
/// returns is written in the message's `edits` with its part, text and time,
/// in the server's order, and the file reads back as the same versions.
#[test]
fn an_exported_message_keeps_its_earlier_versions() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let mut edited = message(
        1,
        "sms-backup-restore",
        "guid-edited",
        "2020-01-06T12:14:00Z",
        "See you at noon",
        json!([]),
    );
    edited["earlier_versions"] = json!([
        { "part_index": 0, "text": "See you at 11", "edited_at": "2020-01-06T12:14:00Z", "matched": false },
        { "part_index": 0, "text": "See you at 11:30", "edited_at": "2020-01-06T12:15:00Z", "matched": false }
    ]);
    let _page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [edited],
            "total": 1,
            "limit": 500,
            "offset": 0
        }));
    });
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");

    run(&config(&out, server.base_url()), None).unwrap();

    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    assert_eq!(doc.messages[0].text, "See you at noon");
    assert_eq!(
        doc.messages[0].edits,
        [
            EarlierVersion {
                part_index: 0,
                text: Some("See you at 11".into()),
                edited_at_unix_ms: Some(1_578_312_840_000),
            },
            EarlierVersion {
                part_index: 0,
                text: Some("See you at 11:30".into()),
                edited_at_unix_ms: Some(1_578_312_900_000),
            },
        ]
    );
}

/// A server that answers each request on its own connection and closes it,
/// for a test that must act while a request is in hand: `answer` gets the
/// request line and returns the whole response to write. Returns the base
/// URL and every request line it saw, in order, without the HTTP version.
///
/// httpmock runs no code of the test's when a request arrives, so a test
/// built on it could press Cancel only after the answer, racing the retry.
/// The Upload's tests use a server of the same shape for the same reason
/// (`serve_answering_batches_with` in `message-crate-import`).
fn serve_by_hand(
    answer: impl Fn(&str) -> String + Send + 'static,
) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            let _ = reader.read_exact(&mut body);
            let request = request_line
                .trim_end()
                .trim_end_matches(" HTTP/1.1")
                .to_string();
            let response = answer(&request);
            recorded.lock().unwrap().push(request);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    (base_url, seen)
}

/// A whole HTTP response with `status` and the JSON `body`.
fn response(status: &str, body: &Value) -> String {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The `500 Internal Server Error` a request that may pass on a later try
/// gets.
fn server_error() -> String {
    response(
        "500 Internal Server Error",
        &json!({
            "type": "about:blank",
            "title": "Internal server error",
            "status": 500,
            "detail": "intentional failure"
        }),
    )
}

/// How many of the requests `seen` start with `prefix`.
fn requests_to(seen: &std::sync::Mutex<Vec<String>>, prefix: &str) -> usize {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|request| request.starts_with(prefix))
        .count()
}

/// The answer to a route every run below calls before the one that fails:
/// login, and the run recorded with every message. `None` for any other
/// request, which the test answers itself.
fn answer_login_and_create(request: &str) -> Option<String> {
    if request == "GET /v1/session" {
        Some(response(
            "200 OK",
            &json!({ "account_id": 1, "username": "alice" }),
        ))
    } else if request == "POST /v1/exports" {
        Some(response(
            "201 Created",
            &export_run(json!({ "kind": "everything" }), "running"),
        ))
    } else {
        None
    }
}

/// A cancel that arrives while a page is failing makes no further retry of
/// it: the wait before the next try ends at once, and the run is closed as
/// canceled, not failed, since a later try might have passed (#2111). The
/// server sets the cancel flag as the page request arrives, before it
/// answers, so the retry wait that follows always sees it. The close of the
/// canceled run fails too, and is tried once.
#[test]
fn a_cancel_while_a_page_fails_makes_no_further_retry_and_cancels_the_run() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, String::new());
    let pressed = cfg.cancel.clone();
    let (base_url, seen) = serve_by_hand(move |request| {
        answer_login_and_create(request).unwrap_or_else(|| {
            if request.starts_with(&format!("GET /v1/exports/{EXPORT_ID}/messages")) {
                pressed.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            server_error()
        })
    });
    let cfg = ExportConfig { base_url, ..cfg };

    let error = run(&cfg, None).unwrap_err();

    assert_eq!(error.to_string(), "canceled");
    let messages = format!("GET /v1/exports/{EXPORT_ID}/messages");
    assert_eq!(
        requests_to(&seen, &messages),
        1,
        "no retry after the cancel"
    );
    let cancel = format!("POST /v1/exports/{EXPORT_ID}/cancel");
    assert_eq!(requests_to(&seen, &cancel), 1, "{:?}", seen.lock().unwrap());
    let complete = format!("POST /v1/exports/{EXPORT_ID}/complete");
    assert_eq!(requests_to(&seen, &complete), 0);
}

/// A cancel that arrives while an Asset fetch is failing makes no further
/// retry of it, and the run is closed as canceled rather than finishing
/// with the Asset listed as missing (#2111).
#[test]
fn a_cancel_while_an_asset_fetch_fails_makes_no_further_retry_and_cancels_the_run() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, String::new());
    let pressed = cfg.cancel.clone();
    let (base_url, seen) = serve_by_hand(move |request| {
        answer_login_and_create(request).unwrap_or_else(|| {
            if request.starts_with(&format!("GET /v1/exports/{EXPORT_ID}/messages")) {
                response(
                    "200 OK",
                    &json!({
                        "items": [message(
                            1, "sms-backup-restore", "guid-1", "2015-03-12T18:05:01Z",
                            "dinner at seven?",
                            json!([menu_attachment(json!("attachments/menu.pdf"))])
                        )],
                        "total": 1,
                        "limit": 2,
                        "offset": 0
                    }),
                )
            } else if request.starts_with(&format!("POST /v1/exports/{EXPORT_ID}/cancel")) {
                response(
                    "200 OK",
                    &export_run(json!({ "kind": "everything" }), "canceled"),
                )
            } else {
                if request.starts_with("GET /v1/assets/") {
                    pressed.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                server_error()
            }
        })
    });
    let cfg = ExportConfig { base_url, ..cfg };

    let error = run(&cfg, None).unwrap_err();

    assert_eq!(error.to_string(), "canceled");
    assert_eq!(
        requests_to(&seen, &format!("GET /v1/assets/{MENU_SHA}")),
        1,
        "no retry after the cancel"
    );
    let cancel = format!("POST /v1/exports/{EXPORT_ID}/cancel");
    assert_eq!(requests_to(&seen, &cancel), 1, "{:?}", seen.lock().unwrap());
    let complete = format!("POST /v1/exports/{EXPORT_ID}/complete");
    assert_eq!(requests_to(&seen, &complete), 0);
    assert!(!out.join(CONVERSATION_FILE).exists());
}

/// A cancel that arrives while the run is being recorded makes no further
/// try to record it, and ends the Export as canceled (#2111).
#[test]
fn a_cancel_while_the_run_cannot_be_recorded_makes_no_further_retry() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = config(&out, String::new());
    let pressed = cfg.cancel.clone();
    let (base_url, seen) = serve_by_hand(move |request| {
        if request == "POST /v1/exports" {
            pressed.store(true, std::sync::atomic::Ordering::SeqCst);
            return server_error();
        }
        answer_login_and_create(request).unwrap_or_else(server_error)
    });
    let cfg = ExportConfig { base_url, ..cfg };

    let error = run(&cfg, None).unwrap_err();

    assert_eq!(error.to_string(), "canceled");
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            "GET /v1/session".to_string(),
            "POST /v1/exports".to_string()
        ],
        "no retry after the cancel, and no run to close"
    );
}

/// A Cancel pressed while the close of a finished run is failing does not
/// cut its retries short: the server closes a run left running only as
/// `canceled`, an hour later, so the run would be recorded as something
/// the person did not choose. The close is tried again and the run is
/// recorded as completed. The retry takes the real first backoff, about half
/// a second; nothing here depends on its length.
#[test]
fn a_cancel_while_a_finished_run_is_closed_keeps_the_close_retrying() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("exported");
    let cfg = ExportConfig {
        skip_attachments: true,
        ..config(&out, String::new())
    };
    let pressed = cfg.cancel.clone();
    let closes = std::sync::atomic::AtomicUsize::new(0);
    let (base_url, seen) = serve_by_hand(move |request| {
        answer_login_and_create(request).unwrap_or_else(|| {
            if request.starts_with(&format!("GET /v1/exports/{EXPORT_ID}/messages")) {
                response(
                    "200 OK",
                    &json!({
                        "items": [message(
                            1, "sms-backup-restore", "guid-1", "2015-03-12T18:05:01Z",
                            "dinner at seven?", json!([])
                        )],
                        "total": 1,
                        "limit": 2,
                        "offset": 0
                    }),
                )
            } else if request.starts_with(&format!("POST /v1/exports/{EXPORT_ID}/complete")) {
                if closes.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    pressed.store(true, std::sync::atomic::Ordering::SeqCst);
                    server_error()
                } else {
                    response(
                        "200 OK",
                        &export_run(json!({ "kind": "everything" }), "completed"),
                    )
                }
            } else {
                server_error()
            }
        })
    });
    let cfg = ExportConfig { base_url, ..cfg };

    let report = run(&cfg, None).unwrap();

    assert_eq!(report.messages, 1);
    let complete = format!("POST /v1/exports/{EXPORT_ID}/complete");
    assert_eq!(
        requests_to(&seen, &complete),
        2,
        "{:?}",
        seen.lock().unwrap()
    );
    let cancel = format!("POST /v1/exports/{EXPORT_ID}/cancel");
    assert_eq!(requests_to(&seen, &cancel), 0);
}
