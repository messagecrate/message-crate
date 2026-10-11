//! Edited messages: which copy's text and earlier versions a message
//! keeps, and the content key and search entries that follow from them.

use super::*;

/// The stored text and content key of the message `g-edit`.
async fn text_and_key(conn: &mut sqlx::SqliteConnection) -> (String, Option<String>) {
    sqlx::query_as("SELECT body, content_key FROM messages WHERE guid = 'g-edit'")
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// One earlier version of `g-edit` with no text, as iMazing records an
/// edit: only that part `part` was edited at `ms`.
fn textless_version(part: u32, ms: i64) -> EarlierVersion {
    EarlierVersion {
        part_index: part,
        text: None,
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

/// A message imported before it was edited takes the later text when an
/// append-mode import carries the edit, and its content key is computed
/// again from that text, the key a first import of the later backup gives
/// it. A key left from the earlier text would let the dedupe join the
/// message to another copy of the text it no longer holds.
#[tokio::test]
async fn append_gives_a_later_edit_to_a_stored_message_with_a_new_content_key() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let options = edit_options(&assets, tmp.path());
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
    let options = edit_options(&assets, tmp.path());
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
    let versions: Vec<Option<String>> =
        sqlx::query_scalar("SELECT text FROM message_versions ORDER BY id")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        versions,
        [
            Some("see you at six".into()),
            Some("see you at seven".into())
        ]
    );
}

/// What a reader sees of the message `g-edit`: its text and content key,
/// its earlier versions, and how many entries of each search index each
/// word finds.
#[derive(Debug, PartialEq)]
struct EditSnapshot {
    text: String,
    content_key: Option<String>,
    /// `(part_index, text, edited_at)`, in the order stored.
    versions: Vec<(i64, Option<String>, Option<String>)>,
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
    let options = edit_options(&assets, tmp.path());
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

/// An edit a source records without the text it replaced, as iMazing does,
/// is stored as a version with no text and its time, and indexed for no
/// word: nothing could find the message by it. The final text is still
/// found (#2030).
#[tokio::test]
async fn a_version_without_text_is_stored_with_its_time_and_not_indexed() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let options = edit_options(&assets, tmp.path());
    let file = edit_file(
        tmp.path(),
        "imazing.jsonl",
        "see you at seven",
        &[textless_version(0, 1426183462000)],
    );
    let db = tmp.path().join("messagecrate.db");
    import_jsonl_files(&db, &[file], &options).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let snapshot = edit_snapshot(&mut conn).await;
    assert_eq!(snapshot.text, "see you at seven");
    assert_eq!(
        snapshot.versions,
        [(0, None, Some("2015-03-12T18:04:22.000Z".into()))]
    );
    assert_eq!(
        snapshot.hits,
        [
            ("six", "messages_fts", 0),
            ("six", "message_versions_fts", 0),
            ("seven", "messages_fts", 1),
            ("seven", "message_versions_fts", 0),
            ("eight", "messages_fts", 0),
            ("eight", "message_versions_fts", 0),
        ]
    );
    let indexed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message_versions_fts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(indexed, 0, "a version with no text has no index row");
}
