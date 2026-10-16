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
            .kind(MessageKind::Tapback)
            .sender("+15555550123")
            .sender_display_name("Bob")
            .imessage(ImessageExtensions {
                associated_guid: Some("g-hi".into()),
                associated_part: Some(0),
                tapback_kind: Some("loved".into()),
                tapback_action: Some(action.into()),
                ..ImessageExtensions::default()
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

/// A 👍 as the iMazing exporter writes it: the emoji alone, with no reactor.
fn thumbs_up_with_no_reactor() -> Reaction {
    Reaction {
        part_index: 0,
        kind: "emoji".into(),
        emoji: Some("👍".into()),
        is_from_me: false,
        reactor_identity: None,
        reactor_display_name: None,
    }
}

/// An iMazing export of one chat, written to `name` under `dir`, holding
/// the message `g-thumbs` with `thumbs` 👍 reactions that name no reactor.
fn imazing_export_with_thumbs(dir: &Path, name: &str, thumbs: usize) -> PathBuf {
    let header = conversation_header("imazing", "+15555550123").participant("+15555550123", None);
    let message = (0..thumbs).fold(
        message_line("g-thumbs", "lunch?")
            .at(1_426_183_463_000)
            .sender("+15555550123"),
        |line, _| line.reaction(thumbs_up_with_no_reactor()),
    );
    write_jsonl(dir, name, &format!("{header}\n{message}\n"))
}

/// Append-mode options for an iMazing import.
fn imazing_append<'a>(assets: &'a Path, root: &'a Path) -> ImportOptions<'a> {
    ImportOptions::fixed(FixedImportArgs {
        assets_dir: assets,
        asset_root: Some(root),
        mode: ImportMode::Append,
        source: "imazing",
        account_id: TEST_ACCOUNT,
        import_id: None,
        phone_country: None,
        now: crate::test_support::fixed_instant(),
    })
}

/// How many 👍 reactions the database `db` holds.
async fn stored_thumbs(db: &Path) -> i64 {
    let (_pool, mut conn) = open_verify(db).await;
    sqlx::query_scalar("SELECT COUNT(*) FROM tapbacks WHERE kind = 'emoji' AND emoji = '👍'")
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// Export A of an iMazing chat holds a message with Bob's 👍, and export
/// B, taken after Ana also reacted 👍, holds it with both. iMazing names
/// neither reactor, so the two reactions are the same row. Importing A and
/// then B used to keep one 👍, because both of B's matched the stored one.
/// The message keeps 👍 ×2, whether B comes in a later import or in the
/// same import as A (#3112).
#[tokio::test]
async fn a_later_imazing_copy_adds_a_second_reaction_with_the_same_emoji() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let options = imazing_append(&assets, tmp.path());
    let a = imazing_export_with_thumbs(tmp.path(), "a.jsonl", 1);
    let b = imazing_export_with_thumbs(tmp.path(), "b.jsonl", 2);

    let apart = tmp.path().join("apart.db");
    for file in [&a, &b] {
        import_jsonl_files(&apart, std::slice::from_ref(file), &options)
            .await
            .unwrap();
    }
    assert_eq!(stored_thumbs(&apart).await, 2, "B imported after A");

    let together = tmp.path().join("together.db");
    import_jsonl_files(&together, &[a, b], &options)
        .await
        .unwrap();
    assert_eq!(stored_thumbs(&together).await, 2, "A and B in one import");
}

/// A copy of a message that holds fewer reactions with no reactor than the
/// message does adds none and removes none: importing export B (👍 ×2),
/// then the earlier export A (👍 ×1), then B again leaves 👍 ×2, and so
/// does one import of B and A (#3112).
#[tokio::test]
async fn a_copy_with_fewer_reactions_with_no_reactor_removes_none() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let options = imazing_append(&assets, tmp.path());
    let a = imazing_export_with_thumbs(tmp.path(), "a.jsonl", 1);
    let b = imazing_export_with_thumbs(tmp.path(), "b.jsonl", 2);

    let apart = tmp.path().join("apart.db");
    for file in [&b, &a, &b] {
        import_jsonl_files(&apart, std::slice::from_ref(file), &options)
            .await
            .unwrap();
    }
    assert_eq!(stored_thumbs(&apart).await, 2, "B, then A, then B again");

    let together = tmp.path().join("together.db");
    import_jsonl_files(&together, &[b, a], &options)
        .await
        .unwrap();
    assert_eq!(stored_thumbs(&together).await, 2, "B and A in one import");
}
