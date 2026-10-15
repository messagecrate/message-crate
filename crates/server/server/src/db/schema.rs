//! Schema management for the database.
//!
//! The schema is made sure of once, by [`ensure_schema`], when
//! [`crate::open_db::OpenDb`] opens the pool and when `reset-demo` builds a
//! database, and never by a request: a request that rebuilt the database
//! would empty it under the server serving it (#2064). DDL lives in the SQL
//! files embedded at compile time (`schema/sql/*.sql`); the functions here
//! apply and evolve it.
//!
//! The embedded SQL is fingerprinted at compile time ([`SCHEMA_FINGERPRINT`])
//! and the fingerprint is stamped into the database as `PRAGMA
//! user_version`. The rule is: any schema change requires a fresh reload of data,
//! so a database stamped with a different fingerprint is rebuilt empty from
//! the embedded DDL instead of being patched in place, once a copy of it is
//! kept beside its file. Nothing is bumped by
//! hand; changing a `schema/sql/*.sql` file is the whole of a schema change.

use anyhow::{Context, Result};
use sqlx::SqliteConnection;

/// Baseline DDL lives in `schema/sql/`. Every column there carries a comment;
/// `tests/schema_column_comments.rs` enforces it.
const ACCOUNTS_DDL: &str = include_str!("../../../../../schema/sql/accounts.sql");
const MESSAGE_TABLES_DDL: &str = include_str!("../../../../../schema/sql/messages.sql");
const STAGING_TABLES_DDL: &str = include_str!("../../../../../schema/sql/staging.sql");
const CONTACTS_TABLES_DDL: &str = include_str!("../../../../../schema/sql/contacts.sql");
const SAVED_SEARCHES_DDL: &str = include_str!("../../../../../schema/sql/saved_searches.sql");
const FTS_VIRTUAL_DDL: &str = include_str!("../../../../../schema/sql/fts_virtual.sql");
const DROP_MESSAGES_FTS_TRIGGERS_SQL: &str =
    include_str!("../../../../../schema/sql/fts_triggers_drop.sql");
const CREATE_MESSAGES_FTS_TRIGGERS_SQL: &str =
    include_str!("../../../../../schema/sql/fts_triggers_create.sql");

/// Fingerprint of the embedded schema: a 31-bit FNV-1a hash over every
/// `schema/sql/*.sql` file, computed at compile time. It is stamped into each
/// database as `PRAGMA user_version`; a database carrying any other value is
/// rebuilt empty (see [`migrate_schema`]).
///
/// A hash rather than a hand-kept number so that a schema change is only a
/// change to the SQL: nothing to bump, nothing to forget. 31 bits because
/// `user_version` is a signed 32-bit integer.
pub const SCHEMA_FINGERPRINT: i64 = schema_fingerprint();

// Fits `user_version`, and is not one of the small hand-kept numbers the old
// scheme stamped, so a database from that scheme is rebuilt rather than mistaken
// for current.
const _: () = assert!(SCHEMA_FINGERPRINT > 1000 && SCHEMA_FINGERPRINT <= i32::MAX as i64);

const fn schema_fingerprint() -> i64 {
    const FILES: [&str; 8] = [
        ACCOUNTS_DDL,
        MESSAGE_TABLES_DDL,
        STAGING_TABLES_DDL,
        CONTACTS_TABLES_DDL,
        SAVED_SEARCHES_DDL,
        FTS_VIRTUAL_DDL,
        DROP_MESSAGES_FTS_TRIGGERS_SQL,
        CREATE_MESSAGES_FTS_TRIGGERS_SQL,
    ];
    fingerprint_of(&FILES)
}

/// The fingerprint of `files`, in order: what [`schema_fingerprint`] computes
/// over the embedded SQL, apart so a test can hand it other text.
const fn fingerprint_of(files: &[&str]) -> i64 {
    // FNV-1a, 32-bit. Each file is followed by a zero byte so that moving
    // text across a file boundary changes the hash.
    let mut hash: u32 = 0x811c_9dc5;
    let mut f = 0;
    while f < files.len() {
        let bytes = files[f].as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u32;
            hash = hash.wrapping_mul(0x0100_0193);
            i += 1;
        }
        hash = hash.wrapping_mul(0x0100_0193);
        f += 1;
    }
    (hash & 0x7fff_ffff) as i64
}

/// The mark that makes a SQLite file a Message Crate database: the value of
/// SQLite's `PRAGMA application_id`, the four bytes `MsCr`. Every database
/// this server builds carries it, and the server changes no file without it
/// except one that holds nothing at all ([`DatabaseKind`]).
///
/// A mark of its own, rather than the absence of anything else, because
/// another program's SQLite file (Apple's `chat.db`, a backup) has tables
/// and a `user_version` of its own, and rebuilding it would drop that
/// program's data (#1070).
pub const APPLICATION_ID: i64 = 0x4d73_4372;

/// What a SQLite file is, read from its mark and its catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseKind {
    /// No mark and nothing in the catalog: a file SQLite has just created,
    /// or one of zero bytes. The server may build a database in it.
    Empty,
    /// Carries [`APPLICATION_ID`]: a database this server, or an earlier
    /// build of it, made.
    MessageCrate,
    /// Anything else: another program's database. The server never changes it.
    Foreign,
}

/// Read what the database behind `conn` is, without changing it.
///
/// # Errors
///
/// Returns an error when the pragma or the catalog cannot be read, as for a
/// file that is not SQLite at all.
pub async fn database_kind(conn: &mut SqliteConnection) -> Result<DatabaseKind> {
    let application_id: i64 = sqlx::query_scalar("PRAGMA application_id")
        .fetch_one(&mut *conn)
        .await?;
    if application_id == APPLICATION_ID {
        return Ok(DatabaseKind::MessageCrate);
    }
    let objects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master")
        .fetch_one(&mut *conn)
        .await?;
    if application_id == 0 && objects == 0 {
        Ok(DatabaseKind::Empty)
    } else {
        Ok(DatabaseKind::Foreign)
    }
}

/// The refusal for a file that is not a Message Crate database, naming it.
pub fn not_a_message_crate_database(path: &std::path::Path) -> anyhow::Error {
    anyhow::anyhow!(
        "{} is not a Message Crate database, so it was left as it is. \
         The server opens only a database it made: give the path of one, or a path \
         where no file exists for `serve` or `create-database` to make one",
        path.display()
    )
}

/// Bring the database to [`SCHEMA_FINGERPRINT`].
///
/// A database already stamped with the current fingerprint is left
/// untouched. An empty file, or a Message Crate database stamped by a server
/// with different SQL, is rebuilt empty and stamped; the user re-imports
/// afterwards. Any other file is refused before a statement changes it
/// ([`DatabaseKind::Foreign`]).
///
/// The only kind of migration is a full rebuild: schema changes require a
/// fresh reload of data, never in-place column patches. The rebuild drops
/// every table, so a database that has any is first copied beside its file
/// ([`keep_copy_before_rebuild`]), and the rebuild does not run when the copy
/// fails (#2068).
async fn migrate_schema(conn: &mut SqliteConnection) -> Result<()> {
    if database_kind(conn).await? == DatabaseKind::Foreign {
        anyhow::bail!(
            "the database is not a Message Crate database (its application_id is not {APPLICATION_ID:#x}), so it was left as it is"
        );
    }
    let stamped = user_version(conn).await?;
    if stamped == SCHEMA_FINGERPRINT {
        return Ok(());
    }
    if has_user_tables(conn).await? {
        let copy = keep_copy_before_rebuild(conn, stamped).await?;
        tracing::warn!(
            stamped = %stamped,
            expected = %SCHEMA_FINGERPRINT,
            copy = %copy.display(),
            "The database's schema differs from this server's, so the database is rebuilt empty and its messages must be imported again. \
             A copy of it as it was is kept at {}",
            copy.display()
        );
    }
    rebuild_schema(conn).await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA application_id = {APPLICATION_ID}"
    )))
    .execute(&mut *conn)
    .await?;
    stamp_user_version(conn, SCHEMA_FINGERPRINT).await?;
    Ok(())
}

/// Whether [`ensure_schema`] would drop tables: the database has some, and
/// its stamp is not [`SCHEMA_FINGERPRINT`]. A new or empty database is built
/// without dropping anything, so it is not a rebuild here.
///
/// # Errors
///
/// Returns an error when a query fails.
pub(crate) async fn rebuild_is_due(conn: &mut SqliteConnection) -> Result<bool> {
    Ok(user_version(conn).await? != SCHEMA_FINGERPRINT && has_user_tables(conn).await?)
}

/// The `user_version` pragma value stamped by [`migrate_schema`].
pub(crate) async fn user_version(conn: &mut SqliteConnection) -> Result<i64> {
    Ok(sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?)
}

/// Record the schema fingerprint in SQLite's `user_version` pragma.
async fn stamp_user_version(conn: &mut SqliteConnection, version: i64) -> Result<()> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA user_version = {version}"
    )))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Whether any user table exists. A fresh file has none, so a first run stays
/// quiet instead of warning about a rebuild.
async fn has_user_tables(conn: &mut SqliteConnection) -> Result<bool> {
    let tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(tables > 0)
}

/// The file of the database behind `conn`, as SQLite names it, or `None`
/// for an in-memory database, which has none.
///
/// # Errors
///
/// Returns an error when the pragma cannot be read.
pub(crate) async fn database_file(
    conn: &mut SqliteConnection,
) -> Result<Option<std::path::PathBuf>> {
    let file: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
            .fetch_one(&mut *conn)
            .await?;
    Ok((!file.is_empty()).then(|| file.into()))
}

/// Copy the database behind `conn`, stamped `stamped`, to a new file beside
/// it, and return the copy's path: `<file>.before-rebuild-<stamped>`, or
/// that name followed by `-2`, `-3` and so on when the name is taken, so no
/// copy is ever written over.
///
/// The copy is written as `<copy>.partial`, a name claimed first by
/// creating it empty with `create_new`, which only one process can do, so
/// two servers rebuilding at once never share one. `VACUUM INTO` accepts
/// that empty file, and only a whole copy is renamed to `<copy>`: a process
/// killed while copying leaves only the `.partial` file, and the copy's own
/// name is never taken by a file that is not whole. `VACUUM INTO` reads the
/// database in one transaction, so the copy is whole even while the
/// write-ahead log holds pages the file does not.
///
/// # Errors
///
/// Returns an error, naming the copy, when the database has no file (an
/// in-memory one), or the copy cannot be written, as on a full disk. Then
/// the caller leaves the database as it is.
async fn keep_copy_before_rebuild(
    conn: &mut SqliteConnection,
    stamped: i64,
) -> Result<std::path::PathBuf> {
    let Some(file) = database_file(conn).await? else {
        anyhow::bail!(
            "the database has no file to keep a copy of before its schema is rebuilt, so it was left as it is"
        );
    };
    let first = format!("{}.before-rebuild-{stamped}", file.display());
    let (copy, partial) = claim_copy_name(&first).with_context(|| {
        format!("could not claim a name for a copy of the database beside {first}, so it was left as it is")
    })?;
    if let Err(error) = copy_database(conn, &partial).await {
        // This call created the `.partial` file, so it is removed with
        // whatever `VACUUM INTO` began in it.
        let _ = std::fs::remove_file(&partial);
        anyhow::bail!(
            "could not keep a copy of the database at {} before rebuilding its schema, \
             so it was left as it is: {error}",
            copy.display()
        );
    }
    if let Err(error) = std::fs::rename(&partial, &copy) {
        // The copy is whole, so it is kept under the name it was written as.
        anyhow::bail!(
            "could not rename the copy of the database at {} to {} before rebuilding its schema, \
             so it was left as it is: {error}",
            partial.display(),
            copy.display()
        );
    }
    Ok(copy)
}

/// Copy the database `conn` is open on to a new file at `copy` with
/// `VACUUM INTO`, which reads it in one transaction, so the copy holds what
/// the write-ahead log holds too.
///
/// # Errors
///
/// Returns an error when the copy cannot be written.
pub async fn copy_database(
    conn: &mut SqliteConnection,
    copy: &std::path::Path,
) -> sqlx::Result<()> {
    sqlx::query("VACUUM INTO $1")
        .bind(copy.to_string_lossy().as_ref())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Claim the first of `first`, `first-2`, `first-3` and so on where neither
/// the copy nor its `.partial` file exists, by creating the `.partial` file
/// empty, and return both paths. `create_new` fails on a name that exists,
/// so the `.partial` file returned is one this call made, and a `.partial`
/// file left by a copy that was cut off is passed over, never reused.
fn claim_copy_name(first: &str) -> std::io::Result<(std::path::PathBuf, std::path::PathBuf)> {
    let mut n = 1;
    loop {
        let copy = if n == 1 {
            first.to_owned()
        } else {
            format!("{first}-{n}")
        };
        n += 1;
        let partial = std::path::PathBuf::from(format!("{copy}.partial"));
        let copy = std::path::PathBuf::from(copy);
        if copy.exists() {
            continue;
        }
        // Owner-only, as the database it copies (#2097); `VACUUM INTO`
        // writes into the file and keeps its mode.
        match file_io::owner_only(std::fs::OpenOptions::new().write(true).create_new(true))
            .open(&partial)
        {
            Ok(_) => return Ok((copy, partial)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

/// Drop every user table and recreate the current schema from the embedded
/// DDL. This is the only kind of migration: schema changes require a fresh
/// reload of data, never in-place column patches.
///
/// Foreign keys are turned OFF for the drop loop: SQLite's FK-aware DROP
/// processing cannot handle a schema whose remaining CREATE statements still
/// reference already-dropped tables ("no such table: main.`<dropped>`"). The
/// constraints themselves have `ON DELETE` actions, so the drops would
/// cascade cleanly; this is a schema-parse limitation, not a data one.
async fn rebuild_schema(conn: &mut SqliteConnection) -> Result<()> {
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await?;
    // A full-text search table's shadow tables are left out: dropping the
    // table drops them, and defensive mode (`crate::db::sqlite_functions`)
    // refuses to drop one on its own. `VACUUM` lists them before their
    // table, so they would come first.
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_list \
         WHERE schema = 'main' AND type IN ('table', 'virtual') AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&mut *conn)
    .await?;
    for table in &tables {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TABLE {}",
            quote_ident(table)
        )))
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await?;
    apply_ddl(conn).await?;
    Ok(())
}

/// Apply the current embedded DDL: accounts, contacts, messages, staging,
/// saved searches, then the FTS index and its sync triggers.
async fn apply_ddl(conn: &mut SqliteConnection) -> Result<()> {
    execute_batch(conn, ACCOUNTS_DDL).await?;
    // Contacts DDL defines `handles`, the FK target of conversations, participants,
    // messages, and tapbacks (messages.sql) plus account_handles (accounts.sql).
    // accounts.sql still goes first: SQLite checks a foreign key when a row is
    // written, not when the table is created.
    execute_batch(conn, CONTACTS_TABLES_DDL).await?;
    execute_batch(conn, MESSAGE_TABLES_DDL).await?;
    execute_batch(conn, STAGING_TABLES_DDL).await?;
    execute_batch(conn, SAVED_SEARCHES_DDL).await?;
    ensure_messages_fts(conn).await?;
    Ok(())
}

/// Quote `name` as a SQL identifier: wrapped in double quotes, with any
/// double quote inside it doubled.
/// Every name that reaches a `DROP TABLE` here came out of a catalog or out
/// of the embedded DDL, and this is the one place that turns such a name
/// into statement text.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Create every table and index required by a current database, and give a
/// new database its Message Crate's id ([`message_crate_id`]).
///
/// The database carries the fingerprint in `PRAGMA user_version` and is
/// rebuilt when it does not match, after a copy of it is kept beside its file
/// ([`migrate_schema`]). A rebuilt database is a new Message Crate, with a
/// new id.
///
/// # Errors
///
/// Returns an error when the file is another program's database, when a
/// rebuild is due and the copy before it cannot be written or the database
/// has no file to copy (then the database is left as it is), or when a DDL
/// statement fails.
pub async fn ensure_schema(conn: &mut SqliteConnection) -> Result<()> {
    migrate_schema(conn).await?;
    sqlx::query(
        "INSERT OR IGNORE INTO message_crate (id, message_crate_id)
         VALUES (1, lower(hex(randomblob(16))))",
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// This Message Crate's id: 32 random hexadecimal digits written when its
/// database is made, in the one row of `message_crate`. Two Message Crates
/// answering at one address, such as the desktop app's own server and a
/// Docker one at `127.0.0.1:8080`, or one rebuilt with `create-database`,
/// have different ids, though their account and Import Run ids start at the
/// same numbers.
///
/// # Errors
///
/// Returns an error when the query fails, or the database has no id because
/// [`ensure_schema`] has not run on it.
pub async fn message_crate_id(conn: &mut SqliteConnection) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT message_crate_id FROM message_crate WHERE id = 1")
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// Marker that current full-text search (FTS) sync trigger definitions are installed.
pub const MESSAGES_FTS_TRIGGERS_META_KEY: &str = "messages_fts_triggers_v1";

/// Full-text search indexes over message body/subject plus attachment text,
/// and over each earlier version of an edited message: contentless FTS5
/// virtual tables with sync triggers.
async fn ensure_messages_fts(conn: &mut SqliteConnection) -> Result<()> {
    execute_batch(conn, FTS_VIRTUAL_DDL).await?;

    let triggers_ready: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_meta WHERE key = $1")
        .bind(MESSAGES_FTS_TRIGGERS_META_KEY)
        .fetch_one(&mut *conn)
        .await?;
    if triggers_ready == 0 {
        install_messages_fts_triggers(conn).await?;
    }

    Ok(())
}

/// Drop full-text search sync triggers (used during bulk promote so inserts skip
/// per-row indexing).
///
/// # Errors
///
/// Returns an error when the drop statements fail.
pub(crate) async fn drop_messages_fts_triggers(conn: &mut SqliteConnection) -> Result<()> {
    execute_batch(conn, DROP_MESSAGES_FTS_TRIGGERS_SQL).await?;
    sqlx::query("DELETE FROM schema_meta WHERE key = $1")
        .bind(MESSAGES_FTS_TRIGGERS_META_KEY)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Install full-text search sync triggers and mark them ready in `schema_meta`.
/// The triggers are dropped first, so installing twice is safe.
///
/// # Errors
///
/// Returns an error when the trigger SQL or metadata write fails.
pub(crate) async fn install_messages_fts_triggers(conn: &mut SqliteConnection) -> Result<()> {
    execute_batch(conn, DROP_MESSAGES_FTS_TRIGGERS_SQL).await?;
    execute_batch(conn, CREATE_MESSAGES_FTS_TRIGGERS_SQL).await?;
    sqlx::query(
        "INSERT INTO schema_meta (key, value) VALUES ($1, '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(MESSAGES_FTS_TRIGGERS_META_KEY)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Non-unique indexes on `messages` (kept out of bulk promote inserts, then rebuilt).
/// Unique `ix_messages_account_source_guid` stays in place for `INSERT OR IGNORE` dedup.
const MESSAGES_SECONDARY_INDEX_DDL: &[(&str, &str)] = &[
    (
        "ix_messages_conversation_timestamp",
        "CREATE INDEX IF NOT EXISTS ix_messages_conversation_timestamp ON messages (conversation_id, timestamp)",
    ),
    (
        "ix_messages_conversation_source_timestamp",
        "CREATE INDEX IF NOT EXISTS ix_messages_conversation_source_timestamp ON messages (conversation_id, source, timestamp)",
    ),
    (
        "ix_messages_account_id",
        "CREATE INDEX IF NOT EXISTS ix_messages_account_id ON messages (account_id)",
    ),
    (
        "ix_messages_content_key",
        "CREATE INDEX IF NOT EXISTS ix_messages_content_key ON messages (content_key) WHERE content_key IS NOT NULL AND content_key != ''",
    ),
    (
        "ix_messages_duplicate_of",
        "CREATE INDEX IF NOT EXISTS ix_messages_duplicate_of ON messages (duplicate_of) WHERE duplicate_of IS NOT NULL",
    ),
    (
        "ix_messages_import_id",
        "CREATE INDEX IF NOT EXISTS ix_messages_import_id ON messages (import_id) WHERE import_id IS NOT NULL",
    ),
    (
        "ix_messages_source",
        "CREATE INDEX IF NOT EXISTS ix_messages_source ON messages (source)",
    ),
];

/// Drop secondary `messages` indexes during bulk promote (same transaction as
/// the promote inserts).
pub(crate) async fn drop_messages_secondary_indexes(conn: &mut SqliteConnection) -> Result<()> {
    for (name, _) in MESSAGES_SECONDARY_INDEX_DDL {
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP INDEX IF EXISTS {name}")))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Recreate secondary `messages` indexes after bulk promote inserts.
pub(crate) async fn create_messages_secondary_indexes(conn: &mut SqliteConnection) -> Result<()> {
    for (_, ddl) in MESSAGES_SECONDARY_INDEX_DDL {
        sqlx::query(*ddl).execute(&mut *conn).await?;
    }
    Ok(())
}

/// The stored messages, those at or below `$1` (`min_new_message_id`), whose
/// index row a promotion removes and writes again: each one that gained an
/// attachment above `$2` (`min_new_attachment_id`), took a later edit, or
/// whose mark `staging::promote_deletion_marks` changed, set or cleared
/// (`_promote_mark_map`). One set for the delete and the insert in
/// [`index_messages_fts_from_promote_map`], so a row removed is always a row
/// considered for writing again.
const STORED_MESSAGES_TO_REINDEX: &str = "
    SELECT message_id FROM attachments
    WHERE id > $2 AND message_id <= $1
    UNION
    SELECT prod_id FROM _promote_edit_map
    UNION
    SELECT prod_id FROM _promote_mark_map WHERE prod_id <= $1";

/// Bulk-index promoted messages (joined via temp `_promote_msg_map`).
/// Call after attachment rows exist so `attachment_text` is complete.
/// Inserts into the contentless `messages_fts` table.
///
/// `_promote_msg_map` also targets messages that already existed before this
/// promotion (so attachments and tapbacks can attach to them), and several
/// staging rows can point at one production row. Those rows are already
/// indexed, so `min_new_message_id`, the highest `messages.id` that existed
/// before this promotion inserted anything, keeps them out: only distinct
/// production ids above it are indexed here.
///
/// The exception is an existing message that gained an attachment in this
/// promotion, which an append does when the first import lacked it. Its
/// index row holds the old attachment text, so it is removed and written
/// again whole. `min_new_attachment_id`, the highest `attachments.id` that
/// existed before the attachments were promoted, names those messages: an
/// attachment above it was inserted by this promotion. The same goes for an
/// existing message that took a later edit, which `_promote_edit_map`
/// names: its index row holds the text the edit replaced, and for an
/// existing message whose mark `staging::promote_deletion_marks` changed,
/// which `_promote_mark_map` names.
///
/// An Unsent message is given no index row, as the sync triggers give it
/// none (`fts_triggers_create.sql`): it shows none of its text, so a word of
/// that text must not find it (#1758). A stored message this promotion marks
/// Unsent loses its row here, and one whose Unsent mark becomes Deleted in
/// the source app, or is cleared by a later backup, is written again.
pub(crate) async fn index_messages_fts_from_promote_map(
    conn: &mut SqliteConnection,
    min_new_message_id: i64,
    min_new_attachment_id: i64,
) -> Result<u64> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM messages_fts WHERE rowid IN ({STORED_MESSAGES_TO_REINDEX})"
    )))
    .bind(min_new_message_id)
    .bind(min_new_attachment_id)
    .execute(&mut *conn)
    .await?;
    let n = sqlx::query(sqlx::AssertSqlSafe(format!(
        r"
        INSERT INTO messages_fts(rowid, body, subject, attachment_text)
        SELECT
            m.id,
            coalesce(m.body, ''),
            coalesce(m.subject, ''),
            coalesce((
                SELECT group_concat(
                    trim(coalesce(a.original_name, '') || ' ' || coalesce(a.transcription, '')),
                    ' '
                )
                FROM attachments a
                WHERE a.message_id = m.id
            ), '')
        FROM (
            SELECT prod_id FROM _promote_msg_map WHERE prod_id > $1
            UNION
            {STORED_MESSAGES_TO_REINDEX}
        ) mm
        JOIN messages m ON m.id = mm.prod_id
        WHERE m.deletion IS NOT 'unsent'
        "
    )))
    .bind(min_new_message_id)
    .bind(min_new_attachment_id)
    .execute(&mut *conn)
    .await?;
    Ok(n.rows_affected())
}

/// Remove the search entries of the earlier versions held by the messages
/// `_promote_edit_map` names, before `staging::promote_later_edits` deletes
/// the versions: the sync triggers are paused during a promotion, and an
/// entry left behind would be found under a version id the next version
/// inserted may take.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub(crate) async fn unindex_versions_of_edited_messages(
    conn: &mut SqliteConnection,
) -> Result<u64> {
    let n = sqlx::query(
        "DELETE FROM message_versions_fts
         WHERE rowid IN (
             SELECT v.id FROM message_versions v
             JOIN _promote_edit_map em ON em.prod_id = v.message_id
         )",
    )
    .execute(&mut *conn)
    .await?;
    Ok(n.rows_affected())
}

/// Index the earlier versions above `versions_before`, the highest
/// `message_versions.id` that existed before this promotion inserted any:
/// the ones the promotion added while the sync triggers were paused. A
/// version with no text has nothing to find it by, so it is not indexed, as
/// the sync trigger (`fts_triggers_create.sql`) leaves it out.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub(crate) async fn index_message_versions_fts(
    conn: &mut SqliteConnection,
    versions_before: i64,
) -> Result<u64> {
    let n = sqlx::query(
        "INSERT INTO message_versions_fts(rowid, text)
         SELECT id, text FROM message_versions WHERE id > $1 AND text IS NOT NULL",
    )
    .bind(versions_before)
    .execute(&mut *conn)
    .await?;
    Ok(n.rows_affected())
}

/// Message ids for one source within one account, bound as `$1` = source, `$2` = account.
const MESSAGE_IDS_FOR_SOURCE: &str = "SELECT m.id FROM messages m \
     JOIN conversations c ON c.id = m.conversation_id \
     WHERE m.source = $1 AND c.account_id = $2";

/// Delete all production messages (and cascaded rows) for one import source within one account.
///
/// # Errors
///
/// Returns an error when a delete or update statement fails.
pub async fn delete_messages_for_source(
    conn: &mut SqliteConnection,
    account_id: i64,
    source: &str,
) -> Result<u64> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM attachments WHERE message_id IN ({MESSAGE_IDS_FOR_SOURCE})"
    )))
    .bind(source)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM tapbacks WHERE message_id IN ({MESSAGE_IDS_FOR_SOURCE})"
    )))
    .bind(source)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE messages SET duplicate_of = NULL
         WHERE duplicate_of IN ({MESSAGE_IDS_FOR_SOURCE})"
    )))
    .bind(source)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    let n = sqlx::query(
        r"
        DELETE FROM messages
        WHERE source = $1
          AND conversation_id IN (
              SELECT id FROM conversations WHERE account_id = $2
          )
        ",
    )
    .bind(source)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(n.rows_affected())
}

/// True when `table` exists. [`crate::process_assets::run`] uses it to fall
/// back to the account directories on a database with no `accounts` table.
pub async fn table_exists(conn: &mut SqliteConnection, name: &str) -> Result<bool> {
    let found: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = $1")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?;
    Ok(found > 0)
}

/// Column names of `table` in ordinal order.
#[cfg(test)]
async fn table_columns(conn: &mut SqliteConnection, table: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT name FROM pragma_table_info($1)")
        .bind(table)
        .fetch_all(&mut *conn)
        .await?)
}

/// True when an index named `name` exists.
#[cfg(test)]
async fn index_exists(conn: &mut SqliteConnection, name: &str) -> Result<bool> {
    let found: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = $1")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?;
    Ok(found > 0)
}

/// True when a trigger named `name` exists.
#[cfg(test)]
async fn trigger_exists(conn: &mut SqliteConnection, name: &str) -> Result<bool> {
    let found: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = $1",
    )
    .bind(name)
    .fetch_one(&mut *conn)
    .await?;
    Ok(found > 0)
}

/// Split a multi-statement DDL batch into individual statements.
///
/// The schema files follow a fixed format: comments are whole `--` lines,
/// ordinary statements end with `;` at end of line, and the only multi-line
/// statements are trigger bodies, which end in a line ending with `END;`.
pub fn split_ddl(batch: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_trigger = false;
    for line in batch.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        if trimmed.starts_with("CREATE TRIGGER") {
            in_trigger = true;
        }
        current.push_str(line);
        current.push('\n');
        if in_trigger {
            if trimmed.ends_with("END;") {
                statements.push(current.trim_end().to_string());
                current.clear();
                in_trigger = false;
            }
        } else if trimmed.ends_with(';') {
            statements.push(current.trim_end().to_string());
            current.clear();
        }
    }
    debug_assert!(
        current.trim().is_empty(),
        "unterminated DDL statement: {current}"
    );
    statements
}

/// Run every statement in a DDL batch against one connection.
async fn execute_batch(conn: &mut SqliteConnection, batch: &str) -> Result<()> {
    for stmt in split_ddl(batch) {
        sqlx::query(sqlx::AssertSqlSafe(stmt))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
