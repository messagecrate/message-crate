//! Participants: who an import makes a participant and a contact, and
//! who it never does.

use super::*;

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
        import_id: None,
        phone_country: None,
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
            conversation_header("sms_backup_plus", "AMAZON").participant("AMAZON", None),
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
        import_id: None,
        phone_country: None,
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
            import_id: None,
            phone_country: None,
        }),
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap();
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
