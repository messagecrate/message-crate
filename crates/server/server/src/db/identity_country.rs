//! Picking the country of a phone number written without its `+` code: the
//! number takes its `+` form, and when an identity already holds that form
//! the two become one (#1676).
//!
//! A number whose country nobody stated is stored as the digits typed, with
//! no region, and matches no `+` number (`docs/architecture/
//! contacts-identities-and-messages.md`, "A phone number carries its
//! country"). Once a person picks the country, the number is the `+` form
//! that country gives it. The identity is rewritten in place when no other
//! identity holds that form. When one does, everything that names the
//! number written without its `+` (conversations, participants, messages,
//! reactions, the account's own identities and the contact link) moves to
//! the identity that holds the `+` form, and the first identity goes.

use anyhow::Result;
use sqlx::SqliteConnection;

use crate::db::contacts::{self, IdentityGoes, Origin};

/// Why a country cannot be picked for an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountryRefusal {
    /// The identity is not a phone number, or its country is already known:
    /// it was written with its `+` code, or a country was stated for it.
    CountryKnown,
    /// The number has no `+` form in that country, such as a short code or
    /// a number with the wrong count of digits there. The reason is
    /// `phone`'s, written for the person.
    NoPlusForm(String),
}

/// The `+` form a country gives one identity, and the identity that already
/// holds it, if any. [`plus_form`] makes it and [`give_country`] applies it.
#[derive(Debug, Clone)]
pub struct PlusForm {
    /// The identity written without its `+` code.
    pub handle_id: i64,
    /// The number in its `+` form, such as `+447700900123`.
    pub key: String,
    /// The country calling code of `key`, without the `+`.
    pub region: String,
    /// The identity on the same service that already holds `key`.
    pub existing: Option<i64>,
}

/// The `+` form `country` gives the account's identity `handle_id`.
///
/// # Errors
///
/// Returns an error when a statement fails. The inner `Err` is a refusal
/// the person can act on.
pub async fn plus_form(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
    country: &'static phone::Country,
) -> Result<Result<PlusForm, CountryRefusal>> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT raw, handle_type, service, region FROM handles WHERE account_id = $1 AND id = $2",
    )
    .bind(account_id)
    .bind(handle_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((raw, handle_type, service, region)) = row else {
        return Ok(Err(CountryRefusal::CountryKnown));
    };
    if handle_type != message_ir::HandleType::Phone.as_str() || !region.is_empty() {
        return Ok(Err(CountryRefusal::CountryKnown));
    }
    let typed = phone::key_typed_handle(&raw, message_ir::HandleType::Phone, Some(country));
    if !typed.key.starts_with('+') {
        let reason = typed
            .note
            .unwrap_or_else(|| format!("{raw} has no + form in {}", country.code));
        return Ok(Err(CountryRefusal::NoPlusForm(reason)));
    }
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM handles
         WHERE account_id = $1 AND normalized = $2 AND handle_type = 'phone' AND service = $3
           AND id <> $4",
    )
    .bind(account_id)
    .bind(&typed.key)
    .bind(&service)
    .bind(handle_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(Ok(PlusForm {
        handle_id,
        key: typed.key,
        region: typed.region,
        existing,
    }))
}

/// Who holds an identity, for the question asked before a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// The contact it is on, by id, with its name; empty for a contact with
    /// no name.
    Contact(i64, String),
    /// The account holder: it is one of the account's own identities.
    Account,
    /// Nothing holds it.
    Nobody,
}

/// Who holds the identity `handle_id`.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn holder(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
) -> Result<Holder> {
    if let Some(contact_id) = contacts::contact_id_for_handle(conn, account_id, handle_id).await? {
        let name: String = sqlx::query_scalar(
            "SELECT preferred_name FROM contacts WHERE account_id = $1 AND id = $2",
        )
        .bind(account_id)
        .bind(contact_id)
        .fetch_one(&mut *conn)
        .await?;
        return Ok(Holder::Contact(contact_id, name));
    }
    let own: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM account_handles WHERE account_id = $1 AND handle_id = $2)",
    )
    .bind(account_id)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(if own { Holder::Account } else { Holder::Nobody })
}

/// Give the identity its `+` form. With no identity holding that form, the
/// row is rewritten in place. Otherwise the identity merges into the one
/// that does: see [`merge_into`]. Answers the messages whose conversation or
/// sender changed and that a dedupe had keyed, whose duplicate flags the
/// caller must put right.
///
/// `edited_contact` is the contact whose screen picked the country, when it
/// was a contact's: the merged identity goes on it, from whatever contact
/// held the `+` form, because the person said on this contact's screen that
/// the two are this person's one number. A contact with no name left with no
/// identity goes.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn give_country(
    conn: &mut SqliteConnection,
    account_id: i64,
    form: &PlusForm,
    edited_contact: Option<i64>,
) -> Result<Vec<i64>> {
    let Some(into) = form.existing else {
        sqlx::query(
            "UPDATE handles
             SET normalized = $3, region = $4, normalized_note = NULL,
                 last_modified = datetime('now')
             WHERE account_id = $1 AND id = $2",
        )
        .bind(account_id)
        .bind(form.handle_id)
        .bind(&form.key)
        .bind(&form.region)
        .execute(&mut *conn)
        .await?;
        return Ok(Vec::new());
    };
    merge_into(conn, account_id, form.handle_id, into, edited_contact).await
}

/// Move everything that names identity `from` to identity `into`, then
/// delete `from`. Both are one number on one service.
///
/// A one-to-one conversation with `from` joins the one with `into`: its
/// messages and participants move there and it goes, because an identity has
/// at most one such conversation (`UNIQUE (account_id, chat_handle_id)`). A
/// group that lists both keeps one seat. The contact link follows
/// `edited_contact` as [`give_country`] says.
async fn merge_into(
    conn: &mut SqliteConnection,
    account_id: i64,
    from: i64,
    into: i64,
    edited_contact: Option<i64>,
) -> Result<Vec<i64>> {
    // The messages whose keys change: those that will sit in another
    // conversation or name another sender. Only those a dedupe keyed matter.
    let changed: Vec<i64> = sqlx::query_scalar(
        "SELECT m.id FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         WHERE m.account_id = $1 AND m.content_key IS NOT NULL
           AND (c.chat_handle_id = $2 OR m.sender_handle_id = $2)",
    )
    .bind(account_id)
    .bind(from)
    .fetch_all(&mut *conn)
    .await?;

    merge_conversation(conn, account_id, from, into).await?;

    // A conversation that lists both keeps the seat `into` has.
    sqlx::query(
        "DELETE FROM participants
         WHERE handle_id = $1
           AND conversation_id IN (SELECT conversation_id FROM participants WHERE handle_id = $2)",
    )
    .bind(from)
    .bind(into)
    .execute(&mut *conn)
    .await?;
    for statement in [
        "UPDATE participants SET handle_id = $2 WHERE handle_id = $1",
        "UPDATE messages SET sender_handle_id = $2 WHERE sender_handle_id = $1",
        "UPDATE messages SET owner_handle_id = $2 WHERE owner_handle_id = $1",
        "UPDATE tapbacks SET sender_handle_id = $2 WHERE sender_handle_id = $1",
    ] {
        sqlx::query(statement)
            .bind(from)
            .bind(into)
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query(
        "INSERT INTO account_handles (account_id, handle_id)
         SELECT account_id, $3 FROM account_handles WHERE account_id = $1 AND handle_id = $2
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(from)
    .bind(into)
    .execute(&mut *conn)
    .await?;

    let from_contact = contacts::contact_id_for_handle(conn, account_id, from).await?;
    // The cascade takes `from`'s contact and account links with it.
    sqlx::query("DELETE FROM handles WHERE account_id = $1 AND id = $2")
        .bind(account_id)
        .bind(from)
        .execute(&mut *conn)
        .await?;
    let into_contact = contacts::contact_id_for_handle(conn, account_id, into).await?;
    let keep_on = edited_contact.or(from_contact);
    if let Some(contact_id) = keep_on {
        if into_contact != Some(contact_id) {
            let goes = IdentityGoes::To(contact_id, Origin::User);
            contacts::move_identity(conn, account_id, into, goes).await?;
            if let Some(held) = into_contact
                && !contacts::delete_if_empty(conn, account_id, held).await?
            {
                contacts::touch_contact(conn, account_id, held).await?;
            }
        }
        contacts::touch_contact(conn, account_id, contact_id).await?;
    }
    if let Some(contact_id) = from_contact.filter(|&c| Some(c) != keep_on) {
        contacts::delete_if_empty(conn, account_id, contact_id).await?;
    }
    Ok(changed)
}

/// Join the one-to-one conversation keyed by `from` to the one keyed by
/// `into`, or re-key it when there is none.
async fn merge_conversation(
    conn: &mut SqliteConnection,
    account_id: i64,
    from: i64,
    into: i64,
) -> Result<()> {
    let keyed = |handle_id: i64| {
        sqlx::query_scalar::<_, i64>(
            "SELECT id FROM conversations WHERE account_id = $1 AND chat_handle_id = $2",
        )
        .bind(account_id)
        .bind(handle_id)
    };
    let Some(source) = keyed(from).fetch_optional(&mut *conn).await? else {
        return Ok(());
    };
    let Some(target) = keyed(into).fetch_optional(&mut *conn).await? else {
        sqlx::query("UPDATE conversations SET chat_handle_id = $2 WHERE id = $1")
            .bind(source)
            .bind(into)
            .execute(&mut *conn)
            .await?;
        return Ok(());
    };
    for statement in [
        "UPDATE messages SET conversation_id = $2 WHERE conversation_id = $1",
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         SELECT $2, handle_id, name_alias FROM participants WHERE conversation_id = $1
         ON CONFLICT DO NOTHING",
        "INSERT INTO message_tag_members (conversation_id, tag_id)
         SELECT $2, tag_id FROM message_tag_members WHERE conversation_id = $1
         ON CONFLICT DO NOTHING",
    ] {
        sqlx::query(statement)
            .bind(source)
            .bind(target)
            .execute(&mut *conn)
            .await?;
    }
    // The conversation that stays keeps its own trash state.
    for statement in [
        "DELETE FROM trashed_conversations WHERE conversation_id = $1",
        "DELETE FROM conversations WHERE id = $1",
    ] {
        sqlx::query(statement)
            .bind(source)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// The id of the account's own identity shown as `address` on `service`,
/// or on the phone service first and then WhatsApp when `service` is `None`.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn account_identity_id(
    conn: &mut SqliteConnection,
    account_id: i64,
    address: &str,
    service: Option<message_ir::HandleService>,
) -> Result<Option<i64>> {
    let id = sqlx::query_scalar(
        "SELECT h.id FROM account_handles ah
         JOIN handles h ON h.id = ah.handle_id
         WHERE ah.account_id = $1 AND (h.raw = $2 OR h.normalized = $2)
           AND ($3 IS NULL OR h.service = $3)
         ORDER BY CASE h.service WHEN 'phone' THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(account_id)
    .bind(address.trim())
    .bind(service.map(message_ir::HandleService::as_str))
    .fetch_optional(&mut *conn)
    .await?;
    Ok(id)
}
