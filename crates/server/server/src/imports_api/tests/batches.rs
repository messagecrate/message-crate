//! The batch route: which batches it refuses and with what answer, and
//! a batch posted again storing each message once.

use super::*;

/// Each server counts its own import slots. With every slot of one server
/// taken, a second server in the same process still imports, and an import
/// waits on its own server's slots: closing them refuses it.
#[tokio::test]
async fn import_slots_belong_to_one_server() {
    let (busy, _busy_fixture, busy_token) = importer().await;
    let (idle, _idle_fixture, idle_token) = importer().await;
    let every_slot = busy
        .import_slots
        .try_acquire_many(MAX_CONCURRENT_IMPORTS as u32)
        .expect("a new server has every import slot free");
    let body = format!(
        "{}\n{}\n",
        conversation_header("imessage", "+15555550123").participant("+15555550123", None),
        message_line("g-slots", "hi").sender("+15555550123"),
    );

    let path = batches_path(&idle, &idle_token, "imessage").await;
    let (status, text) =
        crate::test_support::post_raw(&idle, &path, &idle_token, "application/jsonl", body.clone())
            .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");

    busy.import_slots.close();
    drop(every_slot);
    let path = batches_path(&busy, &busy_token, "imessage").await;
    let (status, text) =
        crate::test_support::post_raw(&busy, &path, &busy_token, "application/jsonl", body).await;
    crate::test_support::expect_internal_problem(status, &text);
}

/// A schema-4 header was read: version 4 named every identity a `handle`,
/// and nothing upgrades it. Its version breaks a rule, so it is 422.
#[tokio::test]
async fn http_import_of_a_schema_4_file_is_a_422_naming_both_versions() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let body = concat!(
        r#"{"schema_version":4,"export":{"source":"whatsapp","tool":"t","tool_version":"1","owner_handle":"+15555550106","owner_display_name":"Me"},"#,
        r#""conversation":{"chat_identifier":"+15555550107","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550107","display_name":"Sam","handle_type":"phone"}]}}"#,
        "\n",
    );
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(
        problem.errors.unwrap(),
        vec![format!(
            "This file is schema version 4; Message Crate reads version {} (line 1 of the batch).",
            message_ir::SCHEMA_VERSION
        )]
    );
    assert_eq!(problem.line, Some(1), "{text}");
}

/// A batch is a request body, not a file the sender has: Upload packs it
/// from parts of one or more staged files. The failure names the line of the
/// batch, in the sentence and as `line`, so the client can turn it into the
/// line of the file it came from. Only a line that is not JSON at all cannot
/// be read, so only it is 400.
#[tokio::test]
async fn http_import_of_a_line_that_is_not_json_is_a_400_naming_the_line_of_the_batch() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let header = conversation_header("whatsapp", "+15555550107")
        .owner("+15555550106", Some("Me"))
        .participant("+15555550107", Some("Sam"));
    let body = format!("{header}\n\nthis is not json\n");
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::MalformedBody,
    );
    assert_eq!(problem.line, Some(3), "{text}");
    let message = problem.detail.unwrap();
    assert!(
        message.starts_with("Could not read line 3 of the batch:"),
        "{message}"
    );
}

/// A line that is not UTF-8 cannot be read as text, let alone as JSON, so
/// it is 400 naming the line, not a 500.
#[tokio::test]
async fn http_import_of_a_line_that_is_not_utf8_is_a_400_naming_the_line() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let mut body = replace_run_batch("+15555550151", &["g-1"]).into_bytes();
    body.extend_from_slice(b"{\"guid\":\"\xff\xfe\"}\n");
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::MalformedBody,
    );
    let message = problem.detail.unwrap();
    assert!(
        message.starts_with("Could not read line 3 of the batch:"),
        "{message}"
    );
}

/// A line that breaks a rule is named by its line in the batch, blank lines
/// counted, in the sentence and as `line`, as a line that is not JSON is: the
/// client maps `line` back to the staged file and line it packed it from.
#[tokio::test]
async fn a_refusal_after_a_blank_line_names_the_line_of_the_batch() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let header = replace_run_batch("+15555550151", &[]);
    let body = format!("{header}\n{{\"guid\":7}}\n");
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(problem.line, Some(3), "{text}");
    assert!(
        problem.errors.unwrap()[0].starts_with("Line 3 of the batch:"),
        "{text}"
    );
}

/// C1-7: a header that is JSON with the wrong fields was read and broke a
/// rule, as the same mistake in a JSON body does, so it answers 422.
#[tokio::test]
async fn a_batch_whose_header_has_the_wrong_fields_is_a_422_naming_the_line() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    // The builder writes only valid headers, so this one is written out; its
    // version is the current one, so the fields are what it breaks.
    let header = serde_json::json!({
        "schema_version": message_ir::SCHEMA_VERSION,
        "export": {"source": "whatsapp"},
        "conversation": {"chat_identifier": 7},
    });
    let body = format!("{header}\n");
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert!(
        problem.errors.unwrap()[0]
            .starts_with("Line 1 of the batch: the conversation header is not valid"),
        "{text}"
    );
}

/// C1-7: an empty batch was read; it holds no conversation, which breaks a
/// rule, so it answers 422.
#[tokio::test]
async fn an_empty_batch_is_a_422() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", "").await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
}

/// One conversation whose single message has one attachment at `path`,
/// stating `sha` when there is one. Every caller tests a refusal of the path
/// or the fingerprint, so the line stays written out. `path` and `sha` go
/// into the JSON as they are, so neither may hold `"` or `\`.
fn one_attachment_batch(path: &str, sha: Option<&str>) -> String {
    let header = conversation_header("whatsapp", "+15555550151")
        .owner("+15555550150", Some("Me"))
        .participant("+15555550151", None);
    let digest = sha.map_or("null".to_string(), |sha| format!(r#""{sha}""#));
    format!(
        r#"{header}
{{"guid":"g-att","timestamp_unix_ms":1700000000000,"time_precision":"milliseconds","direction":"incoming","service":"whatsapp","message_kind":"sms","sender_identity":"+15555550151","sender_display_name":null,"subject":null,"text":"x","attachments":[{{"path":"{path}","original_name":"a.bin","mime_type":"application/octet-stream","digest_sha256":{digest},"is_sticker":false,"transcription":null,"sticker_effect":null}}],"imessage":null,"source":null}}
"#
    )
}

/// S1-11: an attachment path that leaves the directory is the sender's to fix,
/// so it answers 422 naming the path, not 500.
#[tokio::test]
async fn a_batch_with_an_unsafe_attachment_path_is_a_422_naming_the_path() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let body = one_attachment_batch("../secret.txt", None);
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    // Line 1 is the header; the message with the attachment is line 2.
    assert_eq!(problem.line, Some(2), "{text}");
    assert!(
        problem.errors.unwrap()[0].contains("../secret.txt"),
        "{text}"
    );
}

/// A path of spaces is refused as `.` is, even when the batch states the
/// fingerprint of a file the server holds: the row would keep the path as
/// sent, and the server's own check refuses it (#1410).
#[tokio::test]
async fn a_blank_attachment_path_is_refused_even_with_a_stored_fingerprint() {
    let (fixture, account) = fixture_with_account().await;
    let state = fixture.state.clone();
    let path = batches_path(&state, &account.token, "whatsapp").await;
    let assets_dir = state.cfg.paths.assets_dir_for_account(account.account_id);
    fs::create_dir_all(&assets_dir).unwrap();
    let held = fixture.dir().join("held.bin");
    fs::write(&held, b"bytes the server holds").unwrap();
    let mut stats = assets_api::AssetStats::default();
    let stored = assets_api::hash_and_store(&held, &assets_dir, None, &mut stats)
        .unwrap()
        .expect("stored");
    let body = one_attachment_batch("   ", Some(&stored.sha256));

    let (status, text) =
        crate::test_support::post_raw(&state, &path, &account.token, "application/jsonl", body)
            .await;

    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(problem.line, Some(2), "{text}");
    assert!(
        problem.errors.unwrap()[0].contains(message_ir::UNSAFE_ATTACHMENT_PATH),
        "refused as an unsafe path, as `.` is: {text}"
    );
    let rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM attachments) + (SELECT COUNT(*) FROM staging_attachments)",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(rows, 0, "no attachment row is stored");
}

/// S1-11: a file whose bytes do not hash to the SHA-256 the batch states is
/// the sender's to fix, so it answers 422 naming the file, not 500.
#[tokio::test]
async fn a_batch_whose_file_does_not_match_its_sha256_is_a_422_naming_the_file() {
    let (fixture, account) = fixture_with_account().await;
    let state = fixture.state.clone();
    let path = batches_path(&state, &account.token, "whatsapp").await;
    let assets_dir = state.cfg.paths.assets_dir_for_account(account.account_id);
    fs::create_dir_all(&assets_dir).unwrap();
    fs::write(assets_dir.join("photo.bin"), b"the bytes on disk").unwrap();
    let stated = assets_api::sha256_hex(b"the bytes the export saw");
    let body = one_attachment_batch("photo.bin", Some(&stated));
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &account.token, "application/jsonl", body)
            .await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(problem.line, Some(2), "{text}");
    assert!(problem.errors.unwrap()[0].contains("photo.bin"), "{text}");
}

/// A stated SHA-256 that is not 64 hex digits, on a file that is there, is
/// the sender's to fix too: 422 naming the line and the file.
#[tokio::test]
async fn a_batch_whose_stated_sha256_is_not_one_is_a_422_naming_the_file() {
    let (fixture, account) = fixture_with_account().await;
    let state = fixture.state.clone();
    let path = batches_path(&state, &account.token, "whatsapp").await;
    let assets_dir = state.cfg.paths.assets_dir_for_account(account.account_id);
    fs::create_dir_all(&assets_dir).unwrap();
    fs::write(assets_dir.join("photo.bin"), b"the bytes on disk").unwrap();
    let body = one_attachment_batch("photo.bin", Some("not-a-fingerprint"));
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &account.token, "application/jsonl", body)
            .await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(problem.line, Some(2), "{text}");
    assert!(problem.errors.unwrap()[0].contains("photo.bin"), "{text}");
}

/// A batch is refused before its body is read when the run is over: the
/// row, not the request, says what a batch imports under, and a discarded
/// run has nothing to import under.
#[tokio::test]
async fn a_batch_into_a_run_that_is_not_running_is_a_state_conflict() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let id = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .to_string();
    post_json::<serde_json::Value>(
        &state,
        &format!("/v1/imports/{id}/discard"),
        &token,
        serde_json::json!({ "issues": [], "notes": [] }),
    )
    .await;

    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", "{}\n").await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::StateConflict,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some(format!("import {id} is not running (status=cancelled)").as_str())
    );
}

/// A run discarded while a batch uploads: the batch passed the check before
/// its body, and its messages were then stored under a cancelled run. The
/// run is checked again inside the import's write transaction, and the batch
/// is refused the same way.
#[tokio::test]
async fn a_batch_into_a_run_discarded_while_it_uploads_is_a_state_conflict() {
    let (state, fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();

    let mut other_conn = fixture.conn().await;
    let mut other = crate::db::begin_write(&mut other_conn).await.unwrap();
    sqlx::query("UPDATE imports SET status = 'cancelled', stage = NULL WHERE id = $1")
        .bind(id)
        .execute(&mut *other)
        .await
        .unwrap();
    let (status, text) = crate::db::write_tx::commit_during(
        other,
        crate::test_support::post_raw(
            &state,
            &path,
            &token,
            "application/jsonl",
            replace_run_batch("+15555550107", &["g1"]),
        ),
    )
    .await;

    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::StateConflict,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some(format!("import {id} is not running (status=cancelled)").as_str())
    );
    let mut conn = fixture.conn().await;
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE import_id = $1")
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

/// One conversation with `chat`, holding one message per guid, as a
/// replace run's batch.
fn replace_run_batch(chat: &str, guids: &[&str]) -> String {
    let mut lines = vec![
        conversation_header("whatsapp", chat)
            .owner("+15555550106", Some("Me"))
            .participant(chat, None)
            .to_string(),
    ];
    for guid in guids {
        lines.push(whatsapp_line(guid, guid).sender(chat).to_string());
    }
    lines.join("\n") + "\n"
}

/// A replace run wipes the source on its first batch only, and the
/// Upload posts a batch again when the first attempt times out. So the
/// second batch must not wipe the first, and a retried batch must add
/// nothing. This guards both against a change to how a batch picks wipe
/// or append (today: whether the run has stamped a message yet).
///
/// The retry adds nothing because append skips a guid the source already
/// holds, and the import refuses a message without one.
#[tokio::test]
async fn a_retried_batch_in_a_replace_run_keeps_every_message_once() {
    let (state, _fixture, token) = importer().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &state,
        "/v1/imports",
        &token,
        serde_json::json!({ "source": "whatsapp", "mode": "replace" }),
    )
    .await;
    let path = format!("/v1/imports/{}/batches", created["id"].as_i64().unwrap());

    let first = replace_run_batch("+15555550107", &["g-1a", "g-1b"]);
    let second = replace_run_batch("+15555550108", &["g-2a", "g-2b"]);
    for body in [&first, &second, &second] {
        let (status, text) =
            crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body.clone())
                .await;
        assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    }

    let mut conn = state.db.acquire().await.unwrap();
    let guids: Vec<String> =
        sqlx::query_scalar("SELECT guid FROM messages WHERE source = 'whatsapp' ORDER BY guid")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(guids, ["g-1a", "g-1b", "g-2a", "g-2b"]);
}

/// A message without a guid is outside the guid index, so a retried batch
/// would store it a second time (#1162). The batch is refused with `422`,
/// naming the line, and nothing in it is stored.
#[tokio::test]
async fn a_batch_with_a_message_without_a_guid_is_refused_and_stores_nothing() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let body = replace_run_batch("+15555550107", &["g-1", "", "g-2"]);

    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;

    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(
        problem.errors.as_deref(),
        Some(
            &[
                "The message on line 3 of the batch has no guid; every message needs one."
                    .to_string()
            ][..]
        )
    );
    assert_eq!(
        problem.line,
        Some(3),
        "Upload maps the line back to its file"
    );
    let mut conn = state.db.acquire().await.unwrap();
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

/// The steps in #1162: a proxy in front of the server times out while the
/// server commits a batch, answers `504 Gateway Timeout`, and Upload posts
/// the batch again. The second post finds every message already stored by
/// its guid, so each message is stored once.
#[tokio::test]
async fn a_batch_posted_again_after_a_gateway_timeout_stores_each_message_once() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;

    // Stands in for the proxy: the first batch reaches the server and is
    // committed, and the client is told 504 instead of the server's answer.
    let timed_out = Arc::new(AtomicBool::new(false));
    let app = crate::server::http_app(state.clone()).layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let timed_out = Arc::clone(&timed_out);
            async move {
                let is_batch = request.uri().path().ends_with("/batches");
                let response = next.run(request).await;
                if is_batch
                    && response.status().is_success()
                    && !timed_out.swap(true, Ordering::SeqCst)
                {
                    return axum::response::IntoResponse::into_response(
                        axum::http::StatusCode::GATEWAY_TIMEOUT,
                    );
                }
                response
            }
        },
    ));
    let server = crate::test_support::serve_router(app).await;
    let body = replace_run_batch("+15555550107", &["g-a", "g-b", "g-c"]);
    let post = || {
        http_client()
            .post(format!("{}{path}", server.base()))
            .bearer_auth(&token)
            .header(reqwest::header::CONTENT_TYPE, "application/jsonl")
            .body(body.clone())
            .send()
    };

    let first = post().await.unwrap();
    assert_eq!(first.status(), reqwest::StatusCode::GATEWAY_TIMEOUT);
    let second = post().await.unwrap();
    assert_eq!(second.status(), reqwest::StatusCode::OK);
    let answer: serde_json::Value = second.json().await.unwrap();
    assert_eq!(answer["messages_appended"], 0, "{answer}");
    assert_eq!(answer["messages_deduped"], 3, "{answer}");

    let mut conn = state.db.acquire().await.unwrap();
    let guids: Vec<String> =
        sqlx::query_scalar("SELECT guid FROM messages WHERE source = 'whatsapp' ORDER BY guid")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(guids, ["g-a", "g-b", "g-c"]);
}

/// The import body is JSON Lines and nothing else. `multipart/form-data`
/// used to be accepted (a `jsonl` field plus `file` parts) but nothing
/// sent it: message-crate-import posts JSON Lines and uploads attachments through
/// `/v1/assets`. The wrong media type is a 415, not a 400: the request is
/// well formed, it is simply not something this route reads.
#[tokio::test]
async fn a_multipart_body_is_an_unsupported_media_type() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;

    let boundary = "MessageCrateTestBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"jsonl\"\r\n\r\n{{}}\r\n--{boundary}--\r\n"
    );
    let path = batches_path(&fixture.state, &user.token, "imessage").await;
    let (status, text) = crate::test_support::post_raw(
        &fixture.state,
        &path,
        &user.token,
        &format!("multipart/form-data; boundary={boundary}"),
        body,
    )
    .await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::UnsupportedMediaType,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some("Content-Type must be application/x-ndjson or application/jsonl")
    );
}

/// A run belongs to the account whose token created it. Another account's
/// token posting into it finds no such run: the id is scoped to the
/// account, so an outsider cannot tell it exists.
#[tokio::test]
async fn a_batch_into_another_accounts_run_is_not_found() {
    let (fixture, alice) = crate::test_support::fixture_with_account().await;
    let bob = crate::test_support::register_via_api(&fixture.state, "bob", "hunter2hunter2").await;
    let bobs_run = batches_path(&fixture.state, &bob.token, "imessage").await;

    let (status, text) = crate::test_support::post_raw(
        &fixture.state,
        &bobs_run,
        &alice.token,
        "application/jsonl",
        "{}\n",
    )
    .await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);

    // Positive control: her own run takes the batch as far as reading it.
    let own = batches_path(&fixture.state, &alice.token, "imessage").await;
    let (status, _) = crate::test_support::post_raw(
        &fixture.state,
        &own,
        &alice.token,
        "application/jsonl",
        "{}\n",
    )
    .await;
    assert_ne!(status, axum::http::StatusCode::NOT_FOUND);
}

/// #1163: the batch answer counts the contacts the batch made. Staging makes
/// one for a person the account has no contact for, and that count reaches
/// the answer beside the participant row promote added.
#[tokio::test]
async fn the_batch_answer_counts_the_contacts_it_created() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "imessage").await;
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        format!(
            "{}\n{}\n",
            s1_header(),
            s1_message("g1", "+15555550119", 1426183462000, "hi")
        ),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let mut conn = state.db.acquire().await.unwrap();
    let bobs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE preferred_name = 'Bob'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(bobs, 1, "the batch made Bob's contact");
    let answer: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer["contacts_created"], 1, "{text}");
    assert_eq!(answer["participants"], 1, "{text}");
}
