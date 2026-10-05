use axum::http::StatusCode;

use crate::problem::ProblemType;
use crate::test_support::{
    RegisteredAccount, SeedConversation, SeedMessage, TestFixture, expect_problem,
    fixture_with_account, get_json, get_raw, get_status, register_via_api, seed_conversation,
};

/// Two conversations for alice (a direct thread and a group), and one for bob
/// that must never appear in alice's results.
async fn seeded() -> (TestFixture, RegisteredAccount, i64, i64) {
    let (fixture, alice) = fixture_with_account().await;
    let bob = register_via_api(&fixture.state, "bob", "hunter2hunter2").await;
    let direct = seed_conversation(
        &fixture.state,
        &SeedConversation {
            account_id: alice.account_id,
            handle: "+15555550100",
            conversation_type: "individual",
            group_title: None,
            source_file: "t.json",
            messages: &[
                SeedMessage {
                    source: "imessage",
                    timestamp: "2024-01-01T10:00:00Z",
                    is_from_me: false,
                    body: "dentist on tuesday",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: "2024-01-02T10:00:00Z",
                    is_from_me: true,
                    body: "see you there",
                },
            ],
        },
    )
    .await;
    let group = seed_conversation(
        &fixture.state,
        &SeedConversation {
            account_id: alice.account_id,
            handle: "chat100",
            conversation_type: "group",
            group_title: Some("Family"),
            source_file: "t.json",
            messages: &[SeedMessage {
                source: "imessage",
                timestamp: "2024-02-01T10:00:00Z",
                is_from_me: false,
                body: "the dentist called again",
            }],
        },
    )
    .await;
    // A group's chat id is not a person, so the group's participant is a
    // member of its own.
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let member: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550135', '+15555550135', 'phone', 'phone') RETURNING id",
    )
    .bind(alice.account_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO participants (conversation_id, handle_id) VALUES ($1, $2)")
        .bind(group)
        .bind(member)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);
    seed_conversation(
        &fixture.state,
        &SeedConversation {
            account_id: bob.account_id,
            handle: "+15555550167",
            conversation_type: "individual",
            group_title: None,
            source_file: "t.json",
            messages: &[SeedMessage {
                source: "imessage",
                timestamp: "2024-03-01T10:00:00Z",
                is_from_me: false,
                body: "bob's dentist",
            }],
        },
    )
    .await;
    (fixture, alice, direct, group)
}

#[tokio::test]
async fn the_messages_route_is_a_page_across_every_conversation() {
    let (fixture, alice, _direct, _group) = seeded().await;
    let page: serde_json::Value = get_json(&fixture.state, "/v1/messages", &alice.token).await;
    assert_eq!(page["total"], serde_json::json!(3), "{page}");
    assert_eq!(page["limit"], serde_json::json!(40));
    assert_eq!(page["offset"], serde_json::json!(0));
    assert_eq!(page["items"].as_array().unwrap().len(), 3);
    // `docs/architecture/http-api.md`: a list is {items, total, limit, offset} and nothing else.
    let keys: Vec<&str> = page
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["items", "limit", "offset", "total"]);
}

#[tokio::test]
async fn a_page_across_two_conversations_names_each_conversations_own_participants() {
    let (fixture, alice, direct, group) = seeded().await;
    let page: serde_json::Value = get_json(&fixture.state, "/v1/messages", &alice.token).await;

    let handles: Vec<(i64, &str)> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let conversation = &item["conversation"];
            (
                conversation["id"].as_i64().unwrap(),
                conversation["participants"][0]["identity"]
                    .as_str()
                    .unwrap_or_else(|| panic!("a participant is named: {item}")),
            )
        })
        .collect();

    assert_eq!(
        handles,
        [
            (direct, "+15555550100"),
            (direct, "+15555550100"),
            (group, "+15555550135"),
        ]
    );
}

/// A message's conversation says whether it is a group as the conversation
/// list does, so the browser never reads `conversation_type` to decide it
/// again and the two lists cannot name one conversation two ways.
#[tokio::test]
async fn a_messages_conversation_says_whether_it_is_a_group_as_the_conversation_list_does() {
    let (fixture, alice, direct, group) = seeded().await;
    let messages: serde_json::Value = get_json(&fixture.state, "/v1/messages", &alice.token).await;
    let conversations: serde_json::Value =
        get_json(&fixture.state, "/v1/conversations", &alice.token).await;

    let from_messages = |id: i64| {
        messages["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| &item["conversation"])
            .find(|c| c["id"] == serde_json::json!(id))
            .unwrap_or_else(|| panic!("conversation {id} is on the Messages list: {messages}"))
            ["is_group"]
            .clone()
    };
    let from_conversations = |id: i64| {
        conversations["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == serde_json::json!(id))
            .unwrap_or_else(|| panic!("conversation {id} is listed: {conversations}"))["is_group"]
            .clone()
    };

    assert_eq!(from_messages(direct), serde_json::json!(false));
    assert_eq!(from_messages(group), serde_json::json!(true));
    assert_eq!(from_messages(direct), from_conversations(direct));
    assert_eq!(from_messages(group), from_conversations(group));
}

#[tokio::test]
async fn a_query_narrows_to_matching_messages_and_never_leaks_another_account() {
    let (fixture, alice, _direct, _group) = seeded().await;
    let page: serde_json::Value =
        get_json(&fixture.state, "/v1/messages?q=dentist", &alice.token).await;
    assert_eq!(page["total"], serde_json::json!(2), "{page}");
    let bodies: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert!(bodies.iter().all(|b| b.contains("dentist")), "{bodies:?}");
    assert!(
        !bodies.iter().any(|b| b.contains("bob")),
        "bob's message must not reach alice: {bodies:?}"
    );
}

#[tokio::test]
async fn in_narrows_a_find_to_one_conversation() {
    // The thread's find box composes `in:#id <term>`, so a find reaches every
    // message in that conversation and nothing outside it (#313).
    let (fixture, alice, direct, group) = seeded().await;
    let page: serde_json::Value = get_json(
        &fixture.state,
        &format!("/v1/messages?q=in%3A%23{direct}%20dentist"),
        &alice.token,
    )
    .await;
    assert_eq!(page["total"], serde_json::json!(1), "{page}");
    assert_eq!(page["items"][0]["text"], "dentist on tuesday");
    assert_eq!(
        page["items"][0]["conversation"]["id"],
        serde_json::json!(direct)
    );

    let page: serde_json::Value = get_json(
        &fixture.state,
        &format!("/v1/messages?q=in%3A%23{group}"),
        &alice.token,
    )
    .await;
    assert_eq!(page["total"], serde_json::json!(1), "{page}");
    assert_eq!(page["items"][0]["text"], "the dentist called again");
}

#[tokio::test]
async fn the_route_pages_by_offset_and_reports_the_total() {
    let (fixture, alice, _direct, _group) = seeded().await;
    let page: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?limit=2&offset=2",
        &alice.token,
    )
    .await;
    assert_eq!(page["total"], serde_json::json!(3));
    assert_eq!(page["limit"], serde_json::json!(2));
    assert_eq!(page["offset"], serde_json::json!(2));
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_word_the_messages_list_does_not_have_is_a_422_with_a_sentence() {
    let (fixture, alice, _direct, _group) = seeded().await;
    let (status, text) = get_raw(
        &fixture.state,
        "/v1/messages?q=conversations%3A0",
        &alice.token,
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::SearchQueryInvalid);
    assert!(
        problem.detail.as_deref().unwrap().contains("conversations"),
        "{text}"
    );
}

/// `source:` takes the id an import writes, so `sms`, which once stood for
/// SMS Backup & Restore, is an unknown value like any other (#1116).
#[tokio::test]
async fn source_sms_is_a_422_that_names_the_sources() {
    let (fixture, alice) = fixture_with_account().await;
    let (status, text) = get_raw(&fixture.state, "/v1/messages?q=source%3Asms", &alice.token).await;
    let problem = expect_problem(status, &text, ProblemType::SearchQueryInvalid);
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        problem
            .detail
            .as_deref()
            .unwrap()
            .contains("sms-backup-restore"),
        "{text}"
    );
}

/// Every list checks `sort` before it compiles `q`
/// (`paging::ListRequest::read`), so with both wrong the Messages list
/// reports the sort, as the contact and conversation lists do.
#[tokio::test]
async fn a_bad_sort_is_reported_before_a_bad_query() {
    let (fixture, alice) = fixture_with_account().await;
    let (status, text) = get_raw(
        &fixture.state,
        "/v1/messages?q=conversations%3A0&sort=colour",
        &alice.token,
    )
    .await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
}

/// One IR message line for [`import_reactions_and_flags`].
fn ir_message(
    guid: &str,
    ms: i64,
    text: &str,
    is_sticker: bool,
    reactions: serde_json::Value,
    imessage: serde_json::Value,
) -> String {
    serde_json::json!({
        "guid": guid,
        "timestamp_unix_ms": ms,
        "direction": "incoming",
        "service": "imessage",
        "message_kind": "imessage",
        "sender_identity": "+15555550123",
        "sender_display_name": null,
        "subject": null,
        "text": text,
        "attachments": [{
            "path": format!("attachments/{guid}.png"),
            "original_name": format!("{guid}.png"),
            "mime_type": "image/png",
            "digest_sha256": null,
            "is_sticker": is_sticker,
            "transcription": null,
            "sticker_effect": null,
            "size_bytes": 12,
            "missing_reason": "not_found"
        }],
        "reactions": reactions,
        "imessage": imessage,
        "source": null
    })
    .to_string()
}

/// Import, through the whole pipeline, three messages into `account_id`: a
/// reply carrying a sticker and two reactions, an announcement carrying one
/// reaction, and a plain message with none of these.
async fn import_reactions_and_flags(fixture: &TestFixture, account_id: i64) {
    let header = serde_json::json!({
        "schema_version": 9,
        "export": {"source": "imessage", "tool": "test", "tool_version": "0",
                   "owner_identity": null, "owner_display_name": null},
        "conversation": {
            "chat_identifier": "chat-reactions",
            "conversation_type": "group",
            "group_title": "Reactions",
            "participants": [
                {"identity": "+15555550123", "display_name": null},
                {"identity": "+15555550167", "display_name": null},
                {"identity": "+15555550161", "display_name": null}
            ],
            "stats": {"message_count": 3, "attachment_count": 3,
                      "first_timestamp_unix_ms": 1426183462000_i64,
                      "last_timestamp_unix_ms": 1426183464000_i64}
        }
    });
    let reply = ir_message(
        "g-reply",
        1_426_183_462_000,
        "a reply",
        true,
        serde_json::json!([
            {"kind": "liked", "emoji": null, "part_index": 0,
             "is_from_me": false, "reactor_identity": "+15555550167"},
            {"kind": "emoji", "emoji": "🎉", "part_index": 1,
             "is_from_me": false, "reactor_identity": "+15555550161"}
        ]),
        serde_json::json!({
            "is_reply": true
        }),
    );
    let announcement = ir_message(
        "g-announce",
        1_426_183_463_000,
        "an announcement",
        false,
        serde_json::json!([
            {"kind": "loved", "emoji": null, "part_index": 2,
             "is_from_me": false, "reactor_identity": "+15555550167"}
        ]),
        serde_json::json!({
            "is_reply": false,
            "announcement": "named the conversation Reactions"
        }),
    );
    let plain = ir_message(
        "g-plain",
        1_426_183_464_000,
        "a plain message",
        false,
        serde_json::json!([]),
        serde_json::Value::Null,
    );
    let dir = fixture.dir().join("reactions");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("reactions.jsonl");
    std::fs::write(
        &path,
        format!("{header}\n{reply}\n{announcement}\n{plain}\n"),
    )
    .unwrap();
    let assets = dir.join("assets");
    let mut conn = fixture.conn().await;
    let counts = crate::imports_api::import_jsonl_files_on_conn(
        &mut conn,
        &[path],
        &crate::imports_api::ImportOptions::fixed(crate::imports_api::FixedImportArgs {
            assets_dir: &assets,
            asset_root: &dir,
            mode: crate::imports_api::ImportMode::Append,
            source: "imessage",
            account_id,
            fill_content_keys: false,
            import_id: None,
        }),
        crate::imports_api::ImportSchemaMode::Ensure,
    )
    .await
    .unwrap();
    assert_eq!(counts.messages, 3);
    assert_eq!(counts.tapbacks, 3);
}

/// An Apple Messages conversation file holding a message the owner deleted
/// in Messages, one unsent whole, and one only partly unsent, as the Apple
/// Messages exporter writes them from `chat-db-fixture`'s messages 16 to 18.
const APPLE_MESSAGES_DELETIONS: &str =
    include_str!("../../tests/fixtures/apple-messages-deletions.jsonl");

/// Import [`APPLE_MESSAGES_DELETIONS`] into `account_id` through the whole
/// pipeline.
async fn import_deletions(fixture: &TestFixture, account_id: i64) {
    let counts =
        import_apple_messages_file(fixture, account_id, "deletions", APPLE_MESSAGES_DELETIONS)
            .await;
    assert_eq!(
        counts.messages, 3,
        "a marked message is imported like any other"
    );
}

/// An Apple Messages conversation file holding a message edited twice whose
/// earlier versions alone mention the library and the museum, a message
/// edited once that mentions the library in its final text and its earlier
/// version, and a message never edited.
const APPLE_MESSAGES_EDITS: &str = include_str!("../../tests/fixtures/apple-messages-edits.jsonl");

/// Import [`APPLE_MESSAGES_EDITS`] into `account_id` through the whole
/// pipeline.
async fn import_edits(fixture: &TestFixture, account_id: i64) {
    let counts =
        import_apple_messages_file(fixture, account_id, "edits", APPLE_MESSAGES_EDITS).await;
    assert_eq!(counts.messages, 3);
}

/// Import one Apple Messages conversation file, `contents`, into
/// `account_id` through the whole pipeline, from a directory named `name`.
async fn import_apple_messages_file(
    fixture: &TestFixture,
    account_id: i64,
    name: &str,
    contents: &str,
) -> crate::imports_api::ImportCounts {
    let dir = fixture.dir().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jsonl"));
    std::fs::write(&path, contents).unwrap();
    let assets = dir.join("assets");
    let mut conn = fixture.conn().await;
    crate::imports_api::import_jsonl_files_on_conn(
        &mut conn,
        &[path],
        &crate::imports_api::ImportOptions::fixed(crate::imports_api::FixedImportArgs {
            assets_dir: &assets,
            asset_root: &dir,
            mode: crate::imports_api::ImportMode::Append,
            source: "imessage",
            account_id,
            fill_content_keys: false,
            import_id: None,
        }),
        crate::imports_api::ImportSchemaMode::Ensure,
    )
    .await
    .unwrap()
}

/// The guids of a messages page, in its order.
fn guids(page: &serde_json::Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["guid"].as_str().unwrap().to_string())
        .collect()
}

/// The deleted message imports as `deleted_in_source_app`, the one unsent
/// whole as `unsent`, and the partly unsent one with no mark, and the
/// messages route returns each mark as imported. No mark hides a message.
#[tokio::test]
async fn a_deleted_and_an_unsent_message_are_returned_with_their_mark() {
    let (fixture, alice) = fixture_with_account().await;
    import_deletions(&fixture, alice.account_id).await;

    let page: serde_json::Value =
        get_json(&fixture.state, "/v1/messages?sort=date", &alice.token).await;
    let marks: Vec<(String, serde_json::Value)> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["guid"].as_str().unwrap().to_string(),
                m["deletion"].clone(),
            )
        })
        .collect();
    assert_eq!(
        marks,
        [
            (
                "guid-16".to_string(),
                serde_json::json!("deleted_in_source_app")
            ),
            ("guid-17".to_string(), serde_json::json!("unsent")),
            ("guid-18".to_string(), serde_json::Value::Null),
        ],
        "{page}"
    );
}

/// `deleted:yes` narrows the list to the messages carrying either mark and
/// `deleted:no` to the rest; together they split it.
#[tokio::test]
async fn deleted_yes_and_no_split_the_messages_by_their_mark() {
    let (fixture, alice) = fixture_with_account().await;
    import_deletions(&fixture, alice.account_id).await;

    let marked: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=deleted%3Ayes&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(guids(&marked), ["guid-16", "guid-17"], "{marked}");
    let unmarked: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=deleted%3Ano&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(guids(&unmarked), ["guid-18"], "{unmarked}");
    let negated: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=-deleted%3Ayes&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(guids(&negated), ["guid-18"], "{negated}");
    let found: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=delete&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(
        guids(&found),
        ["guid-16"],
        "a message deleted in the source app is found by its text: {found}"
    );
}

/// The message of a messages page whose guid is `guid`.
fn message_by_guid(page: &serde_json::Value, guid: &str) -> serde_json::Value {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["guid"] == guid)
        .unwrap_or_else(|| panic!("no message {guid:?} in {page}"))
        .clone()
}

/// An edited message imports with each earlier version, its part, text and
/// time, oldest first, and the messages route returns them beside the final
/// text. A message never edited has none.
#[tokio::test]
async fn an_edited_message_is_returned_with_its_earlier_versions_and_their_times() {
    let (fixture, alice) = fixture_with_account().await;
    import_edits(&fixture, alice.account_id).await;

    let page: serde_json::Value =
        get_json(&fixture.state, "/v1/messages?sort=date", &alice.token).await;
    let edited = message_by_guid(&page, "guid-edited-twice");
    assert_eq!(edited["text"], "Meet at the bakery", "{page}");
    assert_eq!(
        edited["earlier_versions"],
        serde_json::json!([
            {"part_index": 0, "text": "Meet at the library",
             "edited_at": "2020-01-06T11:10:00Z", "matched": false},
            {"part_index": 0, "text": "Meet at the museum",
             "edited_at": "2020-01-06T11:10:30Z", "matched": false}
        ]),
        "{page}"
    );
    assert_eq!(edited["matched_earlier_version"], false, "{page}");
    let never = message_by_guid(&page, "guid-never-edited");
    assert_eq!(never["earlier_versions"], serde_json::json!([]), "{page}");
}

/// A word that only an earlier version holds finds the message, which says
/// it matched an earlier version and which one. A word the final text holds
/// finds its message as an ordinary hit, even when an earlier version holds
/// it too.
#[tokio::test]
async fn a_word_only_an_earlier_version_holds_finds_the_message_and_says_which_version() {
    let (fixture, alice) = fixture_with_account().await;
    import_edits(&fixture, alice.account_id).await;

    let museum: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=museum&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(guids(&museum), ["guid-edited-twice"], "{museum}");
    let hit = message_by_guid(&museum, "guid-edited-twice");
    assert_eq!(hit["matched_earlier_version"], true, "{museum}");
    let matched: Vec<bool> = hit["earlier_versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["matched"].as_bool().unwrap())
        .collect();
    assert_eq!(
        matched,
        [false, true],
        "the second version holds it: {museum}"
    );

    let library: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=library&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(
        guids(&library),
        ["guid-edited-twice", "guid-edited-final-match"],
        "{library}"
    );
    let only_earlier = message_by_guid(&library, "guid-edited-twice");
    assert_eq!(only_earlier["matched_earlier_version"], true, "{library}");
    assert_eq!(
        only_earlier["earlier_versions"][0]["matched"], true,
        "{library}"
    );
    assert_eq!(
        only_earlier["earlier_versions"][1]["matched"], false,
        "{library}"
    );
    let final_match = message_by_guid(&library, "guid-edited-final-match");
    assert_eq!(final_match["matched_earlier_version"], false, "{library}");
    assert_eq!(
        final_match["earlier_versions"][0]["matched"], false,
        "{library}"
    );

    // The final text alone: a message found by it is never marked.
    let bakery: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=bakery&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(guids(&bakery), ["guid-edited-twice"], "{bakery}");
    assert_eq!(
        bakery["items"][0]["matched_earlier_version"], false,
        "{bakery}"
    );

    // A negated word an earlier version holds leaves the message out, as
    // the same word in its final text would.
    let not_museum: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=meet%20-museum&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(guids(&not_museum), Vec::<String>::new(), "{not_museum}");
}

/// Importing the same file again adds no earlier version a second time, and
/// a word only an earlier version holds still finds its message once.
#[tokio::test]
async fn importing_an_edited_message_again_keeps_one_copy_of_each_earlier_version() {
    let (fixture, alice) = fixture_with_account().await;
    import_edits(&fixture, alice.account_id).await;
    let again = import_apple_messages_file(
        &fixture,
        alice.account_id,
        "edits-again",
        APPLE_MESSAGES_EDITS,
    )
    .await;
    assert_eq!(again.messages, 0, "every message is already stored");

    let mut conn = fixture.conn().await;
    let versions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message_versions")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(versions, 3);
    let museum: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=museum&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(guids(&museum), ["guid-edited-twice"], "{museum}");
}

/// An append from a later backup, in which the message was edited again,
/// leaves the stored message as it was: its text and its earlier versions
/// stay together, rather than the text it holds turning up as an earlier
/// version beside it.
#[tokio::test]
async fn an_append_from_a_later_backup_keeps_a_stored_messages_versions_with_its_text() {
    let (fixture, alice) = fixture_with_account().await;
    import_edits(&fixture, alice.account_id).await;
    let later = APPLE_MESSAGES_EDITS.replace(
        r#""text":"Meet at the bakery","attachments":[],"edits":["#,
        r#""text":"Meet at the park","attachments":[],"edits":[{"part_index":0,"text":"Meet at the bakery","edited_at_unix_ms":1578309090000},"#,
    );
    assert_ne!(
        later, APPLE_MESSAGES_EDITS,
        "the later backup edits the message again"
    );
    import_apple_messages_file(&fixture, alice.account_id, "edits-later", &later).await;

    let page: serde_json::Value =
        get_json(&fixture.state, "/v1/messages?sort=date", &alice.token).await;
    let edited = message_by_guid(&page, "guid-edited-twice");
    assert_eq!(edited["text"], "Meet at the bakery", "{page}");
    let texts: Vec<&str> = edited["earlier_versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        ["Meet at the library", "Meet at the museum"],
        "{page}"
    );
}

/// Deleting an edited message deletes its earlier versions and their search
/// entries with it, so a word only those versions held finds nothing.
#[tokio::test]
async fn deleting_an_edited_message_removes_its_earlier_versions() {
    let (fixture, alice) = fixture_with_account().await;
    import_edits(&fixture, alice.account_id).await;
    let mut conn = fixture.conn().await;
    let versions = |guid: &'static str| {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM message_versions v JOIN messages m ON m.id = v.message_id
             WHERE m.guid = $1",
        )
        .bind(guid)
    };
    assert_eq!(
        versions("guid-edited-twice")
            .fetch_one(&mut *conn)
            .await
            .unwrap(),
        2
    );

    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    sqlx::query("DELETE FROM messages WHERE guid = 'guid-edited-twice'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message_versions")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(left, 1, "only the other edited message's version is left");
    let indexed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM message_versions_fts WHERE message_versions_fts MATCH 'museum'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(indexed, 0, "the deleted version's search entry is gone");
    let museum: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=museum&sort=date",
        &alice.token,
    )
    .await;
    assert_eq!(guids(&museum), Vec::<String>::new(), "{museum}");
}

/// Reactions and the reply, announcement and sticker flags survive the trip
/// from an import to the messages route, both when set and when not.
#[tokio::test]
async fn reactions_and_message_flags_are_read_back_as_imported() {
    let (fixture, alice) = fixture_with_account().await;
    import_reactions_and_flags(&fixture, alice.account_id).await;

    let page: serde_json::Value = get_json(&fixture.state, "/v1/messages", &alice.token).await;
    let by_guid = |guid: &str| {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["guid"] == guid)
            .unwrap_or_else(|| panic!("no message {guid:?} in {page}"))
            .clone()
    };
    let reply = by_guid("g-reply");
    let announcement = by_guid("g-announce");
    let plain = by_guid("g-plain");

    assert_eq!(
        reply["tapbacks"],
        serde_json::json!([
            {"part_index": 0, "kind": "liked", "emoji": null,
             "is_from_me": false, "sender": "+15555550167"},
            {"part_index": 1, "kind": "emoji", "emoji": "🎉",
             "is_from_me": false, "sender": "+15555550161"}
        ])
    );
    assert_eq!(
        announcement["tapbacks"],
        serde_json::json!([
            {"part_index": 2, "kind": "loved", "emoji": null,
             "is_from_me": false, "sender": "+15555550167"}
        ])
    );
    assert_eq!(plain["tapbacks"], serde_json::json!([]));

    let flags = |m: &serde_json::Value| {
        (
            m["is_reply"].as_bool().unwrap(),
            m["is_announcement"].as_bool().unwrap(),
            // `is_sticker` is left out of the JSON when false.
            m["attachments"][0]["is_sticker"] == true,
        )
    };
    assert_eq!(flags(&reply), (true, false, true), "{reply}");
    assert_eq!(flags(&announcement), (false, true, false), "{announcement}");
    assert_eq!(flags(&plain), (false, false, false), "{plain}");
}

#[tokio::test]
async fn one_message_is_read_by_id_and_only_by_the_account_that_owns_it() {
    let (fixture, alice, _direct, _group) = seeded().await;
    let bob = register_via_api(&fixture.state, "carol", "hunter2hunter2").await;
    let page: serde_json::Value =
        get_json(&fixture.state, "/v1/messages?q=dentist", &alice.token).await;
    let id = page["items"][0]["id"].as_i64().unwrap();
    let text = page["items"][0]["text"].as_str().unwrap().to_string();

    let message: serde_json::Value =
        get_json(&fixture.state, &format!("/v1/messages/{id}"), &alice.token).await;
    assert_eq!(message["id"], serde_json::json!(id));
    assert_eq!(message["text"], serde_json::json!(text));

    assert_eq!(
        get_status(&fixture.state, &format!("/v1/messages/{id}"), &bob.token).await,
        StatusCode::NOT_FOUND,
        "another account's message is absent, not forbidden"
    );
    assert_eq!(
        get_status(&fixture.state, "/v1/messages/999999", &alice.token).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_status(&fixture.state, &format!("/v1/messages/{id}"), "not-a-token").await,
        StatusCode::UNAUTHORIZED
    );
    // An id that is not a number is a problem document like every other
    // failure, not Axum's plain-text rejection.
    let (status, text) = get_raw(&fixture.state, "/v1/messages/abc", &alice.token).await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
}

/// An id names one message, so a lookup by id does not depend on how the
/// message was found: a message in a trashed conversation and a duplicate
/// are each returned, as `GET /v1/messages` lists them with `trashed:yes`
/// and `source:imessage` (#1201).
#[tokio::test]
async fn a_message_in_a_trashed_conversation_or_a_duplicate_is_read_by_id() {
    let (fixture, alice, direct, group) = seeded().await;
    {
        let mut conn = fixture.conn().await;
        let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
        sqlx::query(
            "INSERT INTO trashed_conversations (account_id, conversation_id) VALUES ($1, $2)",
        )
        .bind(alice.account_id)
        .bind(group)
        .execute(&mut *tx)
        .await
        .unwrap();
        // The second message of the direct thread becomes a copy of the first.
        sqlx::query(
            "UPDATE messages SET duplicate_of = (
                 SELECT MIN(id) FROM messages WHERE conversation_id = $1
             )
             WHERE id = (SELECT MAX(id) FROM messages WHERE conversation_id = $1)",
        )
        .bind(direct)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    let trashed: serde_json::Value =
        get_json(&fixture.state, "/v1/messages?q=trashed%3Ayes", &alice.token).await;
    let trashed = &trashed["items"][0];
    assert_eq!(trashed["text"], "the dentist called again", "{trashed}");

    let duplicate: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=source%3Aimessage%20there",
        &alice.token,
    )
    .await;
    let duplicate = &duplicate["items"][0];
    assert_eq!(duplicate["text"], "see you there", "{duplicate}");

    for listed in [trashed, duplicate] {
        let id = listed["id"].as_i64().unwrap();
        let message: serde_json::Value =
            get_json(&fixture.state, &format!("/v1/messages/{id}"), &alice.token).await;
        assert_eq!(&message, listed);
    }
}

/// `date:today` means today on the account's clock, not UTC's. The account
/// is on Kiritimati, 14 hours ahead of UTC, so the local day starts at 10:00
/// UTC the day before. A message half an hour into the local day is today; one
/// half an hour before it is not, whatever day it is in UTC. A message stamped
/// now is today in every zone.
#[tokio::test]
async fn date_today_is_the_day_on_the_accounts_clock() {
    use chrono::TimeZone;

    let (fixture, alice) = fixture_with_account().await;
    let zone = chrono_tz::Pacific::Kiritimati;
    let _: serde_json::Value = crate::test_support::patch_json(
        &fixture.state,
        &format!("/v1/accounts/{}", alice.account_id),
        &alice.token,
        serde_json::json!({ "time_zone": zone.name() }),
    )
    .await;

    let today = chrono::Utc::now().with_timezone(&zone).date_naive();
    let local = |day: chrono::NaiveDate, h: u32, m: u32| {
        zone.from_local_datetime(&day.and_hms_opt(h, m, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    let early_today = local(today, 0, 30);
    let late_yesterday = local(today.pred_opt().unwrap(), 23, 30);
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    seed_conversation(
        &fixture.state,
        &SeedConversation {
            account_id: alice.account_id,
            handle: "+15555550100",
            conversation_type: "individual",
            group_title: None,
            source_file: "t.json",
            messages: &[
                SeedMessage {
                    source: "imessage",
                    timestamp: &late_yesterday,
                    is_from_me: false,
                    body: "late yesterday",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: &early_today,
                    is_from_me: false,
                    body: "early today",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: &now,
                    is_from_me: true,
                    body: "right now",
                },
            ],
        },
    )
    .await;

    let page: serde_json::Value =
        get_json(&fixture.state, "/v1/messages?q=date%3Atoday", &alice.token).await;
    let mut bodies: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    bodies.sort_unstable();
    assert_eq!(bodies, ["early today", "right now"], "{page}");
}

/// Bodies of a page's messages, in the order the page lists them.
fn texts(page: &serde_json::Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect()
}

/// One conversation whose best match for `dentist` is its oldest message, so
/// relevance and date put the matches in different orders.
async fn seeded_for_relevance() -> (TestFixture, RegisteredAccount) {
    let (fixture, alice) = fixture_with_account().await;
    seed_conversation(
        &fixture.state,
        &SeedConversation {
            account_id: alice.account_id,
            handle: "+15555550100",
            conversation_type: "individual",
            group_title: None,
            source_file: "t.json",
            messages: &[
                SeedMessage {
                    source: "imessage",
                    timestamp: "2024-01-01T10:00:00Z",
                    is_from_me: false,
                    body: "dentist dentist dentist",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: "2024-01-02T10:00:00Z",
                    is_from_me: false,
                    body: "after work I will call the office of the dentist about next week",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: "2024-01-03T10:00:00Z",
                    is_from_me: true,
                    body: "the dentist moved it",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: "2024-01-04T10:00:00Z",
                    is_from_me: true,
                    body: "nothing to see",
                },
            ],
        },
    )
    .await;
    (fixture, alice)
}

/// `sort=relevance` puts the best match first, by the full-text index's
/// `bm25()` on the query's free-text words (#313), whatever the dates say.
#[tokio::test]
async fn relevance_puts_the_best_match_first() {
    let (fixture, alice) = seeded_for_relevance().await;
    let page: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=dentist&sort=relevance",
        &alice.token,
    )
    .await;
    assert_eq!(page["total"], serde_json::json!(3), "{page}");
    assert_eq!(
        texts(&page),
        [
            "dentist dentist dentist",
            "the dentist moved it",
            "after work I will call the office of the dentist about next week",
        ]
    );

    // The same matches by date, newest first, for contrast.
    let page: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=dentist&sort=-date",
        &alice.token,
    )
    .await;
    assert_eq!(
        texts(&page),
        [
            "the dentist moved it",
            "after work I will call the office of the dentist about next week",
            "dentist dentist dentist",
        ]
    );
}

/// Relevance pages like every other order: the second page carries on where
/// the first stopped.
#[tokio::test]
async fn relevance_pages_by_offset() {
    let (fixture, alice) = seeded_for_relevance().await;
    let page: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=dentist&sort=relevance&limit=2&offset=2",
        &alice.token,
    )
    .await;
    assert_eq!(page["total"], serde_json::json!(3), "{page}");
    assert_eq!(
        texts(&page),
        ["after work I will call the office of the dentist about next week"]
    );
}

/// Only the words a match must have rank it: a word behind `-` or `not`
/// excludes and says nothing about how well a message matches, and a field
/// word is not free text. With no free-text word left there is nothing to
/// rank by, so the server refuses rather than answering in another order.
#[tokio::test]
async fn relevance_without_free_text_words_is_validation_failed() {
    let (fixture, alice) = seeded_for_relevance().await;
    for q in ["", "-dentist", "date%3A2024", "not%20dentist"] {
        let (status, text) = get_raw(
            &fixture.state,
            &format!("/v1/messages?q={q}&sort=relevance"),
            &alice.token,
        )
        .await;
        expect_problem(status, &text, ProblemType::ValidationFailed);
        assert!(text.contains("free-text"), "{q}: {text}");
    }
}

/// A free-text word inside `or` ranks, and a negated word beside a positive
/// one leaves the positive one ranking.
#[tokio::test]
async fn relevance_ranks_by_the_positive_words_only() {
    let (fixture, alice) = seeded_for_relevance().await;
    let page: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=dentist%20-office&sort=relevance",
        &alice.token,
    )
    .await;
    assert_eq!(
        texts(&page),
        ["dentist dentist dentist", "the dentist moved it"],
        "{page}"
    );
    let page: serde_json::Value = get_json(
        &fixture.state,
        "/v1/messages?q=moved%20or%20dentist&sort=relevance",
        &alice.token,
    )
    .await;
    assert_eq!(page["total"], serde_json::json!(3), "{page}");
    assert_eq!(texts(&page)[0], "the dentist moved it", "{page}");
}

/// Relevance has one direction, best match first: `-relevance` would list the
/// unranked messages first and then the worst match, which nobody asks for.
#[tokio::test]
async fn descending_relevance_is_validation_failed() {
    let (fixture, alice) = seeded_for_relevance().await;
    for sort in ["-relevance", "-relevance,date", "-date,-relevance"] {
        let (status, text) = get_raw(
            &fixture.state,
            &format!("/v1/messages?q=dentist&sort={sort}"),
            &alice.token,
        )
        .await;
        expect_problem(status, &text, ProblemType::ValidationFailed);
        assert!(text.contains("-relevance"), "{sort}: {text}");
    }
}

/// A relevance search asks the full-text index once for the whole search,
/// never once per candidate message (#413). SQLite flattens a plain joined
/// subquery into a per-row `rowid = m.id AND MATCH` lookup, which took 22 s
/// for `the` on the medium Demo Account, so the plan of the statement the
/// route runs must materialize the rank.
#[tokio::test]
async fn a_relevance_search_reads_the_index_once() {
    use crate::db::conversation_messages::{MessageListSort, message_list_page_sql};
    use crate::paging::{Direction, SortKey};

    let (fixture, alice) = fixture_with_account().await;
    let clock = (
        chrono_tz::UTC,
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
    );
    let mut conn = fixture.state.db.acquire().await.unwrap();
    for q in ["the", "from:me the -office", "the or dentist"] {
        let filter = crate::messages_api::message_filter(alice.account_id, q, clock).unwrap();
        let order = [SortKey {
            key: MessageListSort::Relevance,
            direction: Direction::Asc,
        }];
        let (sql, params) = message_list_page_sql(&filter, &order, 40, 0).unwrap();
        let rows = sqlx::Executor::fetch_all(
            &mut *conn,
            crate::db::sql::bind_all(&format!("EXPLAIN QUERY PLAN {sql}"), &params),
        )
        .await
        .unwrap();
        let plan: Vec<String> = rows
            .iter()
            .map(|r| sqlx::Row::get::<String, _>(r, 3))
            .collect();
        assert!(
            !plan
                .iter()
                .any(|d| d.contains("messages_fts") && d.contains("LEFT-JOIN")),
            "{q}: per-row full-text lookup: {plan:?}"
        );
        assert!(
            plan.iter().any(|d| d.starts_with("MATERIALIZE")),
            "{q}: {plan:?}"
        );
    }
}

/// The Messages list names both its keys when a sort is refused.
#[tokio::test]
async fn an_unknown_sort_names_date_and_relevance() {
    let (fixture, alice) = fixture_with_account().await;
    let (status, text) = get_raw(&fixture.state, "/v1/messages?sort=colour", &alice.token).await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
    assert!(text.contains("date, relevance"), "{text}");
}

/// One conversation's messages take no `relevance`: the conversation panel
/// reads a conversation in date order and has no query to rank by.
#[tokio::test]
async fn a_conversations_messages_take_no_relevance() {
    let (fixture, alice, direct, _group) = seeded().await;
    let (status, text) = get_raw(
        &fixture.state,
        &format!("/v1/conversations/{direct}/messages?sort=relevance"),
        &alice.token,
    )
    .await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
}
