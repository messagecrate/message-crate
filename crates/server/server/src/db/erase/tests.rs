//! A delete for good leaves no trace of the deleted text in the bytes of the
//! database file or of its write-ahead log (#2108).
//!
//! Each test seeds text made of words found nowhere else, checks the files
//! hold them, deletes for good, and reads the files' bytes again. Each word
//! starts with a letter no other word in the test starts with. The full-text
//! search index stores a word after the letters it shares with the word
//! before it, so a shared first letter would leave the word in the index
//! without its whole spelling, where a test reading for it would not find it.

use std::path::{Path, PathBuf};

use sqlx::SqliteConnection;

use super::*;
use crate::db::account_profile;
use crate::db::audit_trail::AuditActor;
use crate::db::trash::{self, Trashable};
use crate::test_support::{ConversationRow, MessageRow, fake_sha256, fixed_instant};

const ACCOUNT: i64 = 7;
const OTHER_ACCOUNT: i64 = 8;

/// A word in a message body.
const BODY: &str = "bodyxylophone";
/// A word in a message subject.
const SUBJECT: &str = "kumquatsubject";
/// A word in an attachment's file name.
const FILE_NAME: &str = "wombatfilename";
/// A word in an attachment's transcription.
const TRANSCRIPTION: &str = "zebratranscript";
/// A word in an earlier version of an edited message.
const VERSION: &str = "platypusversion";
/// A word in a group conversation's title.
const TITLE: &str = "quokkatitle";
/// A word in a message that is not deleted, so a test shows the files are
/// read where the text is.
const KEPT: &str = "galagokept";

/// Every word a deleted conversation holds.
const DELETED: [&str; 6] = [BODY, SUBJECT, FILE_NAME, TRANSCRIPTION, VERSION, TITLE];

/// The database file behind `conn`, and its write-ahead log beside it.
async fn files(conn: &mut SqliteConnection) -> (PathBuf, PathBuf) {
    let db = crate::db::schema::database_file(conn)
        .await
        .unwrap()
        .expect("the test database has a file");
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    (db, wal)
}

/// Whether the file at `path` holds `word`'s bytes. A file that is not
/// there holds nothing.
fn holds(path: &Path, word: &str) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes
        .windows(word.len())
        .any(|window| window == word.as_bytes())
}

/// The words of `words` the database file or its log hold, each with the
/// file that holds it.
async fn traces(conn: &mut SqliteConnection, words: &[&str]) -> Vec<String> {
    let (db, wal) = files(conn).await;
    let mut found = Vec::new();
    for word in words {
        for path in [&db, &wal] {
            if holds(path, word) {
                found.push(format!("{word} in {}", path.display()));
            }
        }
    }
    found
}

/// Whether either file holds `word`.
async fn either_holds(conn: &mut SqliteConnection, word: &str) -> bool {
    !traces(conn, &[word]).await.is_empty()
}

/// Insert the phone handle `raw` of `account_id`, returning its id.
async fn insert_handle(conn: &mut SqliteConnection, account_id: i64, raw: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'phone') RETURNING id",
    )
    .bind(account_id)
    .bind(raw)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Insert a group conversation of `account_id` titled [`TITLE`] on the
/// handle `raw`, with one message holding every deleted word: its body,
/// subject, an attachment's name and transcription, and an earlier version.
/// Returns the conversation's id.
async fn insert_conversation_with_every_word(
    conn: &mut SqliteConnection,
    account_id: i64,
    raw: &str,
) -> i64 {
    let handle_id = insert_handle(conn, account_id, raw).await;
    let conversation_id = ConversationRow::group(account_id, handle_id, TITLE)
        .insert(conn)
        .await;
    let body = format!("see {BODY} now");
    let message_id = MessageRow {
        body: Some(&body),
        subject: Some(SUBJECT),
        ..MessageRow::new(account_id, conversation_id)
    }
    .insert(conn)
    .await;
    let mut tx = crate::db::begin_write(conn).await.unwrap();
    sqlx::query(
        "INSERT INTO attachments (message_id, sha256, assets_path, original_name, transcription)
         VALUES ($1, $2, 'aa/a.m4a', $3, $4)",
    )
    .bind(message_id)
    .bind(fake_sha256('a'))
    .bind(format!("{FILE_NAME}.m4a"))
    .bind(format!("I said {TRANSCRIPTION} twice"))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO message_versions (message_id, text) VALUES ($1, $2)")
        .bind(message_id)
        .bind(format!("first {VERSION}"))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    conversation_id
}

/// Insert a conversation of `account_id` on the handle `raw` with one
/// message whose body is [`KEPT`].
async fn insert_kept_conversation(conn: &mut SqliteConnection, account_id: i64, raw: &str) {
    let handle_id = insert_handle(conn, account_id, raw).await;
    let conversation_id = ConversationRow::new(account_id, handle_id)
        .insert(conn)
        .await;
    MessageRow {
        body: Some(KEPT),
        ..MessageRow::new(account_id, conversation_id)
    }
    .insert(conn)
    .await;
}

/// Assert the files hold every deleted word and [`KEPT`] before the delete,
/// so the absence the test asserts afterwards is the delete's doing.
async fn assert_every_word_is_there_first(conn: &mut SqliteConnection) {
    for word in DELETED.iter().chain([&KEPT]) {
        assert!(
            either_holds(conn, word).await,
            "{word} is in neither file before the delete"
        );
    }
}

/// Assert no deleted word is left in either file, and [`KEPT`] still is.
async fn assert_only_the_kept_text_is_left(conn: &mut SqliteConnection) {
    assert_eq!(traces(conn, &DELETED).await, Vec::<String>::new());
    assert!(
        either_holds(conn, KEPT).await,
        "the text that was not deleted is still readable"
    );
}

#[tokio::test]
async fn emptying_the_trash_leaves_no_deleted_text_in_the_file_or_its_log() {
    let fixture = crate::test_support::test_fixture().await;
    fixture.account_with_id(ACCOUNT, "a").await;
    let mut conn = fixture.conn().await;
    let id = insert_conversation_with_every_word(&mut conn, ACCOUNT, "+15550101").await;
    insert_kept_conversation(&mut conn, ACCOUNT, "+15550102").await;
    trash::move_to_trash(&mut conn, ACCOUNT, Trashable::Conversation(id))
        .await
        .unwrap();
    assert_every_word_is_there_first(&mut conn).await;

    trash::empty_trash(&mut conn, ACCOUNT, AuditActor::Holder, fixed_instant())
        .await
        .unwrap();

    assert_only_the_kept_text_is_left(&mut conn).await;
}

#[tokio::test]
async fn deleting_one_trashed_conversation_leaves_no_deleted_text_in_the_file_or_its_log() {
    let fixture = crate::test_support::test_fixture().await;
    fixture.account_with_id(ACCOUNT, "a").await;
    let mut conn = fixture.conn().await;
    let id = insert_conversation_with_every_word(&mut conn, ACCOUNT, "+15550101").await;
    insert_kept_conversation(&mut conn, ACCOUNT, "+15550102").await;
    trash::move_to_trash(&mut conn, ACCOUNT, Trashable::Conversation(id))
        .await
        .unwrap();
    assert_every_word_is_there_first(&mut conn).await;

    trash::delete_trashed(
        &mut conn,
        ACCOUNT,
        Trashable::Conversation(id),
        AuditActor::Holder,
        fixed_instant(),
    )
    .await
    .unwrap();

    assert_only_the_kept_text_is_left(&mut conn).await;
}

#[tokio::test]
async fn deleting_a_trashed_contact_leaves_no_trace_of_its_name() {
    const NAME: &str = "Qzcontactocelot";
    let fixture = crate::test_support::test_fixture().await;
    fixture.account_with_id(ACCOUNT, "a").await;
    let mut conn = fixture.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name, origin)
         VALUES ($1, $2, 'user') RETURNING id",
    )
    .bind(ACCOUNT)
    .bind(NAME)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    trash::move_to_trash(&mut conn, ACCOUNT, Trashable::Contact(contact_id))
        .await
        .unwrap();
    assert!(either_holds(&mut conn, NAME).await);

    trash::delete_trashed(
        &mut conn,
        ACCOUNT,
        Trashable::Contact(contact_id),
        AuditActor::Holder,
        fixed_instant(),
    )
    .await
    .unwrap();

    assert_eq!(traces(&mut conn, &[NAME]).await, Vec::<String>::new());
}

#[tokio::test]
async fn deleting_all_of_an_accounts_messages_leaves_no_deleted_text_in_the_file_or_its_log() {
    let fixture = crate::test_support::test_fixture().await;
    fixture.account_with_id(ACCOUNT, "a").await;
    fixture.account_with_id(OTHER_ACCOUNT, "b").await;
    let mut conn = fixture.conn().await;
    insert_conversation_with_every_word(&mut conn, ACCOUNT, "+15550101").await;
    insert_kept_conversation(&mut conn, OTHER_ACCOUNT, "+15550102").await;
    assert_every_word_is_there_first(&mut conn).await;

    account_profile::delete_all_messages_for_account(
        &mut conn,
        ACCOUNT,
        AuditActor::Holder,
        fixed_instant(),
    )
    .await
    .unwrap();

    assert_only_the_kept_text_is_left(&mut conn).await;
}

#[tokio::test]
async fn deleting_an_account_leaves_no_deleted_text_in_the_file_or_its_log() {
    let fixture = crate::test_support::test_fixture().await;
    fixture.account_with_id(ACCOUNT, "a").await;
    fixture.account_with_id(OTHER_ACCOUNT, "b").await;
    let mut conn = fixture.conn().await;
    insert_conversation_with_every_word(&mut conn, ACCOUNT, "+15550101").await;
    insert_kept_conversation(&mut conn, OTHER_ACCOUNT, "+15550102").await;
    assert_every_word_is_there_first(&mut conn).await;

    assert!(
        account_profile::delete_account(&mut conn, ACCOUNT, AuditActor::Holder, fixed_instant())
            .await
            .unwrap()
    );

    assert_only_the_kept_text_is_left(&mut conn).await;
}

/// Another connection still reading keeps the log from being cleared at
/// once. The database is remembered, and the next retry, once the read has
/// ended, clears the log.
#[tokio::test]
async fn a_log_kept_by_a_reader_is_cleared_by_the_next_retry() {
    let fixture = crate::test_support::test_fixture().await;
    fixture.account_with_id(ACCOUNT, "a").await;
    let mut conn = fixture.conn().await;
    let id = insert_conversation_with_every_word(&mut conn, ACCOUNT, "+15550101").await;
    insert_kept_conversation(&mut conn, ACCOUNT, "+15550102").await;
    trash::move_to_trash(&mut conn, ACCOUNT, Trashable::Conversation(id))
        .await
        .unwrap();
    let (db, wal) = files(&mut conn).await;
    assert!(
        holds(&wal, BODY),
        "the log holds the body before the delete"
    );

    // A read transaction on another connection, holding the state before
    // the delete. It is begun by hand because it never writes, and the
    // server's only kind of transaction is a write one (`begin_write`).
    let mut reader = fixture.conn().await;
    sqlx::query("BEGIN").execute(&mut *reader).await.unwrap();
    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *reader)
        .await
        .unwrap();

    trash::empty_trash(&mut conn, ACCOUNT, AuditActor::Holder, fixed_instant())
        .await
        .unwrap();
    assert!(holds(&wal, BODY), "the reader kept the log as it was");
    assert!(logs_to_clear().waiting.contains_key(&db));

    sqlx::query("ROLLBACK").execute(&mut *reader).await.unwrap();
    drop(reader);
    drop(conn);
    retry_clearing(&fixture.state.db).await;

    assert!(!logs_to_clear().waiting.contains_key(&db));
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);
    let mut conn = fixture.conn().await;
    assert_only_the_kept_text_is_left(&mut conn).await;
}

#[tokio::test]
async fn a_log_no_one_else_reads_is_cleared_to_zero_bytes() {
    let fixture = crate::test_support::test_fixture().await;
    fixture.account_with_id(ACCOUNT, "a").await;
    let mut conn = fixture.conn().await;
    insert_kept_conversation(&mut conn, ACCOUNT, "+15550102").await;
    let (_, wal) = files(&mut conn).await;
    assert!(std::fs::metadata(&wal).unwrap().len() > 0);

    assert_eq!(clear_write_ahead_log(&mut conn).await, LogCleared::Cleared);

    assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);
}

/// A checkpoint that clears the log removes the file only when no other
/// checkpoint kept the log while it ran: one that did wrote frames this one
/// may not have cleared, so the file stays for the next retry.
#[test]
fn a_clear_never_drops_a_log_kept_while_it_ran() {
    let file = PathBuf::from("/nowhere/messagecrate.db");
    let mut logs = LogsToClear {
        waiting: std::collections::BTreeMap::new(),
        kept: 0,
    };
    // This checkpoint starts with no entry; another keeps the log meanwhile.
    let seen = logs.waiting.get(&file).copied();
    record(&mut logs, file.clone(), None, LogCleared::Kept);
    record(&mut logs, file.clone(), seen, LogCleared::Cleared);
    assert!(logs.waiting.contains_key(&file));

    // The next checkpoint saw that entry, so its clear removes it.
    let seen = logs.waiting.get(&file).copied();
    record(&mut logs, file.clone(), seen, LogCleared::Cleared);
    assert!(!logs.waiting.contains_key(&file));
}
