//! The search index an import writes: one posting per message, attachment
//! text indexed after promote, and nothing left of a deleted message.

use super::*;

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
        import_id: None,
        phone_country: None,
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
            import_id: None,
            phone_country: None,
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
