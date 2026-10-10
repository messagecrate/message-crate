//! An Upload through the import library, as the desktop app sends one, has
//! its duplicates hidden: nothing in the library or the Import Run turns the
//! dedupe on, because every batch runs it (#1969).
//!
//! It runs the server binary and the library's own `run`, so the Import Run
//! is created and filled the way the desktop app's is.

mod common;

use serde_json::Value;

use common::claim::claimed_with_account;
use common::lines::{conversation_header, message_line};
use common::{empty_message_crate, listen, serve};

/// 2015-03-12T18:04:22Z.
const SECOND: i64 = 1_426_183_462_000;

/// An SMS Backup+ backup that holds one message twice, once timed to the
/// second and once to the millisecond, is shown once, with its
/// milliseconds, after an Upload with the import library's settings.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_with_the_import_librarys_settings_hides_the_whole_second_twin() {
    let root = tempfile::tempdir().unwrap();
    let (data_dir, static_dir) = empty_message_crate(root.path());
    let (_server, address) = listen(&mut serve(&data_dir, &static_dir));
    let base = format!("http://{address}");

    let token = claimed_with_account(&base).await.alice_token;

    let input = root.path().join("export");
    std::fs::create_dir_all(&input).unwrap();
    let header =
        conversation_header("sms-backup-plus", "+15555550123").participant("+15555550123", None);
    let whole = message_line("g-whole", "On my way")
        .at(SECOND)
        .whole_seconds()
        .sms()
        .sender("+15555550123");
    let exact = message_line("g-exact", "On my way")
        .at(SECOND + 250)
        .sms()
        .sender("+15555550123");
    std::fs::write(
        input.join("+15555550123.jsonl"),
        format!("{}{}{}", header.line(), whole.line(), exact.line()),
    )
    .unwrap();

    let config = message_crate_import::ImportConfig {
        input,
        base_url: base.clone(),
        token: token.clone(),
        mode: message_crate_import::ImportMode::Append,
        force: false,
        max_retries: 0,
        batch_size: message_crate_import::NO_MESSAGE_COUNT_LIMIT,
        asset_upload_workers: message_crate_import::DEFAULT_ASSET_UPLOAD_WORKERS,
        prepare_ahead: message_crate_import::DEFAULT_PREPARE_AHEAD,
        prepare_workers: message_crate_import::DEFAULT_PREPARE_WORKERS,
        asset_multipart_threshold: 5 * 1024 * 1024,
        asset_max_bytes: message_crate_import::DEFAULT_ASSET_MAX_BYTES,
        log_path: None,
        cancel: None,
        import_id: None,
        phone_country: None,
    };
    let report = tokio::task::spawn_blocking(move || message_crate_import::run(&config, None))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.messages_failed, 0, "{report:?}");

    let page: Value = common::client::http_client()
        .get(format!("{base}/v1/messages"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "the message is shown once: {page}");
    assert_eq!(items[0]["timestamp"], "2015-03-12T18:04:22.250Z");
    assert_eq!(items[0]["time_precision"], "milliseconds");
}
