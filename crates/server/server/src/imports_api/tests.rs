use super::*;
use crate::assets_api;
use crate::test_support::{
    ConversationHeaderLine, MessageLine, RegisteredAccount, TestFixture,
    assert_every_person_is_on_a_contact, attachment, conversation_header, fixture_with_account,
    get_json, import_ada_conversation, import_jsonl_text, message_line, patch_json,
    post_created_json, post_json, test_fixture,
};
use message_ir::{
    Deletion, EarlierVersion, IrAttachment, IrImessage, IrMessageKind, IrService, Reaction,
};
use tempfile::TempDir;

const TEST_ACCOUNT: i64 = 7;

/// An incoming WhatsApp message line sent at 1700000000000, of the `sms`
/// kind, as the WhatsApp batches in these tests write it.
fn whatsapp_line(guid: &str, text: &str) -> MessageLine {
    message_line(guid, text)
        .at(1_700_000_000_000)
        .sms()
        .service(IrService::Whatsapp)
}

fn write_jsonl(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

/// A fixture holding one running Import Run at `staging_review` whose
/// `summary_json` already carries `summary` — as if an earlier
/// `PATCH /v1/imports/{id}` recorded the Staging Review approval.
async fn run_with_summary(summary: serde_json::Value) -> (TestFixture, RegisteredAccount, i64) {
    let (fixture, account) = fixture_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let import_id = created["id"].as_i64().expect("created run has an id");
    let mut conn = fixture.state.db.acquire().await.unwrap();
    crate::db::imports::set_import_stage(
        &mut conn,
        account.account_id,
        import_id,
        crate::db::imports::ImportStage::StagingReview,
        Some(&summary.to_string()),
    )
    .await
    .unwrap();
    (fixture, account, import_id)
}

/// The run's stored `summary_json`, decoded, or `None` when the
/// column is null.
async fn stored_summary(fixture: &TestFixture, import_id: i64) -> Option<serde_json::Value> {
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let raw: Option<String> = sqlx::query_scalar("SELECT summary_json FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    raw.map(|s| serde_json::from_str(&s).expect("stored summary_json is valid JSON"))
}

#[tokio::test]
async fn a_stage_change_with_a_summary_stores_it() {
    // The Review screen posts what the user approved so it survives a
    // reload — recomputing the summary from the directory is a different
    // question from what was actually approved.
    let (fixture, account) = fixture_with_account().await;
    let (location, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let import_id = created["id"].as_i64().unwrap();
    assert_eq!(location, format!("/v1/imports/{import_id}"));

    patch_json::<serde_json::Value>(
        &fixture.state,
        &format!("/v1/imports/{import_id}"),
        &account.token,
        serde_json::json!({"stage": "staging_review", "summary": {"approved": true}}),
    )
    .await;

    assert_eq!(
        stored_summary(&fixture, import_id).await,
        Some(serde_json::json!({"approved": true}))
    );
}

#[tokio::test]
async fn running_import_run_reports_the_summary_a_stage_change_stored() {
    // The completion call is allowed to overwrite summary_json with the
    // outcome once the run finishes — that is the intended history
    // record. But mid-run, between an approval and completion, a
    // reload has nowhere else to read the approved plan back from:
    // the running run on GET /v1/imports?status=running must expose it too.
    let (fixture, account, import_id) =
        run_with_summary(serde_json::json!({"approved": true})).await;

    let page: serde_json::Value =
        get_json(&fixture.state, "/v1/imports?status=running", &account.token).await;
    let active = &page["items"][0];

    assert_eq!(active["id"], serde_json::json!(import_id));
    assert_eq!(active["summary"], serde_json::json!({"approved": true}));
}

#[tokio::test]
async fn a_stage_change_without_a_summary_does_not_erase_the_stored_one() {
    // Most stage changes carry nothing. Treating absent as null would
    // throw away the plan the outcome is judged against.
    let (fixture, account, import_id) =
        run_with_summary(serde_json::json!({"approved": true})).await;

    let run: serde_json::Value = patch_json(
        &fixture.state,
        &format!("/v1/imports/{import_id}"),
        &account.token,
        serde_json::json!({"stage": "upload"}),
    )
    .await;
    assert_eq!(
        run["id"],
        serde_json::json!(import_id),
        "a PATCH answers the run, the same record GET /v1/imports/{{id}} returns"
    );

    assert_eq!(
        stored_summary(&fixture, import_id).await,
        Some(serde_json::json!({"approved": true}))
    );
}

#[tokio::test]
async fn a_stage_answers_by_the_words_of_context_md_only() {
    // The stages and Reviews of an Import Run are named as CONTEXT.md names
    // them. The old spellings are refused, not read as aliases.
    let (fixture, account) = fixture_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let path = format!("/v1/imports/{}", created["id"]);

    let (status, text) = crate::test_support::patch_raw(
        &fixture.state,
        &path,
        &account.token,
        serde_json::json!({"stage": "awaiting_gate_1"}),
    )
    .await;
    let sentence = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    )
    .sentence();
    assert!(
        sentence.contains(
            "expected one of `parse`, `write`, `staging_review`, `media`, `media_review`, `upload`"
        ),
        "the refusal lists the stages the server knows: {sentence}"
    );

    let run: serde_json::Value = patch_json(
        &fixture.state,
        &path,
        &account.token,
        serde_json::json!({"stage": "staging_review"}),
    )
    .await;
    assert_eq!(run["id"], created["id"]);
}

#[tokio::test]
async fn an_issue_names_the_stage_it_came_from() {
    let (fixture, account) = fixture_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let issue = |stage: &str| {
        serde_json::json!({
            "status": "completed_with_issues",
            "issues": [{ "kind": "skip", "stage": stage, "item": "a.jpg", "reason": "missing" }],
        })
    };

    // A finer part of the desktop app's work is not a Stage.
    let (status, text) = crate::test_support::post_raw(
        &fixture.state,
        &format!("/v1/imports/{id}/complete"),
        &account.token,
        "application/json",
        issue("parse").to_string(),
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );

    let _: serde_json::Value = post_json(
        &fixture.state,
        &format!("/v1/imports/{id}/complete"),
        &account.token,
        issue("staging"),
    )
    .await;
    let run: serde_json::Value =
        get_json(&fixture.state, &format!("/v1/imports/{id}"), &account.token).await;
    assert_eq!(run["issues"][0]["stage"], "staging");
}

/// Completing a run the account does not have answers `404 Not Found`, and
/// completing one that already finished answers `409 Conflict`: each is a
/// run lookup failure, kept apart from a database fault, which answers
/// `500 Internal Server Error` (#1682).
#[tokio::test]
async fn completing_a_missing_or_finished_run_answers_its_own_status() {
    let (fixture, account) = fixture_with_account().await;
    let complete = |id: i64| {
        let state = fixture.state.clone();
        let token = account.token.clone();
        async move {
            crate::test_support::post_raw(
                &state,
                &format!("/v1/imports/{id}/complete"),
                &token,
                "application/json",
                serde_json::json!({ "status": "completed" }).to_string(),
            )
            .await
        }
    };

    let (status, text) = complete(999_999).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);

    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let (status, text) = complete(id).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let (status, text) = complete(id).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::StateConflict);
}

/// Open a verify connection to an on-disk test database.
async fn open_verify(db: &Path) -> (sqlx::SqlitePool, sqlx::pool::PoolConnection<sqlx::Sqlite>) {
    let pool = engine::open_pool_for_path(db).await.unwrap();
    let conn = pool.acquire().await.unwrap();
    (pool, conn)
}

#[tokio::test]
async fn append_skips_existing_guids_and_keeps_id_map() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");

    let first = write_jsonl(
        tmp.path(),
        "a.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms-backup-restore", "+14075550107")
                .participant("+14075550107", None),
            message_line("g-keep", "one")
                .sms()
                .sender("+14075550107")
                .line()
                + &message_line("g-dup", "two")
                    .at(1_426_183_522_000)
                    .outgoing()
                    .sms()
                    .line()
        ),
    );
    let first_stats = import_jsonl_files(
        &db,
        &[first],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Replace,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: true,
            import_id: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(first_stats.messages, 2);

    let second = write_jsonl(
        tmp.path(),
        "b.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms-backup-restore", "+14075550107")
                .participant("+14075550107", None),
            message_line("g-dup", "two again")
                .at(1_426_183_522_000)
                .outgoing()
                .sms()
                .line()
                + &message_line("g-new", "three")
                    .at(1_426_183_582_000)
                    .sms()
                    .sender("+14075550107")
                    .line()
        ),
    );
    let second_stats = import_jsonl_files(
        &db,
        &[second],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(second_stats.messages_appended, 1);
    assert_eq!(second_stats.messages_deduped, 1);
    assert_eq!(
        second_stats.messages, 1,
        "an append reports the messages it added, not the two it read"
    );

    let (_pool, mut conn) = open_verify(&db).await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(n, 3);
    let dup_body: String = sqlx::query_scalar("SELECT body FROM messages WHERE guid = 'g-dup'")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(dup_body, "two");

    // Deferred full-text search during promote must still index new bodies
    // and restore triggers.
    let fts_three: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'three'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(fts_three, 1);
    let fts_one: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'one'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(fts_one, 1);
    let triggers: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE '%_fts_%'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(triggers, 9);
}

fn replace_opts<'a>(assets: &'a Path, root: &'a Path, source: &'a str) -> ImportOptions<'a> {
    ImportOptions::fixed(FixedImportArgs {
        assets_dir: assets,
        asset_root: root,
        mode: ImportMode::Replace,
        source,
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    })
}

fn missing_attachment(name: &str) -> IrAttachment {
    IrAttachment {
        size_bytes: Some(12),
        missing_reason: Some("not_found".into()),
        ..attachment(
            &format!("attachments/{name}"),
            name,
            "application/octet-stream",
        )
    }
}

/// A reaction of `kind` to the first part of a message, from `reactor`.
fn reaction_from(reactor: &str, kind: &str) -> Reaction {
    Reaction {
        part_index: 0,
        kind: kind.into(),
        emoji: None,
        is_from_me: false,
        reactor_identity: Some(reactor.into()),
        reactor_display_name: None,
    }
}

/// The reaction of `reactor`, who liked a message.
fn liked_by(reactor: &str) -> Reaction {
    reaction_from(reactor, "liked")
}

fn chunk_boundary_jsonl() -> String {
    let header = conversation_header("imessage", "+15555550123")
        .participant("+15555550123", None)
        .participant("+15555550167", None);
    let mut lines = vec![header.to_string()];
    for i in 0..56 {
        let guid = format!("g-{i:02}");
        let ts = 1_426_183_462_000i64 + i64::from(i) * 1000;
        let mut message = message_line(&guid, &format!("msg {i}"))
            .at(ts)
            .sender("+15555550123");
        if i == 0 {
            message = message.attachment(missing_attachment("first.bin"));
        } else if i == 55 {
            message = message.attachment(missing_attachment("last.bin"));
        }
        if i == 1 {
            message = message.reaction(liked_by("+15555550167"));
        }
        lines.push(message.to_string());
    }
    lines.join("\n")
}

#[tokio::test]
async fn staging_chunks_56_messages_and_keeps_children_on_right_rows() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(tmp.path(), "chunk-boundary.jsonl", &chunk_boundary_jsonl());
    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();
    assert_eq!(stats.messages, 56);
    assert_eq!(stats.attachments, 2);
    assert_eq!(stats.tapbacks, 1);
    assert_eq!(stats.messages_deduped, 0);

    let (_pool, mut conn) = open_verify(&db).await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(n, 56);
    let first_atts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachments WHERE message_id = (SELECT id FROM messages WHERE guid = 'g-00')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let last_atts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachments WHERE message_id = (SELECT id FROM messages WHERE guid = 'g-55')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let second_taps: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tapbacks WHERE message_id = (SELECT id FROM messages WHERE guid = 'g-01')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(first_atts, 1);
    assert_eq!(last_atts, 1);
    assert_eq!(second_taps, 1);
}

/// An Apple Messages conversation file in which a friend loves a message
/// and the owner reacts to it with an emoji, as the Apple Messages exporter
/// writes it: the reactions on the message reacted to, and each reaction's
/// own row beside it.
fn apple_messages_reactions() -> String {
    crate::test_support::at_current_schema_version(include_str!(
        "../../tests/fixtures/apple-messages-reactions.jsonl"
    ))
}

/// The import stores each of a message's `reactions` under the person who
/// reacted, not the author of the message: the friend's tapback under the
/// friend's identity and the owner's emoji reaction as the owner's, with no
/// sender. The reaction rows themselves are not messages.
#[tokio::test]
async fn an_apple_messages_tapback_and_emoji_reaction_are_stored_under_each_reactor() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(tmp.path(), "reactions.jsonl", &apple_messages_reactions());
    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();
    assert_eq!(stats.messages, 1, "the reaction rows are not messages");
    assert_eq!(stats.tapbacks, 2);

    let (_pool, mut conn) = open_verify(&db).await;
    let author: Option<String> = sqlx::query_scalar(
        "SELECT h.normalized FROM messages m LEFT JOIN handles h ON h.id = m.sender_handle_id",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(author.as_deref(), Some("friend@example.com"));
    let rows: Vec<(String, Option<String>, i64, Option<String>)> = sqlx::query_as(
        "SELECT t.kind, t.emoji, t.is_from_me, h.normalized
         FROM tapbacks t LEFT JOIN handles h ON h.id = t.sender_handle_id
         ORDER BY t.id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            (
                "loved".to_string(),
                None,
                0,
                Some("+15555550107".to_string()),
            ),
            ("emoji".to_string(), Some("🔥".to_string()), 1, None),
        ],
        "the friend's tapback is the friend's and the owner's emoji is the owner's, \
         never the author's"
    );
}

/// Bob hearts the owner's message and then removes the heart. The Apple
/// Messages reader writes both reactions as rows of their own and leaves the
/// removed heart out of the message's `reactions`. The import stores the
/// message alone, with no heart on it (#1213).
#[tokio::test]
async fn a_removed_reaction_leaves_no_message_and_no_reaction() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let header =
        conversation_header("imessage", "+15555550123").participant("+15555550123", Some("Bob"));
    let target = message_line("g-hi", "hi").outgoing();
    let reaction = |guid: &str, ts: i64, text: &str, action: &str| {
        message_line(guid, text)
            .at(ts)
            .kind(IrMessageKind::Tapback)
            .sender("+15555550123")
            .sender_display_name("Bob")
            .imessage(IrImessage {
                associated_guid: Some("g-hi".into()),
                associated_part: Some(0),
                tapback_kind: Some("loved".into()),
                tapback_action: Some(action.into()),
                ..IrImessage::default()
            })
    };
    let loved = reaction("g-love", 1_426_183_463_000, "Loved a message", "add");
    let removed = reaction("g-unlove", 1_426_183_464_000, "Removed Heart", "remove");
    let path = write_jsonl(
        tmp.path(),
        "removed-heart.jsonl",
        &format!("{header}\n{target}\n{loved}\n{removed}\n"),
    );
    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.tapbacks, 0);

    let (_pool, mut conn) = open_verify(&db).await;
    let guids: Vec<String> = sqlx::query_scalar("SELECT guid FROM messages ORDER BY guid")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    assert_eq!(guids, ["g-hi"]);
    let tapbacks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tapbacks")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(tapbacks, 0);
}

/// A second copy of a message in one file adds no message, and the staged
/// message keeps the first copy's text, but the second copy's attachment
/// and reaction are added to it, as a second import of the copy adds them
/// (#1837).
#[tokio::test]
async fn a_second_copy_in_one_file_keeps_the_first_text_and_adds_its_children() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let header = conversation_header("imessage", "+15555550123")
        .participant("+15555550123", None)
        .participant("+15555550167", None);
    let first = message_line("g-once", "first")
        .sender("+15555550123")
        .attachment(missing_attachment("first.bin"));
    let second = message_line("g-once", "second")
        .at(1_426_183_463_000)
        .sender("+15555550123")
        .attachment(missing_attachment("second.bin"))
        .reaction(liked_by("+15555550167"));
    let path = write_jsonl(
        tmp.path(),
        "dup-guid.jsonl",
        &format!("{header}\n{first}\n{second}\n"),
    );
    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.messages_deduped, 1);
    assert_eq!(stats.attachments, 2);
    assert_eq!(stats.tapbacks, 1);

    let (_pool, mut conn) = open_verify(&db).await;
    let (body, attachments, tapbacks): (String, i64, i64) = sqlx::query_as(
        r"
        SELECT m.body, COUNT(DISTINCT a.id), COUNT(DISTINCT t.id)
        FROM messages m
        LEFT JOIN attachments a ON a.message_id = m.id
        LEFT JOIN tapbacks t ON t.message_id = m.id
        WHERE m.guid = 'g-once'
        GROUP BY m.id
        ",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(body, "first");
    assert_eq!(attachments, 2);
    assert_eq!(tapbacks, 1);
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT original_name FROM attachments WHERE message_id = (SELECT id FROM messages WHERE guid = 'g-once') ORDER BY id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(names, ["first.bin", "second.bin"]);
}

#[tokio::test]
async fn staging_keeps_both_rows_when_guids_differ_only_by_whitespace() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let header = conversation_header("imessage", "+15555550123").participant("+15555550123", None);
    // The guids are the input under test, so these lines stay written out.
    let first = r#"{"guid":"g-space","timestamp_unix_ms":1426183462000,"time_precision":"milliseconds","direction":"incoming","service":"imessage","message_kind":"imessage","sender_identity":"+15555550123","sender_display_name":null,"subject":null,"text":"trimmed","attachments":[{"path":"attachments/trim.bin","original_name":"trim.bin","mime_type":"application/octet-stream","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":12,"missing_reason":"not_found"}],"imessage":null,"source":null}"#;
    let second = r#"{"guid":" g-space","timestamp_unix_ms":1426183463000,"time_precision":"milliseconds","direction":"incoming","service":"imessage","message_kind":"imessage","sender_identity":"+15555550123","sender_display_name":null,"subject":null,"text":"padded","attachments":[{"path":"attachments/pad.bin","original_name":"pad.bin","mime_type":"application/octet-stream","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":12,"missing_reason":"not_found"}],"imessage":null,"source":null}"#;
    let path = write_jsonl(
        tmp.path(),
        "guid-whitespace.jsonl",
        &format!("{header}\n{first}\n{second}\n"),
    );
    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();
    assert_eq!(stats.messages, 2);
    assert_eq!(stats.messages_deduped, 0);
    assert_eq!(stats.attachments, 2);

    let (_pool, mut conn) = open_verify(&db).await;
    let names: Vec<String> =
        sqlx::query_scalar("SELECT original_name FROM attachments ORDER BY original_name")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(names, vec!["pad.bin".to_string(), "trim.bin".to_string()]);
}

/// The stored mark of the message `g-mark`.
async fn mark(conn: &mut sqlx::SqliteConnection) -> Option<String> {
    sqlx::query_scalar("SELECT deletion FROM messages WHERE guid = 'g-mark'")
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// A message imported before it was deleted takes the mark when a later
/// append-mode import carries it, and a third import that carries no mark
/// leaves the mark in place: the mark adds to a stored message as its
/// reactions do, and a file without it never takes it away.
#[tokio::test]
async fn append_adds_a_later_deletion_mark_to_a_stored_message_and_keeps_it() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let header = conversation_header("imessage", "+15555550123").participant("+15555550123", None);
    let line = |deletion: Option<Deletion>| {
        let message = message_line("g-mark", "later deleted").sender("+15555550123");
        match deletion {
            Some(deletion) => message.deletion(deletion),
            None => message,
        }
    };
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });

    let before = write_jsonl(
        tmp.path(),
        "before.jsonl",
        &format!("{header}\n{}\n", line(None)),
    );
    import_jsonl_files(&db, std::slice::from_ref(&before), &options)
        .await
        .unwrap();
    let after = write_jsonl(
        tmp.path(),
        "after.jsonl",
        &format!("{header}\n{}\n", line(Some(Deletion::DeletedInSourceApp))),
    );
    import_jsonl_files(&db, &[after], &options).await.unwrap();
    {
        let (_pool, mut conn) = open_verify(&db).await;
        assert_eq!(
            mark(&mut conn).await.as_deref(),
            Some("deleted_in_source_app")
        );
    }

    import_jsonl_files(&db, &[before], &options).await.unwrap();
    let (_pool, mut conn) = open_verify(&db).await;
    assert_eq!(
        mark(&mut conn).await.as_deref(),
        Some("deleted_in_source_app"),
        "a file without the mark leaves it in place"
    );
}

/// The stored text and content key of the message `g-edit`.
async fn text_and_key(conn: &mut sqlx::SqliteConnection) -> (String, Option<String>) {
    sqlx::query_as("SELECT body, content_key FROM messages WHERE guid = 'g-edit'")
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// One earlier version of `g-edit`, as an `edits` entry.
fn edit_version(part: u32, text: &str, ms: i64) -> EarlierVersion {
    EarlierVersion {
        part_index: part,
        text: text.into(),
        edited_at_unix_ms: Some(ms),
    }
}

/// A conversation file holding the one message `g-edit`, whose text is
/// `text` and whose earlier versions are `versions` (from [`edit_version`]),
/// written to `name` under `dir`.
fn edit_file(dir: &Path, name: &str, text: &str, versions: &[EarlierVersion]) -> PathBuf {
    let header = conversation_header("imessage", "+15555550123").participant("+15555550123", None);
    let line = versions.iter().cloned().fold(
        message_line("g-edit", text).sender("+15555550123"),
        MessageLine::edit,
    );
    write_jsonl(dir, name, &format!("{header}\n{line}\n"))
}

/// Append-mode options for the `g-edit` tests.
fn edit_options<'a>(
    assets: &'a Path,
    root: &'a Path,
    fill_content_keys: bool,
) -> ImportOptions<'a> {
    ImportOptions::fixed(FixedImportArgs {
        assets_dir: assets,
        asset_root: root,
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys,
        import_id: None,
    })
}

/// A message imported before it was edited takes the later text when an
/// append-mode import carries the edit, and its content key is computed
/// again from that text, the key a first import of the later backup gives
/// it. A key left from the earlier text would let the dedupe join the
/// message to another copy of the text it no longer holds.
#[tokio::test]
async fn append_gives_a_later_edit_to_a_stored_message_with_a_new_content_key() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let options = edit_options(&assets, tmp.path(), true);
    let before = edit_file(tmp.path(), "before.jsonl", "see you at six", &[]);
    let after = edit_file(
        tmp.path(),
        "after.jsonl",
        "see you at seven",
        &[edit_version(0, "see you at six", 1426183462000)],
    );

    let fresh = tmp.path().join("fresh.db");
    import_jsonl_files(&fresh, std::slice::from_ref(&after), &options)
        .await
        .unwrap();
    let (later_text, later_key) = {
        let (_pool, mut conn) = open_verify(&fresh).await;
        text_and_key(&mut conn).await
    };
    assert_eq!(later_text, "see you at seven");
    assert!(later_key.is_some(), "the import fills the content key");

    let db = tmp.path().join("messagecrate.db");
    import_jsonl_files(&db, &[before], &options).await.unwrap();
    let earlier_key = {
        let (_pool, mut conn) = open_verify(&db).await;
        text_and_key(&mut conn).await.1
    };
    assert_ne!(earlier_key, later_key, "the key hashes the text");
    import_jsonl_files(&db, &[after], &options).await.unwrap();
    let (_pool, mut conn) = open_verify(&db).await;
    assert_eq!(
        text_and_key(&mut conn).await,
        (later_text, later_key),
        "the stored message reads as a first import of the later backup"
    );
}

/// An earlier backup appended after a later one leaves the later edit in
/// place even when it lists more earlier versions. Apple Messages drops the
/// earlier versions of a part unsent after its edits, so the later backup
/// here lists two, both of part 0, and the earlier one three, two of them of
/// part 1, which was unsent since. The later backup's newest version is the
/// newer, and that decides; a count would bring the unsent part back.
#[tokio::test]
async fn append_keeps_a_later_edit_against_an_earlier_backup_listing_more_versions() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let options = edit_options(&assets, tmp.path(), false);
    let later = edit_file(
        tmp.path(),
        "later.jsonl",
        "see you at eight",
        &[
            edit_version(0, "see you at six", 1426183462000),
            edit_version(0, "see you at seven", 1426183600000),
        ],
    );
    let earlier = edit_file(
        tmp.path(),
        "earlier.jsonl",
        "see you at seven bring snacks",
        &[
            edit_version(0, "see you at six", 1426183462000),
            edit_version(1, "bring cake", 1426183462000),
            edit_version(1, "bring pie", 1426183500000),
        ],
    );
    import_jsonl_files(&db, &[later], &options).await.unwrap();
    import_jsonl_files(&db, &[earlier], &options).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let (text, _) = text_and_key(&mut conn).await;
    assert_eq!(text, "see you at eight");
    let versions: Vec<String> = sqlx::query_scalar("SELECT text FROM message_versions ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    assert_eq!(versions, ["see you at six", "see you at seven"]);
}

/// What a reader sees of the message `g-edit`: its text and content key,
/// its earlier versions, and how many entries of each search index each
/// word finds.
#[derive(Debug, PartialEq)]
struct EditSnapshot {
    text: String,
    content_key: Option<String>,
    /// `(part_index, text, edited_at)`, in the order stored.
    versions: Vec<(i64, String, Option<String>)>,
    /// `(word, index, entries found)`.
    hits: Vec<(&'static str, &'static str, i64)>,
}

/// The [`EditSnapshot`] of `g-edit` in `conn`.
async fn edit_snapshot(conn: &mut sqlx::SqliteConnection) -> EditSnapshot {
    let (text, content_key) = text_and_key(conn).await;
    let versions =
        sqlx::query_as("SELECT part_index, text, edited_at FROM message_versions ORDER BY id")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    let mut hits = Vec::new();
    for word in ["six", "seven", "eight"] {
        for table in ["messages_fts", "message_versions_fts"] {
            let n: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM {table} WHERE {table} MATCH $1"
            ))
            .bind(word)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
            hits.push((word, table, n));
        }
    }
    EditSnapshot {
        text,
        content_key,
        versions,
        hits,
    }
}

/// One import carrying an earlier and a later backup of a message the
/// server does not hold yet gives it the later backup's text, exactly the
/// later backup's earlier versions, and the content key and search entries
/// an import of the later backup alone gives, in either file order. Only
/// the first copy staged used to count, so the earlier text could win.
#[tokio::test]
async fn one_import_of_two_backups_gives_a_new_message_the_later_edit_in_either_order() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let options = edit_options(&assets, tmp.path(), true);
    let earlier = edit_file(
        tmp.path(),
        "earlier.jsonl",
        "see you at seven",
        &[edit_version(0, "see you at six", 1426183462000)],
    );
    let later = edit_file(
        tmp.path(),
        "later.jsonl",
        "see you at eight",
        &[
            edit_version(0, "see you at six", 1426183462000),
            edit_version(0, "see you at seven", 1426183600000),
        ],
    );

    let alone = tmp.path().join("alone.db");
    import_jsonl_files(&alone, std::slice::from_ref(&later), &options)
        .await
        .unwrap();
    let expected = {
        let (_pool, mut conn) = open_verify(&alone).await;
        edit_snapshot(&mut conn).await
    };
    assert_eq!(expected.text, "see you at eight");
    assert_eq!(expected.versions.len(), 2);

    for (name, files) in [
        ("earlier-first.db", [earlier.clone(), later.clone()]),
        ("later-first.db", [later.clone(), earlier.clone()]),
    ] {
        let db = tmp.path().join(name);
        import_jsonl_files(&db, &files, &options).await.unwrap();
        let (_pool, mut conn) = open_verify(&db).await;
        assert_eq!(edit_snapshot(&mut conn).await, expected, "{name}");
    }
}

/// One attachment of a message in a conversation file, missing unless
/// `digest` names stored bytes.
fn attachment_with(name: &str, digest: Option<&str>) -> IrAttachment {
    match digest {
        Some(digest) => IrAttachment {
            digest_sha256: Some(digest.into()),
            missing_reason: None,
            ..missing_attachment(name)
        },
        None => missing_attachment(name),
    }
}

/// One reaction of kind `kind` from `+15555550167` to a message.
fn reaction_of(kind: &str) -> Reaction {
    reaction_from("+15555550167", kind)
}

/// A conversation file holding the message `g-kids` with `attachments`
/// and `reactions` (from [`attachment_with`] and [`reaction_of`]),
/// after the messages in `before`, written to `name` under `dir`.
fn file_with_children(
    dir: &Path,
    name: &str,
    before: &[String],
    attachments: &[IrAttachment],
    reactions: &[Reaction],
) -> PathBuf {
    let header = conversation_header("imessage", "+15555550123")
        .participant("+15555550123", None)
        .participant("+15555550167", None);
    let message = message_line("g-kids", "look")
        .at(1_426_183_463_000)
        .sender("+15555550123");
    let message = attachments
        .iter()
        .cloned()
        .fold(message, MessageLine::attachment);
    let message = reactions
        .iter()
        .cloned()
        .fold(message, MessageLine::reaction);
    let mut lines = vec![header.to_string()];
    lines.extend(before.iter().cloned());
    lines.push(message.to_string());
    write_jsonl(dir, name, &(lines.join("\n") + "\n"))
}

/// What a reader sees of the attachments and reactions in `conn`:
/// `(guid, original name, sha256, missing reason)` for each attachment and
/// `(guid, part, kind, reactor)` for each reaction, sorted.
type ChildrenSnapshot = (
    Vec<(String, String, Option<String>, Option<String>)>,
    Vec<(String, i64, String, Option<String>)>,
);

/// The [`ChildrenSnapshot`] of `conn`.
async fn children_snapshot(conn: &mut sqlx::SqliteConnection) -> ChildrenSnapshot {
    let attachments = sqlx::query_as(
        r"
        SELECT m.guid, a.original_name, a.sha256, a.missing_reason
        FROM attachments a JOIN messages m ON m.id = a.message_id
        ORDER BY 1, 2, 3, 4
        ",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let reactions = sqlx::query_as(
        r"
        SELECT m.guid, t.part_index, t.kind, h.raw
        FROM tapbacks t
        JOIN messages m ON m.id = t.message_id
        LEFT JOIN handles h ON h.id = t.sender_handle_id
        ORDER BY 1, 2, 3, 4
        ",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    (attachments, reactions)
}

/// One import of two backups stores the attachments and reactions two
/// separate imports of them store, in either order. The earlier backup
/// holds `g-kids` with `same.bin` and `late.bin` missing and a Love. The
/// later one holds it with `late.bin` found, a new `new.bin`, the Love and
/// a Like. Only the copy staged first used to count, so one import of the
/// earlier backup first stored no Like, no `new.bin` and no `late.bin`
/// file. A child both copies hold is stored once, as an append stores it.
#[tokio::test]
async fn one_import_of_two_backups_keeps_the_attachments_and_reactions_of_both() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let options = edit_options(&assets, tmp.path(), false);
    // `late.bin` is found in the later backup by the bytes of `blob.bin`,
    // which a message staged before it stores: both backups read their
    // files from one directory, so only a claimed digest tells them apart.
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/blob.bin"), b"late-bytes").unwrap();
    let digest = assets_api::sha256_hex(b"late-bytes");
    let blob = message_line("g-blob", "blob")
        .sender("+15555550123")
        .attachment(attachment_with("blob.bin", None))
        .to_string();
    let earlier = file_with_children(
        tmp.path(),
        "earlier.jsonl",
        &[],
        &[
            attachment_with("same.bin", None),
            attachment_with("late.bin", None),
        ],
        &[reaction_of("loved")],
    );
    let later = file_with_children(
        tmp.path(),
        "later.jsonl",
        &[blob],
        &[
            attachment_with("same.bin", None),
            attachment_with("late.bin", Some(&digest)),
            attachment_with("new.bin", None),
        ],
        &[reaction_of("loved"), reaction_of("liked")],
    );

    for (name, files) in [
        ("earlier-first", [earlier.clone(), later.clone()]),
        ("later-first", [later.clone(), earlier.clone()]),
    ] {
        let apart = tmp.path().join(format!("{name}-apart.db"));
        for file in &files {
            import_jsonl_files(&apart, std::slice::from_ref(file), &options)
                .await
                .unwrap();
        }
        let expected = {
            let (_pool, mut conn) = open_verify(&apart).await;
            children_snapshot(&mut conn).await
        };
        let kids: Vec<_> = expected.0.iter().filter(|a| a.0 == "g-kids").collect();
        assert_eq!(kids.len(), 3, "{name}: {kids:?}");
        assert!(
            kids.iter()
                .any(|a| a.1 == "late.bin" && a.2.as_deref() == Some(digest.as_str())),
            "{name}: {kids:?}"
        );
        let kinds: Vec<&str> = expected.1.iter().map(|t| t.2.as_str()).collect();
        assert_eq!(kinds, ["liked", "loved"], "{name}");

        let together = tmp.path().join(format!("{name}-together.db"));
        import_jsonl_files(&together, &files, &options)
            .await
            .unwrap();
        let (_pool, mut conn) = open_verify(&together).await;
        assert_eq!(children_snapshot(&mut conn).await, expected, "{name}");
    }
}

#[tokio::test]
async fn append_existing_guid_adds_missing_children() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let header = conversation_header("imessage", "+15555550123")
        .participant("+15555550123", None)
        .participant("+15555550167", None);
    let first = write_jsonl(
        tmp.path(),
        "children-first.jsonl",
        &format!(
            "{header}\n{}\n",
            message_line("g-children", "original body").sender("+15555550123")
        ),
    );
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    import_jsonl_files(&db, &[first], &options).await.unwrap();

    let second = write_jsonl(
        tmp.path(),
        "children-second.jsonl",
        &format!(
            "{header}\n{}\n",
            message_line("g-children", "replacement body")
                .sender("+15555550123")
                .attachment(IrAttachment {
                    missing_reason: Some("not_found".into()),
                    size_bytes: Some(12),
                    ..attachment(
                        "attachments/missing.bin",
                        "zqinvoice.pdf",
                        "application/octet-stream"
                    )
                })
                .reaction(liked_by("+15555550167"))
                .imessage(IrImessage::default())
        ),
    );

    for _ in 0..2 {
        import_jsonl_files(&db, std::slice::from_ref(&second), &options)
            .await
            .unwrap();
    }

    let (_pool, mut conn) = open_verify(&db).await;
    let (body, attachments, tapbacks): (String, i64, i64) = sqlx::query_as(
        r"
        SELECT m.body, COUNT(DISTINCT a.id), COUNT(DISTINCT t.id)
        FROM messages m
        LEFT JOIN attachments a ON a.message_id = m.id
        LEFT JOIN tapbacks t ON t.message_id = m.id
        WHERE m.guid = 'g-children'
        GROUP BY m.id
        ",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(body, "original body");
    assert_eq!(attachments, 1);
    assert_eq!(tapbacks, 1);

    // The attachment arrived after the message was first indexed, so search
    // finds the message by the attachment's name only when promote indexes
    // the existing message again (#1170).
    let hits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'zqinvoice'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        hits, 1,
        "the added attachment's name is not in the search index"
    );
}

#[tokio::test]
async fn append_with_a_found_file_fills_in_the_missing_attachment() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "found.jsonl",
        &format!(
            "{}\n{}\n",
            conversation_header("imessage", "+15555550123").participant("+15555550123", None),
            message_line("g-found", "see attached")
                .sender("+15555550123")
                .attachment(missing_attachment("found.bin"))
        ),
    );
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });

    // The first import has no file on disk, so the attachment is stored as missing.
    import_jsonl_files(&db, std::slice::from_ref(&path), &options)
        .await
        .unwrap();

    // The person uploads the file and imports the same conversation again.
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/found.bin"), b"found-bytes").unwrap();
    import_jsonl_files(&db, std::slice::from_ref(&path), &options)
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let rows: Vec<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        r"
        SELECT a.sha256, a.assets_path, a.missing_reason
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        WHERE m.guid = 'g-found'
        ",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "one attachment row, not a second one: {rows:?}"
    );
    let (sha256, assets_path, missing_reason) = &rows[0];
    assert_eq!(
        sha256.as_deref(),
        Some(assets_api::sha256_hex(b"found-bytes").as_str())
    );
    assert!(assets_path.is_some());
    assert_eq!(missing_reason, &None);
}

/// One file attached to two messages is stored once, so the storage figure
/// counts its size once.
#[tokio::test]
async fn a_file_two_messages_name_counts_once_in_storage() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let bytes = b"one clip forwarded twice";
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/clip.mov"), bytes).unwrap();
    let message = |guid: &str| {
        message_line(guid, "look")
            .sender("+15555550123")
            .attachment(attachment(
                "attachments/clip.mov",
                "clip.mov",
                "video/quicktime",
            ))
    };
    let path = write_jsonl(
        tmp.path(),
        "forwarded.jsonl",
        &format!(
            "{}\n{}\n{}\n",
            conversation_header("imessage", "+15555550123").participant("+15555550123", None),
            message("g-forward-1"),
            message("g-forward-2"),
        ),
    );
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    import_jsonl_files(&db, std::slice::from_ref(&path), &options)
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE sha256 = $1")
        .bind(assets_api::sha256_hex(bytes))
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(rows, 2, "both messages name the file");
    let size = i64::try_from(bytes.len()).unwrap();
    for scope in [
        crate::db::storage::Scope::Account(TEST_ACCOUNT),
        crate::db::storage::Scope::AllAccounts,
    ] {
        assert_eq!(
            crate::db::storage::attachment_bytes(&mut conn, scope)
                .await
                .unwrap(),
            size,
            "{scope:?}"
        );
    }
}

#[tokio::test]
async fn repeated_append_keeps_one_fts_posting_per_message() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "fts-append.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("imessage", "+15555550123").participant("+15555550123", None),
            message_line("g-fts", "zzuniqueterm body")
                .sender("+15555550123")
                .line()
        ),
    );
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });

    import_jsonl_files(&db, std::slice::from_ref(&path), &options)
        .await
        .unwrap();
    // Rows of the full-text search index storage: a redundant re-index writes a new
    // segment even when the indexed text is unchanged.
    async fn index_rows(db: &Path) -> i64 {
        let (_pool, mut conn) = open_verify(db).await;
        sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts_data")
            .fetch_one(&mut *conn)
            .await
            .unwrap()
    }
    let after_first_import = index_rows(&db).await;
    for _ in 0..2 {
        import_jsonl_files(&db, std::slice::from_ref(&path), &options)
            .await
            .unwrap();
    }
    assert_eq!(
        index_rows(&db).await,
        after_first_import,
        "repeated append must not write additional FTS index entries"
    );

    let (_pool, mut conn) = open_verify(&db).await;
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(messages, 1);

    // `MATCH` collapses repeated postings for one rowid, so read the index
    // itself: fts5vocab reports how many entries each term really has.
    sqlx::query("CREATE VIRTUAL TABLE fts_vocab USING fts5vocab(messages_fts, row);")
        .execute(&mut *conn)
        .await
        .unwrap();
    let term_entries = async |conn: &mut SqliteConnection| {
        let (docs, cnts): (i64, i64) = sqlx::query_as(
            "SELECT COALESCE(SUM(doc), 0), COALESCE(SUM(cnt), 0)
             FROM fts_vocab WHERE term = 'zzuniqueterm'",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        (docs, cnts)
    };
    assert_eq!(
        term_entries(&mut conn).await,
        (1, 1),
        "repeated append must not add extra index entries for an already indexed message"
    );
    let matches: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'zzuniqueterm'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(matches, 1);

    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    sqlx::query("DELETE FROM messages WHERE guid = 'g-fts'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        term_entries(&mut conn).await,
        (0, 0),
        "deleting the message must not leave stale search terms behind"
    );
}

#[tokio::test]
async fn deferred_fts_indexes_attachment_text_after_promote() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(&assets).unwrap();
    let att_dir = tmp.path().join("attachments");
    fs::create_dir_all(&att_dir).unwrap();
    fs::write(att_dir.join("receipt.pdf"), b"%PDF-fixture").unwrap();

    let path = write_jsonl(
        tmp.path(),
        "att.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("imessage", "+15555550123").participant("+15555550123", None),
            message_line("g-att", "see attached")
                .mms()
                .sender("+15555550123")
                .attachment(attachment(
                    "attachments/receipt.pdf",
                    "uniqueinvoice.pdf",
                    "application/pdf"
                ))
                .line()
        ),
    );
    import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let hits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'uniqueinvoice'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        hits, 1,
        "attachment original_name must be searchable after deferred FTS"
    );
}

#[tokio::test]
async fn promote_stamps_messages_with_import_id() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "import-id.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("imessage", "+15555550123").participant("+15555550123", None),
            message_line("g-import", "linked")
                .sms()
                .sender("+15555550123")
                .line()
        ),
    );

    let (_pool, mut conn) = open_verify(&db).await;
    schema::ensure_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let import_id = crate::db::imports::start_import(
        &mut conn,
        &crate::db::imports::StartImportArgs::new(TEST_ACCOUNT, "imessage", "append", Some("test")),
    )
    .await
    .unwrap();

    let stats = import_jsonl_files_on_conn(
        &mut conn,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: Some(import_id),
        }),
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);

    let stamped: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE import_id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(stamped, 1);

    let row = crate::db::imports::complete_import(
        &mut conn,
        TEST_ACCOUNT,
        import_id,
        &crate::db::imports::CompleteImportArgs {
            status: "completed".into(),
            message_count: Some(stats.messages as i64),
            attachment_count: Some(0),
            bytes_uploaded: Some(0),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(row.status.as_str(), "completed");
    assert_eq!(row.message_count, 1);

    let (listed, total) = crate::db::imports::list_imports_page(
        &mut conn,
        TEST_ACCOUNT,
        None,
        &crate::db::imports::DEFAULT_IMPORT_SORT,
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!((listed.len(), total), (1, 1));
    assert_eq!(listed[0].row.source, "imessage");
    assert!(!listed[0].row.started_at.is_empty());
    assert!(listed[0].row.finished_at.is_some());
    assert_eq!(
        crate::db::storage::attachment_bytes(
            &mut conn,
            crate::db::storage::Scope::Account(TEST_ACCOUNT),
        )
        .await
        .unwrap(),
        0
    );
    assert!(
        crate::db::imports::top_attachments_by_size(&mut conn, TEST_ACCOUNT, 5)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn trunk_zero_phone_imports_digits_with_review_note() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "trunk-zero.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("imessage", "020 7946 0000").participant("020 7946 0000", None),
            message_line("g-trunk-zero", "hello")
                .sms()
                .sender("020 7946 0000")
                .line()
        ),
    );

    let (_pool, mut conn) = open_verify(&db).await;
    schema::ensure_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();

    let stats = import_jsonl_files_on_conn(
        &mut conn,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap();
    assert_eq!(stats.phones_needing_review, 1);

    // Guarded policy: normalized mirrors the digits (never +02079460000)
    // and the handles row carries a review note.
    let (normalized, note): (String, Option<String>) = sqlx::query_as(
        "SELECT normalized, normalized_note FROM handles
         WHERE account_id = $1 AND handle_type = 'phone'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(normalized, "02079460000");
    assert!(
        note.as_deref().is_some(),
        "trunk-zero import must carry a review note"
    );
}

#[tokio::test]
async fn source_from_jsonl_stamps_export_source_and_assets() {
    use crate::config::PathsConfig;
    use media::MediaMode;

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let data_dir = tmp.path().join("data");
    let paths = PathsConfig {
        db: db.clone(),
        data_dir: data_dir.clone(),
        assets_dir: "assets".into(),
        assets_converted_dir: "assets_converted".into(),
    };
    let assets_dir = paths.assets_dir_for_account(TEST_ACCOUNT);
    fs::create_dir_all(tmp.path().join("media")).unwrap();
    fs::write(tmp.path().join("media/photo.jpg"), b"jpeg-bytes").unwrap();

    let path = write_jsonl(
        tmp.path(),
        "c.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("go-sms-pro", "+15555550100").participant("+15555550100", None),
            message_line("g1", "hi")
                .sms()
                .sender("+15555550100")
                .attachment(attachment("media/photo.jpg", "photo.jpg", "image/jpeg"))
                .line()
        ),
    );
    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions {
            assets_dir: &assets_dir,
            asset_root: tmp.path(),
            mode: ImportMode::Replace,
            source: "",
            account_id: TEST_ACCOUNT,
            fill_content_keys: true,
            import_id: None,
            source_from_jsonl: true,
            media: MediaMode::Clone,
            wipe_sources: Some(vec!["go-sms-pro".into()]),
        },
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.assets_copied, 1);

    let (_pool, mut conn) = open_verify(&db).await;
    let source: String = sqlx::query_scalar("SELECT source FROM messages WHERE guid = 'g1'")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(source, "go-sms-pro");
    let assets_root = paths.assets_dir_for_account(TEST_ACCOUNT);
    assert!(assets_root.is_dir());
}

#[tokio::test]
async fn media_none_skips_attachment_copy() {
    use crate::config::PathsConfig;
    use media::MediaMode;

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let data_dir = tmp.path().join("data");
    let paths = PathsConfig {
        db: db.clone(),
        data_dir,
        assets_dir: "assets".into(),
        assets_converted_dir: "assets_converted".into(),
    };
    let assets_dir = paths.assets_dir_for_account(TEST_ACCOUNT);
    fs::create_dir_all(tmp.path().join("media")).unwrap();
    fs::write(tmp.path().join("media/photo.jpg"), b"jpeg-bytes").unwrap();

    let path = write_jsonl(
        tmp.path(),
        "c.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms", "+15555550100").participant("+15555550100", None),
            message_line("g1", "hi")
                .sms()
                .sender("+15555550100")
                .attachment(attachment("media/photo.jpg", "photo.jpg", "image/jpeg"))
                .line()
        ),
    );
    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions {
            assets_dir: &assets_dir,
            asset_root: tmp.path(),
            mode: ImportMode::Replace,
            source: "",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
            source_from_jsonl: true,
            media: MediaMode::Disabled,
            wipe_sources: Some(vec!["sms".into()]),
        },
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.attachments, 0);
    assert_eq!(stats.assets_copied, 0);
}

/// `import --media convert` stores the converted file in place of the
/// original: a PNG goes in and the attachment the database holds is a JPEG.
/// Runs the real ffmpeg, so the guard is taken outside the async block, as
/// Clippy's `await_holding_lock` asks.
#[test]
fn media_convert_stores_the_converted_file_not_the_original() {
    use crate::config::PathsConfig;
    use media::MediaMode;
    use media::testutil::PNG_1X1_RGB;

    let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let test = async {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("messagecrate.db");
        let paths = PathsConfig {
            db: db.clone(),
            data_dir: tmp.path().join("data"),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        };
        let assets_dir = paths.assets_dir_for_account(TEST_ACCOUNT);
        fs::create_dir_all(tmp.path().join("attachments")).unwrap();
        fs::write(tmp.path().join("attachments/photo.png"), PNG_1X1_RGB).unwrap();
        let path = write_jsonl(
            tmp.path(),
            "convert.jsonl",
            &conversation_with_attachments(&["attachments/photo.png"])
                .replace("application/octet-stream", "image/png"),
        );

        import_jsonl_files(
            &db,
            &[path],
            &ImportOptions {
                assets_dir: &assets_dir,
                asset_root: tmp.path(),
                mode: ImportMode::Replace,
                source: "",
                account_id: TEST_ACCOUNT,
                fill_content_keys: false,
                import_id: None,
                source_from_jsonl: true,
                media: MediaMode::Convert,
                wipe_sources: Some(vec!["imessage".into()]),
            },
        )
        .await
        .unwrap();

        let (_pool, mut conn) = open_verify(&db).await;
        let (mime_type, assets_path): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT mime_type, assets_path FROM attachments")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(mime_type.as_deref(), Some("image/jpeg"));
        let stored = paths
            .assets_dir_for_account(TEST_ACCOUNT)
            .join(assets_path.expect("the attachment is stored"));
        let bytes = fs::read(&stored).unwrap();
        assert_eq!(&bytes[..2], [0xff, 0xd8], "a JPEG starts with SOI");
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(test);
}

#[tokio::test]
async fn name_only_participant_becomes_an_other_identity_on_a_contact() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    // A rescue export: the source names the other party and records no
    // address for them anywhere.
    let path = write_jsonl(
        tmp.path(),
        "name-only.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("openextract", "name:Sarah Vale")
                .participant_without_identity(Some("Sarah Vale")),
            message_line("g-name-only", "hi")
                .sms()
                .sender_display_name("Sarah Vale")
                .line()
        ),
    );
    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "openextract",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    import_jsonl_files(&db, &[path], &opts).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;

    // The name is the contact's.
    let name: String = sqlx::query_scalar(
        "SELECT preferred_name FROM contacts WHERE account_id = $1 AND preferred_name = 'Sarah Vale'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(name, "Sarah Vale");

    // Her identity is of type `other` and holds the name, not an address
    // invented for her, and it is on that contact.
    let identities: Vec<(String, String)> = sqlx::query_as(
        "SELECT h.handle_type, ct.preferred_name FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         JOIN contacts ct ON ct.id = ch.contact_id
         WHERE h.account_id = $1 AND h.raw = 'Sarah Vale'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        identities,
        [("other".to_string(), "Sarah Vale".to_string())]
    );

    // The promoted participant and the message's sender are that identity.
    let participant_is_her: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM participants p JOIN handles h ON h.id = p.handle_id
                        WHERE h.raw = 'Sarah Vale')
            AND EXISTS (SELECT 1 FROM messages m JOIN handles h ON h.id = m.sender_handle_id
                        WHERE h.raw = 'Sarah Vale')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(participant_is_her);

    // The chat keyed by her name gives her no second contact: she is one
    // contact, and the key's handle belongs to nobody.
    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(contacts, 1);
    let key_has_a_contact: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id
                        WHERE h.raw = 'name:Sarah Vale')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(!key_has_a_contact);
}

/// A person named "AMAZON" with no address and the sender `AMAZON` are two
/// conversations, because a name key never equals an address.
#[tokio::test]
async fn a_name_keyed_chat_is_not_the_chat_of_the_address_it_spells() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let named = write_jsonl(
        tmp.path(),
        "name-AMAZON.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms_backup_plus", "name:AMAZON")
                .participant_without_identity(Some("AMAZON")),
            message_line("g-named", "from a person")
                .sms()
                .sender_display_name("AMAZON")
                .line()
        ),
    );
    let sender = write_jsonl(
        tmp.path(),
        "AMAZON.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms_backup_plus", "AMAZON").typed_participant(
                "AMAZON",
                None,
                message_ir::HandleType::Other
            ),
            message_line("g-sender", "from a sender")
                .at(1_426_183_463_000)
                .sms()
                .sender("AMAZON")
                .line()
        ),
    );
    let opts = replace_opts(&assets, tmp.path(), "sms_backup_plus");
    import_jsonl_files(&db, &[named, sender], &opts)
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let conversations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM conversations WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(conversations, 2);
}

/// The conversation whose rows name nobody is no person, so its chat id
/// gives nobody a contact.
#[tokio::test]
async fn the_conversation_that_names_nobody_never_becomes_a_contact() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "nameless.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("openextract", "nameless:"),
            message_line("g-nameless", "to whom")
                .outgoing()
                .sms()
                .line()
        ),
    );
    let opts = replace_opts(&assets, tmp.path(), "openextract");
    import_jsonl_files(&db, &[path], &opts).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(contacts, 0);
}

/// A group chat's identifier names the conversation, not a person, so only
/// the people in the group become contacts.
#[tokio::test]
async fn a_group_chat_identifier_never_becomes_a_contact() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "group.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("imessage", "chat1000000005")
                .group()
                .title("Trip")
                .participant("+15555550123", None)
                .participant("+15555550167", None),
            message_line("g-group", "hi").sender("+15555550123").line()
        ),
    );
    let opts = replace_opts(&assets, tmp.path(), "imessage");
    import_jsonl_files(&db, &[path], &opts).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;

    let linked: Vec<String> = sqlx::query_scalar(
        "SELECT h.raw FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1
         ORDER BY h.raw",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(linked, ["+15555550123", "+15555550167"]);

    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(contacts, 2, "one contact per person in the group");

    // The conversation itself is still stored under its group identifier.
    let chat: String = sqlx::query_scalar(
        "SELECT h.raw FROM conversations c
         JOIN handles h ON h.id = c.chat_handle_id
         WHERE c.account_id = $1",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(chat, "chat1000000005");
}

/// A participant the source gave neither an address nor a name says nothing,
/// so the import leaves it out rather than make an identity of nothing.
#[tokio::test]
async fn a_participant_with_no_address_and_no_name_is_never_created() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    // Neither the roster entry nor the message's sender names this person
    // or records any address for them.
    let path = write_jsonl(
        tmp.path(),
        "nameless.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("openextract", "Nameless_Chat").participant_without_identity(None),
            message_line("g-nameless", "hi").sms().line()
        ),
    );
    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "openextract",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    import_jsonl_files(&db, &[path], &opts).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT h.raw FROM participants p
         JOIN handles h ON h.id = p.handle_id
         JOIN conversations c ON c.id = p.conversation_id
         WHERE c.account_id = $1",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(rows.is_empty(), "expected no participant, got {rows:?}");
}

#[tokio::test]
async fn persists_missing_reason_with_null_sha256() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(&assets).unwrap();

    let path = write_jsonl(
        tmp.path(),
        "missing-att.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms-backup-restore", "+15555550123")
                .participant("+15555550123", None),
            message_line("g-missing", "see attached")
                .mms()
                .sender("+15555550123")
                .attachment(IrAttachment {
                    missing_reason: Some("too_large".into()),
                    size_bytes: Some(999),
                    ..attachment(
                        "attachments/gone.bin",
                        "gone.bin",
                        "application/octet-stream"
                    )
                })
                .line()
        ),
    );
    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.attachments, 1);

    let (_pool, mut conn) = open_verify(&db).await;
    let (sha256, missing_reason, size_bytes, original_name): (
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT sha256, missing_reason, size_bytes, original_name FROM attachments LIMIT 1",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(sha256.is_none());
    assert_eq!(missing_reason.as_deref(), Some("too_large"));
    assert_eq!(size_bytes, Some(999));
    assert_eq!(original_name.as_deref(), Some("gone.bin"));
}

/// One incoming message from `+15555550123` in its own conversation file,
/// carrying an attachment for each path in `paths`. The records give no
/// sha256 and no size, as an iMessage or WhatsApp export does.
fn conversation_with_attachments(paths: &[&str]) -> String {
    let message = paths.iter().fold(
        message_line("g-att", "see attached").sender("+15555550123"),
        |message, path| {
            message.attachment(IrAttachment {
                original_name: None,
                ..attachment(path, "", "application/octet-stream")
            })
        },
    );
    format!(
        "{}\n{message}\n",
        conversation_header("imessage", "+15555550123").participant("+15555550123", None),
    )
}

#[tokio::test]
async fn an_attachment_with_no_size_in_the_export_is_stored_with_the_size_of_its_file() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/photo.bin"), b"twelve bytes").unwrap();
    let path = write_jsonl(
        tmp.path(),
        "sized.jsonl",
        &conversation_with_attachments(&["attachments/photo.bin"]),
    );

    import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let size_bytes: Option<i64> = sqlx::query_scalar("SELECT size_bytes FROM attachments")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(size_bytes, Some(12));
}

/// Three attachments on one message: a file, the same bytes under another
/// name, and a file that is not there. The result counts one of each.
#[tokio::test]
async fn an_import_counts_the_files_it_copied_already_held_and_could_not_find() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/photo.bin"), b"same bytes").unwrap();
    fs::write(tmp.path().join("attachments/copy.bin"), b"same bytes").unwrap();
    let path = write_jsonl(
        tmp.path(),
        "counted.jsonl",
        &conversation_with_attachments(&[
            "attachments/photo.bin",
            "attachments/copy.bin",
            "attachments/gone.bin",
        ]),
    );

    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();

    assert_eq!(
        (
            stats.assets_copied,
            stats.assets_deduped,
            stats.assets_missing
        ),
        (1, 1, 1)
    );
}

#[tokio::test]
async fn claimed_import_rejects_corrupt_existing_asset() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let sha = assets_api::Sha256::of_bytes(b"expected-asset");
    let corrupt = assets.join(assets_api::shard_rel_path(&sha, ""));
    fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
    fs::write(&corrupt, b"corrupt-asset").unwrap();

    let message = message_line("g-corrupt-asset", "missing asset")
        .sender("+15555550123")
        .attachment(IrAttachment {
            digest_sha256: Some(sha.to_string()),
            ..attachment(
                "attachments/missing.bin",
                "missing.bin",
                "application/octet-stream",
            )
        });
    let jsonl = format!(
        "{}\n{}\n",
        conversation_header("imessage", "+15555550123").participant("+15555550123", None),
        message
    );
    let path = write_jsonl(tmp.path(), "corrupt-existing.jsonl", &jsonl);

    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(stats.assets_deduped, 0);
    assert_eq!(stats.assets_missing, 1);
    let (_pool, mut conn) = open_verify(&db).await;
    let assets_path: Option<String> =
        sqlx::query_scalar("SELECT assets_path FROM attachments LIMIT 1")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert!(assets_path.is_none());
}

#[tokio::test]
async fn rejects_attachment_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let export_dir = tmp.path().join("export");
    fs::create_dir_all(&assets).unwrap();
    fs::create_dir_all(&export_dir).unwrap();
    fs::write(tmp.path().join("secret.txt"), b"secret-bytes").unwrap();

    let path = write_jsonl(
        &export_dir,
        "traverse.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms-backup-restore", "+15555550123")
                .participant("+15555550123", None),
            // The path is the input under test, so the line stays written out.
            r#"{"guid":"g-trav","timestamp_unix_ms":1426183462000,"time_precision":"milliseconds","direction":"incoming","service":"sms","message_kind":"mms","sender_identity":"+15555550123","sender_display_name":null,"subject":null,"text":"x","attachments":[{"path":"../secret.txt","original_name":"secret.txt","mime_type":"text/plain","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":12,"missing_reason":null}],"imessage":null,"source":null}
"#
        ),
    );
    let err = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: &export_dir,
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains(message_ir::UNSAFE_ATTACHMENT_PATH),
        "expected path rejection, got: {err}"
    );
}

#[tokio::test]
async fn failed_replace_keeps_existing_messages() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let export_dir = tmp.path().join("export");
    fs::create_dir_all(&assets).unwrap();
    fs::create_dir_all(&export_dir).unwrap();

    let first = write_jsonl(
        &export_dir,
        "ok.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms-backup-restore", "+14075550107")
                .participant("+14075550107", None),
            message_line("g-keep-replace", "keep me")
                .sms()
                .sender("+14075550107")
                .line()
        ),
    );
    import_jsonl_files(
        &db,
        &[first],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: &export_dir,
            mode: ImportMode::Replace,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();

    let bad = write_jsonl(
        &export_dir,
        "bad.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms-backup-restore", "+14075550107")
                .participant("+14075550107", None),
            // The path is the input under test, so the line stays written out.
            r#"{"guid":"g-bad","timestamp_unix_ms":1426183462000,"time_precision":"milliseconds","direction":"incoming","service":"sms","message_kind":"mms","sender_identity":"+14075550107","sender_display_name":null,"subject":null,"text":"nope","attachments":[{"path":"../secret.txt","original_name":"secret.txt","mime_type":"text/plain","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":1,"missing_reason":null}],"imessage":null,"source":null}
"#
        ),
    );
    let err = import_jsonl_files(
        &db,
        &[bad],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: &export_dir,
            mode: ImportMode::Replace,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains(message_ir::UNSAFE_ATTACHMENT_PATH));

    let (_pool, mut conn) = open_verify(&db).await;
    let body: String =
        sqlx::query_scalar("SELECT body FROM messages WHERE guid = 'g-keep-replace'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(body, "keep me");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

/// One individual conversation with `handle`, holding one incoming message.
fn one_message_conversation(guid: &str, handle: &str) -> String {
    let header = conversation_header("sms-backup-restore", handle).participant(handle, None);
    let message = message_line(guid, "hi").sms().sender(handle);
    format!("{header}\n{message}\n")
}

/// Make every later INSERT INTO messages fail, so an import gets through
/// staging and fails inside promote.
async fn fail_every_message_insert(conn: &mut SqliteConnection) {
    sqlx::raw_sql(
        "CREATE TRIGGER fail_promote BEFORE INSERT ON messages
         BEGIN SELECT RAISE(ABORT, 'promote fails on purpose'); END",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
}

/// An import that fails in promote imports nothing, and that includes its
/// contacts. Staging meets every handle first and, by ADR-0013, discards a
/// trashed contact whose handle the backup holds, and makes contacts for
/// people new to the database. Those writes must not outlive a promote that
/// rolled back: the person keeps the trashed contact's name and groups, and
/// the database gains no contacts with no messages.
#[tokio::test]
async fn failed_promote_keeps_the_trashed_contact_and_adds_no_contacts() {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let assets = dir.path().join("assets");
    let export_dir = dir.path().join("export");
    fs::create_dir_all(&export_dir).unwrap();
    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: &export_dir,
        mode: ImportMode::Append,
        source: "sms-backup-restore",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });

    // A first import makes Ada's contact; the person names it, puts it in a
    // group, and moves it to the Trash.
    let first = write_jsonl(
        &export_dir,
        "ada.jsonl",
        &one_message_conversation("g-ada-1", "+15555550165"),
    );
    import_jsonl_files_on_conn(&mut conn, &[first], &opts, ImportSchemaMode::Ensure)
        .await
        .unwrap();
    let ada: i64 = sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    sqlx::query("UPDATE contacts SET preferred_name = 'Ada (work)', origin = 'user' WHERE id = $1")
        .bind(ada)
        .execute(&mut *conn)
        .await
        .unwrap();
    let group_id: i64 = sqlx::query_scalar(
        "INSERT INTO contact_groups (account_id, name) VALUES ($1, 'Colleagues') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)")
        .bind(ada)
        .bind(group_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert!(
        crate::db::trash::move_to_trash(
            &mut conn,
            TEST_ACCOUNT,
            crate::db::trash::Trashable::Contact(ada)
        )
        .await
        .unwrap()
    );

    // A second backup holds Ada again and someone new; its promote fails.
    fail_every_message_insert(&mut conn).await;
    let again = write_jsonl(
        &export_dir,
        "ada-again.jsonl",
        &one_message_conversation("g-ada-2", "+15555550165"),
    );
    let newcomer = write_jsonl(
        &export_dir,
        "newcomer.jsonl",
        &one_message_conversation("g-new-1", "+15555550166"),
    );
    let err = import_jsonl_files_on_conn(
        &mut conn,
        &[again, newcomer],
        &opts,
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("promote fails on purpose"),
        "expected the promote failure, got: {err:#}"
    );

    let contacts: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, preferred_name FROM contacts WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        contacts,
        vec![(ada, "Ada (work)".to_string())],
        "Ada keeps her name and the failed import leaves no new contact"
    );
    let trashed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1 AND contact_id = $2",
    )
    .bind(TEST_ACCOUNT)
    .bind(ada)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(trashed, 1, "Ada is still in the Trash");
    let groups: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contact_group_members WHERE contact_id = $1")
            .bind(ada)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(groups, 1, "Ada is still in her group");
}

/// A registered account may import with its session token: `can_import`
/// is on by default, which `server.rs`'s `can_import = 0` test relies on
/// to prove the opposite case.
async fn importer() -> (
    crate::server::AppState,
    crate::test_support::TestFixture,
    String,
) {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "importer", "hunter2hunter2").await;
    let state = fixture.state.clone();
    (state, fixture, account.token)
}

/// Create an Import Run for `source` and hand back the path its batches
/// are posted to.
pub(super) async fn batches_path(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
) -> String {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": source }),
    )
    .await;
    format!("/v1/imports/{}/batches", created["id"].as_i64().unwrap())
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
        reqwest::Client::new()
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

/// One `source` conversation with `+15555550107`, one message per guid. The
/// message `g-gone` carries a missing attachment and a tapback, so a wipe
/// that leaves children behind shows.
fn wipe_test_batch(source: &str, guids: &[&str]) -> String {
    let mut lines = vec![
        conversation_header(source, "+15555550107")
            .owner("+15555550106", Some("Me"))
            .participant("+15555550107", None)
            .to_string(),
    ];
    for guid in guids {
        let mut message = message_line(guid, guid)
            .at(1_700_000_000_000)
            .sms()
            .sender("+15555550107");
        if *guid == "g-gone" {
            message = message
                .attachment(missing_attachment("gone.bin"))
                .reaction(liked_by("+15555550107"));
        }
        lines.push(message.to_string());
    }
    lines.join("\n") + "\n"
}

/// Create an Import Run for `source` in `mode`, post `body` as its one
/// batch, and complete the run so the account may start another.
async fn import_one_batch(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    mode: &str,
    body: String,
) {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": source, "mode": mode }),
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

/// A replace run deletes the account's existing messages for its source
/// before it promotes the new ones, so a message dropped from the new
/// export leaves the server, with its attachments and tapbacks. Nothing
/// else moves: another source's messages, another account's messages for
/// the same source, and a message that pointed at a deleted one as its
/// duplicate (which now points nowhere).
#[tokio::test]
async fn a_replace_run_deletes_only_its_own_sources_old_messages() {
    let (state, _fixture, token) = importer().await;
    let other = crate::test_support::register_via_api(&state, "other", "hunter2hunter2").await;

    import_one_batch(
        &state,
        &token,
        "whatsapp",
        "replace",
        wipe_test_batch("whatsapp", &["g-keep", "g-gone"]),
    )
    .await;
    import_one_batch(
        &state,
        &token,
        "sms-backup-restore",
        "append",
        wipe_test_batch("sms-backup-restore", &["g-sms"]),
    )
    .await;
    import_one_batch(
        &state,
        &other.token,
        "whatsapp",
        "replace",
        wipe_test_batch("whatsapp", &["g-other", "g-gone"]),
    )
    .await;

    let mut conn = state.db.acquire().await.unwrap();
    let children = |table: &'static str| {
        format!(
            "SELECT COUNT(*) FROM {table} t JOIN messages m ON m.id = t.message_id \
             WHERE m.guid = 'g-gone'"
        )
    };
    for table in ["attachments", "tapbacks"] {
        let n: i64 = sqlx::query_scalar(&children(table))
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(n, 2, "both accounts' g-gone carry one row in {table}");
    }
    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    sqlx::query(
        "UPDATE messages SET duplicate_of = (
             SELECT id FROM messages WHERE guid = 'g-gone' AND account_id != $1
         ) WHERE guid = 'g-sms'",
    )
    .bind(other.account_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    drop(conn);

    import_one_batch(
        &state,
        &token,
        "whatsapp",
        "replace",
        wipe_test_batch("whatsapp", &["g-keep"]),
    )
    .await;

    let mut conn = state.db.acquire().await.unwrap();
    let rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT account_id, source, guid FROM messages")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    let mut rows: Vec<(bool, &str, &str)> = rows
        .iter()
        .map(|(a, s, g)| (*a == other.account_id, s.as_str(), g.as_str()))
        .collect();
    rows.sort_unstable();
    assert_eq!(
        rows,
        [
            (false, "sms-backup-restore", "g-sms"),
            (false, "whatsapp", "g-keep"),
            (true, "whatsapp", "g-gone"),
            (true, "whatsapp", "g-other"),
        ]
    );
    for table in ["attachments", "tapbacks"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(
            n, 1,
            "only the other account's g-gone keeps a row in {table}"
        );
    }
    let duplicate_of: Option<i64> =
        sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE guid = 'g-sms'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(duplicate_of, None, "no message may point at a deleted one");
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
    let parsed: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("non-JSON body: {text}"));
    assert_eq!(
        status,
        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "{text}"
    );
    assert_eq!(
        parsed["detail"],
        "Content-Type must be application/x-ndjson or application/jsonl"
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

/// What a request can change on an Import Run: status, stage, approved plan
/// and finish time.
async fn run_state(
    state: &crate::server::AppState,
    import_id: i64,
) -> (String, Option<String>, Option<String>, Option<String>) {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_as("SELECT status, stage, summary_json, finished_at FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// The desktop app sends every call of an Import Run with the session logged
/// in at the time, so a run whose account logged out while it ran reaches
/// the server with the next account's session (#1085). Every route on a
/// run refuses another account's session as if the run did not exist, and
/// leaves the run as it was.
#[tokio::test]
async fn every_route_on_another_accounts_run_is_not_found_and_changes_nothing() {
    let (fixture, alice) = crate::test_support::fixture_with_account().await;
    let bob = crate::test_support::register_via_api(&fixture.state, "bob", "hunter2hunter2").await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &bob.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let bobs_run = created["id"].as_i64().unwrap();
    let run = format!("/v1/imports/{bobs_run}");
    let before = run_state(&fixture.state, bobs_run).await;

    let state = &fixture.state;
    let token = alice.token.as_str();
    let refusals = [
        (
            "PATCH stage",
            crate::test_support::patch_raw(
                state,
                &run,
                token,
                serde_json::json!({ "stage": "upload", "summary": { "approved": true } }),
            )
            .await,
        ),
        (
            "POST complete",
            crate::test_support::post_raw(
                state,
                &format!("{run}/complete"),
                token,
                "application/json",
                r#"{"status":"completed"}"#,
            )
            .await,
        ),
        (
            "POST discard",
            crate::test_support::post_raw(
                state,
                &format!("{run}/discard"),
                token,
                "application/json",
                r#"{"issues":[],"notes":[]}"#,
            )
            .await,
        ),
        (
            "POST batches",
            crate::test_support::post_raw(
                state,
                &format!("{run}/batches"),
                token,
                "application/jsonl",
                "{}\n",
            )
            .await,
        ),
        (
            "GET run",
            crate::test_support::get_raw(state, &run, token).await,
        ),
        (
            "GET contacts",
            crate::test_support::get_raw(state, &format!("{run}/contacts"), token).await,
        ),
    ];
    for (route, (status, text)) in refusals {
        assert_eq!(
            status,
            axum::http::StatusCode::NOT_FOUND,
            "{route} on another account's run: {text}"
        );
        crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);
    }

    assert_eq!(run_state(&fixture.state, bobs_run).await, before);
    assert_eq!(before.0, "running");
}

/// The account that owns Import Run `import_id`.
async fn run_account(state: &crate::server::AppState, import_id: i64) -> i64 {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_scalar("SELECT account_id FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// Complete `import_id` and hand back the date its row says it
/// finished on, the `YYYY-MM-DD` the shortcuts are named after.
async fn complete_run(state: &crate::server::AppState, token: &str, import_id: i64) -> String {
    let _: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{import_id}/complete"),
        token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
    let mut conn = state.db.acquire().await.unwrap();
    let finished_at: String = sqlx::query_scalar("SELECT finished_at FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    finished_at[..10].to_string()
}

/// Every saved search the account owns as `(name, query, kind)`, and every
/// Contact Group as `(name, kind, member contact ids ascending)`.
async fn shortcuts(
    state: &crate::server::AppState,
    account_id: i64,
) -> (
    Vec<(String, String, String)>,
    Vec<(String, String, Vec<i64>)>,
) {
    let mut conn = state.db.acquire().await.unwrap();
    let searches: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT name, query, kind FROM saved_searches WHERE account_id = $1 ORDER BY name",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let groups: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, name, kind FROM contact_groups WHERE account_id = $1 ORDER BY name",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let mut out = Vec::new();
    for (id, name, kind) in groups {
        let members: Vec<i64> = sqlx::query_scalar(
            "SELECT contact_id FROM contact_group_members WHERE group_id = $1 ORDER BY contact_id",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        out.push((name, kind, members));
    }
    (searches, out)
}

/// Finishing a run that stored messages leaves two shortcuts behind: a
/// saved search whose query is the run's id, and a Contact Group holding
/// exactly the contacts the run recorded touching. Both carry the source
/// and the day the run finished in their names, and both are marked
/// `import` so the sidebar can tell them from what a person made.
#[tokio::test]
async fn completing_an_import_with_messages_creates_its_saved_search_and_contact_group() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        wipe_test_batch("whatsapp", &["g-1", "g-2"]),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");

    let date = complete_run(&state, &token, import_id).await;
    let (searches, groups) = shortcuts(&state, run_account(&state, import_id).await).await;

    assert_eq!(
        searches,
        [(
            format!("Import whatsapp {date}"),
            format!("import:#{import_id}"),
            "import".to_string()
        )]
    );

    let mut conn = state.db.acquire().await.unwrap();
    let touched = crate::db::import_contacts::contact_ids(&mut conn, import_id)
        .await
        .unwrap();
    assert!(!touched.is_empty(), "the run must have touched a contact");
    assert_eq!(
        groups,
        [(
            format!("whatsapp import {date}"),
            "import".to_string(),
            touched
        )]
    );
    drop(conn);

    // The stored query has to be one the search accepts, and it has to
    // answer with this run's messages only. A second run's message is the
    // one it must leave out (#950).
    import_one_batch(
        &state,
        &token,
        "sms-backup-restore",
        "append",
        wipe_test_batch("sms-backup-restore", &["g-later"]),
    )
    .await;
    let stored = searches[0].1.replace(':', "%3A").replace('#', "%23");
    let page: serde_json::Value =
        get_json(&state, &format!("/v1/messages?q={stored}"), &token).await;
    let mut texts: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    texts.sort_unstable();
    assert_eq!(texts, ["g-1", "g-2"], "{page}");
}

/// The counts of a finished run are the server's own, never the client's.
/// A resumed Upload's report counts only what the resume sent, which
/// is nothing when the run's first `/complete` was refused after every
/// message landed. A count from the client would record that run as
/// holding no messages, and give it no Saved Search.
#[tokio::test]
async fn completing_a_run_counts_its_messages_whatever_the_body_says() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        wipe_test_batch("whatsapp", &["g-1", "g-2"]),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");

    let _: serde_json::Value = post_json(
        &state,
        &format!("/v1/imports/{import_id}/complete"),
        &token,
        serde_json::json!({ "status": "completed", "message_count": 0, "attachment_count": 0 }),
    )
    .await;

    let completed: serde_json::Value =
        get_json(&state, &format!("/v1/imports/{import_id}"), &token).await;
    assert_eq!(completed["message_count"], 2, "{completed}");
    let (searches, _) = shortcuts(&state, run_account(&state, import_id).await).await;
    assert_eq!(searches.len(), 1, "the run's saved search: {searches:?}");
}

/// A run that stored nothing gets no saved search: one matching no
/// messages would only clutter the sidebar, and the run stays visible in
/// Import History regardless. With no contacts touched there is no Contact
/// Group either.
#[tokio::test]
async fn completing_an_import_with_no_messages_creates_no_saved_search() {
    let (state, _fixture, token) = importer().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &state,
        "/v1/imports",
        &token,
        serde_json::json!({ "source": "whatsapp" }),
    )
    .await;
    let import_id = created["id"].as_i64().unwrap();

    complete_run(&state, &token, import_id).await;
    let completed: serde_json::Value =
        get_json(&state, &format!("/v1/imports/{import_id}"), &token).await;
    assert_eq!(completed["message_count"], 0, "{completed}");

    let (searches, groups) = shortcuts(&state, run_account(&state, import_id).await).await;
    assert!(
        searches.is_empty(),
        "no saved search for an empty run: {searches:?}"
    );
    assert!(
        groups.is_empty(),
        "no Contact Group for an empty run: {groups:?}"
    );
}

/// Create an Import Run for `source`, post `body` as its one batch, complete
/// it, and hand back the run's id and the day it finished.
pub(super) async fn completed_run(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    body: String,
) -> (i64, String) {
    let path = batches_path(state, token, source).await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let (status, text) =
        crate::test_support::post_raw(state, &path, token, "application/jsonl", body).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let date = complete_run(state, token, import_id).await;
    (import_id, date)
}

/// Each Import Run gets a Contact Group of its own. A second run from the
/// same source on the same day takes the next free name, and neither group
/// holds the other run's contacts (#956).
#[tokio::test]
async fn two_import_runs_from_one_source_on_one_day_make_two_contact_groups() {
    let (state, _fixture, token) = importer().await;
    let (first, first_date) = completed_run(
        &state,
        &token,
        "whatsapp",
        wipe_test_batch("whatsapp", &["g-1"]),
    )
    .await;
    // A different person, so the second run has a contact of its own to record.
    let second_body = wipe_test_batch("whatsapp", &["g-2"]).replace("+15555550107", "+15555550108");
    let (second, second_date) = completed_run(&state, &token, "whatsapp", second_body).await;

    let mut conn = state.db.acquire().await.unwrap();
    let first_touched = crate::db::import_contacts::contact_ids(&mut conn, first)
        .await
        .unwrap();
    let second_touched = crate::db::import_contacts::contact_ids(&mut conn, second)
        .await
        .unwrap();
    drop(conn);
    assert!(!first_touched.is_empty() && !second_touched.is_empty());
    assert!(
        first_touched.iter().all(|id| !second_touched.contains(id)),
        "the runs must touch different contacts: {first_touched:?} {second_touched:?}"
    );

    // The suffix appears when both runs finished on one UTC day, which is
    // every run of this test except one that straddles midnight.
    let second_name = if second_date == first_date {
        format!("whatsapp import {second_date} 2")
    } else {
        format!("whatsapp import {second_date}")
    };
    let (_, groups) = shortcuts(&state, run_account(&state, first).await).await;
    assert_eq!(
        groups,
        [
            (
                format!("whatsapp import {first_date}"),
                "import".to_string(),
                first_touched
            ),
            (second_name, "import".to_string(), second_touched),
        ]
    );
}

/// A Contact Group a person made is theirs, even under the name an import
/// would use: the import leaves its kind and its members alone and takes
/// the next free name for its own group. A name is taken whatever its
/// case, so the hand-made ` 2` in capitals sends the import to ` 3` (#956).
#[tokio::test]
async fn an_import_leaves_a_hand_made_contact_group_with_its_name_alone() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let account = run_account(&state, import_id).await;
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        wipe_test_batch("whatsapp", &["g-1"]),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");

    // The hand-made groups are named for today and for tomorrow, so the
    // test holds when the run finishes just past midnight UTC.
    let spec = crate::db::named_membership::group_spec();
    let mut conn = state.db.acquire().await.unwrap();
    let friend = crate::db::contacts::create_contact(
        &mut conn,
        account,
        "Friend",
        crate::db::contacts::Origin::User,
    )
    .await
    .unwrap();
    let today = chrono::Utc::now().date_naive();
    for day in [today, today.succ_opt().unwrap()] {
        for name in [
            format!("whatsapp import {day}"),
            format!("WHATSAPP IMPORT {day} 2"),
        ] {
            let (group, _) =
                crate::db::named_membership::create_set(spec, &mut conn, account, &name)
                    .await
                    .unwrap();
            crate::db::named_membership::patch_members(
                spec,
                &mut conn,
                account,
                group,
                &[friend],
                &[],
            )
            .await
            .unwrap();
        }
    }
    drop(conn);

    let date = complete_run(&state, &token, import_id).await;

    let mut conn = state.db.acquire().await.unwrap();
    let touched = crate::db::import_contacts::contact_ids(&mut conn, import_id)
        .await
        .unwrap();
    drop(conn);
    assert!(!touched.is_empty() && !touched.contains(&friend));
    let (_, groups) = shortcuts(&state, account).await;
    for name in [
        format!("whatsapp import {date}"),
        format!("WHATSAPP IMPORT {date} 2"),
    ] {
        let hand_made = groups
            .iter()
            .find(|(group, _, _)| *group == name)
            .expect("the hand-made group is still there");
        assert_eq!(
            (hand_made.1.as_str(), &hand_made.2),
            ("manual", &vec![friend]),
            "{groups:?}"
        );
    }
    let imported: Vec<_> = groups
        .iter()
        .filter(|(_, kind, _)| kind == "import")
        .collect();
    assert_eq!(
        imported,
        [&(
            format!("whatsapp import {date} 3"),
            "import".to_string(),
            touched
        )],
        "{groups:?}"
    );
}

/// Import `path` in append mode on `conn`, as the serve path does once the
/// schema is in place.
async fn append_on_conn(conn: &mut SqliteConnection, path: &Path, root: &Path, source: &str) {
    let assets = root.join("assets");
    import_jsonl_files_on_conn(
        conn,
        &[path.to_path_buf()],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: root,
            mode: ImportMode::Append,
            source,
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap();
}

/// Each conversation of `TEST_ACCOUNT` with its number of participant rows,
/// and the account's number of contacts.
async fn participant_and_contact_counts(conn: &mut SqliteConnection) -> (Vec<(i64, i64)>, i64) {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT c.id, COUNT(p.id) FROM conversations c
         LEFT JOIN participants p ON p.conversation_id = c.id
         WHERE c.account_id = $1
         GROUP BY c.id ORDER BY c.id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    (rows, contacts)
}

/// A participant the source named with no address has no handle, and a
/// uniqueness rule that includes a NULL column never matches. Importing the
/// same file twice must still leave one row per person in the conversation.
#[tokio::test]
async fn reimporting_a_file_with_a_name_only_participant_adds_no_participant() {
    let fixture = test_fixture().await;
    let tmp = TempDir::new().unwrap();
    let path = write_jsonl(
        tmp.path(),
        "group-with-name-only.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("openextract", "chat2000000001")
                .group()
                .title("Trip")
                .participant("+15555550123", Some("Ada"))
                .participant_without_identity(Some("Sarah Vale")),
            message_line("g-name-only-twice", "hi")
                .sms()
                .sender("+15555550123")
                .line()
        ),
    );
    let mut conn = fixture.state.db.acquire().await.unwrap();

    append_on_conn(&mut conn, &path, tmp.path(), "openextract").await;
    let (first_rows, first_contacts) = participant_and_contact_counts(&mut conn).await;
    assert_eq!(first_rows.len(), 1, "one conversation: {first_rows:?}");
    assert_eq!(first_rows[0].1, 2, "Ada and Sarah Vale: {first_rows:?}");
    assert_eq!(first_contacts, 2);

    append_on_conn(&mut conn, &path, tmp.path(), "openextract").await;
    let (second_rows, second_contacts) = participant_and_contact_counts(&mut conn).await;
    assert_eq!(
        second_rows, first_rows,
        "the second import added participants"
    );
    assert_eq!(second_contacts, first_contacts);
}

/// ADR-0013: an import that meets a trashed contact's handle discards the
/// contact and makes a fresh one. The re-import must not add a second row
/// for the same handle beside the existing one; the conversation lists the
/// person once.
#[tokio::test]
async fn reimporting_after_trashing_a_contact_lists_the_person_once() {
    let fixture = test_fixture().await;
    let tmp = TempDir::new().unwrap();
    let path = write_jsonl(
        tmp.path(),
        "ada.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("imessage", "+15555550123")
                .participant("+15555550123", Some("Ada")),
            message_line("g-trashed-twice", "hi")
                .sender("+15555550123")
                .line()
        ),
    );
    let mut conn = fixture.state.db.acquire().await.unwrap();

    append_on_conn(&mut conn, &path, tmp.path(), "imessage").await;
    let ada: i64 = sqlx::query_scalar(
        "SELECT id FROM contacts WHERE account_id = $1 AND preferred_name = 'Ada'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(ada)
        .execute(&mut *conn)
        .await
        .unwrap();

    append_on_conn(&mut conn, &path, tmp.path(), "imessage").await;

    let trashed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(trashed, 0, "the import replaced the trashed contact");

    let conversation_id: i64 =
        sqlx::query_scalar("SELECT id FROM conversations WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let loaded =
        crate::db::participant_names::load_for_conversations(&mut conn, &[conversation_id])
            .await
            .unwrap();
    let names: Vec<&str> = loaded[&conversation_id]
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["Ada"], "the conversation lists Ada once");
}

/// Each message records the account holder's address it was held at: its
/// own owner handle when the backup names one per message, the header's
/// otherwise. The holder is never a participant and never gets a contact
/// (ADR-0015), so neither owner address reaches Contacts.
#[tokio::test]
async fn a_message_is_held_at_its_own_owner_else_the_headers_and_the_owner_gets_no_contact() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let file = write_jsonl(
        tmp.path(),
        "owner.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("imessage", "+14075550107")
                .owner("+14155550100", None)
                .participant("+14075550107", None),
            [
                message_line("g-out", "sent from the phone")
                    .outgoing()
                    .sender("+14155550100"),
                message_line("g-email", "received at the email")
                    .at(1_426_183_522_000)
                    .sender("+14075550107")
                    .owner_identity("me@example.com"),
                message_line("g-in", "received at the phone")
                    .at(1_426_183_582_000)
                    .sender("+14075550107"),
            ]
            .iter()
            .map(MessageLine::line)
            .collect::<String>()
        ),
    );
    import_jsonl_files(&db, &[file], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let held: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT m.guid, h.normalized FROM messages m
         LEFT JOIN handles h ON h.id = m.owner_handle_id
         ORDER BY m.timestamp",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        held,
        vec![
            ("g-out".to_string(), Some("+14155550100".to_string())),
            ("g-email".to_string(), Some("me@example.com".to_string())),
            ("g-in".to_string(), Some("+14155550100".to_string())),
        ]
    );
    let owner_contacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE h.normalized IN ('+14155550100', 'me@example.com')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(owner_contacts, 0, "the holder's addresses get no contact");
    let owner_participants: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM participants p
         JOIN handles h ON h.id = p.handle_id
         WHERE h.normalized IN ('+14155550100', 'me@example.com')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(owner_participants, 0, "the holder is never a participant");
}

/// A run's source names its messages and its attachment directory, and the
/// batch route takes it from the run without checking it again. So a blank
/// one has to be refused here, where the run is created.
#[tokio::test]
async fn creating_an_import_with_a_blank_source_is_a_validation_failure() {
    let (state, _fixture, token) = importer().await;
    for source in ["", "   "] {
        let (status, text) = crate::test_support::post_raw(
            &state,
            "/v1/imports",
            &token,
            "application/json",
            serde_json::json!({ "source": source }).to_string(),
        )
        .await;
        crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::ValidationFailed,
        );
        let errors: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            errors["errors"],
            serde_json::json!(["source is required"]),
            "{text}"
        );
    }
    let runs: serde_json::Value = get_json(&state, "/v1/imports", &token).await;
    assert_eq!(runs["total"], 0, "no run was created: {runs}");
}

/// The bug: `validate_source_id` checked the id trimmed and the run stored
/// it as given, so ` imessage ` made a run whose source held spaces.
#[tokio::test]
async fn creating_an_import_with_a_space_around_the_source_is_a_validation_failure() {
    let (state, _fixture, token) = importer().await;
    for source in [" imessage", "imessage ", " imessage "] {
        let (status, text) = crate::test_support::post_raw(
            &state,
            "/v1/imports",
            &token,
            "application/json",
            serde_json::json!({ "source": source }).to_string(),
        )
        .await;
        crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::ValidationFailed,
        );
    }
    let runs: serde_json::Value = get_json(&state, "/v1/imports", &token).await;
    assert_eq!(runs["total"], 0, "no run was created: {runs}");
}

/// One `whatsapp` conversation with `chat` holding one message `guid` whose
/// text is `text`, with `attachment` as its one attachment when there is one.
fn one_message_batch(
    chat: &str,
    guid: &str,
    text: &str,
    attachment: Option<IrAttachment>,
) -> String {
    let header = conversation_header("whatsapp", chat)
        .owner("+15555550106", Some("Me"))
        .participant(chat, None);
    let mut message = whatsapp_line(guid, text).sender(chat);
    if let Some(attachment) = attachment {
        message = message.attachment(attachment);
    }
    format!("{header}\n{message}\n")
}

/// Import `body` as one batch of a new `whatsapp` run in `mode`.
async fn import_batch(state: &crate::server::AppState, token: &str, mode: &str, body: String) {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "whatsapp", "mode": mode }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let path = format!("/v1/imports/{id}/batches");
    let (status, text) =
        crate::test_support::post_raw(state, &path, token, "application/jsonl", body).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let _: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
}

/// The texts of the messages `GET /v1/messages?q=` finds, and its `total`.
async fn message_search(
    state: &crate::server::AppState,
    token: &str,
    q: &str,
) -> (Vec<String>, u64) {
    let page: serde_json::Value = get_json(state, &format!("/v1/messages?q={q}"), token).await;
    let texts = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap_or_default().to_string())
        .collect();
    (texts, page["total"].as_u64().unwrap())
}

/// SQLite gives a new message the id of the newest deleted one, and the
/// search index is keyed by that id. So when deleting a message leaves its
/// attachment's file name in the index, the next message imported is found
/// by a file name it never carried.
#[tokio::test]
async fn search_forgets_the_attachment_name_of_a_deleted_message() {
    let fixture = test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "importer", "hunter2hunter2").await;
    let (state, token) = (&fixture.state, account.token.as_str());

    import_batch(
        state,
        token,
        "append",
        one_message_batch(
            "+15555550107",
            "g-old",
            "see attached",
            Some(missing_attachment("zzinvoice.pdf")),
        ),
    )
    .await;
    assert_eq!(
        message_search(state, token, "zzinvoice").await,
        (vec!["see attached".to_string()], 1)
    );

    let _: serde_json::Value = crate::test_support::delete_json_with_body(
        state,
        &format!("/v1/accounts/{}/messages", account.account_id),
        token,
        serde_json::json!({ "confirm": true }),
    )
    .await;
    assert_eq!(message_search(state, token, "zzinvoice").await, (vec![], 0));

    import_batch(
        state,
        token,
        "append",
        one_message_batch("+15555550108", "g-new", "lunch tomorrow", None),
    )
    .await;
    assert_eq!(
        message_search(state, token, "lunch").await,
        (vec!["lunch tomorrow".to_string()], 1)
    );
    assert_eq!(
        message_search(state, token, "zzinvoice").await,
        (vec![], 0),
        "a message imported after the delete is not found by the deleted attachment's name"
    );
}

fn s1_header() -> ConversationHeaderLine {
    conversation_header("imessage", "+15555550119").participant("+15555550119", Some("Bob"))
}

fn s1_message(guid: &str, sender: &str, ms: i64, text: &str) -> MessageLine {
    message_line(guid, text).at(ms).sender(sender)
}

/// A group header that names `name` with no address and lists `sender`, and
/// one message from `sender`.
fn name_only_group_batch(chat: &str, name: &str, sender: &str, guid: &str) -> String {
    let header = conversation_header("imessage", chat)
        .group()
        .title("Trip")
        .participant_without_identity(Some(name))
        .participant(sender, None);
    format!(
        "{header}\n{}\n",
        s1_message(guid, sender, 1426183463000, "yo")
    )
}

/// A name-only participant whose name matches a contact in the Trash, in a run
/// that then meets that contact's address. The run discards the trashed
/// contact (ADR 0013), so the name lookup must not bind the participant to it:
/// promote would then fail on the deleted id, on every retry.
#[tokio::test]
async fn a_name_only_participant_matching_a_trashed_contact_does_not_fail_the_import() {
    let (state, _fixture, token) = importer().await;
    import_one_batch(
        &state,
        &token,
        "imessage",
        "append",
        format!(
            "{}\n{}\n",
            s1_header(),
            s1_message("g1", "+15555550119", 1426183462000, "hi")
        ),
    )
    .await;
    // A second contact made after Bob, so Bob's id is not the highest and
    // SQLite does not hand it out again.
    import_one_batch(
        &state,
        &token,
        "imessage",
        "append",
        format!(
            "{}\n{}\n",
            conversation_header("imessage", "+15555550179")
                .participant("+15555550179", Some("Carol")),
            s1_message("g0", "+15555550179", 1426183462000, "hey")
        ),
    )
    .await;
    {
        let mut conn = state.db.acquire().await.unwrap();
        let (bob, account_id): (i64, i64) =
            sqlx::query_as("SELECT id, account_id FROM contacts WHERE preferred_name = 'Bob'")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        crate::db::trash::move_to_trash(
            &mut conn,
            account_id,
            crate::db::trash::Trashable::Contact(bob),
        )
        .await
        .unwrap();
    }
    let path = batches_path(&state, &token, "imessage").await;
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        name_only_group_batch("chat900", "Bob", "+15555550119", "g2"),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
}

/// A live and a trashed contact share a name. A participant named with no
/// address is the identity of type `other` holding the name, never a lookup
/// by name, so it is on a live contact and the trashed one stays as it was.
#[tokio::test]
async fn a_name_only_participant_is_never_bound_to_a_trashed_contact_that_shares_the_name() {
    let (state, _fixture, token) = importer().await;
    let trashed = {
        let mut conn = state.db.acquire().await.unwrap();
        let account_id: i64 =
            sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        let mut ids = Vec::new();
        for _ in 0..2 {
            ids.push(
                crate::db::contacts::create_contact(
                    &mut conn,
                    account_id,
                    "Sarah Vale",
                    crate::db::contacts::Origin::User,
                )
                .await
                .unwrap(),
            );
        }
        let trashed = ids[1];
        crate::db::trash::move_to_trash(
            &mut conn,
            account_id,
            crate::db::trash::Trashable::Contact(trashed),
        )
        .await
        .unwrap();
        trashed
    };
    import_one_batch(
        &state,
        &token,
        "imessage",
        "append",
        name_only_group_batch("chat901", "Sarah Vale", "+15555550177", "g3"),
    )
    .await;
    let mut conn = state.db.acquire().await.unwrap();
    let bound: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT ch.contact_id,
                EXISTS (SELECT 1 FROM trashed_contacts t WHERE t.contact_id = ch.contact_id)
         FROM participants p
         JOIN handles h ON h.id = p.handle_id
         JOIN contact_handles ch ON ch.handle_id = h.id
         WHERE h.handle_type = 'other' AND h.raw = 'Sarah Vale'",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(bound.len(), 1, "{bound:?}");
    assert_ne!(bound[0].0, trashed, "{bound:?}");
    assert!(!bound[0].1, "the participant's contact is live: {bound:?}");
    let still_trashed: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM trashed_contacts WHERE contact_id = $1)")
            .bind(trashed)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert!(still_trashed, "the trashed contact stays in the Trash");
}

/// One incoming message of the one-to-one chat with +15555550119, whose text
/// is its guid.
fn same_second_message(guid: &str, ms: i64) -> MessageLine {
    message_line(guid, guid).at(ms).sender("+15555550119")
}

/// The header of the one-to-one chat with +15555550119.
fn same_second_header() -> ConversationHeaderLine {
    conversation_header("imessage", "+15555550119").participant("+15555550119", Some("Bob"))
}

/// The account's message guids in the order the conversation page and every
/// export read them.
async fn guids_in_conversation_order(state: &crate::server::AppState) -> Vec<String> {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_scalar("SELECT guid FROM messages ORDER BY timestamp, sort_order, id")
        .fetch_all(&mut *conn)
        .await
        .unwrap()
}

/// Post each body as one batch of a single Import Run.
async fn post_batches_of_one_run(
    state: &crate::server::AppState,
    token: &str,
    bodies: Vec<String>,
) {
    let path = batches_path(state, token, "imessage").await;
    for body in bodies {
        let (status, text) =
            crate::test_support::post_raw(state, &path, token, "application/jsonl", body).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    }
}

/// #1168: a conversation split across two batches keeps its order where the
/// split falls inside one second.
#[tokio::test]
async fn a_conversation_split_across_batches_keeps_its_order_within_a_second() {
    let (state, _fixture, token) = importer().await;
    post_batches_of_one_run(
        &state,
        &token,
        vec![
            format!(
                "{}\n{}\n{}\n",
                same_second_header(),
                same_second_message("m1", 1_426_183_462_100),
                same_second_message("m2", 1_426_183_462_200)
            ),
            format!(
                "{}\n{}\n",
                same_second_header(),
                same_second_message("m3", 1_426_183_462_300)
            ),
        ],
    )
    .await;
    assert_eq!(
        guids_in_conversation_order(&state).await,
        ["m1", "m2", "m3"]
    );
}

/// #1168: three messages with one whole-second time, split across two
/// batches, read back in the source's order. Sources that record whole
/// seconds (iMazing, OpenExtract, GO SMS Pro) leave nothing but the source's
/// order to tell them apart.
#[tokio::test]
async fn messages_of_one_whole_second_split_across_batches_keep_the_source_order() {
    let (state, _fixture, token) = importer().await;
    post_batches_of_one_run(
        &state,
        &token,
        vec![
            format!(
                "{}\n{}\n{}\n",
                same_second_header(),
                same_second_message("m1", 1_426_183_462_000),
                same_second_message("m2", 1_426_183_462_000)
            ),
            format!(
                "{}\n{}\n",
                same_second_header(),
                same_second_message("m3", 1_426_183_462_000)
            ),
        ],
    )
    .await;
    assert_eq!(
        guids_in_conversation_order(&state).await,
        ["m1", "m2", "m3"]
    );
}

/// #1168: a later append that adds a message in a second the conversation
/// already holds reads it back after the stored ones.
#[tokio::test]
async fn an_append_in_a_second_already_held_sorts_after_the_stored_messages() {
    let (state, _fixture, token) = importer().await;
    import_one_batch(
        &state,
        &token,
        "imessage",
        "append",
        format!(
            "{}\n{}\n{}\n",
            same_second_header(),
            same_second_message("m1", 1_426_183_462_000),
            same_second_message("m2", 1_426_183_462_000)
        ),
    )
    .await;
    import_one_batch(
        &state,
        &token,
        "imessage",
        "append",
        format!(
            "{}\n{}\n",
            same_second_header(),
            same_second_message("m3", 1_426_183_462_000)
        ),
    )
    .await;
    assert_eq!(
        guids_in_conversation_order(&state).await,
        ["m1", "m2", "m3"]
    );
}

/// The header of an Apple Messages one-to-one chat keyed by `chat`, whose
/// one participant is written as `chat` too.
fn one_to_one_header(chat: &str) -> String {
    conversation_header("imessage", chat)
        .participant(chat, None)
        .to_string()
}

/// #1173: two conversations whose chat ids differ as written and normalise
/// to one identity, in one batch, become one conversation holding both
/// conversations' messages, as they do when they arrive in separate batches.
/// Both messages share one second, so the second conversation's message
/// takes the next `sort_order` rather than repeating the first's.
#[tokio::test]
async fn two_conversations_on_one_identity_in_one_batch_become_one() {
    let (state, _fixture, token) = importer().await;
    let body = format!(
        "{}\n{}\n{}\n{}\n",
        one_to_one_header("+15555550119"),
        same_second_message("m1", 1_426_183_462_000),
        one_to_one_header("5555550119"),
        same_second_message("m2", 1_426_183_462_000),
    );
    post_batches_of_one_run(&state, &token, vec![body]).await;

    let mut conn = state.db.acquire().await.unwrap();
    let conversations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversations")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(conversations, 1);
    let participants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM participants")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(participants, 1, "the one person is listed once");
    let messages: Vec<(String, i64)> =
        sqlx::query_as("SELECT guid, sort_order FROM messages ORDER BY sort_order, id")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        messages,
        [("m1".to_string(), 0), ("m2".to_string(), 1)],
        "both messages are in the one conversation, in the batch's order"
    );
}

/// The header of the Apple Messages group `chat1000000408`, titled `title`
/// (no title when `None`), with one participant.
fn titled_group_header(title: Option<&str>) -> String {
    let header = conversation_header("imessage", "chat1000000408").group();
    let header = match title {
        Some(title) => header.title(title),
        None => header,
    };
    header.participant("+15555550119", None).to_string()
}

/// One copy of the group `chat1000000408`: its header titled `title`, and
/// one message `guid` sent at `ms`.
fn titled_group_copy(title: Option<&str>, guid: &str, ms: i64) -> String {
    format!(
        "{}\n{}\n",
        titled_group_header(title),
        same_second_message(guid, ms)
    )
}

/// The title the account's one conversation ends with.
async fn the_one_group_title(state: &crate::server::AppState) -> Option<String> {
    let mut conn = state.db.acquire().await.unwrap();
    let titles: Vec<Option<String>> = sqlx::query_scalar("SELECT group_title FROM conversations")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    assert_eq!(titles.len(), 1, "the two copies are one conversation");
    titles.into_iter().next().unwrap()
}

/// #1408: two copies of one group, "Old" whose messages end earlier and
/// "New" whose messages end later, end titled "New" whether they arrive in
/// one batch or in two, in either order.
#[tokio::test]
async fn a_merged_group_takes_the_title_of_the_copy_whose_messages_end_later() {
    let old = titled_group_copy(Some("Old"), "m-old", 1_426_183_462_000);
    let new = titled_group_copy(Some("New"), "m-new", 1_426_269_862_000);
    let arrivals = [
        ("one batch, Old first", vec![format!("{old}{new}")]),
        ("one batch, New first", vec![format!("{new}{old}")]),
        ("two batches, Old first", vec![old.clone(), new.clone()]),
        ("two batches, New first", vec![new.clone(), old.clone()]),
    ];
    for (arrival, bodies) in arrivals {
        let (state, _fixture, token) = importer().await;
        post_batches_of_one_run(&state, &token, bodies).await;
        assert_eq!(
            the_one_group_title(&state).await.as_deref(),
            Some("New"),
            "{arrival}"
        );
    }
}

/// #1408: a copy whose messages end later and that has no title keeps the
/// title the group already has, in one batch and in two.
#[tokio::test]
async fn a_later_copy_with_no_title_keeps_the_stored_title() {
    let old = titled_group_copy(Some("Old"), "m-old", 1_426_183_462_000);
    let untitled = titled_group_copy(None, "m-new", 1_426_269_862_000);
    let arrivals = [
        ("one batch, titled first", vec![format!("{old}{untitled}")]),
        (
            "one batch, untitled first",
            vec![format!("{untitled}{old}")],
        ),
        (
            "two batches, titled first",
            vec![old.clone(), untitled.clone()],
        ),
        (
            "two batches, untitled first",
            vec![untitled.clone(), old.clone()],
        ),
    ];
    for (arrival, bodies) in arrivals {
        let (state, _fixture, token) = importer().await;
        post_batches_of_one_run(&state, &token, bodies).await;
        assert_eq!(
            the_one_group_title(&state).await.as_deref(),
            Some("Old"),
            "{arrival}"
        );
    }
}

/// #1408: a later copy titled only with spaces has no title, so it keeps the
/// title the group already has, in one batch and in two.
#[tokio::test]
async fn a_later_copy_titled_with_spaces_keeps_the_stored_title() {
    let old = titled_group_copy(Some("Old"), "m-old", 1_426_183_462_000);
    let blank = titled_group_copy(Some("   "), "m-new", 1_426_269_862_000);
    let arrivals = [
        ("one batch", vec![format!("{old}{blank}")]),
        ("two batches", vec![old.clone(), blank.clone()]),
    ];
    for (arrival, bodies) in arrivals {
        let (state, _fixture, token) = importer().await;
        post_batches_of_one_run(&state, &token, bodies).await;
        assert_eq!(
            the_one_group_title(&state).await.as_deref(),
            Some("Old"),
            "{arrival}"
        );
    }
}

/// #1408: three copies of one group, "Y" ending first, "Z" ending later,
/// and an untitled copy ending last, end titled "Z" in every order, in one
/// batch and in three. The untitled copy's later messages don't stop a newer
/// title from replacing an older one.
#[tokio::test]
async fn an_untitled_copy_ending_last_does_not_keep_an_older_title() {
    let y = titled_group_copy(Some("Y"), "m-y", 1_426_183_462_000);
    let z = titled_group_copy(Some("Z"), "m-z", 1_426_269_862_000);
    let untitled = titled_group_copy(None, "m-x", 1_426_356_262_000);
    let orders = [
        [&y, &z, &untitled],
        [&y, &untitled, &z],
        [&z, &y, &untitled],
        [&z, &untitled, &y],
        [&untitled, &y, &z],
        [&untitled, &z, &y],
    ];
    for (n, order) in orders.iter().enumerate() {
        let one_batch = vec![order.iter().map(|c| c.as_str()).collect::<String>()];
        let three_batches = order.iter().map(|c| (*c).clone()).collect::<Vec<_>>();
        for (arrival, bodies) in [("one batch", one_batch), ("three batches", three_batches)] {
            let (state, _fixture, token) = importer().await;
            post_batches_of_one_run(&state, &token, bodies).await;
            assert_eq!(
                the_one_group_title(&state).await.as_deref(),
                Some("Z"),
                "order {n}, {arrival}"
            );
        }
    }
}

/// #1408: two copies whose messages end at the same time keep the title
/// stored first, in one batch and in two.
#[tokio::test]
async fn copies_whose_messages_end_together_keep_the_stored_title() {
    let first = titled_group_copy(Some("First"), "m-first", 1_426_183_462_000);
    let second = titled_group_copy(Some("Second"), "m-second", 1_426_183_462_000);
    let arrivals = [
        ("one batch", vec![format!("{first}{second}")]),
        ("two batches", vec![first.clone(), second.clone()]),
    ];
    for (arrival, bodies) in arrivals {
        let (state, _fixture, token) = importer().await;
        post_batches_of_one_run(&state, &token, bodies).await;
        assert_eq!(
            the_one_group_title(&state).await.as_deref(),
            Some("First"),
            "{arrival}"
        );
    }
}

/// #1172: a group header that lists one person twice, under two spellings of
/// one number that normalise to one handle, is taken with `200 OK` and the
/// group lists that person once.
#[tokio::test]
async fn a_participant_listed_twice_under_one_identity_is_listed_once() {
    let (state, _fixture, token) = importer().await;
    let header = conversation_header("imessage", "chat1000000172")
        .group()
        .title("Trip")
        .participant("+1 (555) 555-0119", None)
        .participant("5555550119", None);
    let body = format!(
        "{header}\n{}\n",
        same_second_message("m1", 1_426_183_462_000)
    );
    post_batches_of_one_run(&state, &token, vec![body]).await;

    let mut conn = state.db.acquire().await.unwrap();
    let participants: Vec<String> = sqlx::query_scalar(
        "SELECT h.normalized FROM participants p JOIN handles h ON h.id = p.handle_id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        participants,
        ["+15555550119"],
        "the one number is listed once"
    );
}

// --- #1105: an identity is always on a contact, and every participant is a
// contact through an identity ---

/// A text-message group "Trip": Ada at a number, and Sarah Vale, whom the
/// source names with no address.
fn trip_with_a_name_only_member() -> String {
    format!(
        "{}\n{}",
        conversation_header("openextract", "chat2000000001")
            .group()
            .title("Trip")
            .participant("+15555550123", Some("Ada"))
            .participant_without_identity(Some("Sarah Vale")),
        message_line("g-trip-1105", "hi")
            .sms()
            .sender("+15555550123")
            .line(),
    )
}

/// A WhatsApp group that names Sarah Vale with no address too.
fn whatsapp_group_with_a_name_only_member() -> String {
    format!(
        "{}\n{}",
        conversation_header("whatsapp", "120363000000001105@g.us")
            .group()
            .title("Hikes")
            .participant("+15555550124", Some("Bo"))
            .participant_without_identity(Some("Sarah Vale")),
        message_line("g-hikes-1105", "hi")
            .service(IrService::Whatsapp)
            .kind(IrMessageKind::Unknown)
            .sender_display_name("Sarah Vale")
            .line(),
    )
}

async fn contact_named(conn: &mut SqliteConnection, name: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1 AND preferred_name = $2")
        .bind(TEST_ACCOUNT)
        .bind(name)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("no contact named {name:?}: {e}"))
}

/// C2-2: a person has one seat in a conversation. Renaming the contact a
/// name-only participant sits on must not give the next import of the same
/// backup a second seat and a second contact.
#[tokio::test]
async fn c2_2_renaming_a_name_only_contact_then_reimporting_adds_no_seat() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let (first_rows, first_contacts) = participant_and_contact_counts(&mut conn).await;
    // The person renames Sarah Vale's contact.
    sqlx::query(
        "UPDATE contacts SET preferred_name = 'Sarah V.' WHERE account_id = $1 AND preferred_name = 'Sarah Vale'",
    )
    .bind(TEST_ACCOUNT)
    .execute(&mut *conn)
    .await
    .unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let (second_rows, second_contacts) = participant_and_contact_counts(&mut conn).await;
    assert_eq!(
        (second_rows.clone(), second_contacts),
        (first_rows.clone(), first_contacts),
        "seats before {first_rows:?} after {second_rows:?}; contacts {first_contacts} -> {second_contacts}"
    );
    assert_every_person_is_on_a_contact(&mut conn, "a re-import after a rename").await;
}

/// C2-4: a contact the source knows only by name sits in its group, and its
/// totals count that group the way a contact with a number does.
#[tokio::test]
async fn c2_4_a_name_only_contact_counts_its_group() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let sarah = contact_named(&mut conn, "Sarah Vale").await;
    let ada = contact_named(&mut conn, "Ada").await;
    let ada_totals = crate::db::contacts::read::contact_totals(&mut conn, TEST_ACCOUNT, ada)
        .await
        .unwrap();
    let totals = crate::db::contacts::read::contact_totals(&mut conn, TEST_ACCOUNT, sarah)
        .await
        .unwrap();
    assert_eq!(
        totals.groups, 1,
        "Sarah Vale sits in the Trip group but counts {} groups (Ada counts {})",
        totals.groups, ada_totals.groups
    );
}

/// S6-1: search reaches a conversation through the contact its participant's
/// identity is on now, not through the contact the import first gave it.
#[tokio::test]
async fn s6_1_with_follows_an_identity_an_address_book_moved() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_ada_conversation(&mut conn, TEST_ACCOUNT, "+15555550123", "g-s6-1").await;
    let a = contact_named(&mut conn, "Ada").await;
    let conversation: i64 = sqlx::query_scalar("SELECT id FROM conversations")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    // The address book keeps Ada and moves her number to a new contact, Bea.
    crate::db::address_book::load(
        &mut conn,
        TEST_ACCOUNT,
        &format!(
            "contact_id,display_name,groups,service,identity_type,identity\n\
             {a},Ada,,,,\n\
             ,Bea,,phone,phone,+15555550123\n"
        ),
        crate::db::address_book::LoadMode::Append,
    )
    .await
    .unwrap();
    let b = contact_named(&mut conn, "Bea").await;

    use crate::search::ListKind;
    use crate::search::tests::run;
    assert_eq!(
        run(&mut conn, ListKind::Conversations, &format!("with:#{a}")).await,
        Vec::<i64>::new(),
        "Ada no longer holds the number"
    );
    assert_eq!(
        run(&mut conn, ListKind::Conversations, &format!("with:#{b}")).await,
        vec![conversation],
        "Bea holds the number now"
    );
    assert_every_person_is_on_a_contact(&mut conn, "an address book move").await;
}

/// A person the source names with no address is an identity of type `other`
/// holding the name, one on each service. One name on two services is one
/// person, and the run says how many such identities it met.
#[tokio::test]
async fn a_name_with_no_address_is_an_other_identity_on_each_service_and_one_contact() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let texts = import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let whatsapp = import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "whatsapp",
        &whatsapp_group_with_a_name_only_member(),
    )
    .await;
    assert_eq!(texts.other_identities, 1, "{texts:?}");
    assert_eq!(whatsapp.other_identities, 1, "{whatsapp:?}");

    let identities: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT h.service, h.raw, ch.contact_id FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         WHERE h.account_id = $1 AND h.handle_type = 'other' AND h.raw = 'Sarah Vale'
         ORDER BY h.service",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let sarah = contact_named(&mut conn, "Sarah Vale").await;
    assert_eq!(
        identities,
        [
            ("phone".to_string(), "Sarah Vale".to_string(), sarah),
            ("whatsapp".to_string(), "Sarah Vale".to_string(), sarah),
        ]
    );
    let totals = crate::db::contacts::read::contact_totals(&mut conn, TEST_ACCOUNT, sarah)
        .await
        .unwrap();
    assert_eq!(totals.groups, 2, "Trip and Hikes");
    assert_every_person_is_on_a_contact(&mut conn, "two imports naming one person").await;
}

/// A contact whose only identities are of type `other` is a name the import
/// could not tie to an address, so it is Unknown however it is named.
#[tokio::test]
async fn a_contact_with_only_other_identities_is_unknown() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let unknown: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT ct.preferred_name FROM contacts ct
         WHERE ct.account_id = $1 AND {}
         ORDER BY ct.preferred_name",
        crate::db::contacts::UNKNOWN_CONTACT_SQL
    ))
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(unknown, ["Sarah Vale"]);
}

/// ADR-0013 with the rule: when an import discards a trashed contact, the
/// fresh contact takes the identity the import met, and every other
/// identity the trashed contact had that is in a conversation goes to a new
/// contact with no name.
#[tokio::test]
async fn discarding_a_trashed_contact_leaves_none_of_its_identities_on_no_contact() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_ada_conversation(&mut conn, TEST_ACCOUNT, "+15555550123", "g-discard-1").await;
    import_ada_conversation(&mut conn, TEST_ACCOUNT, "ada@example.com", "g-discard-2").await;
    // Both of Ada's addresses are on one contact, which the person trashes.
    let ada: i64 = sqlx::query_scalar(
        "SELECT ch.contact_id FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id
         WHERE h.raw = '+15555550123'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE contact_handles SET contact_id = $1
         WHERE account_id = $2 AND handle_id IN (SELECT id FROM handles WHERE raw = 'ada@example.com')",
    )
    .bind(ada)
    .bind(TEST_ACCOUNT)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("DELETE FROM contacts WHERE account_id = $1 AND id <> $2")
        .bind(TEST_ACCOUNT)
        .bind(ada)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(ada)
        .execute(&mut *conn)
        .await
        .unwrap();

    // The next backup holds only her number.
    import_ada_conversation(&mut conn, TEST_ACCOUNT, "+15555550123", "g-discard-3").await;

    assert_every_person_is_on_a_contact(&mut conn, "an import discarded a trashed contact").await;
    let holders: Vec<(String, String)> = sqlx::query_as(
        "SELECT h.raw, ct.preferred_name FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         JOIN contacts ct ON ct.id = ch.contact_id
         WHERE h.account_id = $1
         ORDER BY h.raw",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        holders,
        [
            ("+15555550123".to_string(), "Ada".to_string()),
            ("ada@example.com".to_string(), String::new()),
        ],
        "the number is on the fresh contact; the address is Unknown again"
    );
}

/// A name-only participant whose contact an import discards keeps its
/// identity, and the identity goes to the fresh contact.
#[tokio::test]
async fn a_name_only_participant_keeps_its_identity_when_its_contact_is_discarded() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let sarah = contact_named(&mut conn, "Sarah Vale").await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(sarah)
        .execute(&mut *conn)
        .await
        .unwrap();
    let (first_rows, _) = participant_and_contact_counts(&mut conn).await;
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let (second_rows, _) = participant_and_contact_counts(&mut conn).await;
    assert_eq!(second_rows, first_rows, "Sarah Vale keeps one seat");
    assert_every_person_is_on_a_contact(&mut conn, "an import discarded a name-only contact").await;
    let fresh = contact_named(&mut conn, "Sarah Vale").await;
    assert_ne!(fresh, sarah, "the trashed contact was replaced");
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

/// A discarded run keeps the Import Errors the desktop app sends with the
/// discard: a run paused and then given up still has the record of what went
/// wrong before it stopped (#1479).
#[tokio::test]
async fn a_discard_records_the_issues_it_carries() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();

    let discarded: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/discard"),
        token,
        serde_json::json!({
            "issues": [{ "kind": "skip", "stage": "media", "item": "IMG_0001.heic", "reason": "convert failed" }],
            "notes": [],
        }),
    )
    .await;

    assert_eq!(discarded["status"], "cancelled", "{discarded}");
    let issues = discarded["issues"].as_array().expect("issues");
    assert_eq!(issues.len(), 1, "{discarded}");
    assert_eq!(issues[0]["kind"], "skip");
    assert_eq!(issues[0]["stage"], "media");
    assert_eq!(issues[0]["item"], "IMG_0001.heic");
    assert_eq!(issues[0]["reason"], "convert failed");
}

/// A completion records the notes the desktop app sends apart from its
/// Import Errors: a note is something the run did that is worth knowing, so
/// the run reads back with it in `notes` and its `issues` stay empty
/// (#1626).
#[tokio::test]
async fn a_completion_records_the_notes_it_carries_apart_from_its_issues() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imazing" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let note = serde_json::json!({
        "stage": "staging",
        "item": "Messages/IMG_0002.jpg",
        "text": "2 rows name this picture; its Live Photo video goes to the first of them in the CSV",
    });

    let completed: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({ "status": "completed", "notes": [note] }),
    )
    .await;

    assert_eq!(completed["status"], "completed", "{completed}");
    assert_eq!(completed["notes"], serde_json::json!([note]), "{completed}");
    assert_eq!(completed["note_count"], 1, "{completed}");
    assert_eq!(completed["issues"], serde_json::json!([]), "{completed}");
    let page: serde_json::Value = get_json(state, "/v1/imports", token).await;
    assert_eq!(page["items"][0]["note_count"], 1, "{page}");
    assert!(page["items"][0].get("notes").is_none(), "{page}");
    let run: serde_json::Value = get_json(state, &format!("/v1/imports/{id}"), token).await;
    assert_eq!(run["notes"], serde_json::json!([note]), "{run}");
}

/// A discarded run keeps the notes the desktop app sends with the discard,
/// as it keeps its Import Errors (#1626).
#[tokio::test]
async fn a_discard_records_the_notes_it_carries() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "sms-backup-plus" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let note = serde_json::json!({
        "stage": "staging",
        "item": "1.eml",
        "text": "This message records no phone number or email address for the other person.",
    });

    let discarded: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/discard"),
        token,
        serde_json::json!({ "issues": [], "notes": [note] }),
    )
    .await;

    assert_eq!(discarded["status"], "cancelled", "{discarded}");
    assert_eq!(discarded["notes"], serde_json::json!([note]), "{discarded}");
}

/// A discard's issues are checked the way a completion's are: a kind that is
/// neither `error` nor `skip` is refused, and the run stays running.
#[tokio::test]
async fn a_discard_with_an_unknown_issue_kind_is_refused() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();

    let (status, text) = crate::test_support::post_raw(
        state,
        &format!("/v1/imports/{id}/discard"),
        token,
        "application/json",
        r#"{"issues":[{"kind":"warning","stage":"staging","item":"a.jsonl","reason":"x"}],"notes":[]}"#,
    )
    .await;

    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    let run: serde_json::Value = get_json(state, &format!("/v1/imports/{id}"), token).await;
    assert_eq!(run["status"], "running", "{run}");
}

/// An Import Run is one record wherever the interface hands one run out:
/// the answer to `complete` and to `discard` and `GET /v1/imports/{id}` are
/// the same JSON, issues included. The run's row in `GET /v1/imports` is the
/// same JSON without the issues, and both count them.
#[tokio::test]
async fn an_import_run_reads_the_same_from_every_route() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();

    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let completed_id = created["id"].as_i64().unwrap();
    let completed: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{completed_id}/complete"),
        token,
        serde_json::json!({
            "status": "completed_with_issues",
            "issues": [{ "kind": "skip", "stage": "staging", "item": "a.jsonl", "reason": "empty" }],
        }),
    )
    .await;
    assert_eq!(completed["issues"][0]["item"], "a.jsonl", "{completed}");

    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let discarded_id = created["id"].as_i64().unwrap();
    let discarded: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{discarded_id}/discard"),
        token,
        serde_json::json!({ "issues": [], "notes": [] }),
    )
    .await;
    assert_eq!(discarded["status"], "cancelled", "{discarded}");

    let page: serde_json::Value = get_json(state, "/v1/imports", token).await;
    for (id, answered) in [(completed_id, &completed), (discarded_id, &discarded)] {
        let got: serde_json::Value = get_json(state, &format!("/v1/imports/{id}"), token).await;
        assert_eq!(&got, answered, "GET /v1/imports/{id}");
        let listed = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|run| run["id"] == id)
            .unwrap_or_else(|| panic!("run {id} is listed: {page}"));
        let mut summary = answered.clone();
        let issues = summary
            .as_object_mut()
            .unwrap()
            .remove("issues")
            .expect("the run carries its issues");
        assert_eq!(summary["issue_count"], issues.as_array().unwrap().len());
        // The list leaves out the notes too, and counts them: one run
        // answers them.
        let notes = summary
            .as_object_mut()
            .unwrap()
            .remove("notes")
            .expect("the run carries its notes");
        assert_eq!(summary["note_count"], notes.as_array().unwrap().len());
        assert_eq!(listed, &summary, "GET /v1/imports, run {id}");
    }
}

/// The SQL form of an Import Run's Contact Group name is the name the run's
/// group is given, before and after the run finishes.
#[tokio::test]
async fn the_sql_form_of_an_import_groups_name_is_the_name_the_group_is_given() {
    let (fixture, account) = fixture_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let import_id = created["id"]
        .as_i64()
        .expect("the created Import Run has an id");
    let mut conn = fixture.state.db.acquire().await.unwrap();
    for finished_at in [None, Some("2031-02-03T04:05:06Z")] {
        sqlx::query("UPDATE imports SET finished_at = $1 WHERE id = $2")
            .bind(finished_at)
            .bind(import_id)
            .execute(&mut *conn)
            .await
            .unwrap();
        let row = crate::db::imports::get_owned_import(&mut conn, account.account_id, import_id)
            .await
            .unwrap();
        let from_sql: String = sqlx::query_scalar(&format!(
            "SELECT {IMPORT_CONTACT_GROUP_NAME_SQL} FROM imports i WHERE i.id = $1"
        ))
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(from_sql, import_contact_group_name(&row), "{finished_at:?}");
    }
}

/// A group header that names the account holder among its members does not
/// make them a participant, on any service. The account holds the number as
/// a Text Message identity, and the exporter did not know it was the
/// holder's, so only the server's check can drop it (#1093).
#[tokio::test]
async fn an_account_identity_listed_among_a_groups_members_is_not_a_participant() {
    let (state, fixture, token) = importer().await;
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let account = format!("/v1/accounts/{account_id}");
    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "identities": [{ "address": "+15555550199", "service": "phone" }] }),
    )
    .await;
    let identities_before: serde_json::Value =
        get_json(&state, &format!("{account}/identities"), &token).await;

    for (source, chat) in [
        ("sms-backup-restore", "trip-sms"),
        ("whatsapp", "trip-whatsapp"),
    ] {
        let path = batches_path(&state, &token, source).await;
        let header = conversation_header(source, chat)
            .group()
            .title("Trip")
            .participant("+15555550101", Some("Ada"))
            .participant("+15555550102", Some("Bob"))
            .participant("+15555550199", Some("Me"));
        let from_ada = message_line(&format!("{chat}-1"), "hi")
            .at(1_400_773_261_000)
            .sms()
            .sender("+15555550101")
            .sender_display_name("Ada");
        // The holder's other device: an incoming message from the holder's
        // own number, which the exporter did not know.
        let from_holder = message_line(&format!("{chat}-2"), "on my way")
            .at(1_400_773_262_000)
            .sms()
            .sender("+15555550199")
            .sender_display_name("Me");
        let body = format!("{header}\n{from_ada}\n{from_holder}\n");
        let (status, text) =
            crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
        assert!(status.is_success(), "{source}: {status} {text}");

        let mut conn = fixture.conn().await;
        let members: Vec<String> = sqlx::query_scalar(
            "SELECT h.normalized FROM participants p
             JOIN conversations c ON c.id = p.conversation_id
             JOIN handles chat ON chat.id = c.chat_handle_id
             JOIN handles h ON h.id = p.handle_id
             WHERE c.account_id = $1 AND chat.normalized = $2
             ORDER BY h.normalized",
        )
        .bind(account_id)
        .bind(chat)
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        assert_eq!(members, ["+15555550101", "+15555550102"], "{source}");

        let complete = path.replace("/batches", "/complete");
        let _: serde_json::Value = post_json(
            &state,
            &complete,
            &token,
            serde_json::json!({ "status": "completed" }),
        )
        .await;
    }

    let holder_contacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND h.normalized = '+15555550199'",
    )
    .bind(account_id)
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap();
    assert_eq!(
        holder_contacts, 0,
        "no contact is made for the holder's address"
    );
    let identities_after: serde_json::Value =
        get_json(&state, &format!("{account}/identities"), &token).await;
    assert_eq!(
        identities_after, identities_before,
        "an import never adds an account identity"
    );
}

/// A one-to-one chat whose identifier is one of the account's identities is a
/// conversation the holder has with themselves: Apple Messages' chat with the
/// owner's own number, WhatsApp's "Message yourself". It makes no contact and
/// no participant; both rows of a note are kept and the received one has no
/// sender. It is titled with the account's display name, or the address
/// without one, and `with:me` finds it and nothing else (#1094).
#[tokio::test]
async fn a_conversation_with_yourself_has_no_participants_and_goes_by_the_accounts_name() {
    let (state, fixture, token) = importer().await;
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let account = format!("/v1/accounts/{account_id}");
    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "identities": [{ "address": "+15555550199", "service": "phone" }] }),
    )
    .await;

    // Apple Messages lists nobody in the chat with the owner's own number;
    // WhatsApp lists the holder's number as the peer of "Message yourself".
    // Each source also carries one ordinary chat, which `with:me` must leave out.
    for (source, self_participant, service) in [
        ("imessage", None, IrService::IMessage),
        ("whatsapp", Some("+15555550199"), IrService::Whatsapp),
    ] {
        let path = batches_path(&state, &token, source).await;
        let message = |guid: &str, sender: Option<&str>, text: &str, reaction: Option<Reaction>| {
            let mut message = message_line(guid, text)
                .at(1_400_773_261_000)
                .service(service)
                .kind(IrMessageKind::Unknown);
            message = match sender {
                Some(sender) => message.sender(sender),
                None => message.outgoing(),
            };
            if let Some(reaction) = reaction {
                message = message.reaction(reaction);
            }
            message.line()
        };
        // The holder's own reaction to the received copy of a note.
        let own_tapback = liked_by("+15555550199");
        let mut self_header = conversation_header(source, "+15555550199");
        // Apple Messages can carry a name the holder gave the chat; the
        // account's name still titles it.
        if source == "imessage" {
            self_header = self_header.title("Notes");
        }
        if let Some(identity) = self_participant {
            self_header = self_header.participant(identity, Some("Me"));
        }
        let ada_header =
            conversation_header(source, "+15555550101").participant("+15555550101", Some("Ada"));
        let body = [
            self_header.line(),
            message(&format!("{source}-note-sent"), None, "Note to self", None),
            message(
                &format!("{source}-note-received"),
                Some("+15555550199"),
                "Note to self",
                Some(own_tapback),
            ),
            ada_header.line(),
            message(&format!("{source}-ada"), Some("+15555550101"), "hi", None),
        ]
        .concat();
        let (status, text) =
            crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
        assert!(status.is_success(), "{source}: {status} {text}");
        let complete = path.replace("/batches", "/complete");
        let _: serde_json::Value = post_json(
            &state,
            &complete,
            &token,
            serde_json::json!({ "status": "completed" }),
        )
        .await;
    }

    let mut conn = fixture.conn().await;
    let holder_contacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND h.normalized = '+15555550199'",
    )
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(holder_contacts, 0, "no contact is made for the holder");
    let self_conversations: Vec<i64> = sqlx::query_scalar(
        "SELECT c.id FROM conversations c JOIN handles h ON h.id = c.chat_handle_id
         WHERE c.account_id = $1 AND h.normalized = '+15555550199' ORDER BY c.id",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(self_conversations.len(), 2, "{self_conversations:?}");
    for &id in &self_conversations {
        let participants: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM participants WHERE conversation_id = $1")
                .bind(id)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(participants, 0, "conversation {id} has no participants");
        let rows: Vec<(i64, Option<i64>)> = sqlx::query_as(
            "SELECT is_from_me, sender_handle_id FROM messages
             WHERE conversation_id = $1 ORDER BY is_from_me",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        assert_eq!(
            rows,
            [(0, None), (1, None)],
            "conversation {id} keeps both rows, and the received one has no sender"
        );
    }
    let tapback_senders: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT t.is_from_me, t.sender_handle_id FROM tapbacks t
         JOIN messages m ON m.id = t.message_id
         WHERE m.conversation_id = $1",
    )
    .bind(self_conversations[0])
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        tapback_senders,
        [(1, None)],
        "the holder's reaction in a conversation with yourself is theirs, with no sender"
    );
    drop(conn);

    let titles = |page: &serde_json::Value| -> Vec<(i64, String)> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["id"].as_i64().unwrap(),
                    c["label"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    };
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=with%3Ame", &token).await;
    let mut found = titles(&page);
    found.sort_unstable();
    assert_eq!(
        found,
        self_conversations
            .iter()
            .map(|&id| (id, "+15555550199".to_string()))
            .collect::<Vec<_>>(),
        "with:me finds the two conversations with yourself, titled by the address: {page}"
    );
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["participants"] == serde_json::json!([])),
        "the holder is not read back as a participant from the chat's own identity: {page}"
    );

    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "preferred_name": "Sam Holder" }),
    )
    .await;
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=with%3Ame", &token).await;
    assert!(
        titles(&page).iter().all(|(_, title)| title == "Sam Holder") && page["total"] == 2,
        "the title follows the account's display name: {page}"
    );
    let page: serde_json::Value = get_json(
        &state,
        "/v1/conversations?q=title%3A%22Sam%20Holder%22",
        &token,
    )
    .await;
    assert_eq!(page["total"], 2, "title: reads the same title: {page}");
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=Holder", &token).await;
    assert_eq!(page["total"], 2, "plain text reads the same title: {page}");
    let page: serde_json::Value =
        get_json(&state, "/v1/messages?q=in%3A%22Sam%20Holder%22", &token).await;
    assert_eq!(page["total"], 4, "in: reads the same title: {page}");

    let page: serde_json::Value = get_json(&state, "/v1/messages?q=with%3Ame", &token).await;
    let messages = page["items"].as_array().unwrap();
    assert_eq!(messages.len(), 4, "{page}");
    for message in messages {
        assert_eq!(message["text"], "Note to self", "{message}");
        assert_eq!(message["conversation"]["label"], "Sam Holder", "{message}");
        assert!(
            message.get("sender").is_none_or(serde_json::Value::is_null),
            "{message}"
        );
    }
}

/// Import one conversation with the holder's number `+15555550199` (a note
/// sent and its received copy, the holder listed by number alone as the
/// chat's participant, as WhatsApp lists them) and one with Ada, and answer the holder's
/// conversation's id.
async fn import_notes_to_yourself(
    state: &crate::server::AppState,
    fixture: &crate::test_support::TestFixture,
    token: &str,
) -> i64 {
    let message = |guid: &str, sender: Option<&str>| {
        let message = message_line(guid, "Note to self")
            .at(1_400_773_261_000)
            .service(IrService::Whatsapp)
            .kind(IrMessageKind::Unknown);
        match sender {
            Some(sender) => message.sender(sender),
            None => message.outgoing(),
        }
        .line()
    };
    let body = [
        conversation_header("whatsapp", "+15555550199")
            .participant("+15555550199", None)
            .line(),
        message("note-sent", None),
        message("note-received", Some("+15555550199")),
        conversation_header("whatsapp", "+15555550101")
            .participant("+15555550101", Some("Ada"))
            .line(),
        message_line("ada", "hi")
            .at(1_400_773_262_000)
            .service(IrService::Whatsapp)
            .sender("+15555550101")
            .line(),
    ]
    .concat();
    import_whatsapp(state, token, body).await;
    sqlx::query_scalar(
        "SELECT c.id FROM conversations c JOIN handles h ON h.id = c.chat_handle_id
         WHERE h.normalized = '+15555550199'",
    )
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// Import `body`, a WhatsApp conversation file, in one completed Import Run.
async fn import_whatsapp(state: &crate::server::AppState, token: &str, body: String) {
    let path = batches_path(state, token, "whatsapp").await;
    let (status, text) =
        crate::test_support::post_raw(state, &path, token, "application/jsonl", body).await;
    assert!(status.is_success(), "{status} {text}");
    let _: serde_json::Value = post_json(
        state,
        &path.replace("/batches", "/complete"),
        token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
}

/// Link (`field` `identities`) or remove (`remove_identities`) the holder's
/// number `+15555550199` on WhatsApp as an identity of the `importer`
/// account.
async fn change_holder_identity(
    state: &crate::server::AppState,
    fixture: &crate::test_support::TestFixture,
    token: &str,
    field: &str,
) {
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let _: serde_json::Value = patch_json(
        state,
        &format!("/v1/accounts/{account_id}"),
        token,
        serde_json::json!({ field: [{ "address": "+15555550199", "service": "whatsapp" }] }),
    )
    .await;
}

/// The contact on the holder's number `+15555550199`, if any.
async fn holder_contact(fixture: &crate::test_support::TestFixture) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT ch.contact_id FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE h.normalized = '+15555550199'",
    )
    .fetch_optional(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// The participants of `conversation_id` as (number, name the backup or the
/// contact gave), by number.
async fn participants_of(
    fixture: &crate::test_support::TestFixture,
    conversation_id: i64,
) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT h.normalized, p.name_alias FROM participants p
         JOIN handles h ON h.id = p.handle_id
         WHERE p.conversation_id = $1
         ORDER BY h.normalized",
    )
    .bind(conversation_id)
    .fetch_all(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// A one-to-one conversation at the holder's number `+15555550199`, its
/// header listing the holder as `holder_name` and, when given, `other` too,
/// with one received note.
fn notes_to_yourself_file(holder_name: Option<&str>, other: Option<&str>) -> String {
    let mut header =
        conversation_header("whatsapp", "+15555550199").participant("+15555550199", holder_name);
    if let Some(other) = other {
        header = header.participant(other, None);
    }
    [
        header.line(),
        message_line("note-received", "Note to self")
            .at(1_400_773_261_000)
            .service(IrService::Whatsapp)
            .kind(IrMessageKind::Unknown)
            .sender("+15555550199")
            .line(),
    ]
    .concat()
}

/// What the account holds about the holder's number: the participants rows
/// of `conversation_id`, and the contacts on `+15555550199`.
async fn holder_rows(
    fixture: &crate::test_support::TestFixture,
    conversation_id: i64,
) -> (i64, i64) {
    let mut conn = fixture.conn().await;
    let participants: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM participants WHERE conversation_id = $1")
            .bind(conversation_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let contacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE h.normalized = '+15555550199'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    (participants, contacts)
}

/// Linking an identity after an import makes the conversations at that
/// address conversations with yourself, and the stored rows follow: the
/// participant goes, and so does the contact the import made for the holder,
/// so the holder is in neither Contacts nor Unknown. The reads agree: `with:me`
/// finds the conversation and it has no participants (#1662).
#[tokio::test]
async fn linking_an_identity_after_an_import_takes_the_holder_off_its_conversations() {
    let (state, fixture, token) = importer().await;
    let conversation_id = import_notes_to_yourself(&state, &fixture, &token).await;
    assert_eq!(
        holder_rows(&fixture, conversation_id).await,
        (1, 1),
        "before the identity is linked, the number is a participant with a contact"
    );
    let unknown: serde_json::Value =
        get_json(&state, "/v1/contacts?q=group%3Aunknown", &token).await;
    assert!(unknown.to_string().contains("+15555550199"), "{unknown}");

    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let _: serde_json::Value = patch_json(
        &state,
        &format!("/v1/accounts/{account_id}"),
        &token,
        serde_json::json!({ "identities": [{ "address": "+15555550199", "service": "whatsapp" }] }),
    )
    .await;

    assert_eq!(
        holder_rows(&fixture, conversation_id).await,
        (0, 0),
        "the conversation with yourself has no participant, and the holder no contact"
    );
    for path in ["/v1/contacts", "/v1/contacts?q=group%3Aunknown"] {
        let page: serde_json::Value = get_json(&state, path, &token).await;
        let text = page.to_string();
        assert!(!text.contains("+15555550199"), "{path}: {page}");
    }
    let contacts: serde_json::Value = get_json(&state, "/v1/contacts", &token).await;
    assert!(
        contacts.to_string().contains("+15555550101"),
        "Ada keeps her contact: {contacts}"
    );
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=with%3Ame", &token).await;
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{page}");
    assert_eq!(items[0]["id"], conversation_id, "{page}");
    assert_eq!(items[0]["participants"], serde_json::json!([]), "{page}");
}

/// Removing an identity after an import makes the conversations at that
/// address ordinary one-to-one conversations again, and the stored rows
/// follow: the chat's address is its participant, on a contact, as an import
/// writes it. The reads agree: `with:me` no longer finds it, and the
/// participant it shows opens that contact (#1662).
#[tokio::test]
async fn removing_an_identity_after_an_import_gives_its_conversations_their_participant_back() {
    let (state, fixture, token) = importer().await;
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let account = format!("/v1/accounts/{account_id}");
    let identity = serde_json::json!([{ "address": "+15555550199", "service": "whatsapp" }]);
    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "identities": identity }),
    )
    .await;
    let conversation_id = import_notes_to_yourself(&state, &fixture, &token).await;
    assert_eq!(
        holder_rows(&fixture, conversation_id).await,
        (0, 0),
        "imported with the identity linked, the conversation is with yourself"
    );

    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "remove_identities": identity }),
    )
    .await;

    assert_eq!(
        holder_rows(&fixture, conversation_id).await,
        (1, 1),
        "the chat's address is its participant again, on a contact"
    );
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=with%3Ame", &token).await;
    assert_eq!(page["total"], 0, "{page}");
    let page: serde_json::Value = get_json(
        &state,
        &format!("/v1/conversations/{conversation_id}"),
        &token,
    )
    .await;
    let participants = page["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 1, "{page}");
    assert_eq!(participants[0]["identity"], "+15555550199", "{page}");
    assert!(participants[0]["contact_id"].is_i64(), "{page}");
    let unknown: serde_json::Value =
        get_json(&state, "/v1/contacts?q=group%3Aunknown", &token).await;
    assert!(unknown.to_string().contains("+15555550199"), "{unknown}");
}

/// The id of the one conversation at `chat`.
async fn conversation_at(fixture: &crate::test_support::TestFixture, chat: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT c.id FROM conversations c JOIN handles h ON h.id = c.chat_handle_id
         WHERE h.normalized = $1",
    )
    .bind(chat)
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// A contact an import named for the holder is kept when the holder's number
/// becomes an identity: an address book load renames a contact without
/// changing its origin, so the name may be the person's, and deleting the
/// contact would lose it. Its participant still goes (#1662).
#[tokio::test]
async fn linking_an_identity_keeps_a_named_holder_contact() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(
        &state,
        &token,
        notes_to_yourself_file(Some("Me (work)"), None),
    )
    .await;
    let conversation_id = conversation_at(&fixture, "+15555550199").await;
    let contact_id = holder_contact(&fixture)
        .await
        .expect("the import made a contact");

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(holder_contact(&fixture).await, Some(contact_id));
    let name: String = sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    assert_eq!(name, "Me (work)");
    assert_eq!(participants_of(&fixture, conversation_id).await, vec![]);
}

/// A holder contact the person put in the Trash is kept, with its Trash
/// entry, when the holder's number becomes an identity (#1662).
#[tokio::test]
async fn linking_an_identity_keeps_a_trashed_holder_contact() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, notes_to_yourself_file(None, None)).await;
    let contact_id = holder_contact(&fixture)
        .await
        .expect("the import made a contact");
    let status = crate::test_support::post_status(
        &state,
        &format!("/v1/contacts/{contact_id}/trash"),
        &token,
        serde_json::json!({}),
    )
    .await;
    assert!(status.is_success(), "{status}");

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(holder_contact(&fixture).await, Some(contact_id));
    let trashed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trashed_contacts WHERE contact_id = $1")
            .bind(contact_id)
            .fetch_one(&mut *fixture.conn().await)
            .await
            .unwrap();
    assert_eq!(trashed, 1);
}

/// Linking the holder's number drops only the participants at an identity,
/// as an import does: another participant the header listed in the same
/// conversation stays, with its contact (#1662).
#[tokio::test]
async fn linking_an_identity_keeps_the_other_participants() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(
        &state,
        &token,
        notes_to_yourself_file(None, Some("+15555550102")),
    )
    .await;
    let conversation_id = conversation_at(&fixture, "+15555550199").await;
    assert_eq!(participants_of(&fixture, conversation_id).await.len(), 2);

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(
        participants_of(&fixture, conversation_id).await,
        vec![("+15555550102".to_string(), None)]
    );
    assert_eq!(holder_contact(&fixture).await, None);
}

/// A group imported before the holder's number was linked lists the holder
/// as a participant. Once the number is an identity the holder is no
/// participant of it, as an import after the link would write it, and the
/// contact the import made for the holder goes (#1093, #1662).
#[tokio::test]
async fn linking_an_identity_takes_the_holder_off_its_groups() {
    let (state, fixture, token) = importer().await;
    let body = [
        conversation_header("whatsapp", "group-1662@g.us")
            .group()
            .title("Family")
            .participant("+15555550199", None)
            .participant("+15555550101", Some("Ada"))
            .line(),
        message_line("group-hi", "hi")
            .at(1_400_773_262_000)
            .service(IrService::Whatsapp)
            .sender("+15555550101")
            .line(),
    ]
    .concat();
    import_whatsapp(&state, &token, body).await;
    let conversation_id: i64 =
        sqlx::query_scalar("SELECT id FROM conversations WHERE group_title = 'Family'")
            .fetch_one(&mut *fixture.conn().await)
            .await
            .unwrap();
    assert_eq!(participants_of(&fixture, conversation_id).await.len(), 2);
    assert!(holder_contact(&fixture).await.is_some());

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(
        participants_of(&fixture, conversation_id).await,
        vec![("+15555550101".to_string(), Some("Ada".to_string()))]
    );
    assert_eq!(holder_contact(&fixture).await, None);
}

/// Linking the holder's number by mistake and removing it again gives the
/// conversation its participant back under the name the backup gave it: the
/// named contact was kept, and the participant takes its name (#1662).
#[tokio::test]
async fn removing_an_identity_linked_by_mistake_keeps_the_backups_name() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(
        &state,
        &token,
        notes_to_yourself_file(Some("Work phone"), None),
    )
    .await;
    let conversation_id = conversation_at(&fixture, "+15555550199").await;
    let contact_id = holder_contact(&fixture)
        .await
        .expect("the import made a contact");
    let before = participants_of(&fixture, conversation_id).await;
    assert_eq!(
        before,
        vec![("+15555550199".to_string(), Some("Work phone".to_string()))]
    );

    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert_eq!(participants_of(&fixture, conversation_id).await, vec![]);
    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert_eq!(participants_of(&fixture, conversation_id).await, before);
    assert_eq!(holder_contact(&fixture).await, Some(contact_id));
}

/// The holder named `holder_name` in the header of a group with Ada and of
/// a one-to-one conversation with Ada, each with one message from Ada whose
/// guid ends in `suffix`.
fn holder_listed_with_ada_file(holder_name: &str, suffix: &str) -> String {
    let message = |guid: &str| {
        message_line(&format!("{guid}-{suffix}"), "hi")
            .at(1_400_773_262_000)
            .service(IrService::Whatsapp)
            .sender("+15555550101")
            .line()
    };
    [
        conversation_header("whatsapp", "group-1662@g.us")
            .group()
            .title("Family")
            .participant("+15555550199", Some(holder_name))
            .participant("+15555550101", Some("Ada"))
            .line(),
        message("group-hi"),
        conversation_header("whatsapp", "+15555550101")
            .participant("+15555550101", Some("Ada"))
            .participant("+15555550199", Some(holder_name))
            .line(),
        message("ada-hi"),
    ]
    .concat()
}

/// The id of the group titled `Family`.
async fn family_group(fixture: &crate::test_support::TestFixture) -> i64 {
    sqlx::query_scalar("SELECT id FROM conversations WHERE group_title = 'Family'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap()
}

/// How many rows the three set-aside tables hold for the holder's number.
async fn set_aside_rows(fixture: &crate::test_support::TestFixture) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM participants_set_aside sa
                 JOIN handles h ON h.id = sa.handle_id WHERE h.normalized = '+15555550199')
              + (SELECT COUNT(*) FROM import_contacts_set_aside sa
                 JOIN handles h ON h.id = sa.handle_id WHERE h.normalized = '+15555550199')
              + (SELECT COUNT(*) FROM contact_group_members_set_aside sa
                 JOIN handles h ON h.id = sa.handle_id WHERE h.normalized = '+15555550199')",
    )
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// Link or remove the holder's number `+15555550199` on `service`.
async fn change_holder_identity_on(
    state: &crate::server::AppState,
    fixture: &crate::test_support::TestFixture,
    token: &str,
    field: &str,
    service: &str,
) {
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let _: serde_json::Value = patch_json(
        state,
        &format!("/v1/accounts/{account_id}"),
        token,
        serde_json::json!({ field: [{ "address": "+15555550199", "service": service }] }),
    )
    .await;
}

/// Linking the holder's number by mistake sets the holder's participant rows
/// aside, in a group and in a one-to-one conversation with someone else
/// alike. Removing it again puts them back with the name the backup gave
/// (#1662).
#[tokio::test]
async fn removing_an_identity_puts_back_the_holder_participants_it_set_aside() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_listed_with_ada_file("Matt", "1")).await;
    let group = family_group(&fixture).await;
    let with_ada = conversation_at(&fixture, "+15555550101").await;
    let expected = vec![
        ("+15555550101".to_string(), Some("Ada".to_string())),
        ("+15555550199".to_string(), Some("Matt".to_string())),
    ];
    assert_eq!(participants_of(&fixture, group).await, expected);
    assert_eq!(participants_of(&fixture, with_ada).await, expected);

    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert_eq!(participants_of(&fixture, group).await, expected[..1]);
    assert_eq!(participants_of(&fixture, with_ada).await, expected[..1]);
    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert_eq!(participants_of(&fixture, group).await, expected);
    assert_eq!(participants_of(&fixture, with_ada).await, expected);
}

/// A participant set aside keeps its identity alive. The holder's number is
/// linked on the phone service, so its WhatsApp row is set aside in the
/// group and stays on the named contact the import made. Taking that row off
/// the contact must not delete it, or its set-aside participant would go
/// with it, and removing the identity would give nothing back (#1662).
#[tokio::test]
async fn taking_a_set_aside_identity_off_its_contact_keeps_it_for_the_unlink() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_listed_with_ada_file("Matt", "1")).await;
    let group = family_group(&fixture).await;
    change_holder_identity_on(&state, &fixture, &token, "identities", "phone").await;
    let contact_id = holder_contact(&fixture)
        .await
        .expect("the named contact stays");
    let _: serde_json::Value = patch_json(
        &state,
        &format!("/v1/contacts/{contact_id}"),
        &token,
        serde_json::json!({
            "remove_identity": { "address": "+15555550199", "service": "whatsapp" }
        }),
    )
    .await;

    change_holder_identity_on(&state, &fixture, &token, "remove_identities", "phone").await;

    assert!(
        participants_of(&fixture, group)
            .await
            .contains(&("+15555550199".to_string(), Some("Matt".to_string())))
    );
}

/// An import while the holder's number is linked writes no participant for
/// the holder, whatever the backup now calls them. Removing the identity
/// then gives back the participant set aside, with the name it had (#1662).
#[tokio::test]
async fn removing_an_identity_keeps_the_set_aside_name_over_a_later_backups() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_listed_with_ada_file("Matt", "1")).await;
    let group = family_group(&fixture).await;
    change_holder_identity(&state, &fixture, &token, "identities").await;
    import_whatsapp(&state, &token, holder_listed_with_ada_file("Matthew", "2")).await;
    assert_eq!(family_group(&fixture).await, group);

    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert!(
        participants_of(&fixture, group)
            .await
            .contains(&("+15555550199".to_string(), Some("Matt".to_string())))
    );
}

/// A group with Ada, where the holder's number `+15555550199` sent a
/// received row but is not in the header.
fn holder_sent_in_a_group_file() -> String {
    let message = |guid: &str, sender: &str| {
        message_line(guid, "hi")
            .at(1_400_773_262_000)
            .service(IrService::Whatsapp)
            .sender(sender)
            .line()
    };
    [
        conversation_header("whatsapp", "group-1662@g.us")
            .group()
            .title("Family")
            .participant("+15555550101", Some("Ada"))
            .line(),
        message("group-ada", "+15555550101"),
        message("group-holder", "+15555550199"),
    ]
    .concat()
}

/// A received row the holder's second number sent in a group is no reason
/// to keep the contact the import made for that number once it is linked:
/// an import after the link gives such a sender no contact (#1093, #1662).
#[tokio::test]
async fn linking_an_identity_deletes_the_contact_of_a_sender_at_it() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_sent_in_a_group_file()).await;
    assert!(holder_contact(&fixture).await.is_some());

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(holder_contact(&fixture).await, None);
    let unknown: serde_json::Value =
        get_json(&state, "/v1/contacts?q=group%3Aunknown", &token).await;
    assert!(!unknown.to_string().contains("+15555550199"), "{unknown}");
}

/// The import run records and the run's Contact Group keep the holder's
/// contact across a link and an unlink: the contact made at the unlink
/// takes the run's record and group membership the deleted one had (#1662).
#[tokio::test]
async fn removing_an_identity_gives_the_new_contact_the_runs_record_back() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_sent_in_a_group_file()).await;
    let record = |contact_id: i64| {
        let fixture = &fixture;
        async move {
            let mut conn = fixture.conn().await;
            let runs: Vec<(i64, String)> = sqlx::query_as(
                "SELECT import_id, reason FROM import_contacts WHERE contact_id = $1",
            )
            .bind(contact_id)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
            let groups: Vec<i64> = sqlx::query_scalar(
                "SELECT gm.group_id FROM contact_group_members gm
                 JOIN contact_groups g ON g.id = gm.group_id
                 WHERE gm.contact_id = $1 AND g.kind = 'import'",
            )
            .bind(contact_id)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
            (runs, groups)
        }
    };
    let before = record(holder_contact(&fixture).await.unwrap()).await;
    assert_eq!(before.0.len(), 1, "{before:?}");
    assert_eq!(before.1.len(), 1, "{before:?}");

    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert_eq!(holder_contact(&fixture).await, None);
    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    let contact_id = holder_contact(&fixture).await.expect("a contact again");
    assert_eq!(record(contact_id).await, before);
}

/// The run records set aside for the holder's number go to the contact the
/// number has when the identity is removed, here Ada's, to which the person
/// added the number while it was linked. None stay behind (#1662).
#[tokio::test]
async fn removing_an_identity_gives_set_aside_records_to_the_contact_it_has() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_sent_in_a_group_file()).await;
    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert!(set_aside_rows(&fixture).await > 0);
    let ada: i64 = sqlx::query_scalar(
        "SELECT ch.contact_id FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id WHERE h.normalized = '+15555550101'",
    )
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap();
    let _: serde_json::Value = patch_json(
        &state,
        &format!("/v1/contacts/{ada}"),
        &token,
        serde_json::json!({
            "add_identity": { "address": "+15555550199", "service": "whatsapp" }
        }),
    )
    .await;

    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert_eq!(holder_contact(&fixture).await, Some(ada));
    assert_eq!(set_aside_rows(&fixture).await, 0);
}

/// The run records set aside for the holder's number are forgotten when the
/// identity is removed and no conversation holds the number any more, so a
/// later link cannot give a new contact a record for an earlier run (#1662).
#[tokio::test]
async fn removing_an_identity_forgets_set_aside_records_nothing_takes() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_sent_in_a_group_file()).await;
    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert!(set_aside_rows(&fixture).await > 0);
    // The group deleted for good while the number was linked.
    let group = family_group(&fixture).await;
    let path = format!("/v1/conversations/{group}");
    let trashed = crate::test_support::post_status(
        &state,
        &format!("{path}/trash"),
        &token,
        serde_json::json!({}),
    )
    .await;
    assert!(trashed.is_success(), "{trashed}");
    let deleted = crate::test_support::delete_status(&state, &path, &token).await;
    assert!(deleted.is_success(), "{deleted}");

    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert_eq!(holder_contact(&fixture).await, None);
    assert_eq!(set_aside_rows(&fixture).await, 0);
}

/// Start an Import Run and complete it with `issues` skips, returning its id.
async fn run_with_issues(state: &crate::server::AppState, token: &str, issues: usize) -> i64 {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "whatsapp" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let issues: Vec<serde_json::Value> = (0..issues)
        .map(|n| {
            serde_json::json!({
                "kind": "skip", "stage": "staging", "item": format!("chat-{n}.txt"), "reason": "empty"
            })
        })
        .collect();
    let _: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({ "status": "completed_with_issues", "issues": issues }),
    )
    .await;
    id
}

/// #1559: a page of the Import Run list carries how many issues each run
/// recorded and none of the issues, however many a run recorded, so its size
/// does not grow with them. `GET /v1/imports/{id}` still answers every one.
#[tokio::test]
async fn the_import_run_list_counts_each_runs_issues_and_carries_none() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let many = run_with_issues(state, token, 600).await;
    let none = run_with_issues(state, token, 0).await;

    for path in [
        "/v1/imports".to_string(),
        format!("/v1/accounts/{}/imports", account.account_id),
    ] {
        let page: serde_json::Value = get_json(state, &path, token).await;
        for (id, count) in [(many, 600), (none, 0)] {
            let listed = page["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|run| run["id"] == id)
                .unwrap_or_else(|| panic!("{path}: run {id} is listed: {page}"));
            assert_eq!(listed["issue_count"], count, "{path}: run {id}");
            assert!(
                listed.get("issues").is_none(),
                "{path}: run {id} carries its issues"
            );
        }
    }

    let run: serde_json::Value = get_json(state, &format!("/v1/imports/{many}"), token).await;
    let issues = run["issues"].as_array().expect("the run's issues");
    assert_eq!(issues.len(), 600);
    assert_eq!(issues[599]["item"], "chat-599.txt");
}

/// #1559: a page of Import Runs is read in the list's own statements, the
/// count and the page, whatever rows the page holds: no statement runs once
/// per row for its issues or its contacts. Both lists shape the rows
/// without the database, the account's and the owner's alike.
#[tokio::test]
async fn a_page_of_import_runs_is_read_without_a_statement_per_row() {
    use sqlx::Connection as _;
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    for issues in [3, 0, 1] {
        run_with_issues(state, token, issues).await;
    }
    let mut conn = state.db.acquire().await.unwrap();
    conn.clear_cached_statements().await.unwrap();

    let query = ListImportsQuery {
        status: None,
        limit: None,
        offset: None,
        sort: None,
    };
    let rows = import_rows_page(&mut conn, account.account_id, query)
        .await
        .unwrap();

    // The connection's statement cache holds each distinct statement once,
    // however often it ran. A read per row is a statement of its own, so it
    // would show here as a third or fourth.
    assert_eq!(
        conn.cached_statements_size(),
        2,
        "the count and the page, and nothing per row"
    );
    let counts: Vec<u64> = rows.items.iter().map(|run| run.issue_count).collect();
    assert_eq!(counts, [1, 0, 3], "newest first");
    let owner: Page<OwnerImportRun> = runs_page(rows);
    assert_eq!(owner.items[2].issue_count, 3);
}

mod backup_dates;
mod changed_content_dedupe;
mod time_precision;
