use super::*;
use crate::db::schema;

const TEST_ACCOUNT: i64 = 7;

/// A database with the schema and one account, and a connection to it. The
/// pool and the directory are returned so they outlive the test body.
async fn account() -> (
    sqlx::pool::PoolConnection<sqlx::Sqlite>,
    sqlx::SqlitePool,
    tempfile::TempDir,
) {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    (conn, pool, dir)
}

#[tokio::test]
async fn an_import_creates_the_contact_with_the_backup_name() {
    let (mut conn, _pool, _dir) = account().await;
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550153', '+15555550153', 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let mut counts = ImportCounts::default();
    let contact_id = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        handle_id,
        Some("Ada"),
        &mut counts,
    )
    .await
    .unwrap();

    let (name, origin): (String, String) =
        sqlx::query_as("SELECT preferred_name, origin FROM contacts WHERE id = $1")
            .bind(contact_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(name, "Ada");
    assert_eq!(origin, "import");
}

#[tokio::test]
async fn a_later_backup_names_a_contact_an_earlier_one_left_nameless() {
    let (mut conn, _pool, _dir) = account().await;
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550160', '+15555550160', 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let mut counts = ImportCounts::default();
    let first =
        ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, None, handle_id, None, &mut counts)
            .await
            .unwrap();
    let second = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        handle_id,
        Some("Ada"),
        &mut counts,
    )
    .await
    .unwrap();
    assert_eq!(first, second, "the same handle keeps the same contact");

    let name = name_of(&mut conn, first).await;
    assert_eq!(name, "Ada");
}

/// A trashed contact stays set aside until an import meets one of its
/// handles. Then the backup wins (ADR-0013): the trashed contact goes, name,
/// Contact Group memberships and every handle link with it, and the import
/// makes a fresh contact carrying only what the backup said. A handle the
/// trashed contact had that the backup did not mention belongs to nobody
/// afterwards.
#[tokio::test]
async fn an_import_replaces_a_trashed_contact_with_a_fresh_one() {
    let (mut conn, _pool, _dir) = account().await;
    let met = insert_handle(&mut conn, "+15555550163", "imessage").await;
    let unmentioned = insert_handle(&mut conn, "+15555550164", "imessage").await;
    let mut counts = ImportCounts::default();
    let old =
        ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, None, met, Some("Ada"), &mut counts)
            .await
            .unwrap();
    // The person then curated the contact: a second handle, a name of their
    // own, a group. None of it survives the trash.
    crate::db::contacts::link_handle_to_contact(
        &mut conn,
        TEST_ACCOUNT,
        unmentioned,
        old,
        crate::db::contacts::Origin::User,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE contacts SET preferred_name = 'Ada (work)', origin = 'user' WHERE id = $1")
        .bind(old)
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
        .bind(old)
        .bind(group_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(old)
        .execute(&mut *conn)
        .await
        .unwrap();

    // Discarding the trashed contact reaches `messages` through a foreign
    // key, so this runs in a write transaction, as it does inside an import.
    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    let fresh = ensure_contact_for_handle(
        &mut tx,
        TEST_ACCOUNT,
        None,
        met,
        Some("Ada Lovelace"),
        &mut counts,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // SQLite may hand the fresh row the id the deleted one had, so the ids
    // say nothing; what the row holds does.
    assert_eq!(counts.contacts_created, 2, "a fresh contact was created");
    let contacts: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, preferred_name, origin FROM contacts WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        contacts,
        vec![(fresh, "Ada Lovelace".to_string(), "import".to_string())],
        "only the fresh contact remains, named by the backup"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1",
            TEST_ACCOUNT
        )
        .await,
        0,
        "the trash marker goes with the contact"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM contact_group_members WHERE contact_id = $1",
            old
        )
        .await,
        0,
        "and so do the group memberships"
    );
    assert_eq!(
        crate::db::contacts::contact_id_for_handle(&mut conn, TEST_ACCOUNT, met)
            .await
            .unwrap(),
        Some(fresh)
    );
    assert_eq!(
        crate::db::contacts::contact_id_for_handle(&mut conn, TEST_ACCOUNT, unmentioned)
            .await
            .unwrap(),
        None,
        "a handle the backup did not mention belongs to nobody"
    );
}

/// One number is one person on every service. When the fresh contact is
/// made, the same number on another service — a sibling handle the trashed
/// contact had — comes along, rather than turning Unknown.
#[tokio::test]
async fn a_fresh_contact_takes_the_number_on_every_service() {
    let (mut conn, _pool, _dir) = account().await;
    let on_whatsapp = insert_handle(&mut conn, "+15555550165", "whatsapp").await;
    let on_phone = insert_handle(&mut conn, "+15555550165", "phone").await;
    let mut counts = ImportCounts::default();
    let old = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        on_whatsapp,
        Some("Ada"),
        &mut counts,
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(old)
        .execute(&mut *conn)
        .await
        .unwrap();

    let fresh = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        on_phone,
        Some("Ada"),
        &mut counts,
    )
    .await
    .unwrap();

    assert_eq!(counts.contacts_created, 2, "a fresh contact was created");
    for handle in [on_whatsapp, on_phone] {
        assert_eq!(
            crate::db::contacts::contact_id_for_handle(&mut conn, TEST_ACCOUNT, handle)
                .await
                .unwrap(),
            Some(fresh),
            "both services' handles are on the fresh contact"
        );
    }
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM contacts WHERE account_id = $1",
            TEST_ACCOUNT
        )
        .await,
        1
    );
}

/// A conversation header line. `participants` is the JSON array body.
fn header(chat: &str, kind: &str, participants: &str) -> String {
    let version = message_ir::SCHEMA_VERSION;
    format!(
        r#"{{"schema_version":{version},"export":{{"source":"imessage","tool":"test","tool_version":"0","owner_identity":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"{chat}","conversation_type":"{kind}","group_title":null,"participants":[{participants}],"stats":{{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}}}}"#
    )
}

/// An incoming message line from `sender`.
fn incoming(guid: &str, sender: &str) -> String {
    format!(
        r#"{{"guid":"{guid}","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_identity":"{sender}","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}}"#
    )
}

/// An outgoing message line: one the account holder sent.
fn outgoing(guid: &str) -> String {
    format!(
        r#"{{"guid":"{guid}","timestamp_unix_ms":1426183462000,"direction":"outgoing","service":"imessage","message_kind":"imessage","sender_identity":null,"sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}}"#
    )
}

/// One participant entry: `identity`, named `name`.
fn person(identity: &str, name: &str) -> String {
    format!(r#"{{"identity":"{identity}","display_name":"{name}"}}"#)
}

/// The orphaned messages one sender sent, as the Apple Messages reader
/// writes them: a conversation of type `orphaned`, keyed `orphaned:` and the
/// sender's address, with the sender as its only participant.
fn orphaned_from(sender: &str, name: &str, guid: &str) -> String {
    header(
        &format!("orphaned:{sender}"),
        "orphaned",
        &person(sender, name),
    ) + "\n"
        + &incoming(guid, sender)
        + "\n"
}

/// The orphaned messages the account holder sent, which record no
/// recipient: one conversation of type `orphaned`, keyed `orphaned:`, with no
/// participants.
fn unknown_recipient(guids: &[&str]) -> String {
    let mut out = header("orphaned:", "orphaned", "") + "\n";
    for guid in guids {
        out += &outgoing(guid);
        out += "\n";
    }
    out
}

/// A backup holding Ada's one-to-one conversation and orphaned messages:
/// one from Ada, one from Bob, and two the account holder sent.
fn backup_with_orphaned_messages() -> String {
    header("+15555550154", "individual", &person("+15555550154", "Ada"))
        + "\n"
        + &incoming("g-ada", "+15555550154")
        + "\n"
        + &orphaned_from("+15555550154", "Ada", "g-ada-orphaned")
        + &orphaned_from("+15555550155", "Bob", "g-bob-orphaned")
        + &unknown_recipient(&["g-mine-1", "g-mine-2"])
}

/// What [`backup_with_orphaned_messages`] imports as, by either path: Ada's
/// one-to-one conversation as it was, and three orphaned conversations, one
/// for each sender and one for the holder's, which `kind:orphaned` lists and
/// `kind:direct` and `kind:group` do not. Only Ada and Bob are people: no
/// conversation key gets a contact.
async fn assert_imported_as_three_orphaned_conversations(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
) {
    let conversations: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT h.raw, c.conversation_type,
                (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id)
         FROM conversations c JOIN handles h ON h.id = c.chat_handle_id
         WHERE c.account_id = $1
         ORDER BY h.raw",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let expected = [
        ("+15555550154", "individual", 1),
        ("orphaned:", "orphaned", 2),
        ("orphaned:+15555550154", "orphaned", 1),
        ("orphaned:+15555550155", "orphaned", 1),
    ]
    .map(|(chat, kind, n)| (chat.to_string(), kind.to_string(), n));
    assert_eq!(conversations, expected);

    let linked: Vec<String> = sqlx::query_scalar(
        "SELECT h.raw FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1
         ORDER BY h.raw",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        linked,
        ["+15555550154", "+15555550155"],
        "only the senders are people"
    );

    assert_eq!(
        listed_conversations(conn, account_id, "kind:orphaned").await,
        [
            (Some("Ada · Missing recipient"), vec!["+15555550154"]),
            (Some("Bob · Missing recipient"), vec!["+15555550155"]),
            (Some("Unknown recipient"), vec![]),
        ]
        .map(|(label, people)| (
            label.map(str::to_string),
            people.into_iter().map(str::to_string).collect::<Vec<_>>()
        ))
    );
    assert_eq!(
        listed_conversations(conn, account_id, "kind:direct").await,
        [(None, vec!["+15555550154".to_string()])],
        "Ada's one-to-one conversation is the only direct one"
    );
    assert_eq!(
        listed_conversations(conn, account_id, "kind:group").await,
        []
    );
}

/// The conversations `q` lists, as (title, participant identities), by title.
async fn listed_conversations(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    q: &str,
) -> Vec<(Option<String>, Vec<String>)> {
    let page = crate::db::conversations::list_conversations_sorted(
        conn,
        account_id,
        q,
        &crate::db::conversations::DEFAULT_CONVERSATION_SORT,
        50,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    let mut out: Vec<(Option<String>, Vec<String>)> = page
        .items
        .into_iter()
        .map(|c| {
            (
                c.label,
                c.participants
                    .into_iter()
                    .map(|p| p.identity.unwrap_or_default())
                    .collect(),
            )
        })
        .collect();
    out.sort();
    out
}

/// Run one import of `files` (name, body) through the real entry point.
async fn import_files(conn: &mut sqlx::SqliteConnection, files: &[(&str, String)]) {
    let tmp = tempfile::TempDir::new().unwrap();
    let paths: Vec<std::path::PathBuf> = files
        .iter()
        .map(|(name, body)| {
            let path = tmp.path().join(name);
            std::fs::write(&path, body).unwrap();
            path
        })
        .collect();
    let assets = tmp.path().join("assets");
    let opts = crate::imports_api::ImportOptions::fixed(crate::imports_api::FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: crate::imports_api::ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    crate::imports_api::import_jsonl_files_on_conn(
        conn,
        &paths,
        &opts,
        crate::imports_api::ImportSchemaMode::Ensure,
    )
    .await
    .unwrap();
}

/// The rule "an import makes a contact for every person it meets": every
/// handle a message was sent from, a participant takes part as, or a
/// one-to-one conversation is with belongs to a contact that is not in the
/// Trash. `contact_handles` is keyed on the handle, so "a contact" is "one".
async fn assert_every_met_handle_has_a_live_contact(conn: &mut sqlx::SqliteConnection) {
    let orphans: Vec<String> = sqlx::query_scalar(
        "SELECT h.raw FROM handles h
         WHERE h.account_id = $1
           AND (h.id IN (SELECT sender_handle_id FROM messages WHERE account_id = $1)
             OR h.id IN (SELECT p.handle_id FROM participants p
                         JOIN conversations c ON c.id = p.conversation_id
                         WHERE c.account_id = $1)
             OR h.id IN (SELECT chat_handle_id FROM conversations
                         WHERE account_id = $1 AND conversation_type = 'individual'))
           AND h.id NOT IN (
             SELECT ch.handle_id FROM contact_handles ch
             WHERE ch.account_id = $1
               AND ch.contact_id NOT IN (SELECT contact_id FROM trashed_contacts
                                         WHERE account_id = $1))
         ORDER BY h.raw",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(
        orphans.is_empty(),
        "handles the import met with no live contact: {orphans:?}"
    );
}

/// Report (a): a group sender the group's header left out used to get a
/// handle and no contact, because the sender path only joined an existing
/// sibling's contact.
#[tokio::test]
async fn a_sender_no_header_names_still_gets_a_contact() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    import_files(
        &mut conn,
        &[(
            "group.jsonl",
            header(
                "chat1000000701",
                "group",
                r#"{"identity":"+15555550123","display_name":null}"#,
            ) + "\n"
                + &incoming("g-group", "+15555550156")
                + "\n",
        )],
    )
    .await;
    assert_every_met_handle_has_a_live_contact(&mut conn).await;
}

/// Orphaned messages from two senders make two conversations on the
/// desktop app's import path, and the holder's a third, as
/// [`assert_imported_as_three_orphaned_conversations`] says.
#[tokio::test]
async fn orphaned_messages_from_two_senders_make_two_conversations() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    import_files(
        &mut conn,
        &[("backup.jsonl", backup_with_orphaned_messages())],
    )
    .await;
    assert_imported_as_three_orphaned_conversations(&mut conn, TEST_ACCOUNT).await;
}

/// The same backup through the HTTP batch path, where every file is
/// `_import.jsonl`, imports the same way: no conversation key becomes a
/// person, and reading an orphaned conversation shows its sender alone
/// (#1169, S1-2).
#[tokio::test]
async fn the_orphaned_conversation_over_http_is_not_a_person() {
    let (fixture, account) = crate::test_support::fixture_with_account().await;
    crate::imports_api::tests::completed_run(
        &fixture.state,
        &account.token,
        "imessage",
        backup_with_orphaned_messages(),
    )
    .await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    assert_imported_as_three_orphaned_conversations(&mut conn, account.account_id).await;
}

/// Report (b): a sender whose number is on a trashed contact under another
/// service used to be linked to that trashed contact. ADR-0013: the import
/// replaces the trashed contact instead.
#[tokio::test]
async fn a_sender_is_never_linked_to_a_trashed_contact() {
    let (mut conn, _pool, _dir) = account().await;
    let on_whatsapp = insert_handle(&mut conn, "+15555550157", "whatsapp").await;
    let mut counts = ImportCounts::default();
    let trashed = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        on_whatsapp,
        Some("Ada"),
        &mut counts,
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(trashed)
        .execute(&mut *conn)
        .await
        .unwrap();

    import_files(
        &mut conn,
        &[(
            "group.jsonl",
            header(
                "chat1000000702",
                "group",
                r#"{"identity":"+15555550123","display_name":null}"#,
            ) + "\n"
                + &incoming("g-trashed", "+15555550157")
                + "\n",
        )],
    )
    .await;

    assert_every_met_handle_has_a_live_contact(&mut conn).await;
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1",
            TEST_ACCOUNT
        )
        .await,
        0,
        "the trashed contact was replaced, not joined"
    );
}

/// Report (c): a one-to-one chat whose handle this run first met as a sender
/// used to skip contact creation, because the handle cache said "seen" and
/// the chat path read that as "has a contact". The sender fix alone makes
/// this pass; it stays to guard the chat path against leaning on the cache
/// again, which would break the moment any path caches a handle without
/// giving it a contact.
#[tokio::test]
async fn a_one_to_one_chat_first_met_as_a_sender_gets_a_contact() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    import_files(
        &mut conn,
        &[
            (
                "group.jsonl",
                header(
                    "chat1000000704",
                    "group",
                    r#"{"identity":"+15555550123","display_name":null}"#,
                ) + "\n"
                    + &incoming("g-group", "+15555550158")
                    + "\n",
            ),
            (
                "direct.jsonl",
                header("+15555550158", "individual", "") + "\n",
            ),
        ],
    )
    .await;
    assert_every_met_handle_has_a_live_contact(&mut conn).await;
}

async fn insert_handle(conn: &mut sqlx::SqliteConnection, raw: &str, service: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', $3) RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .bind(raw)
    .bind(service)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// `sql` is a `SELECT COUNT(*)` with one `$1` bind, `id`.
async fn count(conn: &mut sqlx::SqliteConnection, sql: &str, id: i64) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_second_spelling_does_not_rename_anyone() {
    let (mut conn, _pool, _dir) = account().await;
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550162', '+15555550162', 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let mut counts = ImportCounts::default();
    let contact_id = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        handle_id,
        Some("Ada Lovelace"),
        &mut counts,
    )
    .await
    .unwrap();
    ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        handle_id,
        Some("ada l"),
        &mut counts,
    )
    .await
    .unwrap();

    let name = name_of(&mut conn, contact_id).await;
    assert_eq!(name, "Ada Lovelace", "first backup wins");
}

/// A contact the person made in the app, holding `handle_id`, under `name`.
/// A non-empty name is typed the way the contact drawer types it.
async fn made_by_the_person(conn: &mut sqlx::SqliteConnection, handle_id: i64, name: &str) -> i64 {
    use crate::db::contacts::{self, Origin};
    let contact_id = contacts::create_contact(conn, TEST_ACCOUNT, "", Origin::User)
        .await
        .unwrap();
    contacts::link_handle_to_contact(conn, TEST_ACCOUNT, handle_id, contact_id, Origin::User)
        .await
        .unwrap();
    contacts::propose_name(conn, TEST_ACCOUNT, contact_id, name, Origin::User)
        .await
        .unwrap();
    contact_id
}

async fn name_of(conn: &mut sqlx::SqliteConnection, contact_id: i64) -> String {
    sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// A name the person typed outranks any backup, however many imports later
/// run.
#[tokio::test]
async fn an_import_does_not_overwrite_a_name_the_person_typed() {
    let (mut conn, _pool, _dir) = account().await;
    let handle_id = insert_handle(&mut conn, "+15555550168", "imessage").await;
    let contact_id = made_by_the_person(&mut conn, handle_id, "Ada Lovelace").await;

    let mut counts = ImportCounts::default();
    ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        handle_id,
        Some("Ada"),
        &mut counts,
    )
    .await
    .unwrap();

    assert_eq!(name_of(&mut conn, contact_id).await, "Ada Lovelace");
}

/// A blank name has nothing to protect, so an import names a nameless
/// contact whatever made it: here the person, with no name.
#[tokio::test]
async fn an_import_names_a_nameless_contact_the_person_made() {
    let (mut conn, _pool, _dir) = account().await;
    let handle_id = insert_handle(&mut conn, "+15555550169", "imessage").await;
    let contact_id = made_by_the_person(&mut conn, handle_id, "").await;

    let mut counts = ImportCounts::default();
    let met = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        handle_id,
        Some("Ann"),
        &mut counts,
    )
    .await
    .unwrap();

    assert_eq!(met, contact_id);
    assert_eq!(name_of(&mut conn, contact_id).await, "Ann");
}

/// An address book load can make a contact that holds a number and has no
/// name. A later import that knows the number's name fills it in, as it
/// does for a nameless contact an import made.
#[tokio::test]
async fn an_import_names_a_nameless_contact_a_load_made() {
    use crate::db::address_book::{LoadMode, load};
    let (mut conn, _pool, _dir) = account().await;
    let text = "contact_id,display_name,groups,service,identity_type,identity\n\
                ,,,phone,phone,+15555550170\n";
    load(&mut conn, TEST_ACCOUNT, text, LoadMode::Append)
        .await
        .unwrap();
    let (contact_id, handle_id, origin): (i64, i64, String) = sqlx::query_as(
        "SELECT c.id, h.id, c.origin FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         JOIN contacts c ON c.id = ch.contact_id
         WHERE h.account_id = $1 AND h.normalized = '+15555550170'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(origin, "address_book");
    assert_eq!(name_of(&mut conn, contact_id).await, "");

    let mut counts = ImportCounts::default();
    let met = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        None,
        handle_id,
        Some("Ann"),
        &mut counts,
    )
    .await
    .unwrap();

    assert_eq!(met, contact_id);
    assert_eq!(name_of(&mut conn, contact_id).await, "Ann");
}

#[tokio::test]
async fn sibling_contact_link_bumps_last_modified_only_on_insert() {
    let (mut conn, _pool, _dir) = account().await;

    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Pat') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let phone_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550100', '+15555550100', 'phone', 'phone') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(TEST_ACCOUNT)
    .bind(phone_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    let wa_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550100', '+15555550100', 'phone', 'whatsapp') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    const OLD: &str = "2000-01-01 00:00:00";
    sqlx::query("UPDATE contacts SET last_modified = $1 WHERE id = $2")
        .bind(OLD)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let linked = ensure_sibling_contact_link(&mut conn, TEST_ACCOUNT, None, wa_id)
        .await
        .unwrap()
        .expect("sibling link");
    assert_eq!(linked, contact_id);
    let after_insert: String =
        sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1")
            .bind(contact_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_ne!(after_insert, OLD);

    sqlx::query("UPDATE contacts SET last_modified = $1 WHERE id = $2")
        .bind(OLD)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let again = ensure_sibling_contact_link(&mut conn, TEST_ACCOUNT, None, wa_id)
        .await
        .unwrap()
        .expect("already linked");
    assert_eq!(again, contact_id);
    let after_noop: String = sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(after_noop, OLD);
}
