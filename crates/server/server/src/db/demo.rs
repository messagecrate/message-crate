//! The reads a `reset-demo` run makes to prove it left every other account
//! as it was: every table's rows outside the Demo Account, the search
//! indexes' entries and what a search finds there, and the Audit Trail rows
//! that outlive an account.
//!
//! These reads go across every table, so they have no table's module to
//! live in, and are kept here, for `reset_demo`'s verifier
//! (`reset_demo/verify.rs`), which hashes and compares what they read. Each
//! of the build's statements that is about one table is in that table's
//! module: the Demo Account's row in [`crate::db::account_profile`], the
//! last Audit Trail entry in [`crate::db::audit_trail`], the database copy
//! in [`crate::db::schema`], the checkpoint in [`crate::db::maintenance`].

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use sqlx::{AssertSqlSafe, SqlSafeStr, SqliteConnection};

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

/// One table of the database, as [`tables`] lists it.
pub struct Table {
    /// The table's name.
    pub name: String,
    /// Whether it is a virtual table, such as a search index.
    pub is_virtual: bool,
}

/// Every table of the database but SQLite's own, by name.
///
/// # Errors
///
/// Returns an error when the schema cannot be read.
pub async fn tables(conn: &mut SqliteConnection) -> Result<Vec<Table>> {
    let tables: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, COALESCE(sql, '') FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(tables
        .into_iter()
        .map(|(name, sql)| Table {
            name,
            is_virtual: sql
                .trim_start()
                .to_ascii_uppercase()
                .starts_with("CREATE VIRTUAL TABLE"),
        })
        .collect())
}

/// Pass every row of `table` that is not one of `demo`'s to `add`, as one
/// line of text, in a fixed order: the rows of every other account, the
/// owner's among them, and the rows that belong to no account, such as
/// `server_settings`. A row belongs to the account in its `account_id`
/// column, or, for a table without one, to the account of the row a
/// `NOT NULL` foreign key points at; a row whose account column is NULL,
/// such as the Import Run of a deleted account, belongs to no account and is
/// passed, unless the Demo Account's deletion during the reset unlinked it
/// ([`DemoRows`]).
///
/// # Errors
///
/// Returns an error when the table cannot be read, or its rows cannot be
/// told apart by account ([`row_owner`]).
pub async fn each_row_outside(
    conn: &mut SqliteConnection,
    table: &str,
    demo: DemoRows,
    mut add: impl FnMut(&str),
) -> Result<()> {
    use futures_util::TryStreamExt;

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
    let mut rows = query.fetch(&mut *conn);
    while let Some(row) = rows
        .try_next()
        .await
        .with_context(|| format!("read {table}"))?
    {
        add(&row);
    }
    Ok(())
}

/// One full-text search index, the only virtual tables the schema has, and
/// how an entry's rowid reaches the message whose account it belongs to.
pub struct SearchIndex {
    /// The FTS5 table.
    pub table: &'static str,
    /// Joins that bind `m` to the message the entry `{rowid}` belongs to,
    /// `LEFT` so an entry whose rows are gone is still read.
    owner_join: &'static str,
}

/// The search index over messages, whose rowid is the message's id, and the
/// one over earlier versions, whose rowid is the version's id.
pub const SEARCH_INDEXES: [SearchIndex; 2] = [
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

/// Pass each entry outside the Demo Account that a search index holds to
/// `add`, as its id and how many terms it holds in each column, read from
/// the index's `_docsize` table, in id order. An entry whose message is gone
/// belongs to no account and is passed too. Deleting a message's entry
/// removes its row there (#1450).
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub async fn each_index_entry_outside(
    conn: &mut SqliteConnection,
    index: &SearchIndex,
    demo_id: i64,
    mut add: impl FnMut(&str),
) -> Result<()> {
    use futures_util::TryStreamExt;

    let table = index.table;
    let sql = format!(
        "SELECT d.id || ',' || hex(d.sz)
         FROM {table}_docsize d {join}
         WHERE m.account_id IS NOT $1
         ORDER BY d.id",
        join = index.owner_join("d.id"),
    );
    let mut rows = sqlx::query_scalar::<_, String>(AssertSqlSafe(sql))
        .bind(demo_id)
        .fetch(&mut *conn);
    while let Some(row) = rows
        .try_next()
        .await
        .with_context(|| format!("read {table}"))?
    {
        add(&row);
    }
    Ok(())
}

/// Up to `limit` message bodies outside the Demo Account, picked at random.
///
/// # Errors
///
/// Returns an error when the messages cannot be read.
pub async fn sample_bodies_outside(
    conn: &mut SqliteConnection,
    demo_id: i64,
    limit: i64,
) -> Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT body FROM messages
         WHERE account_id IS NOT $1 AND body IS NOT NULL
         ORDER BY random() LIMIT $2",
    )
    .bind(demo_id)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await
    .context("pick messages to search")
}

/// Search a search index for each of `terms` as a phrase, the way a search
/// runs it, and pass each entry outside the Demo Account it finds to `add`,
/// with the term, in rowid order. An entry whose message is gone is found
/// too. Passed as they come: a common word finds most messages.
///
/// # Errors
///
/// Returns an error when a search fails.
pub async fn each_match_outside(
    conn: &mut SqliteConnection,
    index: &SearchIndex,
    demo_id: i64,
    terms: &[String],
    mut add: impl FnMut(&str, i64),
) -> Result<()> {
    use futures_util::TryStreamExt;

    let table = index.table;
    let sql = AssertSqlSafe(format!(
        "SELECT f.rowid FROM {table} f {join}
         WHERE {table} MATCH $1 AND m.account_id IS NOT $2
         ORDER BY f.rowid",
        join = index.owner_join("f.rowid"),
    ))
    .into_sql_str();
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
            add(term, message);
        }
    }
    Ok(())
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
