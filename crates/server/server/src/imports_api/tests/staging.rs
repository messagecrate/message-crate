//! Staging: chunked writes, copies of one message in one file or across
//! appends, and the marks, attachments and reactions a later copy adds.

use super::*;

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
            import_id: None,
            phone_country: None,
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
            import_id: None,
            phone_country: None,
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

fn chunk_boundary_jsonl() -> String {
    let header = conversation_header("imessage", "+15555550123")
        .participant("+15555550123", None)
        .participant("+15555550167", None);
    let mut lines = vec![header.to_string()];
    for i in 0..56 {
        let guid = format!("g-{i:02}");
        let timestamp_ms = 1_426_183_462_000i64 + i64::from(i) * 1000;
        let mut message = message_line(&guid, &format!("msg {i}"))
            .at(timestamp_ms)
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
        import_id: None,
        phone_country: None,
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
    let options = edit_options(&assets, tmp.path());
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
        import_id: None,
        phone_country: None,
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
        import_id: None,
        phone_country: None,
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
