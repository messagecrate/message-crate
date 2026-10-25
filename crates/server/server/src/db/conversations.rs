//! The conversation list and one conversation's summary and sources: what
//! `GET /v1/conversations`, `GET /v1/conversations/{id}` and its `sources`
//! read. `conversations_api` answers the routes; the queries live here.
//! So does the list of conversations with yourself that an identity change
//! measures against (`imports_api::with_yourself`).

use std::collections::{BTreeMap, HashMap};

use anyhow::Context;
use serde::Serialize;
use sqlx::{SqlSafeStr, SqliteConnection};

use crate::db::SearchListError;
use crate::db::ownership::owns_conversation;
use crate::db::participant_names::{Participant, load_for_conversations};
use crate::db::sql::{SqlParam, bind_args, fold_in_id_chunks, in_placeholders};
use crate::paging::{Direction, Page, PageParams, SortKey, page_from_rows};

/// The keys `GET /v1/conversations` accepts in `sort=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationSort {
    /// Timestamp of the most recent non-duplicate message in the thread.
    Date,
    /// Number of non-duplicate messages in the thread.
    Messages,
}

/// The accepted keys, as `sort=` spells them.
pub const CONVERSATION_SORT_KEYS: [(&str, ConversationSort); 2] = [
    ("date", ConversationSort::Date),
    ("messages", ConversationSort::Messages),
];

/// Newest activity first: what the list shows when `sort` is absent.
pub const DEFAULT_CONVERSATION_SORT: [SortKey<ConversationSort>; 1] = [SortKey {
    key: ConversationSort::Date,
    direction: Direction::Desc,
}];

/// The `ORDER BY` body for a parsed `sort`.
///
/// Every part is a fixed literal chosen by matching on the enum, so no part
/// of the request reaches the SQL text. Both columns are output aliases of
/// the page query, which SQLite allows in `ORDER BY`.
/// `c.id` breaks ties, in the direction of the last key, so paging cannot
/// repeat or skip a row.
///
/// `last_message_at` is NULL for a thread whose every message is a
/// duplicate, and SQLite sorts NULLs lowest. Leading with
/// `(last_message_at IS NULL)`, false before true, pins those threads to
/// the end in either direction.
fn conversation_order_by(keys: &[SortKey<ConversationSort>]) -> String {
    let mut parts: Vec<String> = keys
        .iter()
        .map(|k| match k.key {
            ConversationSort::Date => format!(
                "(last_message_at IS NULL) ASC, last_message_at {}",
                k.direction.sql()
            ),
            ConversationSort::Messages => format!("message_count {}", k.direction.sql()),
        })
        .collect();
    let tie = keys.last().map_or(Direction::Desc, |k| k.direction);
    parts.push(format!("c.id {}", tie.sql()));
    parts.join(", ")
}

/// Conversation row for the list: participants, counts, tags.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ConversationSummary {
    /// The conversation's id; search for it as `in:#<id>`.
    pub id: i64,
    /// Participants with names and identities.
    pub participants: Vec<Participant>,
    /// Messages in the conversation (excluding hidden duplicates).
    pub message_count: u64,
    /// Timestamp of the conversation's first message. `null` when every
    /// message in the conversation is a duplicate, so none is left to date it.
    pub first_message_at: Option<String>,
    /// Timestamp of the conversation's last message. `null` when every
    /// message in the conversation is a duplicate, so none is left to date it.
    pub last_message_at: Option<String>,
    /// The service the conversation's messages were sent on, named from
    /// each message's own transport and never from the import it came from:
    /// `text_message` for SMS, MMS, iMessage and RCS, `whatsapp` for
    /// WhatsApp. `unknown` when a message's transport could not be read, or
    /// when the messages name two different services.
    pub service: ConversationService,
    /// True for group conversations.
    pub is_group: bool,
    /// The title the conversation is shown by: for a conversation the account
    /// holder has with themselves, the account's display name or, without
    /// one, the conversation's own address; for one of orphaned messages, its
    /// person's name and "Orphaned", or "Orphaned · Unknown person" for the
    /// one that names nobody; for any other, the export's title. `null` when
    /// there is none, and the conversation goes by its participants.
    pub shown_title: Option<String>,
    /// Message tags on this conversation.
    pub tags: Vec<String>,
}

/// Whether a stored `conversation_type` is a group: `group` in any case.
/// The conversation list and a message's conversation both answer by this,
/// so the two never disagree about one conversation.
pub(crate) fn is_group_type(conversation_type: &str) -> bool {
    conversation_type.eq_ignore_ascii_case("group")
}

/// One row of a [`conversation_row_select`]-shaped query.
#[derive(sqlx::FromRow)]
struct RawConversation {
    id: i64,
    conversation_type: String,
    shown_title: Option<String>,
    message_count: i64,
    first_message_at: Option<String>,
    last_message_at: Option<String>,
}

/// One page of the conversation list for `q`, a query in the search language.
///
/// # Errors
///
/// [`SearchListError::Query`] for a query the language refuses;
/// [`SearchListError::Internal`] when a statement fails.
pub async fn list_conversations_sorted(
    conn: &mut SqliteConnection,
    account_id: i64,
    q: &str,
    order: &[SortKey<ConversationSort>],
    page: PageParams,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<Page<ConversationSummary>, SearchListError> {
    let (zone, today) = clock;
    let filter = crate::search::compile(crate::search::CompileRequest {
        list: crate::search::ListKind::Conversations,
        query: q,
        account_id,
        today,
        zone,
    })?;
    let where_sql = filter.where_sql();

    let count_sql = format!("SELECT COUNT(*) FROM conversations c WHERE {where_sql}");
    let total: i64 =
        sqlx::query_scalar_with(sqlx::AssertSqlSafe(count_sql), bind_args(filter.params()))
            .fetch_one(&mut *conn)
            .await
            .context("count the conversations a list query matches")?;
    let total = total.max(0) as u64;

    let mut params = filter.params().to_vec();
    params.push(SqlParam::Int(page.limit as i64));
    params.push(SqlParam::Int(
        i64::try_from(page.offset).map_err(anyhow::Error::from)?,
    ));
    // The sort reads computed columns (`last_message_at`, `message_count`)
    // inside expressions. Sorting the rows as a derived table makes those
    // aliases real columns.
    let sql = format!(
        "SELECT * FROM ({select} WHERE {where_sql}) AS c ORDER BY {order_by} LIMIT ? OFFSET ?",
        select = conversation_row_select(),
        order_by = conversation_order_by(order),
    );
    let out = load_conversation_rows(conn, account_id, sqlx::AssertSqlSafe(sql), &params).await?;
    Ok(page_from_rows(out, total, page))
}

/// One conversation by id, scoped to `account_id`. `None` when the id does
/// not exist or belongs to another account. The two cases look identical to
/// the caller, which is what keeps this a 404 rather than a 403.
///
/// # Errors
///
/// `Internal` when a statement fails.
pub async fn get_conversation_summary(
    conn: &mut SqliteConnection,
    account_id: i64,
    conversation_id: i64,
) -> anyhow::Result<Option<ConversationSummary>> {
    let sql = format!(
        "{} WHERE c.id = ? AND c.account_id = ?",
        conversation_row_select()
    );
    let params = [SqlParam::Int(conversation_id), SqlParam::Int(account_id)];
    let out = load_conversation_rows(conn, account_id, sqlx::AssertSqlSafe(sql), &params).await?;
    Ok(out.into_iter().next())
}

/// A SQL condition, true when conversation `c` is one the account holder has
/// with themselves: a one-to-one conversation whose own identity is one of
/// the account's identities, such as notes sent to their own number (#1094).
/// `c` is the alias of a `conversations` row. `with:me` asks this, and
/// [`shown_title_sql`] names such a conversation by it.
#[must_use]
pub fn is_with_yourself_sql(c: &str) -> String {
    format!(
        "(lower({c}.conversation_type) = 'one-to-one'
          AND EXISTS (SELECT 1 FROM handles hy WHERE hy.id = {c}.chat_handle_id AND {}))",
        crate::db::account_profile::is_account_identity_sql("hy", &format!("{c}.account_id"))
    )
}

/// What an orphaned conversation's title adds to its person's name.
pub const ORPHANED: &str = "Orphaned";

/// The title of the orphaned conversation that names nobody: the account
/// holder's orphaned messages with no recipient recorded, and received ones
/// with no sender or with one of the holder's own addresses as sender.
pub const ORPHANED_UNKNOWN_PERSON: &str = "Orphaned · Unknown person";

/// The title conversation `c` is shown by, as a SQL expression. A
/// conversation with yourself ([`is_with_yourself_sql`]) goes by the
/// account's display name, or its own address when the account has none,
/// whatever title the export gave it. An orphaned conversation goes by its
/// person's name and [`ORPHANED`], "Ada · Orphaned", whichever way its
/// messages went, or, naming nobody, by [`ORPHANED_UNKNOWN_PERSON`] (#1095,
/// #1778). Any other conversation goes by the export's title, and is NULL
/// without one, going by its participants. Computed on every read, so it
/// follows a change of the display name or the person's. The list, the
/// single-conversation read, the message rows, `title:`, `in:` and plain
/// text all read this one expression.
#[must_use]
pub fn shown_title_sql(c: &str) -> String {
    format!(
        "CASE WHEN {with_yourself} THEN COALESCE(
                  (SELECT NULLIF(trim(ay.preferred_name), '') FROM accounts ay
                   WHERE ay.id = {c}.account_id),
                  (SELECT hy.raw FROM handles hy WHERE hy.id = {c}.chat_handle_id))
              WHEN {orphaned} THEN COALESCE({person} || ' · {ORPHANED}',
                                            '{ORPHANED_UNKNOWN_PERSON}')
              ELSE NULLIF(trim({c}.group_title), '') END",
        with_yourself = is_with_yourself_sql(c),
        orphaned = is_orphaned_sql(c),
        person = crate::db::participant_names::first_participant_name_sql(c),
    )
}

/// A SQL condition, true when conversation `c` holds orphaned messages: its
/// type is [`ir::ORPHANED_CONVERSATION_TYPE`], in any case. `c` is
/// the alias of a `conversations` row.
#[must_use]
pub fn is_orphaned_sql(c: &str) -> String {
    format!(
        "lower({c}.conversation_type) = '{}'",
        ir::ORPHANED_CONVERSATION_TYPE
    )
}

/// The row shape shared by the conversation list and the single-conversation
/// read: id, type, title ([`shown_title_sql`]), and the
/// counts/timestamps computed from `messages`. Callers append their own
/// `WHERE`, `ORDER BY`, and paging.
fn conversation_row_select() -> String {
    format!(
        "SELECT c.id,
                c.conversation_type,
                {shown_title} AS shown_title,
                (SELECT COUNT(*) FROM messages m
                 WHERE m.conversation_id = c.id AND m.duplicate_of IS NULL) AS message_count,
                (SELECT MIN(m.timestamp) FROM messages m
                 WHERE m.conversation_id = c.id AND m.duplicate_of IS NULL) AS first_message_at,
                (SELECT MAX(m.timestamp) FROM messages m
                 WHERE m.conversation_id = c.id AND m.duplicate_of IS NULL) AS last_message_at
         FROM conversations c",
        shown_title = shown_title_sql("c")
    )
}

/// Run a [`conversation_row_select`]-shaped query and assemble
/// [`ConversationSummary`] rows: participants, service, and tags, exactly as
/// the list builds them. Shared so the list and the single-conversation read
/// cannot drift into two different notions of what a conversation summary is.
async fn load_conversation_rows(
    conn: &mut SqliteConnection,
    account_id: i64,
    sql: impl SqlSafeStr,
    params: &[SqlParam],
) -> anyhow::Result<Vec<ConversationSummary>> {
    let rows = sqlx::query_as_with::<_, RawConversation, _>(sql, bind_args(params))
        .fetch_all(&mut *conn)
        .await
        .context("read conversation rows")?;

    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let mut participants = load_for_conversations(conn, &ids)
        .await
        .context("read the participants of the conversations")?;
    let services = conversation_services(conn, &ids).await?;
    let mut tag_sets = crate::db::named_membership::names_for_items(
        crate::db::named_membership::tag_spec(),
        conn,
        account_id,
        &ids,
    )
    .await
    .context("read the tags of the conversations")?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let is_group = is_group_type(&row.conversation_type);
        let service = services
            .get(&row.id)
            .copied()
            .unwrap_or(ConversationService::Unknown);
        let parts = participants.remove(&row.id).unwrap_or_default();
        out.push(ConversationSummary {
            id: row.id,
            participants: parts,
            message_count: row.message_count.max(0) as u64,
            first_message_at: row.first_message_at,
            last_message_at: row.last_message_at,
            service,
            is_group,
            shown_title: row
                .shown_title
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            tags: tag_sets.remove(&row.id).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// The service a conversation's messages were sent on: `text_message` (Text
/// Message: SMS, MMS, iMessage and RCS) or `whatsapp`, the two an identity
/// is on (`IdentityService`); `discord`, `signal`, `telegram` or `slack`,
/// which the conversation file names and no source reads today; or
/// `unknown`, when a message's transport could not be read or the messages
/// name two different services. `unknown` is never a default: each one is a
/// fault, and the import that wrote it logs a warning.
//
// The rule, its reason and what was rejected:
// docs/architecture/rules/contacts-identities-and-messages.md, CONTACT-56.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationService {
    /// Text Message: SMS, MMS, iMessage and RCS.
    TextMessage,
    /// WhatsApp.
    Whatsapp,
    /// Discord.
    Discord,
    /// Signal.
    Signal,
    /// Telegram.
    Telegram,
    /// Slack.
    Slack,
    /// A message's transport could not be read, or the messages name two
    /// different services. Never a default: each one is a fault to look
    /// into, and the import that promotes such a message logs a warning.
    Unknown,
}

impl ConversationService {
    /// The service one message's transport belongs to.
    #[must_use]
    pub fn of_transport(transport: ir::Transport) -> Self {
        match transport {
            ir::Transport::Sms | ir::Transport::IMessage | ir::Transport::Rcs => Self::TextMessage,
            ir::Transport::Whatsapp => Self::Whatsapp,
            ir::Transport::Discord => Self::Discord,
            ir::Transport::Signal => Self::Signal,
            ir::Transport::Telegram => Self::Telegram,
            ir::Transport::Slack => Self::Slack,
            ir::Transport::Unknown => Self::Unknown,
        }
    }

    /// The service of a conversation whose messages carry the stored
    /// transports `transports` (`messages.service`): the one service they
    /// all belong to, and [`Self::Unknown`] when one is NULL or a word the
    /// conversation file does not name, when they belong to two services,
    /// or when there are none.
    #[must_use]
    pub fn of_transports<'a>(transports: impl IntoIterator<Item = Option<&'a str>>) -> Self {
        let mut found = None;
        for transport in transports {
            let service = transport.map_or(Self::Unknown, |t| {
                Self::of_transport(ir::Transport::parse(t))
            });
            match found {
                None => found = Some(service),
                Some(earlier) if earlier == service => {}
                Some(_) => return Self::Unknown,
            }
        }
        found.unwrap_or(Self::Unknown)
    }

    /// The value as the wire spells it, such as `text_message`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextMessage => "text_message",
            Self::Whatsapp => "whatsapp",
            Self::Discord => "discord",
            Self::Signal => "signal",
            Self::Telegram => "telegram",
            Self::Slack => "slack",
            Self::Unknown => "unknown",
        }
    }
}

/// One stored transport of one conversation, for [`conversation_transports`].
#[derive(sqlx::FromRow)]
struct TransportRow {
    conversation_id: i64,
    service: Option<String>,
}

/// The distinct stored transports (`messages.service`) of each of
/// `conversation_ids`, NULL as `None`. A conversation with no message is
/// left out. Every message counts, a hidden duplicate included, because it
/// is still a message of the conversation.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub(crate) async fn conversation_transports(
    conn: &mut SqliteConnection,
    conversation_ids: &[i64],
) -> anyhow::Result<HashMap<i64, Vec<Option<String>>>> {
    fold_in_id_chunks(conn, conversation_ids, |conn, chunk| {
        Box::pin(async move {
            let placeholders = in_placeholders(1, chunk.len());
            let sql = format!(
                "SELECT conversation_id, service
                 FROM messages
                 WHERE conversation_id IN ({placeholders})
                 GROUP BY conversation_id, service"
            );
            let mut q = sqlx::query_as::<_, TransportRow>(sqlx::AssertSqlSafe(sql));
            for id in chunk {
                q = q.bind(*id);
            }
            let rows = q
                .fetch_all(&mut *conn)
                .await
                .context("read the transports of the conversations")?;
            Ok(rows
                .into_iter()
                .map(|r| (r.conversation_id, r.service))
                .collect())
        })
    })
    .await
}

/// The service of each of `conversation_ids`, by
/// [`ConversationService::of_transports`]. A conversation with no message is
/// left out; its service is [`ConversationService::Unknown`].
///
/// # Errors
///
/// Returns an error when the statement fails.
pub(crate) async fn conversation_services(
    conn: &mut SqliteConnection,
    conversation_ids: &[i64],
) -> anyhow::Result<HashMap<i64, ConversationService>> {
    Ok(conversation_transports(conn, conversation_ids)
        .await?
        .into_iter()
        .map(|(id, transports)| {
            let service =
                ConversationService::of_transports(transports.iter().map(Option::as_deref));
            (id, service)
        })
        .collect())
}

/// What a promotion wrote to one conversation, for the warning of
/// CONTACT-56: the distinct transports and sources of the messages it
/// added, and the distinct transports of the messages the conversation held
/// before.
#[derive(Debug, Default)]
pub(crate) struct PromotedTransports {
    /// Each distinct pair of transport (`messages.service`, NULL as `None`)
    /// and source of the added messages, in order.
    pub added: Vec<(Option<String>, String)>,
    /// Each distinct transport of the earlier messages; empty for a
    /// conversation the promotion made.
    pub earlier: Vec<Option<String>>,
}

/// One added message's transport and source, for [`transports_promoted_above`].
#[derive(sqlx::FromRow)]
struct AddedRow {
    conversation_id: i64,
    service: Option<String>,
    source: String,
}

/// Each conversation of `account_id` that holds a message with an id above
/// `message_id`, with [`PromotedTransports`] split at that id: when
/// `message_id` is the highest id before a promotion, the conversations it
/// wrote to, what it added and what was there before.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub(crate) async fn transports_promoted_above(
    conn: &mut SqliteConnection,
    account_id: i64,
    message_id: i64,
) -> anyhow::Result<BTreeMap<i64, PromotedTransports>> {
    let added: Vec<AddedRow> = sqlx::query_as(
        "SELECT conversation_id, service, source FROM messages
         WHERE account_id = $1 AND id > $2
         GROUP BY conversation_id, service, source
         ORDER BY conversation_id, service, source",
    )
    .bind(account_id)
    .bind(message_id)
    .fetch_all(&mut *conn)
    .await
    .context("read the transports of the messages the import added")?;
    let mut out: BTreeMap<i64, PromotedTransports> = BTreeMap::new();
    for row in added {
        out.entry(row.conversation_id)
            .or_default()
            .added
            .push((row.service, row.source));
    }
    let ids: Vec<i64> = out.keys().copied().collect();
    let earlier = fold_in_id_chunks(conn, &ids, |conn, chunk| {
        Box::pin(async move {
            let placeholders = in_placeholders(2, chunk.len());
            let sql = format!(
                "SELECT conversation_id, service FROM messages
                 WHERE id <= $1 AND conversation_id IN ({placeholders})
                 GROUP BY conversation_id, service"
            );
            let mut q =
                sqlx::query_as::<_, TransportRow>(sqlx::AssertSqlSafe(sql)).bind(message_id);
            for id in chunk {
                q = q.bind(*id);
            }
            let rows = q
                .fetch_all(&mut *conn)
                .await
                .context("read the transports of the conversations' earlier messages")?;
            anyhow::Ok(
                rows.into_iter()
                    .map(|r| (r.conversation_id, r.service))
                    .collect(),
            )
        })
    })
    .await?;
    for (id, transports) in earlier {
        if let Some(entry) = out.get_mut(&id) {
            entry.earlier = transports;
        }
    }
    Ok(out)
}

/// One backup source with message counts and share.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ConversationSource {
    /// Backup source name.
    pub backup_name: String,
    /// Messages in this conversation from this source.
    pub message_count: u64,
    /// Messages only this source has (not hidden duplicates).
    pub unique_count: u64,
    /// Share of the conversation's unique messages, 0–100.
    pub percentage: f64,
}

/// Per-source message counts for the Sources panel.
///
/// # Errors
///
/// Returns an internal error when a database statement fails.
pub async fn list_conversation_source_stats(
    conn: &mut SqliteConnection,
    account_id: i64,
    conversation_id: i64,
) -> anyhow::Result<Option<Vec<ConversationSource>>> {
    if !owns_conversation(conn, account_id, conversation_id)
        .await
        .context("check that the account holds the conversation")?
    {
        return Ok(None);
    }

    #[derive(sqlx::FromRow)]
    struct Row {
        source: String,
        message_count: i64,
        unique_count: i64,
    }
    let rows = sqlx::query_as::<_, Row>(
        "SELECT source,
                    COUNT(*) AS message_count,
                    SUM(CASE WHEN duplicate_of IS NULL THEN 1 ELSE 0 END) AS unique_count
             FROM messages
             WHERE conversation_id = $1
             GROUP BY source
             ORDER BY source",
    )
    .bind(conversation_id)
    .fetch_all(&mut *conn)
    .await
    .context("count a conversation's messages by source")?;

    let total_unique: i64 = rows.iter().map(|row| row.unique_count).sum();
    let sources = rows
        .into_iter()
        .map(
            |Row {
                 source,
                 message_count,
                 unique_count,
             }| {
                let percentage = if total_unique > 0 {
                    (unique_count as f64) * 100.0 / (total_unique as f64)
                } else {
                    0.0
                };
                ConversationSource {
                    backup_name: source,
                    message_count: message_count.max(0) as u64,
                    unique_count: unique_count.max(0) as u64,
                    percentage: (percentage * 10.0).round() / 10.0,
                }
            },
        )
        .collect();
    Ok(Some(sources))
}

/// The ids of the account's conversations with yourself, by the identities
/// it has now ([`is_with_yourself_sql`]).
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn with_yourself_ids(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> anyhow::Result<Vec<i64>> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT c.id FROM conversations c
         WHERE c.account_id = $1 AND {}
         ORDER BY c.id",
        is_with_yourself_sql("c")
    )))
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .context("read the account's conversations with yourself")
}

/// The chat handle of `conversation_id`, or `None` when the account has no
/// such conversation.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn chat_handle_id(
    conn: &mut SqliteConnection,
    account_id: i64,
    conversation_id: i64,
) -> anyhow::Result<Option<i64>> {
    sqlx::query_scalar("SELECT chat_handle_id FROM conversations WHERE account_id = $1 AND id = $2")
        .bind(account_id)
        .bind(conversation_id)
        .fetch_optional(&mut *conn)
        .await
        .context("read a conversation's chat handle")
}
