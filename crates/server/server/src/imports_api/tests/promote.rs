//! Promote: what it stamps on each message, and what a replace or a
//! failed import leaves in place.

use super::*;

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
            import_id: Some(import_id),
            phone_country: None,
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
            import_id: None,
            phone_country: None,
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
            import_id: None,
            source_from_jsonl: true,
            media: MediaMode::Clone,
            wipe_sources: Some(vec!["go-sms-pro".into()]),
            phone_country: None,
            progress: Progress::Log,
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
            import_id: None,
            phone_country: None,
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
            import_id: None,
            phone_country: None,
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
        import_id: None,
        phone_country: None,
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
