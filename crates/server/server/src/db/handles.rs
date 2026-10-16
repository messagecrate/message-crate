//! Shared handle identity helpers: one key for matching, and one type for an
//! address the source did not type.

use std::collections::HashMap;

use anyhow::{Context, Result};
use message_ir::{IdentityService, IdentityType};
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

    /// The country a phone number written without its `+` code is read in:
    /// `stated`, the country the conversation file names for that identity,
    /// else the run's.
    #[must_use]
    pub fn country_for(
        &self,
        stated: Option<&'static phone::Country>,
    ) -> Option<&'static phone::Country> {
        stated.or(self.country)
    }
}

/// One standard form of a handle for identity matching, per type, with the
/// country of a phone number whose form is certain and a human-readable note
/// when it is not (guarded policy).
///
/// Phone: E.164 when the raw is unambiguous: written with its `+`, or
/// written without it in `country`, the country a run or a person stated.
/// Otherwise the digits as written, with a review note and no `+`:
/// `020 7946 0000` with no country is `02079460000`, never `+02079460000`
/// and never a US number (#1676). Email: lowercased. Username/Other:
/// verbatim (trimmed).
pub fn normalize_handle(
    raw: &str,
    handle_type: IdentityType,
    country: Option<&'static phone::Country>,
) -> phone::TypedKey {
    phone::key_typed_handle(raw, handle_type, country)
}

/// The shape of an address the source did not type: [`phone::Handle::parse`],
/// the one rule for what an address looks like. A blank address is `Other`.
/// No identity is stored with this type without asking the service first:
/// [`handle_type_on`] does both.
pub fn handle_type_of(address: &str) -> IdentityType {
    phone::Handle::parse(address).map_or(IdentityType::Other, |handle| handle.kind())
}

/// The type an address takes on `service`: the service decides which types
/// are valid, and the shape ([`handle_type_of`]) picks among them. Every
/// address an import meets is typed here, and the conversation file states
/// no type (#1933). So is every identity a person adds by hand to a contact
/// or to their own account, so one typed in matches the row an import
/// stored for the same address (#2709).
///
/// WhatsApp carries phone numbers and its own ids (`…@lid`, `…@g.us`,
/// `…@s.whatsapp.net`), which are `Other`, so an address with an `@` on
/// WhatsApp is `Other` however it looks (#1671). Text Message carries phone
/// numbers and email addresses, and an `@` address there is `Email` whatever
/// transport carried it, because one conversation holds both SMS and
/// iMessage and an address has one type on one service (#1958). A phone
/// number is a phone number on every service (#1144). A name that is neither
/// a number nor an address is `Other`.
pub fn handle_type_on(address: &str, service: IdentityService) -> IdentityType {
    service.type_on(handle_type_of(address))
}

/// A new identity refused because its service cannot carry its type
/// ([`IdentityService::carries`]), such as an email address on WhatsApp.
/// WhatsApp reaches a person by phone number, and keeps an internal id for
/// one it knows by no number as `other`, so it carries no email address.
#[derive(Debug, thiserror::Error)]
#[error(
    "{address} is {}, and {} carries no {}",
    type_noun(*handle_type).0,
    service.label(),
    type_noun(*handle_type).1
)]
pub struct ServiceCannotCarry {
    /// The address as the request gave it.
    pub address: String,
    /// The service the identity was asked for on.
    pub service: IdentityService,
    /// The identity's type.
    pub handle_type: IdentityType,
}

/// One identity of the type, and many, as a refusal names them.
fn type_noun(handle_type: IdentityType) -> (&'static str, &'static str) {
    match handle_type {
        IdentityType::Phone => ("a phone number", "phone numbers"),
        IdentityType::Email => ("an email address", "email addresses"),
        IdentityType::Username => ("a username", "usernames"),
        IdentityType::Other => ("an identity of type other", "identities of type other"),
    }
}

/// Refuse a new identity of `handle_type` on `service` when the service
/// does not carry it.
///
/// # Errors
///
/// [`ServiceCannotCarry`] for every pair [`IdentityService::carries`]
/// refuses.
pub fn check_service_carries(
    address: &str,
    service: IdentityService,
    handle_type: IdentityType,
) -> std::result::Result<(), ServiceCannotCarry> {
    if service.carries(handle_type) {
        Ok(())
    } else {
        Err(ServiceCannotCarry {
            address: address.to_string(),
            service,
            handle_type,
        })
    }
}

/// The service an identity is on, as a request names it and the profile
/// reports it: `phone` or `whatsapp`. `phone` is Text Message, which carries
/// SMS, MMS, iMessage and RCS, and reaches an email address through iMessage.
/// The address's shape picks the identity's type among those the service
/// carries, so an address with an `@` is `other` on WhatsApp.
//
// Any other word is refused, naming the two. Read as `phone`, a misspelled
// `whatsap` put an identity on Text Message without a word (#1630), and
// `email` lived on as a second name for `phone` (#1631).
//
// This is the HTTP API's enum; the OpenAPI reference publishes it as
// `IdentityService`. `message_ir::IdentityService` is the conversation file's
// enum of the same two services, whose `parse` reads every word but
// `whatsapp` and `wa` as `phone`; the `From` impls below convert between the
// two. The `Api` prefix keeps the two apart in Rust, because one bare name
// would mean either enum depending on a file's imports
// (docs/architecture/http-api.md, "Code").
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
#[schema(as = IdentityService)]
pub enum ApiIdentityService {
    /// Text Message: SMS, MMS, iMessage and RCS.
    Phone,
    /// WhatsApp.
    Whatsapp,
}

impl From<IdentityService> for ApiIdentityService {
    fn from(service: IdentityService) -> Self {
        match service {
            IdentityService::Phone => Self::Phone,
            IdentityService::Whatsapp => Self::Whatsapp,
        }
    }
}

impl From<ApiIdentityService> for IdentityService {
    fn from(service: ApiIdentityService) -> Self {
        match service {
            ApiIdentityService::Phone => Self::Phone,
            ApiIdentityService::Whatsapp => Self::Whatsapp,
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
    service: IdentityService,
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
    .await
    .context("read the identity the account holds for an address")?;
    Ok(id)
}

/// One `handles` row as a country pick reads it: the address as written,
/// its type, its service, and its key.
#[derive(Debug, Clone)]
pub struct KeyedRow {
    /// The address exactly as the source wrote it.
    pub raw: String,
    /// `phone`, `email`, `username` or `other`.
    pub handle_type: String,
    /// `phone` or `whatsapp`.
    pub service: String,
    /// The key it is matched under (`normalized`).
    pub key: String,
}

/// The account's `handles` row `handle_id`, if it has one.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn keyed_row(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
) -> Result<Option<KeyedRow>> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT raw, handle_type, service, normalized FROM handles
         WHERE account_id = $1 AND id = $2",
    )
    .bind(account_id)
    .bind(handle_id)
    .fetch_optional(&mut *conn)
    .await
    .context("read an identity's address, type, service and key")?;
    Ok(row.map(|(raw, handle_type, service, key)| KeyedRow {
        raw,
        handle_type,
        service,
        key,
    }))
}

/// The account's phone identity on `service` keyed `key`, other than
/// `except`, if there is one.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn phone_holding_key(
    conn: &mut SqliteConnection,
    account_id: i64,
    key: &str,
    service: &str,
    except: i64,
) -> Result<Option<i64>> {
    sqlx::query_scalar(
        "SELECT id FROM handles
         WHERE account_id = $1 AND normalized = $2 AND handle_type = 'phone' AND service = $3
           AND id <> $4",
    )
    .bind(account_id)
    .bind(key)
    .bind(service)
    .bind(except)
    .fetch_optional(&mut *conn)
    .await
    .context("read the phone identity holding a key")
}

/// Give the account's `handles` row `handle_id` the key `key`, a phone
/// number's `+` form, and clear its review note: its country is now known.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn set_plus_form(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
    key: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE handles
         SET normalized = $3, normalized_note = NULL, last_modified = datetime('now')
         WHERE account_id = $1 AND id = $2",
    )
    .bind(account_id)
    .bind(handle_id)
    .bind(key)
    .execute(&mut *conn)
    .await
    .context("give a phone identity its + form")?;
    Ok(())
}

/// Insert or reuse a `handles` row for an address no run or person stated a
/// country for. Returns the id and whether this call newly inserted a
/// flagged (review-note) row.
pub async fn upsert_handle_row(
    conn: &mut SqliteConnection,
    account_id: i64,
    raw: &str,
    handle_type: IdentityType,
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
    handle_type: IdentityType,
    service: Option<&str>,
    country: Option<&'static phone::Country>,
) -> Result<(i64, bool)> {
    let phone::TypedKey {
        key: normalized,
        note,
    } = normalize_handle(raw, handle_type, country);
    let platform = IdentityService::parse(service.unwrap_or(IdentityService::Phone.as_str()));
    let service_str = platform.as_str();
    let inserted = sqlx::query(
        "INSERT INTO handles
           (account_id, raw, normalized, normalized_note, handle_type, service)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(raw)
    .bind(normalized.as_str())
    .bind(note.as_deref())
    .bind(handle_type.as_str())
    .bind(service_str)
    .execute(&mut *conn)
    .await
    .context("insert an identity")?
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
    .await
    .context("read the id of an identity")?;
    Ok((id, inserted > 0 && note.is_some()))
}

/// Same as [`upsert_handle_row`], but skip the two SQL statements when this
/// import already resolved the same identity. Third value is `true` on a
/// cache hit so callers can skip leftover per-row work (sibling contact link).
///
/// A phone number written without its `+` code is read in `stated`, the
/// country the conversation file names for this identity, else in the run's
/// country ([`HandleIdCache::in_country`]).
pub async fn upsert_handle_row_cached(
    conn: &mut SqliteConnection,
    cache: &mut HandleIdCache,
    account_id: i64,
    raw: &str,
    handle_type: IdentityType,
    service: Option<&str>,
    stated: Option<&'static phone::Country>,
) -> Result<(i64, bool, bool)> {
    let country = cache.country_for(stated);
    let normalized = normalize_handle(raw, handle_type, country).key;
    let platform = IdentityService::parse(service.unwrap_or(IdentityService::Phone.as_str()));
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
        upsert_handle_row_in(conn, account_id, raw, handle_type, service, country).await?;
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
    /// One-to-one, group, and orphaned conversations: for a contact, those the identity
    /// takes part in, whoever wrote in them; for an account, those holding
    /// at least one of the identity's messages. Trashed conversations are
    /// excluded.
    pub conversations: u64,
    /// The identity's messages in one-to-one conversations, trashed
    /// conversations and duplicates excluded.
    pub one_to_one_messages: u64,
    /// The identity's messages in group conversations, on the same terms.
    pub group_messages: u64,
    /// The identity's messages in conversations of orphaned messages, on the
    /// same terms: the three message counts add up to all of its messages.
    pub orphaned_messages: u64,
}

/// Whether the `handles` row `h` is a phone number whose country is unknown,
/// as a SQL condition: stored as the digits typed, with no `+`, and long
/// enough to have a `+` form once a country is picked
/// ([`phone::MIN_NATIONAL_DIGITS`]).
#[must_use]
pub fn country_unknown_sql() -> String {
    format!(
        "(h.handle_type = 'phone' AND h.normalized NOT LIKE '+%' \
         AND length(h.normalized) >= {})",
        phone::MIN_NATIONAL_DIGITS
    )
}

/// Whose identities [`identities`] reads.
#[derive(Debug, Clone, Copy)]
pub enum IdentitiesOf {
    /// The identities the account holds as its own.
    Account(i64),
    /// The identities linked to one of the account's contacts.
    Contact { account_id: i64, contact_id: i64 },
}

/// One row of [`identities`]: address, service, whether its country is
/// unknown, first and last timestamp, conversation count, one-to-one, group, and
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
    let not_trashed = crate::db::trash::not_trashed_conversation("c");
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
    let country_unknown = country_unknown_sql();
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
                (SELECT COUNT(*) {messages} AND c.conversation_type = 'one-to-one'),
                (SELECT COUNT(*) {messages} AND c.conversation_type = 'group'),
                (SELECT COUNT(*) {messages} AND c.conversation_type = 'orphaned')
         FROM linked l
         JOIN handles h ON h.id = l.handle_id
         {scope}
         ORDER BY CASE WHEN h.handle_type = 'email' THEN 1 ELSE 0 END, h.normalized",
    );
    let mut query = sqlx::query_as::<_, IdentityRow>(sqlx::AssertSqlSafe(sql)).bind(account_id);
    if let IdentitiesOf::Contact { contact_id, .. } = of {
        query = query.bind(contact_id);
    }
    let rows = query
        .fetch_all(&mut *conn)
        .await
        .context("read the identities and their message counts")?;
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
                one_to_one,
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
                    one_to_one_messages: one_to_one.max(0) as u64,
                    group_messages: group.max(0) as u64,
                    orphaned_messages: orphaned.max(0) as u64,
                }
            },
        )
        .collect())
}

/// The ids of the account's handles that are at one of its identities, on
/// any service ([`crate::db::account_profile::is_account_identity_sql`]).
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn identity_handle_ids(conn: &mut SqliteConnection, account_id: i64) -> Result<Vec<i64>> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT h.id FROM handles h WHERE h.account_id = $1 AND {} ORDER BY h.id",
        crate::db::account_profile::is_account_identity_sql("h", "$1"),
    )))
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .context("read the account's own identities on every service")
}

/// True when an import would give `handle_id` a contact for what the
/// account's conversations hold: it is a participant, the chat handle of a
/// one-to-one conversation, or the sender of a received row.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn is_met_in_conversations(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM participants p
                        JOIN conversations c ON c.id = p.conversation_id
                        WHERE c.account_id = $1 AND p.handle_id = $2)
             OR EXISTS (SELECT 1 FROM conversations c
                        WHERE c.account_id = $1 AND c.chat_handle_id = $2
                          AND c.conversation_type = 'one-to-one' COLLATE NOCASE)
             OR EXISTS (SELECT 1 FROM messages m
                        WHERE m.account_id = $1 AND m.sender_handle_id = $2
                          AND m.is_from_me = 0)",
    )
    .bind(account_id)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .context("check whether an identity is met in the account's conversations")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    const TEST_ACCOUNT: i64 = 7;

    /// A new identity is refused for every pair its service does not carry
    /// ([`IdentityService::carries`]), with a reason naming the service and
    /// the type, and let through for every pair it carries. A username on
    /// Text Message was let through, because only an email address on
    /// WhatsApp was refused (#2212). The two loops list every
    /// `IdentityService` and `IdentityType`, and must follow the enums.
    #[test]
    fn a_new_identity_is_refused_for_every_type_its_service_does_not_carry() {
        let refusals = [
            (
                IdentityService::Phone,
                IdentityType::Username,
                "x is a username, and Text Message carries no usernames",
            ),
            (
                IdentityService::Whatsapp,
                IdentityType::Email,
                "x is an email address, and WhatsApp carries no email addresses",
            ),
            (
                IdentityService::Whatsapp,
                IdentityType::Username,
                "x is a username, and WhatsApp carries no usernames",
            ),
        ];
        let mut refused = 0;
        for service in [IdentityService::Phone, IdentityService::Whatsapp] {
            for handle_type in [
                IdentityType::Phone,
                IdentityType::Email,
                IdentityType::Username,
                IdentityType::Other,
            ] {
                let answer =
                    check_service_carries("x", service, handle_type).map_err(|e| e.to_string());
                if service.carries(handle_type) {
                    assert_eq!(answer, Ok(()), "{service:?} {handle_type:?}");
                } else {
                    let (_, _, reason) = refusals
                        .iter()
                        .find(|(s, t, _)| *s == service && *t == handle_type)
                        .unwrap_or_else(|| panic!("no reason for {service:?} {handle_type:?}"));
                    assert_eq!(answer, Err((*reason).to_string()));
                    refused += 1;
                }
            }
        }
        assert_eq!(
            refused,
            refusals.len(),
            "every listed reason is a refused pair"
        );
    }

    /// The count of numbers that need a look takes one for each identity the
    /// call both created and flagged, so a plain new number and a flagged
    /// number met again add nothing.
    #[tokio::test]
    async fn only_the_first_insert_of_a_flagged_handle_is_reported_as_flagged() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_schema(&mut conn).await.unwrap();
        crate::db::account_profile::create_account_row_if_missing(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap();

        let (_, plain) = upsert_handle_row(
            &mut conn,
            TEST_ACCOUNT,
            "+15555550100",
            IdentityType::Phone,
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
            IdentityType::Phone,
            Some("phone"),
        )
        .await
        .unwrap();
        assert!(first, "a new number with a review note is flagged");

        let (second_id, second) = upsert_handle_row(
            &mut conn,
            TEST_ACCOUNT,
            ambiguous,
            IdentityType::Phone,
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
        crate::db::account_profile::create_account_row_if_missing(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap();
        let mut cache = HandleIdCache::default();
        let (first, _, first_cached) = upsert_handle_row_cached(
            &mut conn,
            &mut cache,
            TEST_ACCOUNT,
            "+15555550100",
            IdentityType::Phone,
            Some("phone"),
            None,
        )
        .await
        .unwrap();
        let (second, flagged, second_cached) = upsert_handle_row_cached(
            &mut conn,
            &mut cache,
            TEST_ACCOUNT,
            "+15555550100",
            IdentityType::Phone,
            Some("phone"),
            None,
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
