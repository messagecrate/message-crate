//! Reactions: each one is stored under the person who reacted, and a
//! removed one leaves nothing.

use super::*;

/// An Apple Messages conversation file in which a friend loves a message
/// and the owner reacts to it with an emoji, as the Apple Messages exporter
/// writes it: the reactions on the message reacted to, and each reaction's
/// own row beside it.
fn apple_messages_reactions() -> String {
    crate::test_support::at_current_schema_version(include_str!(
        "../../../tests/fixtures/apple-messages-reactions.jsonl"
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
    let reaction = |guid: &str, timestamp_ms: i64, text: &str, action: &str| {
        message_line(guid, text)
            .at(timestamp_ms)
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
