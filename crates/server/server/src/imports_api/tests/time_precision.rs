//! A message's `time_precision` through import, the API and an Export Run,
//! and the whole-second twin of a message one source holds with
//! milliseconds too (#1923).

use super::*;

/// 2015-03-12T18:04:22Z, the second every message here falls in.
const SECOND: i64 = 1_426_183_462_000;

/// Create an Import Run for `source` with dedupe on, post `body` as its one
/// batch, and complete it.
async fn import_with_dedupe(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    body: String,
) {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": source, "mode": "append", "dedupe": true }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let (status, text) = crate::test_support::post_raw(
        state,
        &format!("/v1/imports/{id}/batches"),
        token,
        "application/jsonl",
        body,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let _: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
}

/// An SMS Backup+ file holding `line` as its one message.
fn sms_backup_plus_file(line: MessageLine) -> String {
    let header =
        conversation_header("sms-backup-plus", "+15555550123").participant("+15555550123", None);
    format!("{header}\n{}\n", line.sms().sender("+15555550123"))
}

/// The same SMS Backup+ message imported once from a file timed in whole
/// seconds and once from a file timed in milliseconds, in either order, is
/// shown once, with the millisecond time.
#[tokio::test]
async fn a_whole_second_copy_and_a_millisecond_copy_are_shown_once_with_the_milliseconds() {
    let whole = || {
        sms_backup_plus_file(
            message_line("g-whole", "On my way")
                .at(SECOND)
                .whole_seconds(),
        )
    };
    let exact = || sms_backup_plus_file(message_line("g-exact", "On my way").at(SECOND + 250));
    for (label, files) in [
        ("whole second first", [whole(), exact()]),
        ("milliseconds first", [exact(), whole()]),
    ] {
        let (state, _fixture, token) = importer().await;
        for file in files {
            import_with_dedupe(&state, &token, "sms-backup-plus", file).await;
        }
        let page: serde_json::Value = get_json(&state, "/v1/messages", &token).await;
        let items = page["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "{label}: {page}");
        assert_eq!(items[0]["timestamp"], "2015-03-12T18:04:22.250Z", "{label}");
        assert_eq!(items[0]["time_precision"], "milliseconds", "{label}");
    }
}

/// A message whose source recorded whole seconds and one whose source
/// recorded milliseconds that end in `.000` keep their precision through
/// the import, the API and an Export Run, and neither hides the other: they
/// are two messages, and the flag, never the time, says which has
/// milliseconds.
#[tokio::test]
async fn each_precision_is_kept_through_import_the_api_and_an_export_run() {
    let (state, _fixture, token) = importer().await;
    let header =
        conversation_header("sms-backup-plus", "+15555550123").participant("+15555550123", None);
    let whole = message_line("g-whole", "whole second")
        .at(SECOND)
        .whole_seconds()
        .sms()
        .sender("+15555550123");
    let exact = message_line("g-exact", "milliseconds that end in .000")
        .at(SECOND + 1000)
        .sms()
        .sender("+15555550123");
    import_with_dedupe(
        &state,
        &token,
        "sms-backup-plus",
        format!("{header}\n{whole}\n{exact}\n"),
    )
    .await;

    let precisions = |page: &serde_json::Value| -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["guid"].as_str().unwrap().to_string(),
                    m["time_precision"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        out.sort();
        out
    };
    let expected = vec![
        ("g-exact".to_string(), "milliseconds".to_string()),
        ("g-whole".to_string(), "seconds".to_string()),
    ];

    let listed: serde_json::Value = get_json(&state, "/v1/messages", &token).await;
    assert_eq!(precisions(&listed), expected, "{listed}");

    let (_, run): (String, serde_json::Value) = post_created_json(
        &state,
        "/v1/exports",
        &token,
        serde_json::json!({ "scope": { "kind": "everything" } }),
    )
    .await;
    let exported: serde_json::Value = get_json(
        &state,
        &format!("/v1/exports/{}/messages", run["id"]),
        &token,
    )
    .await;
    assert_eq!(precisions(&exported), expected, "{exported}");
}
