//! Imports a test runs on a connection, and the checks on what an import
//! leaves behind.

use tempfile::TempDir;

use super::{conversation_header, message_line};

/// Import one JSON Lines text into `account_id` in append mode on `conn`, as
/// the serve path does once the schema is in place, and answer the run's
/// counts. The file and its asset directory live in a temporary directory that
/// is gone when this returns.
pub async fn import_jsonl_text(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    source: &str,
    body: &str,
) -> crate::imports_api::ImportCounts {
    use crate::imports_api::{
        FixedImportArgs, ImportMode, ImportOptions, import_jsonl_files_on_conn,
    };
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("conversation.jsonl");
    std::fs::write(&path, body).unwrap();
    let assets = tmp.path().join("assets");
    // The import never makes the account (#2061), so the test does.
    crate::db::account_profile::create_account_row_if_missing(conn, account_id)
        .await
        .unwrap();
    import_jsonl_files_on_conn(
        conn,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: Some(tmp.path()),
            mode: ImportMode::Append,
            source,
            account_id,
            import_id: None,
            phone_country: None,
        }),
    )
    .await
    .unwrap()
}

/// Import a one-to-one Text Message conversation with Ada at `address`
/// into `account_id` on `conn`: one incoming message from `address`, "hi",
/// whose guid is `guid`. The import names Ada's contact "Ada", from her
/// participant's display name.
pub async fn import_ada_conversation(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    address: &str,
    guid: &str,
) {
    let header = conversation_header("imessage", address).participant(address, Some("Ada"));
    let message = message_line(guid, "hi").sender(address);
    import_jsonl_text(
        conn,
        account_id,
        "imessage",
        &format!("{header}\n{message}\n"),
    )
    .await;
}

/// Panic unless the people rule of
/// `docs/architecture/contacts-identities-and-messages.md` holds: every
/// participant has an identity, and every identity a participant, a message's
/// sender or a reaction's sender uses is on a contact. `after` names the act
/// the check follows, for the message.
pub async fn assert_every_person_is_on_a_contact(conn: &mut sqlx::SqliteConnection, after: &str) {
    let without_identity: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM participants WHERE handle_id IS NULL")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        without_identity, 0,
        "after {after}: {without_identity} participants have no identity"
    );
    let on_no_contact: Vec<String> = sqlx::query_scalar(
        "SELECT h.raw FROM handles h
         WHERE (EXISTS (SELECT 1 FROM participants p WHERE p.handle_id = h.id)
                OR EXISTS (SELECT 1 FROM messages m WHERE m.sender_handle_id = h.id)
                OR EXISTS (SELECT 1 FROM tapbacks t WHERE t.sender_handle_id = h.id))
           AND NOT EXISTS (SELECT 1 FROM contact_handles ch
                           WHERE ch.account_id = h.account_id AND ch.handle_id = h.id)
         ORDER BY h.raw",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(
        on_no_contact.is_empty(),
        "after {after}: identities in conversations on no contact: {on_no_contact:?}"
    );
}

/// A conversation file an exporter wrote and the tests keep in
/// `tests/fixtures/`, `contents`, with its header's `schema_version` set to
/// [`message_ir::SCHEMA_VERSION`]. A schema version bump then reaches the
/// committed files the way it reaches [`conversation_header`], with no file
/// edited by hand (#1855). Only the header line is rewritten. Every message
/// line stays as the exporter wrote it.
pub fn at_current_schema_version(contents: &str) -> String {
    let (header, messages) = contents
        .split_once('\n')
        .expect("a conversation file opens with its header line");
    let mut header: serde_json::Value =
        serde_json::from_str(header).expect("the header line is JSON");
    header["schema_version"] = message_ir::SCHEMA_VERSION.into();
    format!("{header}\n{messages}")
}
