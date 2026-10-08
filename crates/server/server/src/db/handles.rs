//! Shared handle identity helpers: one key for matching, and one type for an
//! address the source did not type.

use std::collections::HashMap;

use anyhow::Result;
use message_ir::{HandleService, HandleType};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::search::bridge::{TrashScope, contact_sent_messages_from};

/// The handle ids one import has resolved, and the country its run states
/// for phone numbers written without their `+` code.
///
/// The country belongs with the cache because it is part of the key: the
/// same `07700 900123` is `+447700900123` in a run whose country is the
/// United Kingdom and the digits as written in a run with none (#1676).
#[derive(Debug, Default)]
pub struct HandleIdCache {
    /// `(account_id, normalized, handle_type, service)` → handle id.
    ids: HashMap<(String, String, String, String), i64>,
    /// The run's country, when it states one.
    country: Option<&'static phone::Country>,
}

impl HandleIdCache {
    /// An empty cache for a run that states `country`.
    #[must_use]
    pub fn in_country(country: Option<&'static phone::Country>) -> Self {
        Self {
            ids: HashMap::new(),
            country,
        }
    }

    /// The run's country, when it states one.
    #[must_use]
    pub fn country(&self) -> Option<&'static phone::Country> {
        self.country
    }
}

/// One standard form of a handle for identity matching, per type, with the
/// country of a phone number whose form is certain and a human-readable note
/// when it is not (guarded policy).
///
/// Phone: E.164 when the raw is unambiguous: written with its `+`, or
/// written without it in `country`, the country a run or a person stated.
/// Otherwise the digits as written, with a review note and no region:
/// `020 7946 0000` with no country is `02079460000`, never `+02079460000`
/// and never a US number (#1676). Email: lowercased. Username/Other:
/// verbatim (trimmed).
pub fn normalize_handle(
    raw: &str,
    handle_type: HandleType,
    country: Option<&'static phone::Country>,
) -> phone::TypedKey {
    phone::key_typed_handle(raw, handle_type, country)
}

/// The region a stored key carries: the calling code of a phone key written
/// in its `+` form, else empty. For a row whose key was made elsewhere, such
/// as an address book load.
#[must_use]
pub fn region_of_key(normalized: &str, handle_type: &str) -> String {
    match normalized.strip_prefix('+') {
        Some(digits) if handle_type == HandleType::Phone.as_str() => phone::calling_code_of(digits)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

/// The shape of an address the source did not type: [`phone::Handle::parse`],
/// the one rule for what an address looks like. A blank address is `Other`.
/// An import never stores this type without asking the service first:
/// [`handle_type_on`] does both.
pub fn handle_type_of(address: &str) -> HandleType {
    phone::Handle::parse(address).map_or(HandleType::Other, |handle| handle.kind())
}

/// The type an import gives an address it meets on `service`: the service
/// decides which types are valid, and the shape picks among them.
///
/// `stated` is the type the source gave, else the shape ([`handle_type_of`])
/// decides. WhatsApp carries phone numbers and its own ids (`…@lid`,
/// `…@g.us`, `…@s.whatsapp.net`), which are `Other`, so an address with an
/// `@` on WhatsApp is `Other` however it looks (#1671). iMessage reaches an
/// email address and shares the phone service with SMS, and an address has
/// one type on one service, so the shape's type stands there; how SMS's
/// phone-only rule is held on that service is #1958. A phone number is a
/// phone number on every service (#1144).
pub fn handle_type_on(
    address: &str,
    stated: Option<HandleType>,
    service: HandleService,
) -> HandleType {
    match (service, stated.unwrap_or_else(|| handle_type_of(address))) {
        (HandleService::Whatsapp, HandleType::Email) => HandleType::Other,
        (_, kind) => kind,
    }
}

/// A new identity refused because its service cannot carry its type: an
/// email address on WhatsApp, the one pair refused. WhatsApp reaches a person
/// by phone number, and keeps an internal id for one it knows by no number as
/// `other`, so it carries no email address. The phone service carries every
/// type: iMessage reaches an email address too.
#[derive(Debug, thiserror::Error)]
#[error("{address} is an email address, and WhatsApp carries no email addresses")]
pub struct EmailOnWhatsapp {
    /// The address as the request gave it.
    pub address: String,
}

/// Refuse a new identity of `handle_type` on `service` when the service
/// cannot carry it.
///
/// # Errors
///
/// [`EmailOnWhatsapp`] for an email address on WhatsApp.
pub fn check_service_carries(
    address: &str,
    service: HandleService,
    handle_type: HandleType,
) -> std::result::Result<(), EmailOnWhatsapp> {
    match (service, handle_type) {
        (HandleService::Whatsapp, HandleType::Email) => Err(EmailOnWhatsapp {
            address: address.to_string(),
        }),
        _ => Ok(()),
    }
}

/// The service an identity is on, as a request names it and the profile
/// reports it: `phone` or `whatsapp`. `phone` is Text Message, which carries
/// SMS, MMS, iMessage and RCS, and reaches an email address through iMessage.
/// The service never decides the identity's type, which comes from the
/// address.
//
// Any other word is refused, naming the two. Read as `phone`, a misspelt
// `whatsap` put an identity on Text Message without a word (#1630), and
// `email` lived on as a second name for `phone` (#1631).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum IdentityService {
    /// Text Message: SMS, MMS, iMessage and RCS.
    Phone,
    /// WhatsApp.
    Whatsapp,
}

impl From<HandleService> for IdentityService {
    fn from(service: HandleService) -> Self {
        match service {
            HandleService::Phone => Self::Phone,
            HandleService::Whatsapp => Self::Whatsapp,
        }
    }
}

impl From<IdentityService> for HandleService {
    fn from(service: IdentityService) -> Self {
        match service {
            IdentityService::Phone => Self::Phone,
            IdentityService::Whatsapp => Self::Whatsapp,
        }
    }
}

/// The id of the `handles` row the account already holds for `raw` on
/// `service`, matched by the address as written or by its key, if any.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn existing_handle_id(
    conn: &mut SqliteConnection,
    account_id: i64,
    raw: &str,
    service: HandleService,
) -> Result<Option<i64>> {
    let raw = raw.trim();
    let key = phone::Handle::parse(raw).map_or_else(|| raw.to_string(), phone::Handle::into_key);
    let id = sqlx::query_scalar(
        "SELECT id FROM handles
         WHERE account_id = $1 AND service = $2 AND (raw = $3 OR normalized = $4)
         ORDER BY id
         LIMIT 1",
    )
    .bind(account_id)
    .bind(service.as_str())
    .bind(raw)
    .bind(key.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    Ok(id)
}

/// Insert or reuse a `handles` row for an address no run or person stated a
/// country for. Returns the id and whether this call newly inserted a
/// flagged (review-note) row.
pub async fn upsert_handle_row(
    conn: &mut SqliteConnection,
    account_id: i64,
    raw: &str,
    handle_type: HandleType,
    service: Option<&str>,
) -> Result<(i64, bool)> {
    upsert_handle_row_in(conn, account_id, raw, handle_type, service, None).await
}

/// Insert or reuse a `handles` row, reading a phone number written without
/// its `+` code in `country`. Returns the id and whether this call newly
/// inserted a flagged (review-note) row.
pub async fn upsert_handle_row_in(
    conn: &mut SqliteConnection,
    account_id: i64,
    raw: &str,
    handle_type: HandleType,
    service: Option<&str>,
    country: Option<&'static phone::Country>,
) -> Result<(i64, bool)> {
    let phone::TypedKey {
        key: normalized,
        region,
        note,
    } = normalize_handle(raw, handle_type, country);
    let platform = HandleService::parse(service.unwrap_or(HandleService::Phone.as_str()));
    let service_str = platform.as_str();
    let inserted = sqlx::query(
        "INSERT INTO handles
           (account_id, raw, normalized, region, normalized_note, handle_type, service)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(raw)
    .bind(normalized.as_str())
    .bind(region.as_str())
    .bind(note.as_deref())
    .bind(handle_type.as_str())
    .bind(service_str)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM handles
         WHERE account_id = $1 AND normalized = $2 AND handle_type = $3 AND service = $4",
    )
    .bind(account_id)
    .bind(normalized.as_str())
    .bind(handle_type.as_str())
    .bind(service_str)
    .fetch_one(&mut *conn)
    .await?;
    Ok((id, inserted > 0 && note.is_some()))
}

/// Same as [`upsert_handle_row`], but skip the two SQL statements when this
/// import already resolved the same identity. Third value is `true` on a
/// cache hit so callers can skip leftover per-row work (sibling contact link).
pub async fn upsert_handle_row_cached(
    conn: &mut SqliteConnection,
    cache: &mut HandleIdCache,
    account_id: i64,
    raw: &str,
    handle_type: HandleType,
    service: Option<&str>,
) -> Result<(i64, bool, bool)> {
    let normalized = normalize_handle(raw, handle_type, cache.country).key;
    let platform = HandleService::parse(service.unwrap_or(HandleService::Phone.as_str()));
    let key = (
        account_id.to_string(),
        normalized,
        handle_type.as_str().to_string(),
        platform.as_str().to_string(),
    );
    if let Some(&id) = cache.ids.get(&key) {
        return Ok((id, false, true));
    }
    let (id, flagged) =
        upsert_handle_row_in(conn, account_id, raw, handle_type, service, cache.country).await?;
    cache.ids.insert(key, id);
    Ok((id, flagged, false))
}

/// One identity of a contact or an account, and its messages: for a contact,
/// the messages the contact sent from it; for an account, the messages held
/// at it, sent from or received at that address (ADR-0015). The contact
/// drawer and the Profile screen show the same table, so they read the same
/// row.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Identity {
    /// The identity as the database stores it: E.164 for a number, lower case
    /// for an address.
    pub address: String,
    /// `phone`, `email`, or `whatsapp`.
    pub service: String,
    /// True for a phone number written without its `+` code whose country
    /// nobody has stated: it is stored as the digits typed and matches no
    /// number written with its `+` until a country is picked for it. A
    /// short code, under 7 digits, has no `+` form and is never flagged.
    pub country_unknown: bool,
    /// When the identity's oldest message was sent, or null when there is
    /// none.
    pub start_date: Option<String>,
    /// When the newest such message was sent, or null when there is none.
    pub end_date: Option<String>,
    /// Direct, group, and orphaned conversations: for a contact, those the identity
    /// takes part in, whoever wrote in them; for an account, those holding
    /// at least one of the identity's messages. Trashed conversations are
    /// excluded.
    pub conversations: u64,
    /// The identity's messages in one-to-one conversations, trashed
    /// conversations and duplicates excluded.
    pub direct_messages: u64,
    /// The identity's messages in group conversations, on the same terms.
    pub group_messages: u64,
    /// The identity's messages in conversations of orphaned messages, on the
    /// same terms: the three message counts add up to all of its messages.
    pub orphaned_messages: u64,
}

/// Whether the `handles` row `h` is a phone number whose country is unknown:
/// stored as the digits typed, with no region, and long enough to have a `+`
/// form once a country is picked (`phone`'s minimum of 7 national digits).
pub const COUNTRY_UNKNOWN_SQL: &str = "(h.handle_type = 'phone' AND h.region = '' \
     AND h.normalized NOT LIKE '+%' AND length(h.normalized) >= 7)";

/// Whose identities [`identities`] reads.
#[derive(Debug, Clone, Copy)]
pub enum IdentitiesOf {
    /// The identities the account holds as its own.
    Account(i64),
    /// The identities linked to one of the account's contacts.
    Contact { account_id: i64, contact_id: i64 },
}

/// One row of [`identities`]: address, service, whether its country is
/// unknown, first and last timestamp, conversation count, direct, group, and
/// orphaned message counts.
type IdentityRow = (
    String,
    String,
    bool,
    Option<String>,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
);

/// The identities of an account or of a contact, each with its messages,
/// phones before emails and each in order.
pub async fn identities(conn: &mut SqliteConnection, of: IdentitiesOf) -> Result<Vec<Identity>> {
    let not_trashed = crate::search::emit::NOT_TRASHED_CONVERSATION;
    // `messages` is a `FROM … WHERE …` over identity `l`'s messages. It ends
    // in its WHERE clause, so a column can narrow it by conversation type.
    //
    // The account holder is never a participant, so an account's identity
    // counts the messages held at it and the conversations holding them
    // (ADR-0015). A contact's identity counts the messages it sent, by the
    // one definition every contact count reads, and the conversations it
    // takes part in, whoever wrote in them (#913).
    let (account_id, linked, scope, messages, conversations) = match of {
        IdentitiesOf::Account(account_id) => {
            let messages = format!(
                "FROM messages m
                   JOIN conversations c ON c.id = m.conversation_id AND {not_trashed}
                 WHERE m.owner_handle_id = l.handle_id AND m.account_id = $1
                   AND m.duplicate_of IS NULL"
            );
            let conversations = format!("(SELECT COUNT(DISTINCT c.id) {messages})");
            (
                account_id,
                "SELECT handle_id FROM account_handles WHERE account_id = $1",
                "",
                messages,
                conversations,
            )
        }
        IdentitiesOf::Contact { account_id, .. } => (
            account_id,
            "SELECT handle_id FROM contact_handles WHERE account_id = $1 AND contact_id = $2",
            "JOIN contacts ct ON ct.account_id = $1 AND ct.id = $2",
            contact_sent_messages_from("l.handle_id", TrashScope::LeftOut),
            format!(
                "(SELECT COUNT(*) FROM conversations c
                  WHERE c.account_id = $1
                    AND (c.chat_handle_id = l.handle_id
                         OR EXISTS (
                           SELECT 1 FROM participants p
                           WHERE p.conversation_id = c.id AND p.handle_id = l.handle_id
                         ))
                    AND {not_trashed})"
            ),
        ),
    };
    let country_unknown = COUNTRY_UNKNOWN_SQL;
    let sql = format!(
        "WITH linked AS ({linked})
         SELECT h.normalized,
                CASE WHEN h.handle_type = 'email' THEN 'email'
                     WHEN h.service = 'whatsapp' THEN 'whatsapp'
                     ELSE 'phone' END AS service,
                {country_unknown},
                (SELECT MIN(m.timestamp) {messages}),
                (SELECT MAX(m.timestamp) {messages}),
                {conversations},
                (SELECT COUNT(*) {messages} AND c.conversation_type = 'individual'),
                (SELECT COUNT(*) {messages} AND c.conversation_type = 'group'),
                (SELECT COUNT(*) {messages} AND c.conversation_type = 'orphaned')
         FROM linked l
         JOIN handles h ON h.id = l.handle_id
         {scope}
         ORDER BY CASE WHEN h.handle_type = 'email' THEN 1 ELSE 0 END, h.normalized",
    );
    let mut query = sqlx::query_as::<_, IdentityRow>(&sql).bind(account_id);
    if let IdentitiesOf::Contact { contact_id, .. } = of {
        query = query.bind(contact_id);
    }
    let rows = query.fetch_all(&mut *conn).await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                handle,
                service,
                country_unknown,
                start_date,
                end_date,
                conversations,
                direct,
                group,
                orphaned,
            )| {
                Identity {
                    address: handle,
                    service,
                    country_unknown,
                    start_date,
                    end_date,
                    conversations: conversations.max(0) as u64,
                    direct_messages: direct.max(0) as u64,
                    group_messages: group.max(0) as u64,
                    orphaned_messages: orphaned.max(0) as u64,
                }
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    const TEST_ACCOUNT: i64 = 7;

    /// The count of numbers that need a look takes one for each identity the
    /// call both created and flagged, so a plain new number and a flagged
    /// number met again add nothing.
    #[tokio::test]
    async fn only_the_first_insert_of_a_flagged_handle_is_reported_as_flagged() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_schema(&mut conn).await.unwrap();
        crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap();

        let (_, plain) = upsert_handle_row(
            &mut conn,
            TEST_ACCOUNT,
            "+15555550100",
            HandleType::Phone,
            Some("phone"),
        )
        .await
        .unwrap();
        assert!(!plain, "a new number with no review note is not flagged");

        // A trunk-zero number cannot be made E.164, so it carries a note.
        let ambiguous = "020 7946 0000";
        let (first_id, first) = upsert_handle_row(
            &mut conn,
            TEST_ACCOUNT,
            ambiguous,
            HandleType::Phone,
            Some("phone"),
        )
        .await
        .unwrap();
        assert!(first, "a new number with a review note is flagged");

        let (second_id, second) = upsert_handle_row(
            &mut conn,
            TEST_ACCOUNT,
            ambiguous,
            HandleType::Phone,
            Some("phone"),
        )
        .await
        .unwrap();
        assert_eq!(first_id, second_id);
        assert!(
            !second,
            "a flagged number already known is not counted again"
        );
    }

    #[tokio::test]
    async fn upsert_handle_row_cached_reuses_id_without_second_row() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_schema(&mut conn).await.unwrap();
        crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap();
        let mut cache = HandleIdCache::default();
        let (first, _, first_cached) = upsert_handle_row_cached(
            &mut conn,
            &mut cache,
            TEST_ACCOUNT,
            "+15555550100",
            HandleType::Phone,
            Some("phone"),
        )
        .await
        .unwrap();
        let (second, flagged, second_cached) = upsert_handle_row_cached(
            &mut conn,
            &mut cache,
            TEST_ACCOUNT,
            "+15555550100",
            HandleType::Phone,
            Some("phone"),
        )
        .await
        .unwrap();
        assert_eq!(first, second);
        assert!(!first_cached);
        assert!(!flagged);
        assert!(second_cached);
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM handles WHERE account_id = $1 AND normalized = '+15555550100'",
        )
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(n, 1);
    }
}
