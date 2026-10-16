//! An Upload through the import library, as the desktop app sends one: the
//! test starts the Import Run, the library posts every conversation file
//! into it, and the test completes it. The library neither starts nor
//! completes a run (#2053).

use std::path::PathBuf;

use reqwest::Method;
use reqwest::StatusCode;
use serde_json::json;

use super::claim::{call, json_body};

/// Upload every conversation file under `input` to `base` as `token`, into
/// an Import Run of `source` the test starts and then completes. The
/// completion runs the end of the run, which hides the account's
/// duplicates. The Upload must be clean: every conversation sent, none
/// failed or left by a pause, and no message failed. The helper completes
/// each run as `completed`, the status the desktop app gives only a clean
/// Upload, and a test that read a partly imported account would fail far
/// from the cause.
pub async fn upload(base: &str, token: &str, source: &str, input: PathBuf) {
    let (status, started) = call(
        base,
        Method::POST,
        "/v1/imports",
        Some(token),
        json_body(&json!({ "source": source, "mode": "append" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let import_id = started["id"].as_i64().unwrap();

    let config = import::ImportConfig {
        input,
        base_url: base.to_string(),
        token: token.to_string(),
        mode: import::ImportMode::Append,
        force: false,
        max_retries: 0,
        batch_size: import::NO_MESSAGE_COUNT_LIMIT,
        asset_upload_workers: import::DEFAULT_ASSET_UPLOAD_WORKERS,
        prepare_ahead: import::DEFAULT_PREPARE_AHEAD,
        prepare_workers: import::DEFAULT_PREPARE_WORKERS,
        asset_multipart_threshold: 5 * 1024 * 1024,
        asset_max_bytes: import::DEFAULT_ASSET_MAX_BYTES,
        log_path: None,
        cancel: Default::default(),
        import_id,
    };
    // tokio's own call: `request_id::spawn_blocking_in_current_span` is the
    // server crate's own, and a test under `tests/` cannot call it.
    #[allow(clippy::disallowed_methods)]
    let report = tokio::task::spawn_blocking(move || import::run(&config, None))
        .await
        .unwrap()
        .unwrap();
    // `ok` is false when a conversation failed or the Upload paused.
    assert!(report.ok && report.messages_failed == 0, "{report:?}");

    let (status, completed) = call(
        base,
        Method::POST,
        &format!("/v1/imports/{import_id}/complete"),
        Some(token),
        json_body(&json!({ "status": "completed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{completed}");
}
