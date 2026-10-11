//! The statements of the dedupe (`crate::dedupe`): every read and write of
//! `messages.content_key` and `messages.duplicate_of` the dedupe makes, the
//! reads of participants and attachments its content key and near-time pass
//! need, and the temp tables (`_content_keys` and the flag tables) its writes
//! go through.
//!
//! `crate::dedupe` sequences these statements, hashes the content keys,
//! picks the copy shown and keeps the counts, and holds no SQL
//! (`docs/architecture/http-api.md`, "Code").

use std::collections::HashMap;
use std::fmt::Write as _;

use anyhow::Result;
use sqlx::SqliteConnection;

use super::schema;
use super::sql::SQLITE_IN_CHUNK;
use crate::progress::Progress;

const CONTENT_KEY_WRITE_LOG_EVERY: usize = 50_000;

/// Whether the message `m` has a content key, as a SQL condition: whether a
/// dedupe, or an import that filled the keys, has seen it.
pub(crate) const HAS_CONTENT_KEY_SQL: &str = "m.content_key IS NOT NULL AND m.content_key != ''";

/// SQL for the Import Run size of each message: join it as `rc` on
/// `rc.import_id = m.import_id` and read `COALESCE(rc.n, 0)`. The account is
/// bound as `$1`.
const RUN_MESSAGES_SQL: &str = "SELECT import_id, COUNT(*) AS n
            FROM messages
            WHERE account_id = $1 AND import_id IS NOT NULL
            GROUP BY import_id";

/// The sender a message is matched by, as a SQL expression over the message's
/// conversation `c` and its sender's handle `hs`: the sender's normalized
/// address, or NULL in a conversation with yourself.
///
/// The holder is nobody's sender in a conversation with yourself, so an
/// import drops the sender of each received note there (#1094). A copy
/// imported before the chat's address was linked still names the holder, and
/// one imported after does not; matching both with no sender lets the two
/// copies of a received note pair (#1661). The question is asked of the
/// identities the account has now, as every read of a conversation with
/// yourself asks it.
fn sender_for_key_sql() -> String {
    format!(
        "CASE WHEN {with_yourself} THEN NULL ELSE hs.normalized END",
        with_yourself = super::conversations::is_with_yourself_sql("c"),
    )
}

/// Source preference for survivors: first imported source (min message id), then name.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn source_priority(conn: &mut SqliteConnection, account_id: i64) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r"
        SELECT m.source, MIN(m.id) AS first_id
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1
          AND m.source IS NOT NULL
          AND TRIM(m.source) != ''
        GROUP BY m.source
        ORDER BY first_id ASC, m.source ASC
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(|(source,)| source).collect())
}

// ── Content keys ─────────────────────────────────────────────────────────

/// One production message that needs a content fingerprint, as
/// [`content_key_inputs`] selects it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ContentKeyRow {
    pub id: i64,
    pub conversation_id: i64,
    /// The chat handle's normalized address.
    pub chat_id: String,
    pub conversation_type: String,
    pub is_from_me: i64,
    pub timestamp: String,
    pub body: Option<String>,
    /// The sender the message is matched by ([`sender_for_key_sql`]).
    pub sender_normalized: Option<String>,
}

/// Which of the account's messages [`content_key_inputs`] loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScope<'a> {
    /// Every message.
    All,
    /// The messages without a key.
    Missing,
    /// The messages with these ids.
    Changed(&'a [i64]),
}

/// Everything the content-key hash reads, loaded in three queries so the
/// hashing runs off the database thread with no lookups of its own.
pub struct ContentKeyInputs {
    pub rows: Vec<ContentKeyRow>,
    /// Sorted participant handles per group conversation: one shared identity
    /// across import sources.
    pub group_handles: HashMap<i64, Vec<String>>,
    /// Attachment digests per message.
    pub shas_by_msg: HashMap<i64, Vec<String>>,
}

/// What the content keys of the account's messages in `scope` are made
/// from; `None` when no message is in `scope`.
///
/// # Errors
///
/// Returns an error when a query fails.
pub async fn content_key_inputs(
    conn: &mut SqliteConnection,
    account_id: i64,
    scope: KeyScope<'_>,
) -> Result<Option<ContentKeyInputs>> {
    let filter = match scope {
        KeyScope::All => "WHERE c.account_id = $1",
        KeyScope::Missing => {
            "WHERE (m.content_key IS NULL OR m.content_key = '') AND c.account_id = $1"
        }
        KeyScope::Changed(_) => {
            "WHERE m.id IN (SELECT value FROM json_each($2)) AND c.account_id = $1"
        }
    };
    let sql = format!(
        r"
        SELECT m.id, m.conversation_id, h.normalized AS chat_id, c.conversation_type,
               m.is_from_me, m.timestamp, m.body,
               {sender} AS sender_normalized
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        JOIN handles h ON h.id = c.chat_handle_id
        LEFT JOIN handles hs ON hs.id = m.sender_handle_id
        {filter}
        ORDER BY m.id
        ",
        sender = sender_for_key_sql(),
    );
    let mut query = sqlx::query_as(&sql).bind(account_id);
    if let KeyScope::Changed(ids) = scope {
        query = query.bind(serde_json::to_string(ids)?);
    }
    let rows: Vec<ContentKeyRow> = query.fetch_all(&mut *conn).await?;
    if rows.is_empty() {
        return Ok(None);
    }

    // The holder is never a participant: a group imported before one of
    // these identities was linked still lists it, and one imported after
    // does not, so it is left out of the key on both (#1093).
    let participant_sql = format!(
        r"
        SELECT p.conversation_id, h.normalized
        FROM participants p
        JOIN conversations c ON c.id = p.conversation_id
        JOIN handles h ON h.id = p.handle_id
        WHERE c.account_id = $1
          AND h.normalized IS NOT NULL AND h.normalized != ''
          AND NOT {holder}
        ORDER BY p.conversation_id, h.normalized
        ",
        holder = super::account_profile::is_account_identity_sql("h", "c.account_id"),
    );
    let participant_rows: Vec<(i64, String)> = sqlx::query_as(&participant_sql)
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?;
    let mut group_handles: HashMap<i64, Vec<String>> = HashMap::new();
    for (conversation_id, handle) in participant_rows {
        group_handles
            .entry(conversation_id)
            .or_default()
            .push(handle);
    }

    // One scan for attachment hashes belonging to this account's message id range.
    let min_id = rows.first().map_or(0, |r| r.id);
    let max_id = rows.last().map_or(0, |r| r.id);
    let att_rows: Vec<(i64, String)> = sqlx::query_as(
        r"
        SELECT a.message_id, a.sha256
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1
          AND a.message_id BETWEEN $2 AND $3
          AND a.sha256 IS NOT NULL AND a.sha256 != ''
        ORDER BY a.message_id
        ",
    )
    .bind(account_id)
    .bind(min_id)
    .bind(max_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut shas_by_msg: HashMap<i64, Vec<String>> = HashMap::new();
    for (message_id, sha) in att_rows {
        shas_by_msg.entry(message_id).or_default().push(sha);
    }

    Ok(Some(ContentKeyInputs {
        rows,
        group_handles,
        shas_by_msg,
    }))
}

/// The content key stored for each of the account's messages that has one.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn stored_content_keys(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<HashMap<i64, String>> {
    Ok(sqlx::query_as(
        r"
        SELECT m.id, m.content_key
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1 AND m.content_key IS NOT NULL
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect())
}

/// Clear the content key of the messages `ids`.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn clear_content_keys(conn: &mut SqliteConnection, ids: &[i64]) -> Result<()> {
    sqlx::query(
        "UPDATE messages SET content_key = NULL WHERE id IN (SELECT value FROM json_each($1))",
    )
    .bind(serde_json::to_string(ids)?)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Write the `(message id, content key)` pairs onto `messages` through the
/// `_content_keys` temp table, which is dropped again afterwards.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn write_content_keys(
    conn: &mut SqliteConnection,
    keys: &[(i64, String)],
    progress: Progress,
) -> Result<()> {
    for stmt in schema::split_ddl(
        r"
        CREATE TEMP TABLE IF NOT EXISTS _content_keys (
            id BIGINT PRIMARY KEY,
            content_key TEXT NOT NULL
        );
        DELETE FROM _content_keys;
        ",
    ) {
        sqlx::query(&stmt).execute(&mut *conn).await?;
    }
    insert_content_key_rows(conn, keys, progress).await?;
    sqlx::query(
        r"
        UPDATE messages AS m
        SET content_key = k.content_key
        FROM _content_keys AS k
        WHERE m.id = k.id
        ",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query("DROP TABLE IF EXISTS _content_keys")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Bulk-insert fingerprints into the `_content_keys` temp table in chunks that fit the bind limit.
async fn insert_content_key_rows(
    conn: &mut SqliteConnection,
    keys: &[(i64, String)],
    progress: Progress,
) -> Result<()> {
    let total = keys.len();
    let mut written = 0usize;
    for chunk in keys.chunks(SQLITE_IN_CHUNK) {
        let mut sql = "INSERT INTO _content_keys (id, content_key) VALUES ".to_string();
        for (i, _) in chunk.iter().enumerate() {
            if i > 0 {
                sql.push(',');
            }
            let _ = write!(sql, "(${}, ${})", i * 2 + 1, i * 2 + 2);
        }
        let mut q = sqlx::query(&sql);
        for (id, key) in chunk {
            q = q.bind(*id).bind(key);
        }
        q.execute(&mut *conn).await?;
        let previous = written;
        written += chunk.len();
        let crossed_log_mark =
            written / CONTENT_KEY_WRITE_LOG_EVERY != previous / CONTENT_KEY_WRITE_LOG_EVERY;
        if written == total || crossed_log_mark {
            progress.say(format_args!("Wrote {written} of {total} content keys"));
        }
    }
    Ok(())
}

// ── The exact and near-time passes ───────────────────────────────────────

/// One message with a content key, as the exact pass reads it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ExactRow {
    pub id: i64,
    pub source: String,
    pub content_key: String,
    /// How many of its attachments have a digest.
    pub att_count: i64,
    pub time_precision: String,
    /// How many messages its Import Run brought; 0 for a message no run
    /// stamped.
    pub run_messages: i64,
}

/// Every message of the account with a content key, for the exact pass.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn exact_rows(conn: &mut SqliteConnection, account_id: i64) -> Result<Vec<ExactRow>> {
    // One scan of messages + one aggregated attachment pass, then group in Rust.
    // Avoids N round-trips (one SELECT + several UPDATEs per duplicate key).
    Ok(sqlx::query_as(&format!(
        r"
        SELECT m.id, m.source, m.content_key, COALESCE(ac.n, 0) AS att_count,
               m.time_precision, COALESCE(rc.n, 0) AS run_messages
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        LEFT JOIN (
            SELECT a.message_id, COUNT(*) AS n
            FROM attachments a
            JOIN messages m2 ON m2.id = a.message_id
            JOIN conversations c2 ON c2.id = m2.conversation_id
            WHERE c2.account_id = $1
              AND a.sha256 IS NOT NULL AND a.sha256 != ''
            GROUP BY a.message_id
        ) ac ON ac.message_id = m.id
        LEFT JOIN ({RUN_MESSAGES_SQL}) rc ON rc.import_id = m.import_id
        WHERE c.account_id = $1
          AND {HAS_CONTENT_KEY_SQL}
        ",
    ))
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?)
}

/// Which messages the near-time pass reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NearRows {
    /// Every message of the account.
    All,
    /// The messages with a content key: those a dedupe has already seen,
    /// and the changed ones `dedupe_changed_messages` gave one.
    Keyed,
}

/// One message as the near-time pass reads it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NearMessageRow {
    pub id: i64,
    pub conversation_id: i64,
    pub source: String,
    pub is_from_me: i64,
    pub timestamp: String,
    pub body: Option<String>,
    /// The sender it is matched by ([`sender_for_key_sql`]), or `""`.
    pub sender_normalized: String,
    /// Its content key, or `""`.
    pub content_key: String,
    pub time_precision: String,
    /// How many messages its Import Run brought; 0 for a message no run
    /// stamped.
    pub run_messages: i64,
}

/// The account's messages in `rows`, for the near-time pass.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn near_message_rows(
    conn: &mut SqliteConnection,
    account_id: i64,
    rows: NearRows,
) -> Result<Vec<NearMessageRow>> {
    let sql = format!(
        r"
        SELECT m.id, m.conversation_id, m.source, m.is_from_me, m.timestamp, m.body,
               COALESCE({sender}, '') AS sender_normalized,
               COALESCE(m.content_key, '') AS content_key, m.time_precision,
               COALESCE(rc.n, 0) AS run_messages
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        LEFT JOIN handles hs ON hs.id = m.sender_handle_id
        LEFT JOIN ({RUN_MESSAGES_SQL}) rc ON rc.import_id = m.import_id
        WHERE c.account_id = $1 {keyed}
        ",
        sender = sender_for_key_sql(),
        keyed = match rows {
            NearRows::All => String::new(),
            NearRows::Keyed => format!("AND {HAS_CONTENT_KEY_SQL}"),
        },
    );
    Ok(sqlx::query_as(&sql)
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?)
}

/// The attachment digests of each of the account's messages, each message's
/// in digest order.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn attachment_digests(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<HashMap<i64, Vec<String>>> {
    let att_rows: Vec<(i64, String)> = sqlx::query_as(
        r"
        SELECT a.message_id, a.sha256
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1
          AND a.sha256 IS NOT NULL AND a.sha256 != ''
        ORDER BY a.message_id, a.sha256
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut shas_by_msg: HashMap<i64, Vec<String>> = HashMap::new();
    for (message_id, sha) in att_rows {
        shas_by_msg.entry(message_id).or_default().push(sha);
    }
    Ok(shas_by_msg)
}

// ── Duplicate flags ──────────────────────────────────────────────────────

/// The `(loser, winner)` duplicate flag of each of the account's hidden
/// messages.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn duplicate_flags(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<HashMap<i64, i64>> {
    Ok(sqlx::query_as(
        r"
        SELECT m.id, m.duplicate_of
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1 AND m.duplicate_of IS NOT NULL
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect())
}

/// Show every message of the account: clear each duplicate flag.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn clear_account_duplicate_flags(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<()> {
    sqlx::query(
        r"
        UPDATE messages
        SET duplicate_of = NULL
        WHERE conversation_id IN (
            SELECT id FROM conversations WHERE account_id = $1
        )
        ",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Clear the duplicate flag of the messages `ids`.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn clear_duplicate_flags(conn: &mut SqliteConnection, ids: &[i64]) -> Result<()> {
    sqlx::query(
        "UPDATE messages SET duplicate_of = NULL WHERE id IN (SELECT value FROM json_each($1))",
    )
    .bind(serde_json::to_string(ids)?)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Apply `(message id, duplicate-of id)` pairs through the temp table
/// `table`, so one UPDATE covers them all; the table is dropped again
/// afterwards.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn write_duplicate_flags(
    conn: &mut SqliteConnection,
    table: &str,
    flags: &[(i64, i64)],
) -> Result<()> {
    for stmt in schema::split_ddl(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {table} (
            id BIGINT PRIMARY KEY,
            winner BIGINT NOT NULL
        );
        DELETE FROM {table};"
    )) {
        sqlx::query(&stmt).execute(&mut *conn).await?;
    }
    {
        let insert_sql = format!("INSERT INTO {table} (id, winner) VALUES ($1, $2)");
        for (id, winner) in flags {
            sqlx::query(&insert_sql)
                .bind(id)
                .bind(winner)
                .execute(&mut *conn)
                .await?;
        }
    }
    sqlx::query(&format!(
        "UPDATE messages AS m
         SET duplicate_of = f.winner
         FROM {table} AS f
         WHERE m.id = f.id"
    ))
    .execute(&mut *conn)
    .await?;
    for stmt in schema::split_ddl(&format!("DROP TABLE IF EXISTS {table};")) {
        sqlx::query(&stmt).execute(&mut *conn).await?;
    }
    Ok(())
}
