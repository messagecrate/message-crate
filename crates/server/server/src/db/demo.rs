//! The SQL of a Demo Account build and of the `reset-demo` command: the
//! Demo Account's row, the copy of the database a reset rebuilds, the
//! checkpoint before a file is moved, and the reads that prove a reset left
//! every other account as it was.
//!
//! `reset_demo` sequences these; every statement it runs is here, as
//! `docs/architecture/http-api.md` says every query lives in `db/`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use sqlx::{AssertSqlSafe, SqlSafeStr, SqliteConnection};

/// Write the Demo Account's row at `account_id`, named `username` and
/// `preferred_name`, with no password, and take every identity off it, so
/// the build links the seed's alone.
///
/// The Demo Account has no password, so anyone at the login card can enter
/// it. The server takes its grant from its id (`DEMO_ACCOUNT_PERMISSIONS`)
/// and does not read these flags for it. The row still says the same grant,
/// export and neither import nor delete, so the database matches what the
/// server applies (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn write_demo_account_row(
    conn: &mut SqliteConnection,
    account_id: i64,
    username: &str,
    preferred_name: &str,
) -> Result<()> {
    sqlx::query(
        r"
        INSERT INTO accounts (
            id, username, password_hash, preferred_name, can_import, can_export, can_delete
        )
        VALUES ($1, $2, NULL, $3, 0, 1, 0)
        ON CONFLICT(id) DO UPDATE SET
            username = excluded.username,
            preferred_name = excluded.preferred_name,
            can_import = excluded.can_import,
            can_export = excluded.can_export,
            can_delete = excluded.can_delete
        ",
    )
    .bind(account_id)
    .bind(username)
    .bind(preferred_name)
    .execute(&mut *conn)
    .await?;
    // Phone and email identities that mark messages as from "you" live in
    // `handles`, linked through `account_handles`.
    sqlx::query("DELETE FROM account_handles WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Copy the database `conn` is open on to a new file at `copy` with
/// `VACUUM INTO`, which reads it in one transaction, so the copy holds what
/// the write-ahead log holds too.
///
/// # Errors
///
/// Returns an error when the copy cannot be written.
pub async fn copy_database(conn: &mut SqliteConnection, copy: &Path) -> sqlx::Result<()> {
    sqlx::query("VACUUM INTO $1")
        .bind(copy.to_string_lossy().as_ref())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Copy every page of the write-ahead log into the database file and
/// truncate the log (`PRAGMA wal_checkpoint(TRUNCATE)`). Returns whether a
/// reader or writer kept the checkpoint from finishing.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn checkpoint_truncate(conn: &mut SqliteConnection) -> sqlx::Result<bool> {
    let (busy, _log, _checkpointed) =
        sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&mut *conn)
            .await?;
    Ok(busy != 0)
}

/// The id of the last Audit Trail entry, or 0 when there is none.
pub async fn last_audit_entry(conn: &mut SqliteConnection) -> Result<i64> {
    sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM audit_entries")
        .fetch_one(&mut *conn)
        .await
        .context("read the last Audit Trail entry")
}

/// The rows a reset may change: those of the Demo Account, `account`, and
/// those its deletion during the reset unlinked from it. A deleted account's
/// entries and runs keep no account id; they name the `account_deleted`
/// entry instead (ADR 0020). An `account_deleted` entry after
/// `last_entry_before_reset`, the last entry of the active database, is the
/// one the reset wrote when it deleted the Demo Account. The reset's own
/// `demo_account_reset` entry, written just before, is among the entries
/// that deletion unlinked. The entry of a reset that added the Demo Account
/// names no deletion ([`build_recorded_by_reset_sql`]).
#[derive(Debug, Clone, Copy)]
pub struct DemoRows {
    /// The Demo Account's id.
    pub account: i64,
    /// The id of the last Audit Trail entry in the active database.
    pub last_entry_before_reset: i64,
}

/// The column a row that outlives its account names its `account_deleted`
/// entry in (ADR 0020).
const DELETION_ENTRY_COLUMN: &str = "deletion_entry_id";

/// The SQL condition that the entry id in `column` names an
/// `account_deleted` entry written after the entry id bound to `param`: one
/// the reset's deletion of the Demo Account wrote.
fn unlinked_by_reset_sql(column: &str, param: &str) -> String {
    format!(
        "{column} IN (SELECT id FROM audit_entries
                      WHERE action = 'account_deleted' AND id > {param})"
    )
}

/// The SQL condition that an `audit_entries` row, `t`, is the entry a reset
/// that found no Demo Account wrote of its own build, after the entry id
/// bound to `param`: `demo_account_added`. It is about no account
/// ([`crate::db::audit_trail::record_demo_build`]), so no deletion unlinks
/// it and nothing else marks it as the reset's own. A reset that replaced a
/// Demo Account wrote `demo_account_reset` about that account, which the
/// account's deletion unlinked ([`unlinked_by_reset_sql`]).
fn build_recorded_by_reset_sql(param: &str) -> String {
    format!(
        "t.action = 'demo_account_added'
         AND t.actor = 'command_line' AND t.account_id IS NULL
         AND t.{DELETION_ENTRY_COLUMN} IS NULL AND t.id > {param}"
    )
}

/// Which rows [`outliving_rows`] reads.
#[derive(Debug, Clone, Copy)]
pub enum OutlivingRows {
    /// The rows still linked to this account.
    OfAccount(i64),
    /// The rows a reset unlinked, by an `account_deleted` entry written
    /// after this entry id.
    UnlinkedAfter(i64),
}

/// The ids of the rows `which` picks in every table whose rows outlive their
/// account: one with `account_id` and [`DELETION_ENTRY_COLUMN`].
pub async fn outliving_rows(
    conn: &mut SqliteConnection,
    which: OutlivingRows,
) -> Result<BTreeMap<String, BTreeSet<i64>>> {
    let (filter, value) = match which {
        OutlivingRows::OfAccount(account) => ("account_id = $1".to_owned(), account),
        OutlivingRows::UnlinkedAfter(entry) => {
            (unlinked_by_reset_sql(DELETION_ENTRY_COLUMN, "$1"), entry)
        }
    };
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT m.name FROM sqlite_master m
         WHERE m.type = 'table'
           AND EXISTS (SELECT 1 FROM pragma_table_info(m.name) WHERE name = 'account_id')
           AND EXISTS (SELECT 1 FROM pragma_table_info(m.name) WHERE name = $1)
         ORDER BY m.name",
    )
    .bind(DELETION_ENTRY_COLUMN)
    .fetch_all(&mut *conn)
    .await?;
    let mut rows = BTreeMap::new();
    for table in tables {
        let ids: Vec<i64> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT id FROM {} WHERE {filter}",
            quote_ident(&table)
        )))
        .bind(value)
        .fetch_all(&mut *conn)
        .await
        .with_context(|| format!("read {table}"))?;
        rows.insert(table, ids.into_iter().collect());
    }
    Ok(rows)
}

/// The rows of one table that a reset must leave as they were: how many, and
/// a SHA-256 over all of them in a fixed order.
#[derive(Debug, PartialEq, Eq)]
pub struct TableDigest {
    /// How many rows the table holds outside the Demo Account.
    pub rows: u64,
    /// A SHA-256 over those rows, in hex.
    pub sha256: String,
}

/// Tables a reset compares no rows of. `demo_account_build` is the build's
/// own record; it is written and removed by the build itself.
const UNCOMPARED_TABLES: [&str; 1] = ["demo_account_build"];

/// A digest per table of every row that is not one of `demo`'s:
/// the rows of every other account, the owner's among them, and the rows that
/// belong to no account, such as `server_settings`. A row belongs to the
/// account in its `account_id` column, or, for a table without one, to the
/// account of the row a `NOT NULL` foreign key points at; a row whose
/// account column is NULL, such as the Import Run of a deleted account,
/// belongs to no account and is compared, unless the Demo Account's deletion
/// during the reset unlinked it ([`DemoRows`]).
///
/// The search index is contentless, so it has no rows to read back. It is
/// compared by which messages it holds and how many terms each has
/// ([`search_index_digest`]), and by what `search_terms` find
/// ([`search_sample_digest`]). Its other shadow tables are left out, because
/// their bytes differ for the same entries.
pub async fn non_demo_state_on_conn(
    conn: &mut SqliteConnection,
    demo: DemoRows,
    search_terms: &[String],
) -> Result<BTreeMap<String, TableDigest>> {
    let tables: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, COALESCE(sql, '') FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await?;
    let virtual_tables: Vec<&str> = tables
        .iter()
        .filter(|(_, sql)| {
            sql.trim_start()
                .to_ascii_uppercase()
                .starts_with("CREATE VIRTUAL TABLE")
        })
        .map(|(name, _)| name.as_str())
        .collect();
    let mut state = BTreeMap::new();
    for table in &virtual_tables {
        let Some(index) = SEARCH_INDEXES.iter().find(|index| index.table == *table) else {
            bail!("cannot compare the virtual table {table}");
        };
        state.insert(
            (*table).to_owned(),
            search_index_digest(conn, index, demo.account).await?,
        );
        state.insert(
            format!("{table} searches"),
            search_sample_digest(conn, index, demo.account, search_terms).await?,
        );
    }
    for (table, _) in &tables {
        let is_index = virtual_tables
            .iter()
            .any(|vt| table == vt || table.starts_with(&format!("{vt}_")));
        if is_index || UNCOMPARED_TABLES.contains(&table.as_str()) {
            continue;
        }
        let columns = table_columns(conn, table).await?;
        let row_text = columns
            .iter()
            .map(|column| format!("quote(t.{})", quote_ident(column)))
            .collect::<Vec<_>>()
            .join(" || ',' || ");
        let owner = row_owner(conn, table, &columns).await?;
        let selection = match &owner {
            // `IS NOT`, so a row whose account is NULL is kept (#1450).
            Some(owner) => format!(
                "FROM {} t {} WHERE {} IS NOT $1",
                quote_ident(table),
                owner.join,
                owner.account
            ),
            None => format!("FROM {} t", quote_ident(table)),
        };
        let unlinked = owner.as_ref().and_then(|owner| owner.deletion.as_ref());
        let selection = match unlinked {
            Some(deletion) => format!(
                "{selection} AND NOT COALESCE({}, 0)",
                unlinked_by_reset_sql(deletion, "$2")
            ),
            None => selection,
        };
        let selection = if table == "audit_entries" && unlinked.is_some() {
            format!(
                "{selection} AND NOT ({})",
                build_recorded_by_reset_sql("$2")
            )
        } else {
            selection
        };
        let sql = format!("SELECT {row_text} {selection} ORDER BY 1");
        let query = sqlx::query_scalar::<_, String>(AssertSqlSafe(sql));
        // Only a table whose rows belong to accounts leaves the Demo
        // Account's rows out.
        let query = match (&owner, unlinked) {
            (Some(_), Some(_)) => query.bind(demo.account).bind(demo.last_entry_before_reset),
            (Some(_), None) => query.bind(demo.account),
            (None, _) => query,
        };
        let digest = digest_rows(query.fetch(&mut *conn))
            .await
            .with_context(|| format!("read {table}"))?;
        state.insert(table.clone(), digest);
    }
    Ok(state)
}

/// A [`TableDigest`] being built, one row at a time.
#[derive(Default)]
struct DigestBuilder {
    hasher: sha2::Sha256,
    rows: u64,
}

impl DigestBuilder {
    fn add(&mut self, row: &str) {
        use sha2::Digest;
        self.hasher.update(row.as_bytes());
        self.hasher.update(b"\n");
        self.rows += 1;
    }

    fn finish(self) -> TableDigest {
        use sha2::Digest;
        TableDigest {
            rows: self.rows,
            sha256: hex::encode(self.hasher.finalize()),
        }
    }
}

/// The count of `rows` and a SHA-256 over them, in the order they come.
async fn digest_rows(
    mut rows: impl futures_util::Stream<Item = Result<String, sqlx::Error>> + Unpin,
) -> Result<TableDigest> {
    use futures_util::TryStreamExt;

    let mut digest = DigestBuilder::default();
    while let Some(row) = rows.try_next().await? {
        digest.add(&row);
    }
    Ok(digest.finish())
}

/// One full-text search index, the only virtual tables the schema has, and
/// how an entry's rowid reaches the message whose account it belongs to.
struct SearchIndex {
    /// The FTS5 table.
    table: &'static str,
    /// Joins that bind `m` to the message the entry `{rowid}` belongs to,
    /// `LEFT` so an entry whose rows are gone is still read.
    owner_join: &'static str,
}

/// The search index over messages, whose rowid is the message's id, and the
/// one over earlier versions, whose rowid is the version's id.
const SEARCH_INDEXES: [SearchIndex; 2] = [
    SearchIndex {
        table: "messages_fts",
        owner_join: "LEFT JOIN messages m ON m.id = {rowid}",
    },
    SearchIndex {
        table: "message_versions_fts",
        owner_join: "LEFT JOIN message_versions v ON v.id = {rowid} \
                     LEFT JOIN messages m ON m.id = v.message_id",
    },
];

impl SearchIndex {
    /// [`Self::owner_join`] for the entry whose rowid is `rowid`.
    fn owner_join(&self, rowid: &str) -> String {
        self.owner_join.replace("{rowid}", rowid)
    }
}

/// Which entries outside the Demo Account a search index holds, and how many
/// terms each holds in each column, read from its `_docsize` table. An entry
/// whose message is gone belongs to no account and is read too. Deleting a
/// message's entry removes its row there (#1450).
async fn search_index_digest(
    conn: &mut SqliteConnection,
    index: &SearchIndex,
    demo_id: i64,
) -> Result<TableDigest> {
    let table = index.table;
    let sql = format!(
        "SELECT d.id || ',' || hex(d.sz)
         FROM {table}_docsize d {join}
         WHERE m.account_id IS NOT $1
         ORDER BY d.id",
        join = index.owner_join("d.id"),
    );
    let rows = sqlx::query_scalar::<_, String>(AssertSqlSafe(sql))
        .bind(demo_id)
        .fetch(&mut *conn);
    digest_rows(rows)
        .await
        .with_context(|| format!("read {table}"))
}

/// How many messages outside the Demo Account [`sample_search_terms`] takes
/// its terms from, and the most terms it takes.
const SEARCH_SAMPLE_MESSAGES: i64 = 16;
const SEARCH_SAMPLE_TERMS: usize = 32;

/// Up to [`SEARCH_SAMPLE_TERMS`] words from [`SEARCH_SAMPLE_MESSAGES`]
/// messages outside the Demo Account, picked at random: the searches
/// [`search_sample_digest`] runs on both databases.
pub async fn sample_search_terms(conn: &mut SqliteConnection, demo_id: i64) -> Result<Vec<String>> {
    let bodies: Vec<String> = sqlx::query_scalar(
        "SELECT body FROM messages
         WHERE account_id IS NOT $1 AND body IS NOT NULL
         ORDER BY random() LIMIT $2",
    )
    .bind(demo_id)
    .bind(SEARCH_SAMPLE_MESSAGES)
    .fetch_all(&mut *conn)
    .await
    .context("pick messages to search")?;
    let mut terms = BTreeSet::new();
    for body in bodies {
        for word in body.split(|c: char| !c.is_alphanumeric()) {
            if terms.len() == SEARCH_SAMPLE_TERMS {
                return Ok(terms.into_iter().collect());
            }
            if !word.is_empty() {
                terms.insert(word.to_lowercase());
            }
        }
    }
    Ok(terms.into_iter().collect())
}

/// Every entry outside the Demo Account each of `terms` finds in a search
/// index, as a search runs it. An entry whose message is gone is
/// found too. Two indexes that hold the same messages with the same term
/// counts ([`search_index_digest`]) can still find different messages for a
/// word; this catches that for a sample of words (#1450).
async fn search_sample_digest(
    conn: &mut SqliteConnection,
    index: &SearchIndex,
    demo_id: i64,
    terms: &[String],
) -> Result<TableDigest> {
    use futures_util::TryStreamExt;

    let table = index.table;
    let sql = AssertSqlSafe(format!(
        "SELECT f.rowid FROM {table} f {join}
         WHERE {table} MATCH $1 AND m.account_id IS NOT $2
         ORDER BY f.rowid",
        join = index.owner_join("f.rowid"),
    ))
    .into_sql_str();
    // Hashed as they come: a common word finds most messages.
    let mut digest = DigestBuilder::default();
    for term in terms {
        let phrase = format!("\"{}\"", term.replace('"', "\"\""));
        let mut found = sqlx::query_scalar::<_, i64>(sql.clone())
            .bind(&phrase)
            .bind(demo_id)
            .fetch(&mut *conn);
        while let Some(message) = found
            .try_next()
            .await
            .with_context(|| format!("search {table} for {phrase}"))?
        {
            digest.add(&format!("{term},{message}"));
        }
    }
    Ok(digest.finish())
}

/// How to find the account a row of a table belongs to: a join to add after
/// the table, aliased `t`, the expression that names the account, and, for a
/// row that outlives its account, the expression that names the
/// `account_deleted` entry it was unlinked by.
struct RowOwner {
    join: String,
    account: String,
    deletion: Option<String>,
}

/// The account a row of `table` belongs to, or `None` for a table whose rows
/// belong to no account (it has no `account_id` and no foreign key).
///
/// # Errors
///
/// Returns an error for a table with a foreign key but no `NOT NULL` one to a
/// table that has `account_id`: its rows cannot be told apart, and a new
/// table like that needs a rule here.
async fn row_owner(
    conn: &mut SqliteConnection,
    table: &str,
    columns: &[String],
) -> Result<Option<RowOwner>> {
    if table == "accounts" {
        return Ok(Some(RowOwner {
            join: String::new(),
            account: "t.id".into(),
            deletion: None,
        }));
    }
    let deletion = |alias: &str, columns: &[String]| {
        columns
            .iter()
            .any(|column| column == DELETION_ENTRY_COLUMN)
            .then(|| format!("{alias}.{DELETION_ENTRY_COLUMN}"))
    };
    if columns.iter().any(|column| column == "account_id") {
        return Ok(Some(RowOwner {
            join: String::new(),
            account: "t.account_id".into(),
            deletion: deletion("t", columns),
        }));
    }
    let keys: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT fk.\"table\", fk.\"from\", fk.\"to\"
         FROM pragma_foreign_key_list($1) fk
         JOIN pragma_table_info($1) col ON col.name = fk.\"from\"
         WHERE col.\"notnull\" = 1
         ORDER BY fk.id",
    )
    .bind(table)
    .fetch_all(&mut *conn)
    .await?;
    for (parent, from, to) in &keys {
        let parent_columns = table_columns(conn, parent).await?;
        if parent_columns.iter().any(|c| c == "account_id") {
            return Ok(Some(RowOwner {
                join: format!(
                    "JOIN {} p ON p.{} = t.{}",
                    quote_ident(parent),
                    quote_ident(to),
                    quote_ident(from)
                ),
                account: "p.account_id".into(),
                deletion: deletion("p", &parent_columns),
            }));
        }
    }
    let any_key: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_list($1)")
        .bind(table)
        .fetch_one(&mut *conn)
        .await?;
    if any_key > 0 {
        bail!("cannot tell which account a row of {table} belongs to");
    }
    Ok(None)
}

/// The column names of `table`, in declaration order.
async fn table_columns(conn: &mut SqliteConnection, table: &str) -> Result<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT name FROM pragma_table_info($1) ORDER BY cid")
            .bind(table)
            .fetch_all(&mut *conn)
            .await?,
    )
}

/// `name` as a quoted SQLite identifier.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
