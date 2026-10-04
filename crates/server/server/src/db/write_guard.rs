//! In tests, every SQLite connection refuses a write to `messages`,
//! `attachments` or `messages_fts` that is not inside a transaction.
//!
//! The full-text search triggers fire on a write to `messages` or
//! `attachments`. A connection whose copy of the schema is out of date (a
//! promote dropped and re-created those triggers on another connection)
//! fails such a statement as "no such table" when it runs on its own, because
//! SQLite cannot reload the schema while it connects to `messages_fts` in the
//! middle of preparing it (#1628). [`crate::db::begin_write`] brings the copy
//! up to date before its first statement (#1600), so every such write goes
//! through a write transaction. Deleting a conversation, a handle or an
//! account deletes messages through `ON DELETE CASCADE`, and is held to the
//! same rule.
//!
//! The guard is an SQLite authorizer: while a statement is prepared, SQLite
//! names each table it writes, cascades and trigger bodies included, and the
//! guard refuses the statement when one of the three is among them and the
//! connection is in autocommit mode. The refused statement fails with
//! "not authorized", and the guard names the table on standard error. It
//! is installed on every connection the test binary opens, through
//! `sqlite3_auto_extension`, the same hook [`crate::db::sqlite_functions`]
//! uses. The server binary does not have it: the guard exists to fail a test
//! that reaches such a write, not to refuse one in use.
//!
//! Dropping a table is not writing to it. SQLite asks about a `DROP TABLE`
//! as a drop of the table followed by a delete of it, so the guard lets
//! through the delete that comes straight after the drop of the same table:
//! the schema rebuild at startup drops every table on a bare connection,
//! with foreign keys off. With foreign keys on, SQLite also asks about a
//! second delete of the table and a delete of each table that refers to it;
//! on a bare connection the guard refuses the second delete, and the delete
//! of a referring table that is one of the three.
//!
//! What the guard does not see:
//!
//! - sqlx keeps prepared statements per connection, and SQLite asks the
//!   authorizer only while preparing. A statement first prepared inside a
//!   transaction and run again later on its own on the same connection is
//!   not asked about again.
//! - It asks whether a transaction is open, not whether `begin_write` opened
//!   it. Clippy refuses sqlx's own `begin` (`clippy.toml`), but a `BEGIN`
//!   statement run by hand would satisfy the guard.
//! - It is in the library's unit tests only. The integration tests under
//!   `tests/` that use a database start the server binary, which does not
//!   have it.

use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::Once;

use libsqlite3_sys as ffi;

/// The tables a write to which runs only inside a transaction.
const GUARDED_TABLES: [&str; 3] = ["messages", "attachments", "messages_fts"];

thread_local! {
    /// The table whose drop SQLite asked about last on this thread. SQLite
    /// asks the authorizer on the thread that prepares the statement, and
    /// asks about the drop and then the delete of the table one after the
    /// other.
    static DROPPING: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Install the guard on every SQLite connection opened after this call. Safe
/// to call any number of times; only the first does work.
pub fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: as in `sqlite_functions::register`: a plain function
        // pointer with the entry-point signature SQLite documents.
        let status = unsafe { ffi::sqlite3_auto_extension(Some(install_on_connection)) };
        assert_eq!(
            status,
            ffi::SQLITE_OK,
            "sqlite3_auto_extension refused the write guard (code {status})"
        );
    });
}

/// The auto-extension entry point: set the authorizer on each new
/// connection, with the connection itself as the callback's data so the
/// callback can ask whether a transaction is open.
unsafe extern "C" fn install_on_connection(
    db: *mut ffi::sqlite3,
    _err_msg: *mut *mut c_char,
    _api: *const ffi::sqlite3_api_routines,
) -> c_int {
    // SAFETY: `db` is the connection SQLite is opening; the authorizer and
    // its data pointer live exactly as long as that connection.
    unsafe { ffi::sqlite3_set_authorizer(db, Some(authorize), db.cast::<c_void>()) }
}

/// The authorizer: deny an insert, update or delete of a guarded table on a
/// connection in autocommit mode, except the delete a drop of that table
/// asks about, and allow everything else.
unsafe extern "C" fn authorize(
    db: *mut c_void,
    action: c_int,
    table: *const c_char,
    _column: *const c_char,
    _database: *const c_char,
    _trigger: *const c_char,
) -> c_int {
    if table.is_null() {
        return ffi::SQLITE_OK;
    }
    // SAFETY: SQLite passes the table name as a NUL-terminated string valid
    // for this call, and `db` is the connection the authorizer was set on.
    let (table, autocommit) = unsafe {
        (
            CStr::from_ptr(table).to_string_lossy(),
            ffi::sqlite3_get_autocommit(db.cast::<ffi::sqlite3>()) != 0,
        )
    };
    let dropped = DROPPING.with_borrow_mut(|dropping| {
        let dropped = dropping.take();
        if matches!(
            action,
            ffi::SQLITE_DROP_TABLE | ffi::SQLITE_DROP_TEMP_TABLE | ffi::SQLITE_DROP_VTABLE
        ) {
            *dropping = Some(table.to_string());
        }
        dropped
    });
    let writes = match action {
        ffi::SQLITE_INSERT | ffi::SQLITE_UPDATE => true,
        ffi::SQLITE_DELETE => dropped.as_deref() != Some(table.as_ref()),
        _ => false,
    };
    if writes && autocommit && GUARDED_TABLES.contains(&table.as_ref()) {
        eprintln!(
            "a write to `{table}` outside a transaction: run it in crate::db::begin_write (#1628)"
        );
        return ffi::SQLITE_DENY;
    }
    ffi::SQLITE_OK
}

mod tests {
    use crate::db::{begin_write, schema};
    use crate::test_support::{MessageRow, SeedConversation, SeedMessage, seed_conversation};

    /// "not authorized" is the error SQLite gives a statement the
    /// authorizer denies.
    fn refused(result: sqlx::Result<sqlx::sqlite::SqliteQueryResult>) -> bool {
        matches!(result, Err(e) if e.to_string().contains("not authorized"))
    }

    /// A delete of messages, of attachments, and of a conversation (whose
    /// messages go by `ON DELETE CASCADE`) on a bare connection is refused,
    /// and the same statements inside `begin_write` run. The first is the
    /// statement #1628 found failing on a connection with an out-of-date
    /// schema.
    #[tokio::test]
    async fn a_write_to_messages_runs_only_inside_a_write_transaction() {
        let fixture = crate::test_support::test_fixture().await;
        let account = fixture.account("guarded").await;
        let conversation = seed_conversation(
            &fixture.state,
            &SeedConversation {
                account_id: account,
                handle: "+15555550100",
                conversation_type: "individual",
                group_title: None,
                source_file: "guard.jsonl",
                messages: &[SeedMessage {
                    source: "imessage",
                    timestamp: "2020-01-01T00:00:00Z",
                    is_from_me: false,
                    body: "kept",
                }],
            },
        )
        .await;
        let mut conn = fixture.conn().await;
        let statements = [
            "DELETE FROM attachments WHERE message_id IN (SELECT id FROM messages WHERE account_id = $1)",
            "DELETE FROM messages WHERE id IN (SELECT id FROM messages WHERE account_id = $1)",
            "DELETE FROM conversations WHERE account_id = $1",
        ];
        for sql in statements {
            let alone = sqlx::query(sql).bind(account).execute(&mut *conn).await;
            assert!(refused(alone), "{sql} ran outside a transaction");
        }
        MessageRow::new(account, conversation)
            .insert(&mut conn)
            .await;
        let mut tx = begin_write(&mut conn).await.unwrap();
        for sql in statements {
            sqlx::query(sql)
                .bind(account)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }

    /// An insert on a bare connection is refused too, while a write to
    /// another table is not the guard's business.
    #[tokio::test]
    async fn an_insert_of_an_attachment_outside_a_transaction_is_refused() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_schema(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO accounts (id, username) VALUES (1, 'a')")
            .execute(&mut *conn)
            .await
            .unwrap();
        let alone = sqlx::query("INSERT INTO attachments (message_id) VALUES (1)")
            .execute(&mut *conn)
            .await;
        assert!(
            refused(alone),
            "an insert of an attachment ran outside a transaction"
        );
    }

    /// The schema rebuild drops `messages` on a bare connection with foreign
    /// keys off, and SQLite asks about that drop as a delete of the table:
    /// the guard lets it through. This tests the guard's exemption for a
    /// drop, not a write the guard catches, so it passes without the guard
    /// too.
    #[tokio::test]
    async fn dropping_the_messages_table_with_foreign_keys_off_is_not_a_write_to_it() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_schema(&mut conn).await.unwrap();
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await
            .unwrap();
        for table in ["messages_fts", "attachments", "messages"] {
            sqlx::query(&format!("DROP TABLE {table}"))
                .execute(&mut *conn)
                .await
                .unwrap_or_else(|e| panic!("drop {table}: {e}"));
        }
    }
}
