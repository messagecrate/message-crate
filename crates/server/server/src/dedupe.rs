//! Cross-source content fingerprint and soft-hide dedupe.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::time::Instant;

use anyhow::{Context, Result};
use rayon::prelude::*;
use sqlx::SqliteConnection;

use crate::counts::words;
use crate::db::conversations::is_group_type;
use crate::db::schema;
use crate::db::sql::SQLITE_IN_CHUNK;
use crate::db::{WriteTx, begin_write};
use crate::progress::Progress;

const CONTENT_KEY_WRITE_LOG_EVERY: usize = 50_000;

/// One production message that still needs a content fingerprint, as
/// [`ContentKeyInputs::load`] selects it.
#[derive(Debug, Clone, sqlx::FromRow)]
struct ContentKeyRow {
    id: i64,
    conversation_id: i64,
    /// The chat handle's normalized address.
    chat_id: String,
    conversation_type: String,
    is_from_me: i64,
    timestamp: String,
    body: Option<String>,
    /// The sender the message is matched by ([`sender_for_key_sql`]).
    sender_normalized: Option<String>,
}

/// Collapse whitespace so minor text differences do not split the same SMS.
pub fn normalize_body(body: Option<&str>) -> String {
    message_ir::collapse_whitespace(body.unwrap_or(""))
}

/// Stable chat identity for content keys.
///
/// 1:1 chats use the conversation handle's normalized form (E.164 phones,
/// lowercased emails, verbatim usernames). Groups use sorted normalized
/// participant handles so the same people across exporters (different
/// `chat_identifier`s) share one fingerprint.
pub fn chat_identity_for_content_key(
    chat_identifier: &str,
    group_handles: Option<&[String]>,
) -> String {
    match group_handles {
        Some(handles) if !handles.is_empty() => {
            let mut sorted: Vec<&str> = handles
                .iter()
                .map(|h| h.as_str())
                .filter(|h| !h.is_empty())
                .collect();
            sorted.sort_unstable();
            sorted.dedup();
            format!("group:{}", sorted.join("|"))
        }
        _ => chat_identifier.to_string(),
    }
}

/// Build a content key from chat + direction + sender + UTC second + body + attachment hashes.
///
/// The key is the message's [`message_ir::MessageIdentity`] at whole seconds,
/// made by the same function as an exported message's `guid` (which is made at
/// milliseconds): it matches one message across sources, and iMazing,
/// OpenExtract and GO SMS Pro's PDU files record whole seconds only.
///
/// `timestamp` is the stored UTC instant; `None` when it does not parse.
/// For groups, pass the sorted-participant identity from [`chat_identity_for_content_key`].
/// Incoming group messages include the normalized sender so two peers sending the same
/// text at the same second do not collide; outgoing (`is_from_me`) uses an empty sender.
pub fn compute_content_key(
    chat_identifier: &str,
    is_from_me: bool,
    sender_normalized: Option<&str>,
    timestamp: &str,
    body: Option<&str>,
    attachment_shas: &[String],
) -> Option<String> {
    let secs = parse_rfc3339_utc_secs(timestamp.trim())?;
    let identity = message_ir::MessageIdentity {
        chat: chat_identifier,
        is_from_me,
        sender: sender_normalized,
        timestamp_unix_ms: secs.saturating_mul(1000),
        text: body.unwrap_or(""),
        attachment_digests: attachment_shas,
        vendor_key: None,
    };
    Some(identity.key(message_ir::TimePrecision::Seconds))
}

/// Fingerprint one message row from its chat identity, direction, sender, time, body, and attachment hashes.
///
/// `None` when the row's time does not parse; such a row keeps no content key.
fn content_key_for_row(
    row: &ContentKeyRow,
    group_handles: &HashMap<i64, Vec<String>>,
    shas_by_msg: &HashMap<i64, Vec<String>>,
) -> Option<(i64, String)> {
    let empty: &[String] = &[];
    let shas = shas_by_msg.get(&row.id).map_or(empty, Vec::as_slice);
    let group_identity = if is_group_type(&row.conversation_type) {
        Some(chat_identity_for_content_key(
            &row.chat_id,
            group_handles.get(&row.conversation_id).map(Vec::as_slice),
        ))
    } else {
        None
    };
    let identity = group_identity.as_deref().unwrap_or(&row.chat_id);
    let key = compute_content_key(
        identity,
        row.is_from_me != 0,
        row.sender_normalized.as_deref(),
        &row.timestamp,
        row.body.as_deref(),
        shas,
    )?;
    Some((row.id, key))
}

/// Fingerprint every row in parallel.
fn hash_content_keys(
    rows: &[ContentKeyRow],
    group_handles: &HashMap<i64, Vec<String>>,
    shas_by_msg: &HashMap<i64, Vec<String>>,
) -> Vec<(i64, String)> {
    rows.par_iter()
        .filter_map(|row| content_key_for_row(row, group_handles, shas_by_msg))
        .collect()
}

/// Counts reported by one cross-source dedupe pass.
#[derive(Debug, Default)]
pub struct DedupeStats {
    /// Content keys written: those missing and those whose inputs changed
    /// (not a duplicate count).
    pub keys_filled: u64,
    /// Groups of messages sharing one content key in which a message was
    /// hidden.
    pub exact_groups: u64,
    /// Messages hidden as exact duplicates: all but as many per group as one
    /// source holds, and each whole-second message whose own source holds
    /// it with milliseconds too.
    pub exact_flagged: u64,
    /// Messages flagged as near duplicates.
    pub near_flagged: u64,
}

/// Source preference for survivors: first imported source (min message id), then name.
pub async fn source_priority_from_db(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Vec<String>> {
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

/// Refresh the content keys, clear prior flags, then soft-hide cross-source
/// duplicates and the whole-second twins of a message one source holds with
/// milliseconds too, all in one transaction.
///
/// The copy shown is the first by [`rank`]. Optional `source_priority`
/// overrides the order of the sources (tests); `None` loads it from the DB.
pub async fn dedupe_cross_source(
    conn: &mut SqliteConnection,
    account_id: i64,
    source_priority: Option<&[String]>,
    near_window_secs: i64,
    progress: Progress,
) -> Result<DedupeStats> {
    // One write transaction for the whole run: the flags are cleared and set
    // again, so a pass that fails after the clearing would otherwise leave
    // every duplicate in the account shown until the next successful run. It
    // holds the write lock from the first read, so an import that commits
    // while the keys are hashed waits instead of failing the pass.
    let mut tx = begin_write(conn).await?;
    let owned_priority;
    let priority = if let Some(p) = source_priority {
        p
    } else {
        owned_priority = source_priority_from_db(&mut tx, account_id).await?;
        owned_priority.as_slice()
    };
    let mut stats = DedupeStats::default();
    let prio: HashMap<&str, usize> = priority
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let started = Instant::now();
    let exact_hidden: HashSet<i64>;

    {
        progress.say("Refreshing the content keys that match the same message across sources…");
        stats.keys_filled = refresh_content_keys(&mut tx, account_id, progress).await?;
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
        .execute(&mut *tx)
        .await?;
        progress.say(format_args!(
            "Wrote {} in {:.1} s",
            words(stats.keys_filled, "1 content key", "{n} content keys"),
            started.elapsed().as_secs_f64()
        ));
    }

    {
        progress.say("Hiding exact duplicates, the messages that share a content key…");
        let (groups, flags) = flag_exact_content_key_dupes(&mut tx, account_id, &prio).await?;
        stats.exact_groups = groups;
        stats.exact_flagged = flags.len() as u64;
        exact_hidden = flags.into_iter().map(|(loser, _)| loser).collect();
        progress.say(format_args!(
            "Found {} and hid {}, {:.1} s in all",
            words(
                stats.exact_groups,
                "1 group of exact duplicates",
                "{n} groups of exact duplicates"
            ),
            words(stats.exact_flagged, "1 message", "{n} messages"),
            started.elapsed().as_secs_f64()
        ));
    }

    {
        progress.say(format_args!(
            "Flagging near duplicates, sent within {near_window_secs} s of each other…"
        ));
        stats.near_flagged =
            flag_near_time_dupes(&mut tx, account_id, &prio, near_window_secs, &exact_hidden)
                .await?;
        progress.say(format_args!(
            "Flagged {}, {:.1} s in all",
            words(
                stats.near_flagged,
                "1 near duplicate",
                "{n} near duplicates"
            ),
            started.elapsed().as_secs_f64()
        ));
    }
    tx.commit().await?;

    Ok(stats)
}

/// The near-time window, in seconds, of a dedupe nobody chose one for: the
/// one after each import batch, and the pass a phone country change runs
/// for the messages it changed.
pub const NEAR_WINDOW_SECS: i64 = 2;

/// What [`dedupe_changed_messages`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChangedDedupe {
    /// Messages that were hidden and are shown now.
    pub shown: u64,
    /// Messages that were shown and are hidden now, or hidden behind
    /// another message than before.
    pub hidden: u64,
}

/// Put right the duplicate flags of the messages `changed`, whose content
/// a change in the transaction `conn` is in made different, and of every
/// message whose flag depends on theirs (#1805). A phone number given its
/// country merges conversations and senders, which changes the content
/// keys of the messages that moved (`crate::identity_country`); an import
/// leaves this to the full dedupe after each batch instead.
///
/// This computes the changed messages' content keys again, then runs both
/// passes of [`dedupe_cross_source`] over the messages with a content key,
/// so a message without one is neither hidden nor a winner here. Only the
/// flags of the messages tied to a changed one are written: those it was
/// hidden behind or hid, before or now, and the messages tied to those in
/// turn. Every other flag stays as it is.
///
/// # Errors
///
/// Returns an error when a statement fails or the hashing task panics.
pub async fn dedupe_changed_messages(
    conn: &mut SqliteConnection,
    account_id: i64,
    changed: &[i64],
    near_window_secs: i64,
    progress: Progress,
) -> Result<ChangedDedupe> {
    if changed.is_empty() {
        return Ok(ChangedDedupe::default());
    }
    let changed_json = serde_json::to_string(changed)?;
    sqlx::query(
        "UPDATE messages SET content_key = NULL WHERE id IN (SELECT value FROM json_each($1))",
    )
    .bind(&changed_json)
    .execute(&mut *conn)
    .await?;
    recompute_content_keys(conn, KeyScope::Changed(&changed_json), account_id, progress).await?;

    let priority = source_priority_from_db(conn, account_id).await?;
    let prio: HashMap<&str, usize> = priority
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let (_, mut flags) = exact_flags(conn, account_id, &prio).await?;
    let exact_hidden: HashSet<i64> = flags.iter().map(|&(loser, _)| loser).collect();
    let by_conversation = load_near_rows(conn, account_id, NearRows::Keyed, &exact_hidden).await?;
    flags.extend(cluster_near_dupes(by_conversation, &prio, near_window_secs));
    let now: HashMap<i64, i64> = flags.into_iter().collect();

    let stored: HashMap<i64, i64> = sqlx::query_as(
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
    .collect();

    let tied = tied_messages(changed, &stored, &now);
    let mut result = ChangedDedupe::default();
    let mut flags: Vec<(i64, i64)> = Vec::new();
    for &id in &tied {
        match (stored.get(&id), now.get(&id)) {
            (Some(_), None) => result.shown += 1,
            (before, Some(&winner)) => {
                if before != Some(&winner) {
                    result.hidden += 1;
                }
                flags.push((id, winner));
            }
            (None, None) => {}
        }
    }
    let tied: Vec<i64> = tied.into_iter().collect();
    sqlx::query(
        "UPDATE messages SET duplicate_of = NULL WHERE id IN (SELECT value FROM json_each($1))",
    )
    .bind(serde_json::to_string(&tied)?)
    .execute(&mut *conn)
    .await?;
    if !flags.is_empty() {
        apply_duplicate_flags(conn, "_changed_flags", &flags).await?;
    }
    Ok(result)
}

/// The messages tied to one in `changed`: `changed` itself, and every
/// message one hides or is hidden behind, in the `(loser → winner)` maps
/// `stored` (the flags as they are) or `now` (as the passes would set
/// them), followed to the end.
///
/// The set is closed both ways, so writing the flags `now` gives its
/// messages leaves no stored flag outside it pointing at a message inside
/// it, and no new flag inside it pointing outside: every hidden message
/// still points at a shown one.
fn tied_messages(
    changed: &[i64],
    stored: &HashMap<i64, i64>,
    now: &HashMap<i64, i64>,
) -> BTreeSet<i64> {
    let mut neighbours: HashMap<i64, Vec<i64>> = HashMap::new();
    for (&loser, &winner) in stored.iter().chain(now.iter()) {
        neighbours.entry(loser).or_default().push(winner);
        neighbours.entry(winner).or_default().push(loser);
    }
    let mut tied: BTreeSet<i64> = changed.iter().copied().collect();
    let mut queue: Vec<i64> = changed.to_vec();
    while let Some(id) = queue.pop() {
        for &next in neighbours.get(&id).into_iter().flatten() {
            if tied.insert(next) {
                queue.push(next);
            }
        }
    }
    tied
}

/// Compute `content_key` for production rows that still lack one (after attachments exist).
pub async fn fill_missing_content_keys(
    conn: &mut SqliteConnection,
    account_id: i64,
    progress: Progress,
) -> Result<u64> {
    recompute_content_keys(conn, KeyScope::Missing, account_id, progress).await
}

/// Recompute every content key of the account and write the ones that differ
/// from what is stored. Returns how many were written.
///
/// A key is first written at import, but a later append import can add
/// attachments to a message the database already holds, or participants to a
/// group, and both are part of the key. A key left as it was stops matching
/// the same message from another source.
async fn refresh_content_keys(
    conn: &mut SqliteConnection,
    account_id: i64,
    progress: Progress,
) -> Result<u64> {
    recompute_content_keys(conn, KeyScope::All, account_id, progress).await
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

/// Which of the account's messages [`recompute_content_keys`] computes a key
/// for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyScope<'a> {
    /// Every message, writing only the keys that changed.
    All,
    /// The messages without a key.
    Missing,
    /// The messages whose ids the JSON array names, writing every key.
    Changed(&'a str),
}

/// Compute and store content keys for the account's messages in `scope`.
/// Returns how many were written.
///
/// # Errors
///
/// Returns an error when a query fails or the hashing task panics.
async fn recompute_content_keys(
    conn: &mut SqliteConnection,
    scope: KeyScope<'_>,
    account_id: i64,
    progress: Progress,
) -> Result<u64> {
    let Some(inputs) = ContentKeyInputs::load(conn, account_id, scope).await? else {
        return Ok(0);
    };
    progress.say(format_args!(
        "Hashing the content keys of {}…",
        words(inputs.rows.len() as u64, "1 message", "{n} messages")
    ));
    let keys = tokio::task::spawn_blocking(move || inputs.hash())
        .await
        .context("content-key hash task panicked")?;
    let keys = if scope != KeyScope::All {
        keys
    } else {
        let stored: HashMap<i64, String> = sqlx::query_as(
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
        .collect();
        keys.into_iter()
            .filter(|(id, key)| stored.get(id) != Some(key))
            .collect()
    };
    if keys.is_empty() {
        return Ok(0);
    }
    apply_content_keys(conn, &keys, progress).await?;
    Ok(keys.len() as u64)
}

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
        with_yourself = crate::db::conversations::is_with_yourself_sql("c"),
    )
}

/// Whether the message `m` has a content key, as a SQL condition: whether a
/// dedupe, or an import that filled the keys, has seen it.
pub(crate) const HAS_CONTENT_KEY_SQL: &str = "m.content_key IS NOT NULL AND m.content_key != ''";

/// Everything the content-key hash reads, loaded in three queries so the
/// hashing runs off the database thread with no lookups of its own.
struct ContentKeyInputs {
    rows: Vec<ContentKeyRow>,
    /// Sorted participant handles per group conversation: one shared identity
    /// across import sources.
    group_handles: HashMap<i64, Vec<String>>,
    /// Attachment digests per message.
    shas_by_msg: HashMap<i64, Vec<String>>,
}

impl ContentKeyInputs {
    /// `None` when no message needs a key.
    async fn load(
        conn: &mut SqliteConnection,
        account_id: i64,
        scope: KeyScope<'_>,
    ) -> Result<Option<Self>> {
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
            query = query.bind(ids);
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
            holder = crate::db::account_profile::is_account_identity_sql("h", "c.account_id"),
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

        Ok(Some(Self {
            rows,
            group_handles,
            shas_by_msg,
        }))
    }

    /// `(message id, content key)` for every row, hashed in parallel.
    fn hash(&self) -> Vec<(i64, String)> {
        hash_content_keys(&self.rows, &self.group_handles, &self.shas_by_msg)
    }
}

/// Write the keys onto `messages` through the `_content_keys` temp table,
/// which is dropped again afterwards.
async fn apply_content_keys(
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

/// One copy of a message, with what decides whether it is the copy shown
/// ([`rank`]).
#[derive(Clone)]
struct Cand {
    id: i64,
    source: String,
    att_count: i64,
    /// Whether its source recorded its time in whole seconds. The flag
    /// decides, never the time: a millisecond time can end in `.000`.
    whole_seconds: bool,
    /// How many messages its Import Run brought: the account's messages
    /// stamped with the run, which are the ones it added; 0 for a message
    /// no run stamped.
    run_messages: i64,
}

/// SQL for the Import Run size of each message: join it as `rc` on
/// `rc.import_id = m.import_id` and read `COALESCE(rc.n, 0)`. The account is
/// bound as `$1`.
const RUN_MESSAGES_SQL: &str = "SELECT import_id, COUNT(*) AS n
            FROM messages
            WHERE account_id = $1 AND import_id IS NOT NULL
            GROUP BY import_id";

/// Whether `time_precision`, as `messages.time_precision` stores it, is
/// whole seconds.
fn is_whole_seconds(id: i64, time_precision: &str) -> Result<bool> {
    Ok(message_ir::TimePrecision::parse(time_precision)
        .with_context(|| format!("message {id}: time_precision {time_precision:?}"))?
        == message_ir::TimePrecision::Seconds)
}

/// Hide the messages that share a fingerprint with a preferred-source twin,
/// keeping as many as one source holds (see [`exact_group_flags`]), and
/// each whole-second message whose own source holds it with milliseconds
/// too (see [`content_key_group_flags`]). Returns the groups in which a
/// message was hidden, and the `(loser, winner)` pairs.
async fn flag_exact_content_key_dupes(
    tx: &mut WriteTx<'_>,
    account_id: i64,
    prio: &HashMap<&str, usize>,
) -> Result<(u64, Vec<(i64, i64)>)> {
    let conn: &mut SqliteConnection = tx;
    let (groups, flags) = exact_flags(conn, account_id, prio).await?;
    if !flags.is_empty() {
        apply_duplicate_flags(conn, "_pass_a_flags", &flags).await?;
    }
    Ok((groups, flags))
}

/// The exact pass over every message of the account with a content key,
/// as though none were hidden: the groups in which a message would be
/// hidden, and the `(loser, winner)` pairs. Nothing is written.
async fn exact_flags(
    conn: &mut SqliteConnection,
    account_id: i64,
    prio: &HashMap<&str, usize>,
) -> Result<(u64, Vec<(i64, i64)>)> {
    // One scan of messages + one aggregated attachment pass, then group in Rust.
    // Avoids N round-trips (one SELECT + several UPDATEs per duplicate key).
    let rows: Vec<(i64, String, String, i64, String, i64)> = sqlx::query_as(&format!(
        r"
        SELECT m.id, m.source, m.content_key, COALESCE(ac.n, 0), m.time_precision,
               COALESCE(rc.n, 0)
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
    .await?;

    let mut by_key: HashMap<String, Vec<Cand>> = HashMap::new();
    for (id, source, content_key, att_count, time_precision, run_messages) in rows {
        by_key.entry(content_key).or_default().push(Cand {
            id,
            source,
            att_count,
            whole_seconds: is_whole_seconds(id, &time_precision)?,
            run_messages,
        });
    }

    let mut flags: Vec<(i64, i64)> = Vec::new(); // (loser_id, winner_id)
    let mut groups = 0u64;
    for cands in by_key.into_values() {
        let group_flags = content_key_group_flags(cands, prio);
        if !group_flags.is_empty() {
            groups += 1;
        }
        flags.extend(group_flags);
    }
    Ok((groups, flags))
}

/// The `(loser, winner)` pairs of the messages that share one content key.
///
/// A whole-second message whose own source also holds a message of the same
/// key with milliseconds is first set aside as that message's twin: the key
/// is taken at whole seconds, so the two match in everything else and fall
/// in the same second, and the source recorded the message twice, once
/// without its milliseconds (an SMS Backup+ mail timed by its `Date` header
/// beside one timed by `X-smssync-date`). The rest are flagged by
/// [`exact_group_flags`] when two or more sources hold them, and each twin
/// is hidden under the rest's winner, which is always shown, so the message
/// is shown once. It keeps its milliseconds unless another source's copy
/// wins the cross-source comparison ([`rank`]), as one with more attachments
/// does. A source that holds the message only in whole seconds keeps
/// every copy, as one that holds it only with milliseconds does.
fn content_key_group_flags(cands: Vec<Cand>, prio: &HashMap<&str, usize>) -> Vec<(i64, i64)> {
    let with_milliseconds: HashSet<String> = cands
        .iter()
        .filter(|c| !c.whole_seconds)
        .map(|c| c.source.clone())
        .collect();
    let (twins, rest): (Vec<Cand>, Vec<Cand>) = cands
        .into_iter()
        .partition(|c| c.whole_seconds && with_milliseconds.contains(c.source.as_str()));
    let sources: HashSet<&str> = rest.iter().map(|c| c.source.as_str()).collect();
    let mut flags = if sources.len() < 2 {
        Vec::new()
    } else {
        exact_group_flags(&rest, prio)
    };
    if !twins.is_empty() {
        let winner = pick_winner(&rest, prio);
        flags.extend(twins.into_iter().map(|t| (t.id, winner)));
    }
    flags
}

/// The `(loser, winner)` pairs of one group of copies that two or more
/// sources hold: the messages of one content key in the exact pass, or one
/// cluster of the near-time pass ([`cluster_near_dupes`]).
///
/// One source that holds a message twice holds two messages, so the group
/// stays shown as many times as the source that holds it most often. Those
/// places go to the copies first by [`rank`], the winner among them, and
/// every other copy is hidden as a duplicate of the winner.
fn exact_group_flags(cands: &[Cand], prio: &HashMap<&str, usize>) -> Vec<(i64, i64)> {
    let mut per_source: HashMap<&str, usize> = HashMap::new();
    for c in cands {
        *per_source.entry(c.source.as_str()).or_default() += 1;
    }
    let shown = per_source.values().copied().max().unwrap_or(0);
    let winner = pick_winner(cands, prio);
    let mut ranked: Vec<&Cand> = cands.iter().collect();
    ranked.sort_by(|a, b| rank(a, b, prio));
    ranked
        .into_iter()
        .skip(shown)
        .map(|c| (c.id, winner))
        .collect()
}

/// The message to keep from a duplicate group: the first by [`rank`].
fn pick_winner(cands: &[Cand], prio: &HashMap<&str, usize>) -> i64 {
    cands
        .iter()
        .min_by(|a, b| rank(a, b, prio))
        .map_or(cands[0].id, |c| c.id)
}

/// The order in which copies of one message are shown, the first shown
/// before the rest (`docs/architecture/contacts-identities-and-messages.md`,
/// "Which copy is shown"): the copy with more attachments; then the copy
/// timed to the millisecond over one timed to the second; then the copy
/// from the Import Run that brought more messages; then the copy of the
/// source imported first (`prio`); then the lower id. The last two only
/// make the order stable.
fn rank(a: &Cand, b: &Cand, prio: &HashMap<&str, usize>) -> std::cmp::Ordering {
    let priority = |c: &Cand| prio.get(c.source.as_str()).copied().unwrap_or(usize::MAX);
    b.att_count
        .cmp(&a.att_count)
        .then(a.whole_seconds.cmp(&b.whole_seconds))
        .then(b.run_messages.cmp(&a.run_messages))
        .then_with(|| priority(a).cmp(&priority(b)))
        .then(a.id.cmp(&b.id))
}

/// Parse an RFC3339 timestamp into Unix UTC seconds, honoring Z / ±HH:MM
/// offsets and dropping the milliseconds: the content key and the near-time
/// pass match at whole seconds.
///
/// Strict RFC3339 is sufficient: `messages.timestamp` is only ever written by
/// `models::utc_timestamp_text`, so no lenient spellings reach this path.
/// Unparseable input yields `None`.
fn parse_rfc3339_utc_secs(timestamp_rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp_rfc3339.trim())
        .ok()
        .map(|dt| dt.timestamp())
}

#[derive(Clone)]
struct NearRow {
    id: i64,
    source: String,
    is_from_me: i64,
    sender_norm: String,
    secs: i64,
    body_norm: String,
    att_fp: String,
    att_count: i64,
    content_key: String,
    whole_seconds: bool,
    run_messages: i64,
}

impl NearRow {
    /// Whether `other`, a later row of the same conversation, is a near-time
    /// twin of this one: same direction and sender, a different source, a
    /// different content key, and the same body or the same attachments.
    ///
    /// Two shown rows with one content key are rows the exact pass chose to
    /// keep, because one source holds the message that many times, so they
    /// are never twins here.
    fn is_twin_of(&self, other: &NearRow) -> bool {
        let same_body = !self.body_norm.is_empty() && other.body_norm == self.body_norm;
        let same_attachments = !self.att_fp.is_empty() && other.att_fp == self.att_fp;
        let same_key = !self.content_key.is_empty() && other.content_key == self.content_key;
        other.is_from_me == self.is_from_me
            && other.sender_norm == self.sender_norm
            && other.source != self.source
            && !same_key
            && (same_body || same_attachments)
    }

    /// The row as a winner candidate.
    fn candidate(&self) -> Cand {
        Cand {
            id: self.id,
            source: self.source.clone(),
            att_count: self.att_count,
            whole_seconds: self.whole_seconds,
            run_messages: self.run_messages,
        }
    }
}

/// Flag messages that match one from another source within `window_secs` on chat,
/// direction, sender, and body or attachments, leaving out those the exact
/// pass hid (`exact_hidden`). Returns how many were flagged.
async fn flag_near_time_dupes(
    tx: &mut WriteTx<'_>,
    account_id: i64,
    prio: &HashMap<&str, usize>,
    window_secs: i64,
    exact_hidden: &HashSet<i64>,
) -> Result<u64> {
    let conn: &mut SqliteConnection = tx;
    let by_conversation = load_near_rows(conn, account_id, NearRows::All, exact_hidden).await?;
    let flags = cluster_near_dupes(by_conversation, prio, window_secs);
    if flags.is_empty() {
        return Ok(0);
    }
    apply_duplicate_flags(conn, "_pass_b_flags", &flags).await?;
    Ok(flags.len() as u64)
}

/// Which messages the near-time pass reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NearRows {
    /// Every message of the account.
    All,
    /// The messages with a content key: those a dedupe has already seen,
    /// and the changed ones [`dedupe_changed_messages`] gave one.
    Keyed,
}

/// The messages of the account in `rows` that are not in `hidden`, with
/// their attachment fingerprints, grouped by conversation. Two queries and
/// the grouping happen here so the clustering does no per-message lookups.
async fn load_near_rows(
    conn: &mut SqliteConnection,
    account_id: i64,
    rows: NearRows,
    hidden: &HashSet<i64>,
) -> Result<HashMap<i64, Vec<NearRow>>> {
    type NearDedupeRow = (
        i64,
        i64,
        String,
        i64,
        String,
        Option<String>,
        String,
        String,
        String,
        i64,
    );
    let msg_sql = format!(
        r"
        SELECT m.id, m.conversation_id, m.source, m.is_from_me, m.timestamp, m.body,
               COALESCE({sender}, ''), COALESCE(m.content_key, ''), m.time_precision,
               COALESCE(rc.n, 0)
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
    let msg_rows: Vec<NearDedupeRow> = sqlx::query_as(&msg_sql)
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?;

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

    let mut by_conversation: HashMap<i64, Vec<NearRow>> = HashMap::new();
    for (
        id,
        conversation_id,
        source,
        is_from_me,
        timestamp_rfc3339,
        body,
        sender_norm,
        content_key,
        time_precision,
        run_messages,
    ) in msg_rows
    {
        if hidden.contains(&id) {
            continue;
        }
        let Some(secs) = parse_rfc3339_utc_secs(&timestamp_rfc3339) else {
            continue;
        };
        let shas = shas_by_msg.remove(&id).unwrap_or_default();
        by_conversation
            .entry(conversation_id)
            .or_default()
            .push(NearRow {
                id,
                source,
                is_from_me,
                // Outgoing rows have no sender of their own.
                sender_norm: if is_from_me != 0 {
                    String::new()
                } else {
                    sender_norm
                },
                secs,
                body_norm: normalize_body(body.as_deref()),
                att_count: shas.len() as i64,
                att_fp: shas.join(","),
                content_key,
                whole_seconds: is_whole_seconds(id, &time_precision)?,
                run_messages,
            });
    }
    Ok(by_conversation)
}

/// Walk each conversation in time order, gather each message's twins within
/// the window, and flag each cluster by the rule of the exact pass
/// ([`exact_group_flags`]): the cluster stays shown as many times as the
/// source that holds it most often. A cluster counts only when it spans two
/// sources: same-source near-duplicates are left alone. Returns
/// `(loser, winner)` pairs.
///
/// A cluster is a star around its first row, and a row from the first row's
/// own source is never its twin, so the first row's source counts once in
/// it however often that source holds the message. A row the cluster kept,
/// other than the winner, therefore stays free to start or join a later
/// cluster, where it can pair with that source's next copy. The winner and
/// the hidden rows join no other cluster: every hidden row points at a
/// winner, and a winner hidden later would leave a chain.
fn cluster_near_dupes(
    by_conversation: HashMap<i64, Vec<NearRow>>,
    prio: &HashMap<&str, usize>,
    window_secs: i64,
) -> Vec<(i64, i64)> {
    let mut clustered: HashSet<i64> = HashSet::new();
    let mut flags: Vec<(i64, i64)> = Vec::new();
    for mut rows in by_conversation.into_values() {
        rows.sort_by(|a, b| a.secs.cmp(&b.secs).then(a.id.cmp(&b.id)));
        for i in 0..rows.len() {
            let first = &rows[i];
            if clustered.contains(&first.id) {
                continue;
            }
            let cluster: Vec<Cand> = std::iter::once(first)
                .chain(
                    rows[i + 1..]
                        .iter()
                        .take_while(|row| row.secs - first.secs <= window_secs)
                        .filter(|row| first.is_twin_of(row) && !clustered.contains(&row.id)),
                )
                .map(NearRow::candidate)
                .collect();
            let sources: HashSet<&str> = cluster.iter().map(|c| c.source.as_str()).collect();
            if sources.len() < 2 {
                continue;
            }
            let cluster_flags = exact_group_flags(&cluster, prio);
            clustered.extend(
                cluster_flags
                    .iter()
                    .flat_map(|&(loser, winner)| [loser, winner]),
            );
            flags.extend(cluster_flags);
        }
    }
    flags
}

/// Apply (message id, duplicate-of id) pairs through a temp table so one UPDATE covers them all.
async fn apply_duplicate_flags(
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

#[cfg(test)]
mod tests;
