//! Two copies of one message from one source, from two backups of one
//! phone: the copy from the later backup decides the message's mark and
//! text, in one import in either file order and across imports in either
//! order (#1924, #1741, #1804). A file without a backup date keeps the
//! rules for files without one: marks add, and edits compare their times.

use super::*;

/// The earlier backup of the phone: 2026-09-01T10:00:00Z.
const EARLIER_BACKUP: i64 = 1_788_256_800_000;
/// The later backup of the phone: 2026-09-30T18:45:12Z.
const LATER_BACKUP: i64 = 1_790_793_912_000;

/// One backup's copy of the message `g-backup`.
struct Copy<'a> {
    /// When the backup was made, or `None` for a file that does not say.
    backup: Option<i64>,
    text: &'a str,
    versions: &'a [EarlierVersion],
    deletion: Option<Deletion>,
}

/// A conversation file holding `copy` as the one message `g-backup`,
/// written to `name` under `dir`.
fn backup_file(dir: &Path, name: &str, copy: &Copy<'_>) -> PathBuf {
    let header = conversation_header("imessage", "+15555550123").participant("+15555550123", None);
    let header = match copy.backup {
        Some(ms) => header.backup_taken_at(ms),
        None => header,
    };
    let line = copy.versions.iter().cloned().fold(
        message_line("g-backup", copy.text).sender("+15555550123"),
        MessageLine::edit,
    );
    let line = match copy.deletion {
        Some(deletion) => line.deletion(deletion),
        None => line,
    };
    write_jsonl(dir, name, &format!("{header}\n{line}\n"))
}

/// What the server holds of `g-backup`: its text, its mark, its earlier
/// versions as `(part_index, text)`, and its backup's date.
#[derive(Debug, PartialEq)]
struct Held {
    text: String,
    deletion: Option<String>,
    versions: Vec<(i64, String)>,
    backup_taken_at: Option<String>,
}

/// The [`Held`] of `g-backup` in the database at `db`.
async fn held(db: &Path) -> Held {
    let (_pool, mut conn) = open_verify(db).await;
    let (text, deletion, backup_taken_at): (String, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT body, deletion, backup_taken_at FROM messages WHERE guid = 'g-backup'",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let versions = sqlx::query_as(
        "SELECT v.part_index, v.text FROM message_versions v
         JOIN messages m ON m.id = v.message_id
         WHERE m.guid = 'g-backup' ORDER BY v.id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    Held {
        text,
        deletion,
        versions,
        backup_taken_at,
    }
}

/// How many messages a search of the message text for `word` finds in the
/// database at `db`.
async fn text_hits(db: &Path, word: &str) -> i64 {
    fts_hits(db, "messages_fts", word).await
}

/// How many earlier versions a search for `word` finds in the database at
/// `db`. The index keeps them for an Unsent message too.
async fn version_hits(db: &Path, word: &str) -> i64 {
    fts_hits(db, "message_versions_fts", word).await
}

/// How many rows of the search index `table` match `word` in the database
/// at `db`.
async fn fts_hits(db: &Path, table: &str, word: &str) -> i64 {
    let (_pool, mut conn) = open_verify(db).await;
    sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM {table} WHERE {table} MATCH $1"
    ))
    .bind(word)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Import `files` into `db` in one import, appending to what it holds.
async fn import(db: &Path, assets: &Path, root: &Path, files: &[PathBuf]) {
    import_jsonl_files(db, files, &edit_options(assets, root, false))
        .await
        .unwrap();
}

/// One way of importing two files, and what it gave `g-backup`.
struct Imported {
    /// The way, such as `apart-reversed`.
    order: &'static str,
    /// The database the files were imported into.
    db: PathBuf,
    held: Held,
}

/// What `files` give `g-backup`: imported in one import in the order
/// given, in one import in the other order, one import after another in
/// the order given, and one after another in the other order. The four
/// must agree, so each is returned to compare, with its database to look
/// into further.
async fn every_order(tmp: &Path, label: &str, files: [&PathBuf; 2]) -> [Imported; 4] {
    let assets = tmp.join("assets");
    let [a, b] = files;
    let mut out = Vec::new();
    for (order, batches) in [
        ("together", vec![vec![a.clone(), b.clone()]]),
        ("together-reversed", vec![vec![b.clone(), a.clone()]]),
        ("apart", vec![vec![a.clone()], vec![b.clone()]]),
        ("apart-reversed", vec![vec![b.clone()], vec![a.clone()]]),
    ] {
        let db = tmp.join(format!("{label}-{order}.db"));
        for batch in batches {
            import(&db, &assets, tmp, &batch).await;
        }
        let held = held(&db).await;
        out.push(Imported { order, db, held });
    }
    out.try_into()
        .unwrap_or_else(|_| unreachable!("four orders"))
}

/// The stored date of [`LATER_BACKUP`], in the form a timestamp takes.
const LATER_BACKUP_AT: &str = "2026-09-30T18:45:12.000Z";
/// The stored date of [`EARLIER_BACKUP`].
const EARLIER_BACKUP_AT: &str = "2026-09-01T10:00:00.000Z";

/// Backup A marks the message Deleted in the source app; backup B, made
/// later, after the person recovered it, does not. Whichever is imported
/// first, and whether the two arrive in one import or two, the message
/// is unmarked, and swapping the dates keeps the mark.
#[tokio::test]
async fn the_later_backup_decides_the_deletion_mark_in_every_order() {
    let tmp = TempDir::new().unwrap();
    let file = |name: &str, backup: i64, deletion: Option<Deletion>| {
        backup_file(
            tmp.path(),
            name,
            &Copy {
                backup: Some(backup),
                text: "recovered later",
                versions: &[],
                deletion,
            },
        )
    };
    let marked = file(
        "marked-earlier.jsonl",
        EARLIER_BACKUP,
        Some(Deletion::DeletedInSourceApp),
    );
    let recovered = file("recovered-later.jsonl", LATER_BACKUP, None);
    for Imported { held, .. } in every_order(tmp.path(), "recovered", [&marked, &recovered]).await {
        assert_eq!(held.deletion, None, "{held:?}");
        assert_eq!(held.backup_taken_at.as_deref(), Some(LATER_BACKUP_AT));
    }

    let marked = file(
        "marked-later.jsonl",
        LATER_BACKUP,
        Some(Deletion::DeletedInSourceApp),
    );
    let unmarked = file("unmarked-earlier.jsonl", EARLIER_BACKUP, None);
    for Imported { held, .. } in every_order(tmp.path(), "deleted", [&unmarked, &marked]).await {
        assert_eq!(
            held.deletion.as_deref(),
            Some("deleted_in_source_app"),
            "{held:?}"
        );
        assert_eq!(held.backup_taken_at.as_deref(), Some(LATER_BACKUP_AT));
    }
}

/// Backup A marks the message Unsent, so it has no search index row
/// (#1758); backup B, made later, carries it unmarked with the same text.
/// In every order the message is unmarked and a word of its text finds it:
/// a later backup that clears the mark gives the index row back.
#[tokio::test]
async fn a_later_backup_that_clears_an_unsent_mark_makes_the_text_searchable() {
    let tmp = TempDir::new().unwrap();
    let file = |name: &str, backup: i64, deletion: Option<Deletion>| {
        backup_file(
            tmp.path(),
            name,
            &Copy {
                backup: Some(backup),
                text: "zqlighthouse",
                versions: &[],
                deletion,
            },
        )
    };
    let unsent = file(
        "unsent-earlier.jsonl",
        EARLIER_BACKUP,
        Some(Deletion::Unsent),
    );
    let shown = file("shown-later.jsonl", LATER_BACKUP, None);
    for Imported { order, db, held } in every_order(tmp.path(), "unsent", [&unsent, &shown]).await {
        assert_eq!(held.deletion, None, "{order}: {held:?}");
        assert_eq!(
            text_hits(&db, "zqlighthouse").await,
            1,
            "{order}: the text is searchable again"
        );
    }
}

/// Two files with the same backup date, as two reads of one Mac's
/// `chat.db` that Messages did not write between give: the date cannot say which is newer,
/// so the rules for files without one hold, and the mark one of them
/// carries is added in every order rather than lost to the other.
#[tokio::test]
async fn equal_backup_dates_fall_back_to_the_rules_for_files_without_one() {
    let tmp = TempDir::new().unwrap();
    let file = |name: &str, deletion: Option<Deletion>| {
        backup_file(
            tmp.path(),
            name,
            &Copy {
                backup: Some(LATER_BACKUP),
                text: "read twice",
                versions: &[],
                deletion,
            },
        )
    };
    let unmarked = file("unmarked-same.jsonl", None);
    let marked = file("marked-same.jsonl", Some(Deletion::Unsent));
    for Imported { held, .. } in every_order(tmp.path(), "same-date", [&unmarked, &marked]).await {
        assert_eq!(held.deletion.as_deref(), Some("unsent"), "{held:?}");
        assert_eq!(held.backup_taken_at.as_deref(), Some(LATER_BACKUP_AT));
    }
}

/// The scenario of #1804. A two-part message is sent at t0; part 1 is
/// edited at t100 and again at t500, which gives it its current text;
/// backup A is made; part 1 is unsent at t600, which drops its versions
/// and marks the message Unsent; part 0 is edited at t700; backup B is
/// made. A lists part 1's [x@t0, y@t100] and no mark; B lists part 0's
/// [a@t0] and the Unsent mark. B's newest version is older than A's, so
/// the version times say A is later.
///
/// With B the later backup, every order, one import of both files in
/// either order included, holds B's text, versions and mark: search finds
/// neither copy's text, finds B's earlier version, and finds none of A's.
/// With the dates swapped, every order holds A's text and versions with no
/// mark, and search finds A's text. With no dates, the version times
/// decide the text as #1801 left them, and B's mark adds.
#[tokio::test]
async fn the_later_backup_decides_the_text_whatever_the_version_times_say() {
    let tmp = TempDir::new().unwrap();
    let t0 = 1_426_183_462_000;
    let a_versions = [
        edit_version(1, "zqxylo", t0),
        edit_version(1, "zqyarrow", t0 + 100_000),
    ];
    let b_versions = [edit_version(0, "zqharbor", t0)];
    let a = |backup| Copy {
        backup,
        text: "zqharbor zqyonder",
        versions: &a_versions,
        deletion: None,
    };
    let b = |backup| Copy {
        backup,
        text: "zqbeacon",
        versions: &b_versions,
        deletion: Some(Deletion::Unsent),
    };
    let a_held = |deletion: Option<&str>, backup: Option<&str>| Held {
        text: "zqharbor zqyonder".into(),
        deletion: deletion.map(Into::into),
        versions: vec![(1, "zqxylo".into()), (1, "zqyarrow".into())],
        backup_taken_at: backup.map(Into::into),
    };

    let a_earlier = backup_file(tmp.path(), "a-earlier.jsonl", &a(Some(EARLIER_BACKUP)));
    let b_later = backup_file(tmp.path(), "b-later.jsonl", &b(Some(LATER_BACKUP)));
    for Imported { order, db, held } in
        every_order(tmp.path(), "b-later", [&a_earlier, &b_later]).await
    {
        assert_eq!(
            held,
            Held {
                text: "zqbeacon".into(),
                deletion: Some("unsent".into()),
                versions: vec![(0, "zqharbor".into())],
                backup_taken_at: Some(LATER_BACKUP_AT.into()),
            },
            "{order}"
        );
        assert_eq!(text_hits(&db, "zqbeacon").await, 0, "{order}");
        assert_eq!(text_hits(&db, "zqyonder").await, 0, "{order}");
        assert_eq!(version_hits(&db, "zqharbor").await, 1, "{order}");
        assert_eq!(version_hits(&db, "zqxylo").await, 0, "{order}");
        assert_eq!(version_hits(&db, "zqyarrow").await, 0, "{order}");
    }

    let a_later = backup_file(tmp.path(), "a-later.jsonl", &a(Some(LATER_BACKUP)));
    let b_earlier = backup_file(tmp.path(), "b-earlier.jsonl", &b(Some(EARLIER_BACKUP)));
    for Imported { order, db, held } in
        every_order(tmp.path(), "a-later", [&a_later, &b_earlier]).await
    {
        assert_eq!(held, a_held(None, Some(LATER_BACKUP_AT)), "{order}");
        assert_eq!(text_hits(&db, "zqyonder").await, 1, "{order}");
        assert_eq!(text_hits(&db, "zqbeacon").await, 0, "{order}");
    }

    let a_undated = backup_file(tmp.path(), "a-undated.jsonl", &a(None));
    let b_undated = backup_file(tmp.path(), "b-undated.jsonl", &b(None));
    for Imported { order, held, .. } in
        every_order(tmp.path(), "undated", [&a_undated, &b_undated]).await
    {
        assert_eq!(held, a_held(Some("unsent"), None), "{order}");
    }
}

/// An append of the older backup after the newer one changes nothing:
/// not the mark, not the text, not the earlier versions, not the date,
/// even though the older copy carries a mark and lists more versions.
#[tokio::test]
async fn an_append_of_the_older_backup_after_the_newer_changes_nothing() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let db = tmp.path().join("messagecrate.db");
    let t0 = 1_426_183_462_000;
    let newer = backup_file(
        tmp.path(),
        "newer.jsonl",
        &Copy {
            backup: Some(LATER_BACKUP),
            text: "see you at seven",
            versions: &[edit_version(0, "see you at six", t0)],
            deletion: None,
        },
    );
    let older = backup_file(
        tmp.path(),
        "older.jsonl",
        &Copy {
            backup: Some(EARLIER_BACKUP),
            text: "see you at eight",
            versions: &[
                edit_version(0, "see you at six", t0),
                edit_version(0, "see you at seven", t0 + 60_000),
            ],
            deletion: Some(Deletion::Unsent),
        },
    );
    import(&db, &assets, tmp.path(), std::slice::from_ref(&newer)).await;
    let before = held(&db).await;
    assert_eq!(before.text, "see you at seven");
    import(&db, &assets, tmp.path(), &[older]).await;
    assert_eq!(held(&db).await, before);
}

/// Where either file has no backup date, the rules for files without one
/// hold. In one import, a second file's mark is added and its edit is
/// taken when its versions are newer; across imports, a file without the
/// mark leaves it, and a stored message from an undated file keeps no date
/// when a dated file later gives it nothing.
#[tokio::test]
async fn without_a_backup_date_marks_add_and_edits_compare_their_times() {
    let tmp = TempDir::new().unwrap();
    let t0 = 1_426_183_462_000;
    let unmarked_undated = backup_file(
        tmp.path(),
        "unmarked-undated.jsonl",
        &Copy {
            backup: None,
            text: "see you at seven",
            versions: &[edit_version(0, "see you at six", t0)],
            deletion: None,
        },
    );
    let marked_dated = backup_file(
        tmp.path(),
        "marked-dated.jsonl",
        &Copy {
            backup: Some(EARLIER_BACKUP),
            text: "see you at six",
            versions: &[],
            deletion: Some(Deletion::Unsent),
        },
    );
    // Undated against dated: the mark adds, and the copy with the later edit
    // keeps its text, whichever is the dated one.
    for Imported { held, .. } in
        every_order(tmp.path(), "mixed", [&unmarked_undated, &marked_dated]).await
    {
        assert_eq!(held.deletion.as_deref(), Some("unsent"), "{held:?}");
        assert_eq!(held.text, "see you at seven", "{held:?}");
        assert_eq!(held.versions, vec![(0, "see you at six".into())]);
    }

    // Both undated, the mark in the second file of one import: the mark
    // adds rather than being lost to the copy staged first.
    let marked_undated = backup_file(
        tmp.path(),
        "marked-undated.jsonl",
        &Copy {
            backup: None,
            text: "see you at seven",
            versions: &[edit_version(0, "see you at six", t0)],
            deletion: Some(Deletion::DeletedInSourceApp),
        },
    );
    for Imported { held, .. } in
        every_order(tmp.path(), "undated", [&unmarked_undated, &marked_undated]).await
    {
        assert_eq!(
            held.deletion.as_deref(),
            Some("deleted_in_source_app"),
            "{held:?}"
        );
        assert_eq!(held.backup_taken_at, None);
    }
}

/// One import holds a dated file and an undated one: the staged message
/// keeps the dated file's date in either file order, so a later import
/// compares against it. Import 1 holds backup A, dated earlier with no
/// mark, and a file without a date that marks the message Unsent, so the
/// mark adds. Import 2 is backup C, made later, with no mark: it clears
/// the mark whichever file import 1 read first.
#[tokio::test]
async fn one_import_of_a_dated_and_an_undated_file_keeps_the_date_in_either_order() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let file = |name: &str, backup: Option<i64>, deletion: Option<Deletion>| {
        backup_file(
            tmp.path(),
            name,
            &Copy {
                backup,
                text: "back again",
                versions: &[],
                deletion,
            },
        )
    };
    let dated = file("a-earlier.jsonl", Some(EARLIER_BACKUP), None);
    let undated = file("b-undated.jsonl", None, Some(Deletion::Unsent));
    let later = file("c-later.jsonl", Some(LATER_BACKUP), None);
    for (name, first) in [
        ("dated-first.db", [dated.clone(), undated.clone()]),
        ("undated-first.db", [undated.clone(), dated.clone()]),
    ] {
        let db = tmp.path().join(name);
        import(&db, &assets, tmp.path(), &first).await;
        let held_first = held(&db).await;
        assert_eq!(held_first.deletion.as_deref(), Some("unsent"), "{name}");
        assert_eq!(
            held_first.backup_taken_at.as_deref(),
            Some(EARLIER_BACKUP_AT),
            "{name}"
        );
        import(&db, &assets, tmp.path(), std::slice::from_ref(&later)).await;
        let held_after = held(&db).await;
        assert_eq!(held_after.deletion, None, "{name}");
        assert_eq!(
            held_after.backup_taken_at.as_deref(),
            Some(LATER_BACKUP_AT),
            "{name}"
        );
    }
}

/// Two backups in one import give the message the later backup's text, and
/// so the later backup's duplicate flag: with another source holding the
/// later text, the dedupe hides one of the two, in either file order, and
/// with the earlier text winning it would hide neither. The earlier backup's
/// newest earlier version is the newer one, so the version times alone pick
/// the earlier text.
#[tokio::test]
async fn the_later_backup_decides_the_duplicate_flag() {
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    let t0 = 1_426_183_462_000;
    let earlier = backup_file(
        tmp.path(),
        "earlier.jsonl",
        &Copy {
            backup: Some(EARLIER_BACKUP),
            text: "see you at six",
            versions: &[edit_version(1, "bring cake", t0 + 100_000)],
            deletion: None,
        },
    );
    let later = backup_file(
        tmp.path(),
        "later.jsonl",
        &Copy {
            backup: Some(LATER_BACKUP),
            text: "see you at seven",
            versions: &[edit_version(0, "see you at six", t0)],
            deletion: None,
        },
    );
    let header =
        conversation_header("sms-backup-restore", "+15555550123").participant("+15555550123", None);
    let sms = write_jsonl(
        tmp.path(),
        "sms.jsonl",
        &format!(
            "{header}\n{}\n",
            message_line("g-sms", "see you at seven")
                .sender("+15555550123")
                .sms()
        ),
    );
    for (name, files) in [
        ("earlier-first.db", [earlier.clone(), later.clone()]),
        ("later-first.db", [later.clone(), earlier.clone()]),
    ] {
        let db = tmp.path().join(name);
        let options = edit_options(&assets, tmp.path(), true);
        import_jsonl_files(&db, &files, &options).await.unwrap();
        let sms_options = ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: true,
            import_id: None,
        });
        import_jsonl_files(&db, std::slice::from_ref(&sms), &sms_options)
            .await
            .unwrap();
        let (_pool, mut conn) = open_verify(&db).await;
        crate::dedupe::dedupe_cross_source(&mut conn, TEST_ACCOUNT, None, 2)
            .await
            .unwrap();
        let hidden: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE duplicate_of IS NOT NULL")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(
            hidden, 1,
            "{name}: the two copies of the later text are one"
        );
    }
}

/// The Import Run says when the backup it read was made: the newest date
/// its files name, as `backup_taken_at`, and null for a run whose files
/// name none.
#[tokio::test]
async fn the_import_run_says_when_its_backup_was_made() {
    let (state, _fixture, token) = importer().await;
    let batch = |backup: Option<i64>, guid: &str| {
        let header =
            conversation_header("imessage", "+15555550123").participant("+15555550123", None);
        let header = match backup {
            Some(ms) => header.backup_taken_at(ms),
            None => header,
        };
        format!(
            "{header}\n{}\n",
            message_line(guid, "hello").sender("+15555550123")
        )
    };
    let run = |body: Vec<String>| {
        let state = state.clone();
        let token = token.clone();
        async move {
            let (_, created): (String, serde_json::Value) = post_created_json(
                &state,
                "/v1/imports",
                &token,
                serde_json::json!({ "source": "imessage", "mode": "append" }),
            )
            .await;
            let id = created["id"].as_i64().unwrap();
            assert_eq!(created["backup_taken_at"], serde_json::Value::Null);
            for body in body {
                let (status, text) = crate::test_support::post_raw(
                    &state,
                    &format!("/v1/imports/{id}/batches"),
                    &token,
                    "application/jsonl",
                    body,
                )
                .await;
                assert_eq!(status, axum::http::StatusCode::OK, "{text}");
            }
            let read: serde_json::Value =
                get_json(&state, &format!("/v1/imports/{id}"), &token).await;
            let _: serde_json::Value = post_json(
                &state,
                &format!("/v1/imports/{id}/complete"),
                &token,
                serde_json::json!({ "status": "completed" }),
            )
            .await;
            read["backup_taken_at"].clone()
        }
    };
    assert_eq!(
        run(vec![
            batch(Some(LATER_BACKUP), "g-1"),
            batch(Some(EARLIER_BACKUP), "g-2"),
            batch(None, "g-3"),
        ])
        .await,
        serde_json::json!(LATER_BACKUP_AT)
    );
    assert_eq!(
        run(vec![batch(Some(EARLIER_BACKUP), "g-4")]).await,
        serde_json::json!(EARLIER_BACKUP_AT)
    );
    assert_eq!(run(vec![batch(None, "g-5")]).await, serde_json::Value::Null);
}

/// A message read back through the API carries the date of the backup that
/// decided it, which an Export Run writes back into the conversation file.
#[tokio::test]
async fn a_message_read_back_carries_its_backup_date() {
    let (state, _fixture, token) = importer().await;
    let header = conversation_header("imessage", "+15555550123")
        .participant("+15555550123", None)
        .backup_taken_at(LATER_BACKUP);
    import_one_batch(
        &state,
        &token,
        "imessage",
        "append",
        format!(
            "{header}\n{}\n",
            message_line("g-read", "hello").sender("+15555550123")
        ),
    )
    .await;
    let page: serde_json::Value = get_json(&state, "/v1/messages", &token).await;
    assert_eq!(
        page["items"][0]["backup_taken_at"],
        serde_json::json!(LATER_BACKUP_AT)
    );
}
