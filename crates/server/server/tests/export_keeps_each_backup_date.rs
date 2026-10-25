//! A conversation stored from two backups is exported with each message's
//! own Backup Date, and an import of that file into another Message Crate
//! stores each message with the date it had (#1967).
//!
//! It runs two server binaries and the desktop app's own import and export
//! libraries, so the Import Runs and the Export Run go the way the desktop
//! app's do.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use common::lines::{conversation_header, message_line};
use common::setup::set_up_with_account;
use common::upload::upload;
use common::{empty_message_crate, listen, serve};

/// 2026-09-01T10:00:00Z, the older backup.
const EARLIER_BACKUP: i64 = 1_788_256_800_000;
/// 2026-09-30T18:45:12Z, the newer backup.
const LATER_BACKUP: i64 = 1_790_793_912_000;
/// The Export Date the Export Run is given.
const EXPORTED_AT: i64 = 1_791_000_000_000;

/// A backup of the conversation with `+15555550123` holding `messages`
/// (guid, time, Backup Date), written as one conversation file under
/// `root/name`.
fn backup(root: &Path, name: &str, messages: &[(&str, i64, Option<i64>)]) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let header =
        conversation_header("sms-backup-restore", "+15555550123").participant("+15555550123", None);
    let lines: String = messages
        .iter()
        .map(|(guid, at, backup)| {
            message_line(guid, "hello")
                .at(*at)
                .sms()
                .sender("+15555550123")
                .backup_taken_at(*backup)
                .line()
        })
        .collect();
    std::fs::write(
        dir.join("+15555550123.jsonl"),
        format!("{}{lines}", header.line()),
    )
    .unwrap();
    dir
}

/// Each message's `backup_taken_at` as `GET /v1/messages` answers it, by guid.
async fn stored_dates(base: &str, token: &str) -> BTreeMap<String, Value> {
    let page: Value = common::client::http_client()
        .get(format!("{base}/v1/messages"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["guid"].as_str().unwrap().to_owned(),
                item["backup_taken_at"].clone(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_export_of_a_conversation_from_two_backups_keeps_each_messages_date_elsewhere() {
    let root = tempfile::tempdir().unwrap();

    // The first Message Crate holds one conversation from two backups.
    let (data_dir, static_dir) = empty_message_crate(&root.path().join("first"));
    let (_first, address) = listen(&mut serve(&data_dir, &static_dir));
    let first = format!("http://{address}");
    let first_token = set_up_with_account(&first).await.alice_token;
    let older = backup(
        root.path(),
        "older",
        &[("g-older", 1_426_183_462_000, Some(EARLIER_BACKUP))],
    );
    let newer = backup(
        root.path(),
        "newer",
        &[
            ("g-newer", 1_426_183_463_000, Some(LATER_BACKUP)),
            ("g-undated", 1_426_183_464_000, None),
        ],
    );
    upload(&first, &first_token, "sms-backup-restore", older).await;
    upload(&first, &first_token, "sms-backup-restore", newer).await;
    let expected = BTreeMap::from([
        (
            "g-older".to_owned(),
            Value::from("2026-09-01T10:00:00.000Z"),
        ),
        (
            "g-newer".to_owned(),
            Value::from("2026-09-30T18:45:12.000Z"),
        ),
        ("g-undated".to_owned(), Value::Null),
    ]);
    assert_eq!(stored_dates(&first, &first_token).await, expected);

    // An Export Run writes each message's stored date into one file.
    let exported = root.path().join("exported");
    let config = export::ExportConfig {
        out_dir: exported.clone(),
        base_url: first.clone(),
        token: first_token.clone(),
        query: String::new(),
        list: export::ExportQueryList::Messages,
        skip_attachments: true,
        page_limit: export::DEFAULT_PAGE_LIMIT,
        cancel: Default::default(),
        asset_fetch_workers: 1,
        exported_at_unix_ms: EXPORTED_AT,
    };
    #[allow(clippy::disallowed_methods)]
    tokio::task::spawn_blocking(move || export::run(&config, None))
        .await
        .unwrap()
        .unwrap();
    let text =
        std::fs::read_to_string(exported.join("+15555550123__sms-backup-restore.jsonl")).unwrap();
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: ir::ConversationHeader = serde_json::from_str(lines.next().unwrap()).unwrap();
    let messages: Vec<ir::Message> = lines
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(header.export.exported_at_unix_ms, Some(EXPORTED_AT));
    assert_eq!(
        messages
            .iter()
            .map(|m| (m.guid.as_str(), m.backup_taken_at_unix_ms))
            .collect::<Vec<_>>(),
        [
            ("g-older", Some(EARLIER_BACKUP)),
            ("g-newer", Some(LATER_BACKUP)),
            ("g-undated", None),
        ]
    );

    // A second Message Crate stores each message with the date it had.
    let (data_dir, static_dir) = empty_message_crate(&root.path().join("second"));
    let (_second, address) = listen(&mut serve(&data_dir, &static_dir));
    let second = format!("http://{address}");
    let second_token = set_up_with_account(&second).await.alice_token;
    upload(&second, &second_token, "sms-backup-restore", exported).await;
    assert_eq!(stored_dates(&second, &second_token).await, expected);
}
