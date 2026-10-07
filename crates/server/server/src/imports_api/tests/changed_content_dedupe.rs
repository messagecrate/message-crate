//! An import that changes a stored message's content puts its duplicate
//! flag right, whatever the import's dedupe setting (#1805).

use super::*;

/// 2015-03-12T18:04:22Z, the second every message here falls in.
const SECOND: i64 = 1_426_183_462_000;

/// The chat every message here is in, and its one other participant, who
/// sends every message.
const CHAT: &str = "+15555550123";

/// Create an Import Run for `source` with `dedupe` on or off, post `lines`
/// under one conversation header as its one batch, and complete it.
async fn import(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    dedupe: bool,
    lines: &[MessageLine],
) {
    let header = conversation_header(source, CHAT).participant(CHAT, None);
    let mut body = format!("{header}\n");
    for line in lines {
        body.push_str(&format!("{}\n", line.clone().sender(CHAT).at(SECOND)));
    }
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": source, "mode": "append", "dedupe": dedupe }),
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

/// The message `guid` edited once, from "see you at six" to `text`.
fn edited(guid: &str, text: &str) -> MessageLine {
    message_line(guid, text).edit(EarlierVersion {
        part_index: 0,
        text: "see you at six".into(),
        edited_at_unix_ms: Some(SECOND + 60_000),
    })
}

/// The guid of the message `guid` is hidden behind, or `None` when it is
/// shown.
async fn hidden_behind(state: &crate::server::AppState, guid: &str) -> Option<String> {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_scalar(
        r"
        SELECT w.guid
        FROM messages m
        LEFT JOIN messages w ON w.id = m.duplicate_of
        WHERE m.guid = $1
        ",
    )
    .bind(guid)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// The texts a search for `word` finds.
async fn found(state: &crate::server::AppState, token: &str, word: &str) -> Vec<String> {
    let page: serde_json::Value = get_json(state, &format!("/v1/messages?q={word}"), token).await;
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap().to_string())
        .collect()
}

/// The issue's scenario: a message hidden behind another source's copy of
/// its text takes a later edit from an append with dedupe off. It no longer
/// matches that copy, so it is shown, and a search for its new text finds
/// it. A new message the same import brings stays shown beside the copy
/// it duplicates, because the import's dedupe is off.
#[tokio::test]
async fn an_edit_with_dedupe_off_shows_a_message_hidden_behind_a_copy_of_its_old_text() {
    let (state, _fixture, token) = importer().await;
    import(
        &state,
        &token,
        "sms",
        true,
        &[
            message_line("n-six", "see you at six"),
            message_line("n-lunch", "lunch?"),
        ],
    )
    .await;
    import(
        &state,
        &token,
        "imessage",
        true,
        &[message_line("m-six", "see you at six")],
    )
    .await;
    assert_eq!(
        hidden_behind(&state, "m-six").await.as_deref(),
        Some("n-six")
    );

    import(
        &state,
        &token,
        "imessage",
        false,
        &[
            edited("m-six", "see you at seven"),
            message_line("m-lunch", "lunch?"),
        ],
    )
    .await;

    assert_eq!(hidden_behind(&state, "m-six").await, None);
    assert_eq!(found(&state, &token, "seven").await, ["see you at seven"]);
    assert_eq!(
        hidden_behind(&state, "m-lunch").await,
        None,
        "dedupe off leaves the import's new messages as they came"
    );
}

/// A message other copies are hidden behind takes a later edit from an
/// append with dedupe off. The copy of its old text is shown again, and a
/// copy of its new text from a third source is hidden behind it.
#[tokio::test]
async fn an_edit_with_dedupe_off_evaluates_the_copies_around_the_message_again() {
    let (state, _fixture, token) = importer().await;
    import(
        &state,
        &token,
        "imessage",
        true,
        &[message_line("m-six", "see you at six")],
    )
    .await;
    import(
        &state,
        &token,
        "sms",
        true,
        &[message_line("r-six", "see you at six")],
    )
    .await;
    import(
        &state,
        &token,
        "whatsapp",
        true,
        &[message_line("q-seven", "see you at seven")],
    )
    .await;
    assert_eq!(
        hidden_behind(&state, "r-six").await.as_deref(),
        Some("m-six")
    );
    assert_eq!(hidden_behind(&state, "q-seven").await, None);

    import(
        &state,
        &token,
        "imessage",
        false,
        &[edited("m-six", "see you at seven")],
    )
    .await;

    assert_eq!(hidden_behind(&state, "m-six").await, None);
    assert_eq!(
        hidden_behind(&state, "r-six").await,
        None,
        "the old text no longer matches"
    );
    assert_eq!(
        hidden_behind(&state, "q-seven").await.as_deref(),
        Some("m-six"),
        "the new text matches"
    );
    assert_eq!(
        found(&state, &token, "seven").await,
        ["see you at seven"],
        "the two copies of the new text are found once"
    );
}

/// A message another copy is hidden behind gains its attachment's file from
/// an append with dedupe off. The copy, with the same text and no
/// attachment, still matches it within the near-time window and stays
/// hidden, and the message's content key hashes the file it now has.
///
/// The duplicate-flag assertions hold without the fix too, because no flag
/// changes here: only the content key computed again, the last assertion,
/// fails without it.
#[tokio::test]
async fn an_attachment_with_dedupe_off_keeps_a_copy_that_still_matches_hidden() {
    let (state, _fixture, token) = importer().await;
    let missing = IrAttachment {
        size_bytes: Some(12),
        missing_reason: Some("not_found".into()),
        ..attachment(
            "attachments/photo.bin",
            "photo.bin",
            "application/octet-stream",
        )
    };
    import(
        &state,
        &token,
        "imessage",
        true,
        &[message_line("m-photo", "see attached").attachment(missing)],
    )
    .await;
    import(
        &state,
        &token,
        "sms",
        true,
        &[message_line("r-photo", "see attached")],
    )
    .await;
    assert_eq!(
        hidden_behind(&state, "r-photo").await.as_deref(),
        Some("m-photo")
    );
    let key = |state: crate::server::AppState| async move {
        let mut conn = state.db.acquire().await.unwrap();
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT content_key FROM messages WHERE guid = 'm-photo'",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap()
    };
    let before = key(state.clone()).await;

    let bytes = b"photo-bytes".to_vec();
    let sha = assets_api::sha256_hex(&bytes);
    let (status, text) = crate::test_support::put_raw(
        &state,
        &format!("/v1/assets/{sha}"),
        &token,
        "application/octet-stream",
        bytes.clone(),
    )
    .await;
    assert!(status.is_success(), "{status}: {text}");
    let found_file = IrAttachment {
        digest_sha256: Some(sha),
        size_bytes: Some(bytes.len() as u64),
        ..attachment(
            "attachments/photo.bin",
            "photo.bin",
            "application/octet-stream",
        )
    };
    import(
        &state,
        &token,
        "imessage",
        false,
        &[message_line("m-photo", "see attached").attachment(found_file)],
    )
    .await;

    let mut conn = state.db.acquire().await.unwrap();
    let files: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachments a JOIN messages m ON m.id = a.message_id \
         WHERE m.guid = 'm-photo' AND a.sha256 IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    drop(conn);
    assert_eq!(files, 1, "the append gives the stored attachment its file");
    assert_eq!(hidden_behind(&state, "m-photo").await, None);
    assert_eq!(
        hidden_behind(&state, "r-photo").await.as_deref(),
        Some("m-photo")
    );
    assert_ne!(
        key(state.clone()).await,
        before,
        "the content key hashes the attachment's file"
    );
}

/// An append with dedupe off that changes no stored message's content runs
/// no dedupe: a flag the full pass would set stays unset.
///
/// A guard, not a test of the fix: it passes without the fix, and fails
/// when the fix runs a dedupe over the whole account.
#[tokio::test]
async fn an_append_with_dedupe_off_that_changes_nothing_stored_runs_no_dedupe() {
    let (state, _fixture, token) = importer().await;
    import(
        &state,
        &token,
        "sms",
        true,
        &[message_line("n-six", "see you at six")],
    )
    .await;
    import(
        &state,
        &token,
        "imessage",
        true,
        &[message_line("m-six", "see you at six")],
    )
    .await;
    {
        let mut conn = state.db.acquire().await.unwrap();
        let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
        sqlx::query("UPDATE messages SET duplicate_of = NULL WHERE guid = 'm-six'")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    import(
        &state,
        &token,
        "imessage",
        false,
        &[message_line("m-six", "see you at six")],
    )
    .await;

    assert_eq!(hidden_behind(&state, "m-six").await, None);
}

/// A message an import with dedupe off brought, with no content key, takes
/// a later edit from another append with dedupe off. Its new text matches
/// a copy from a source imported with dedupe on, but no dedupe was asked
/// for its own source, so it stays as it came: shown, without a content
/// key, and the copy stays shown beside it.
#[tokio::test]
async fn an_edit_with_dedupe_off_leaves_a_message_no_dedupe_has_seen_as_it_came() {
    let (state, _fixture, token) = importer().await;
    import(
        &state,
        &token,
        "sms",
        true,
        &[
            message_line("n-six", "see you at six"),
            message_line("n-seven", "see you at seven"),
        ],
    )
    .await;
    import(
        &state,
        &token,
        "imessage",
        false,
        &[message_line("m-six", "see you at six")],
    )
    .await;
    assert_eq!(hidden_behind(&state, "m-six").await, None);

    import(
        &state,
        &token,
        "imessage",
        false,
        &[edited("m-six", "see you at seven")],
    )
    .await;

    assert_eq!(hidden_behind(&state, "m-six").await, None);
    assert_eq!(hidden_behind(&state, "n-seven").await, None);
    let mut conn = state.db.acquire().await.unwrap();
    let key: Option<String> =
        sqlx::query_scalar("SELECT content_key FROM messages WHERE guid = 'm-six'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(key, None, "no dedupe has seen the message");
}
