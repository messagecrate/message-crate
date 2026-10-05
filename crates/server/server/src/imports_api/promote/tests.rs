use super::*;

const TEST_ACCOUNT: i64 = 7;

/// Import one individual conversation holding one message, `guid`, through
/// the whole pipeline: staging, then promote, in the import's transaction.
async fn import_one_message(conn: &mut SqliteConnection, dir: &std::path::Path, guid: &str) {
    let path = dir.join(format!("{guid}.jsonl"));
    std::fs::write(
        &path,
        format!(
            r#"{{"schema_version":9,"export":{{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_identity":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"+15555550143","conversation_type":"individual","group_title":null,"participants":[{{"identity":"+15555550143","display_name":null}}],"stats":{{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}}}}
{{"guid":"{guid}","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_identity":"+15555550143","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}}
"#
        ),
    )
    .unwrap();
    let assets = dir.join("assets");
    let counts = super::super::import_jsonl_files_on_conn(
        conn,
        &[path],
        &super::super::ImportOptions::fixed(super::super::FixedImportArgs {
            assets_dir: &assets,
            asset_root: dir,
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
        super::super::ImportSchemaMode::Ensure,
    )
    .await
    .unwrap();
    assert_eq!(counts.messages, 1, "the import must insert its message");
}

/// Promotion reads only rows staging accepted, so a promote that fails is
/// the server's fault: the import takes its error as internal, never as a
/// refusal the sender could fix, and the cause survives for the log.
#[tokio::test]
async fn a_promote_that_fails_is_an_internal_import_failure() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_schema(&mut conn).await.unwrap();
    sqlx::raw_sql("DROP TABLE staging_conversations")
        .execute(&mut *conn)
        .await
        .unwrap();

    let err: super::super::ImportError =
        promote_append(&mut conn, ImportMode::Append, TEST_ACCOUNT, false, &[])
            .await
            .expect_err("promote fails")
            .into();

    match err {
        super::super::ImportError::Internal(cause) => assert!(
            format!("{cause:#}").contains("staging_conversations"),
            "{cause:#}"
        ),
        other => panic!("expected an internal failure, got {other:?}"),
    }
}

/// The import runs ANALYZE before it opens its transaction, so promote's
/// guid join has statistics to plan with.
#[tokio::test]
async fn an_import_analyzes_import_tables_before_begin() {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    import_one_message(&mut conn, dir.path(), "analyze-guid-1").await;
    let stat_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_stat1 WHERE tbl IN ('messages', 'attachments', 'tapbacks')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(
        stat_rows >= 1,
        "ANALYZE before BEGIN must write sqlite_stat1 for import tables"
    );
    import_one_message(&mut conn, dir.path(), "analyze-guid-2").await;
}

#[tokio::test]
async fn promote_message_map_ignores_other_accounts() {
    use crate::test_support::{MessageRow, SeedConversation, seed_conversation, test_fixture};

    let fixture = test_fixture().await;
    let other_account = TEST_ACCOUNT + 1;
    for (account_id, message_id) in [(TEST_ACCOUNT, 1), (other_account, 2)] {
        fixture
            .account_with_id(account_id, &format!("user{account_id}"))
            .await;
        let conversation_id = seed_conversation(
            &fixture.state,
            &SeedConversation {
                account_id,
                handle: "+15555550100",
                conversation_type: "individual",
                group_title: None,
                source_file: "t.json",
                messages: &[],
            },
        )
        .await;
        let mut conn = fixture.conn().await;
        MessageRow {
            id: Some(message_id),
            ..MessageRow::new(account_id, conversation_id)
        }
        .insert(&mut conn)
        .await;
    }
    let mut conn = fixture.conn().await;
    let mut promote = Promote {
        tx: &mut conn,
        account_id: TEST_ACCOUNT,
        mode: ImportMode::Append,
        stats: PromoteStats::default(),
        started: Instant::now(),
    };
    let mut map = HashMap::new();

    promote
        .zip_new_message_ids(&mut map, vec![101], 0, |n, p| {
            format!("unexpected mapping counts: staging={n} production={p}")
        })
        .await
        .unwrap();

    assert_eq!(map, HashMap::from([(101, 1)]));
}

/// The line that ends a promotion words each count singular for one and
/// plural for every other count (#1825).
#[test]
fn the_promoted_line_counts_one_and_many() {
    let one = PromoteStats {
        conversations: 1,
        participants: 1,
        messages: 1,
        attachments: 1,
        tapbacks: 1,
        ..PromoteStats::default()
    };
    assert_eq!(
        promoted_line(&one),
        "promoted 1 conversation, 1 participant, 1 message, 1 attachment and 1 tapback"
    );
    let many = PromoteStats {
        conversations: 2,
        participants: 3,
        messages: 0,
        attachments: 4,
        tapbacks: 5,
        ..PromoteStats::default()
    };
    assert_eq!(
        promoted_line(&many),
        "promoted 2 conversations, 3 participants, 0 messages, 4 attachments and 5 tapbacks"
    );
}
