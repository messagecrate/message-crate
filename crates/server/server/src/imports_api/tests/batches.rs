//! The batch route: which batches it refuses and with what answer, and
//! a batch posted again storing each message once.

use super::*;

/// Each server counts its own import slots. With every slot of one server
/// taken, a second server in the same process still imports.
#[tokio::test]
async fn import_slots_belong_to_one_server() {
    let (busy, _busy_fixture, _busy_token) = importer().await;
    let (idle, _idle_fixture, idle_token) = importer().await;
    let _every_slot = busy
        .import_slots
        .try_acquire_many(MAX_CONCURRENT_IMPORTS as u32)
        .expect("a new server has every import slot free");

    let path = batches_path(&idle, &idle_token, "imessage").await;
    let (status, text) = crate::test_support::post_raw(
        &idle,
        &path,
        &idle_token,
        "application/jsonl",
        one_message_batch(),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
}

/// The Upload pauses for a server that is stopping or busy, rather than
/// failing the batch, by reading these types' slugs from
/// `message_crate_api_types::problem_slug`. A type renamed here alone would
/// turn every stop into "could not be reached" with no other test failing.
#[test]
fn the_upload_reads_the_stopping_and_busy_types_by_their_slugs() {
    use message_crate_api_types::problem_slug;
    assert_eq!(
        crate::problem::ProblemType::ServerStopping.slug(),
        problem_slug::SERVER_STOPPING
    );
    assert_eq!(
        crate::problem::ProblemType::DatabaseBusy.slug(),
        problem_slug::DATABASE_BUSY
    );
}

/// A batch still waiting for an import slot when the server is told to stop
/// is refused with `server-stopping`, which the Upload retries, and
/// stores nothing. Before #2203 nothing closed the slots, so the batch took
/// one when an earlier batch ended and imported in full while the server
/// drained, making the stop wait for it.
#[tokio::test]
async fn a_batch_waiting_for_a_slot_when_the_server_stops_is_refused_and_stores_nothing() {
    let (state, _fixture, token) = importer().await;
    let every_slot = state
        .import_slots
        .try_acquire_many(MAX_CONCURRENT_IMPORTS as u32)
        .expect("a new server has every import slot free");
    let path = batches_path(&state, &token, "imessage").await;

    // Every slot is taken, so the batch cannot start until one is freed.
    // Whether it is already waiting on the slots or not yet there when they
    // close, it meets them closed: the outcome does not depend on timing.
    let waiting = spawn_batch(&state, path, &token, one_message_batch());
    state.on_stop_signal()();
    // The earlier batches end, which is what let the waiting one in.
    drop(every_slot);

    let (status, text) = waiting.await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ServerStopping,
    );
    assert!(
        problem
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("send it again"),
        "{text}"
    );
    assert_eq!(
        stored_messages(&state).await,
        0,
        "the refused batch stored a message"
    );
}

/// A batch waiting for an import slot has not read its body: the slot
/// comes first, so batches queued behind the running imports hold no disk.
/// With the slots closed, even an empty body, which reading would refuse,
/// is refused as `server-stopping`, and nothing is written under the Data
/// Directory. Before #2095 the body was written out first, so every waiting
/// batch held its body on disk while two imported.
#[tokio::test]
async fn a_batch_waiting_for_a_slot_has_not_read_its_body() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "imessage").await;
    state.on_stop_signal()();

    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", "").await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::ServerStopping);
    assert!(
        !crate::imports_api::batch_bodies::root(&state.cfg.paths.data_dir).exists(),
        "the waiting batch wrote its body"
    );
}

/// Every one of `state`'s import slots, each held on its own as one running
/// import holds its slot, so a test can free them one at a time.
fn hold_every_import_slot(state: &crate::server::AppState) -> Vec<OwnedSemaphorePermit> {
    (0..MAX_CONCURRENT_IMPORTS)
        .map(|_| {
            Arc::clone(&state.import_slots)
                .try_acquire_owned()
                .expect("a new server has every import slot free")
        })
        .collect()
}

/// Post `body` to the batches `path` as `token` in a task of its own, so the
/// test can act while the batch waits. Return the task's handle, which
/// yields the status and the body of the response.
fn spawn_batch(
    state: &crate::server::AppState,
    path: String,
    token: &str,
    body: impl Into<reqwest::Body> + Send + 'static,
) -> tokio::task::JoinHandle<(axum::http::StatusCode, String)> {
    let state = state.clone();
    let token = token.to_string();
    crate::request_id::spawn_in_current_span(async move {
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await
    })
}

/// The number of messages stored in `state`'s database.
async fn stored_messages(state: &crate::server::AppState) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&state.db)
        .await
        .unwrap()
}

/// Free one of the `held` import slots once `batch` waits for one, so the
/// batch takes it, and check before each try that the batch has neither
/// ended nor stored a message while every slot was held.
///
/// The slots hand a freed slot straight to a batch waiting for one, so a
/// slot freed while a batch waits is taken at once and none is free. With
/// no batch waiting yet the freed slot stays free, and the test takes it
/// back and yields, which lets the batch run on to the slots. Nothing here
/// reads a clock: the loop ends when the batch takes the slot, or fails
/// when it ends or stores without one.
async fn hand_a_slot_to_the_waiting_batch<T>(
    state: &crate::server::AppState,
    held: &mut Vec<OwnedSemaphorePermit>,
    batch: &tokio::task::JoinHandle<T>,
) {
    loop {
        tokio::task::yield_now().await;
        assert!(
            !batch.is_finished(),
            "the batch ended while every import slot was held"
        );
        assert_eq!(
            stored_messages(state).await,
            0,
            "a batch imported while every import slot was held"
        );
        drop(held.pop().expect("a held import slot"));
        if state.import_slots.available_permits() == 0 {
            return;
        }
        held.push(
            Arc::clone(&state.import_slots)
                .try_acquire_owned()
                .expect("the slot just freed is free"),
        );
    }
}

/// While every import slot is held by a running import, another batch
/// waits for one of them to end: it imports nothing until a slot is freed,
/// takes that slot, and then imports. Each import holds a pooled
/// connection for its whole run, so with the two slots a third import
/// running at once would leave login and search waiting for a connection
/// (#2128). The test holds every slot in place of the running imports.
#[tokio::test]
async fn a_batch_waits_while_every_import_slot_is_held() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "imessage").await;
    let mut held = hold_every_import_slot(&state);

    let batch = spawn_batch(&state, path, &token, one_message_batch());
    hand_a_slot_to_the_waiting_batch(&state, &mut held, &batch).await;

    let (status, text) = batch.await.unwrap();
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    assert_eq!(stored_messages(&state).await, 1);
    drop(held);
    assert_eq!(
        state.import_slots.available_permits(),
        MAX_CONCURRENT_IMPORTS,
        "the batch gave its import slot back"
    );
}

/// A batch refused after it took its import slot gives the slot back: an
/// empty body is refused once it is read, which is after the slot is
/// taken. A slot kept by a refused batch would leave one import fewer to
/// run, and none once two had been refused.
#[tokio::test]
async fn a_batch_refused_after_taking_its_slot_gives_the_slot_back() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "imessage").await;
    let mut held = hold_every_import_slot(&state);

    let batch = spawn_batch(&state, path, &token, "");
    hand_a_slot_to_the_waiting_batch(&state, &mut held, &batch).await;

    let (status, text) = batch.await.unwrap();
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    drop(held);
    assert_eq!(
        state.import_slots.available_permits(),
        MAX_CONCURRENT_IMPORTS,
        "the refused batch gave its import slot back"
    );
}

/// A batch over the Upload's own cap is refused with `413 Payload Too
/// Large`, so the server never reads more than its client sends. It used to
/// accept up to the 512 MiB every other body may reach (#2075).
#[tokio::test]
async fn a_batch_one_byte_over_the_cap_is_payload_too_large() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "imessage").await;
    let body = vec![b'\n'; message_crate_api_types::MAX_IMPORT_BATCH_BYTES + 1];

    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::PayloadTooLarge,
    );
}

/// A body sent with no `Content-Length` is cut off at the same cap as it is
/// read, so a script streaming one large batch is refused as well.
#[tokio::test]
async fn a_streamed_batch_over_the_cap_is_payload_too_large() {
    use futures_util::StreamExt;
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "imessage").await;
    let mib = vec![b'\n'; 1024 * 1024];
    let chunks = message_crate_api_types::MAX_IMPORT_BATCH_BYTES / mib.len();
    let body = futures_util::stream::iter(0..chunks)
        .map(move |_| Ok::<_, std::io::Error>(axum::body::Bytes::from(mib.clone())))
        .chain(futures_util::stream::iter([Ok(
            axum::body::Bytes::from_static(b"\n"),
        )]));

    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        reqwest::Body::wrap_stream(body),
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::PayloadTooLarge,
    );
}

/// A batch whose body is still arriving when the server is told to stop
/// holds a slot already, but stops reading at once and is refused with
/// `server-stopping`, storing nothing, so the stop waits neither for the rest
/// of the body nor for a whole import more (#2203). The body here never ends:
/// before #2878 the read went on until the client stopped sending or the
/// upload time limits ran out, and the stop waited for it.
#[tokio::test]
async fn a_batch_still_arriving_when_the_server_stops_is_refused_and_stores_nothing() {
    use futures_util::StreamExt;
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "imessage").await;
    let body = futures_util::stream::iter([Ok::<_, std::io::Error>(axum::body::Bytes::from(
        one_message_batch(),
    ))])
    .chain(futures_util::stream::pending());
    let posting = spawn_batch(&state, path, &token, reqwest::Body::wrap_stream(body));

    // The batch's directory exists once it holds a slot and reads its body.
    let root = crate::imports_api::batch_bodies::root(&state.cfg.paths.data_dir);
    while std::fs::read_dir(&root).map_or(true, |mut entries| entries.next().is_none()) {
        tokio::task::yield_now().await;
    }
    state.on_stop_signal()();

    let (status, text) = posting.await.unwrap();
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::ServerStopping);
    assert_eq!(
        stored_messages(&state).await,
        0,
        "the refused batch stored a message"
    );
}

/// One iMessage conversation with one message.
fn one_message_batch() -> String {
    format!(
        "{}\n{}\n",
        conversation_header("imessage", "+15555550123").participant("+15555550123", None),
        message_line("g-slots", "hi").sender("+15555550123"),
    )
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
            "this file is schema version 4; Message Crate reads version {} (line 1 of the batch)",
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
        message.starts_with("could not read line 3 of the batch:"),
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
        message.starts_with("could not read line 3 of the batch:"),
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
        problem.errors.unwrap()[0].starts_with("line 3 of the batch:"),
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
            .starts_with("line 1 of the batch: the conversation header is not valid"),
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

/// A stated SHA-256 that is not 64 hex digits is the sender's to fix: 422
/// naming the line and the attachment, though a batch carries no file
/// (#2259).
#[tokio::test]
async fn a_batch_whose_stated_sha256_is_not_one_is_a_422_naming_the_file() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let body = one_attachment_batch("photo.bin", Some("not-a-fingerprint"));
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(problem.line, Some(2), "{text}");
    let error = &problem.errors.unwrap()[0];
    assert!(
        error.contains("photo.bin") && error.contains("not-a-fingerprint"),
        "{text}"
    );
}

/// #2259: an HTTP batch never carries files, so an attachment's `path` is
/// only a label. A path that names a file in the account's own asset store
/// (a live upload temp, a media type sidecar, a stored original) stores
/// nothing: only a stated fingerprint finds an Asset.
#[tokio::test]
async fn a_batch_path_naming_a_file_in_the_asset_store_stores_no_asset() {
    let (fixture, account) = fixture_with_account().await;
    let state = fixture.state.clone();
    let assets_dir = state.cfg.paths.assets_dir_for_account(account.account_id);
    let sha = file_io::sha256_hex(b"an upload still arriving");
    let shard = &sha[..2];
    let files = [
        format!(".incoming/{sha}-1.part"),
        format!("{shard}/.{sha}.mime"),
        format!("{shard}/{sha}"),
    ];
    for rel in &files {
        let file = assets_dir.join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, format!("bytes of {rel}")).unwrap();
    }

    let path = batches_path(&state, &account.token, "whatsapp").await;
    for rel in &files {
        let body = one_attachment_batch(rel, None);
        let (status, text) =
            crate::test_support::post_raw(&state, &path, &account.token, "application/jsonl", body)
                .await;
        assert_eq!(status, axum::http::StatusCode::OK, "{rel}: {text}");
        let answer: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(answer["assets_copied"], 0, "{rel}: {text}");
        assert_eq!(answer["assets_missing"], 1, "{rel}: {text}");
    }

    let stored: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE sha256 IS NOT NULL")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(stored, 0, "no attachment names an Asset");
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
                "the message on line 3 of the batch has no guid; every message needs one"
                    .to_string()
            ][..]
        )
    );
    assert_eq!(
        problem.line,
        Some(3),
        "Upload maps the line back to its file"
    );
    assert_eq!(stored_messages(&state).await, 0);
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
        server
            .post(&path)
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

/// Whether `accounts` holds a row at `account_id`.
async fn account_row_exists(fixture: &TestFixture, account_id: i64) -> bool {
    let mut conn = fixture.conn().await;
    crate::db::account_profile::account_exists(&mut conn, account_id)
        .await
        .unwrap()
}

/// A batch whose account is deleted after its credential and run were
/// checked, while it waits for its slot and the account's lock, is refused
/// as a credential whose account is gone, and puts no row back. Before #2061
/// the import made the row again, under the deleted id with no password, so
/// anyone could log in to it with an empty one.
#[tokio::test]
async fn a_batch_for_an_account_deleted_while_it_waited_brings_no_account_back() {
    let (state, fixture, token) = importer().await;
    let path = batches_path(&state, &token, "imessage").await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let mut conn = fixture.conn().await;
    let account = crate::server::resolve_auth_on_conn(
        &mut conn,
        &token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap()
    .account_id;
    // What the batch learned when it arrived, before it waited.
    let row = crate::db::imports::require_running_import(&mut conn, account, import_id)
        .await
        .unwrap();
    let context = BatchContext::from_row(&row);
    crate::db::account_profile::delete_account(
        &mut conn,
        account,
        crate::db::audit_trail::AuditActor::Owner,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    drop(conn);

    let dir = TempDir::new().unwrap();
    let jsonl = write_jsonl(dir.path(), "batch.jsonl", &one_message_batch());
    let permit = take_import_slot(&state).await.unwrap();
    let err = run_import_path(state, permit, context, jsonl)
        .await
        .expect_err("the deleted account's batch must be refused");

    assert!(matches!(err, ApiError::TokenRefused(_)), "{err:?}");
    assert!(
        !account_row_exists(&fixture, account).await,
        "the refused batch put the deleted account's row back"
    );
}

/// Starting an Import Run for an account deleted after its credential was
/// checked is refused the same way, and puts no row back (#2061).
#[tokio::test]
async fn an_import_run_for_a_deleted_account_brings_no_account_back() {
    let (state, fixture, token) = importer().await;
    let mut conn = fixture.conn().await;
    let auth = crate::server::resolve_auth_on_conn(
        &mut conn,
        &token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    let account = auth.account_id;
    crate::db::account_profile::delete_account(
        &mut conn,
        account,
        crate::db::audit_trail::AuditActor::Owner,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    drop(conn);

    let body: CreateImportRequest =
        serde_json::from_value(serde_json::json!({ "source": "imessage" })).unwrap();
    let err = create_import(
        axum::extract::State(state),
        ImportAccess(auth),
        crate::extract::Json(body),
    )
    .await
    .expect_err("the deleted account's run must be refused");

    assert!(matches!(err, ApiError::TokenRefused(_)), "{err:?}");
    assert!(
        !account_row_exists(&fixture, account).await,
        "the refused run put the deleted account's row back"
    );
}
