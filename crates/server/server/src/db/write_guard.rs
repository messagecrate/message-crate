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
//! sqlx keeps prepared statements per connection, and SQLite asks the
//! authorizer only while preparing. A statement first prepared inside a
//! transaction and run again later on its own on the same connection is not
//! asked about again.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::Once;

use libsqlite3_sys as ffi;

/// The tables a write to which runs only inside a transaction.
const GUARDED_TABLES: [&str; 3] = ["messages", "attachments", "messages_fts"];

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
/// connection in autocommit mode, and allow everything else.
unsafe extern "C" fn authorize(
    db: *mut c_void,
    action: c_int,
    table: *const c_char,
    _column: *const c_char,
    _database: *const c_char,
    _trigger: *const c_char,
) -> c_int {
    if !matches!(
        action,
        ffi::SQLITE_INSERT | ffi::SQLITE_UPDATE | ffi::SQLITE_DELETE
    ) || table.is_null()
    {
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
    if autocommit && GUARDED_TABLES.contains(&table.as_ref()) {
        eprintln!(
            "a write to `{table}` outside a transaction: run it in crate::db::begin_write (#1628)"
        );
        return ffi::SQLITE_DENY;
    }
    ffi::SQLITE_OK
}
