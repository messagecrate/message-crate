//! Picking the country of a phone number written without its `+` code: the
//! number takes its `+` form, and when an identity already holds that form
//! the two become one (#1676).
//!
//! A number whose country nobody stated is stored as the digits typed, with
//! no `+`, and matches no `+` number (`docs/architecture/
//! contacts-identities-and-messages.md`, "A phone number carries its
//! country"). Once a person picks the country, the number is the `+` form
//! that country gives it. The identity is rewritten in place when no other
//! identity holds that form. When one does, everything that names the
//! number written without its `+` (conversations, participants, messages,
//! reactions, the account's own identities and the contact link) moves to
//! the identity that holds the `+` form, and the first identity goes.

use anyhow::Result;
use sqlx::SqliteConnection;

use message_crate_api_types::IdentityHolder;

use crate::db::contacts::{self, IdentityGoes, Origin};
use crate::db::dedupe::HAS_CONTENT_KEY_SQL;
use crate::db::{account_profile, handles};

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
    let Some(row) = handles::keyed_row(conn, account_id, handle_id).await? else {
        return Ok(Err(CountryRefusal::CountryKnown));
    };
    // A key that starts with `+` names its country already.
    if row.handle_type != message_ir::IdentityType::Phone.as_str() || row.key.starts_with('+') {
        return Ok(Err(CountryRefusal::CountryKnown));
    }
    let typed = phone::key_typed_handle(&row.raw, message_ir::IdentityType::Phone, Some(country));
    if !typed.key.starts_with('+') {
        let reason = typed
            .note
            .unwrap_or_else(|| format!("{} has no + form in {}", row.raw, country.code));
        return Ok(Err(CountryRefusal::NoPlusForm(reason)));
    }
    let existing =
        handles::phone_holding_key(conn, account_id, &typed.key, &row.service, handle_id).await?;
    Ok(Ok(PlusForm {
        handle_id,
        key: typed.key,
        existing,
    }))
}

/// Who holds the identity `handle_id`, for the question asked before a
/// merge: the contact it is on, the account when it is one of the account's
/// own identities, or nobody.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn holder(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
) -> Result<IdentityHolder> {
    if let Some(contact_id) = contacts::contact_id_for_handle(conn, account_id, handle_id).await? {
        let name = contacts::preferred_name(conn, account_id, contact_id).await?;
        return Ok(IdentityHolder::Contact { contact_id, name });
    }
    Ok(
        if account_profile::is_account_identity(conn, account_id, handle_id).await? {
            IdentityHolder::Account
        } else {
            IdentityHolder::Nobody
        },
    )
}

/// The messages whose content keys name the identity `handle_id`, among
/// those a dedupe has keyed: every message of a conversation it is the chat
/// handle of, a member of, or a sender in. A content key is made from the
/// chat handle's key, the sender's and a group's members' (`dedupe.rs`,
/// `db::dedupe::content_key_inputs`), so when the identity's key or the identity
/// itself changes, every one of these needs its key again.
async fn messages_keyed_by(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT m.id FROM messages m
         WHERE m.account_id = $1 AND {HAS_CONTENT_KEY_SQL}
           AND m.conversation_id IN (
             SELECT id FROM conversations WHERE account_id = $1 AND chat_handle_id = $2
             UNION SELECT conversation_id FROM participants WHERE handle_id = $2
             UNION SELECT conversation_id FROM messages
                   WHERE account_id = $1 AND sender_handle_id = $2)
         ORDER BY m.id",
    ))
    .bind(account_id)
    .bind(handle_id)
    .fetch_all(&mut *conn)
    .await?)
}

/// Give the identity its `+` form. With no identity holding that form, the
/// row is rewritten in place. Otherwise the identity merges into the one
/// that does: see [`merge_into`]. Answers the messages a dedupe had keyed
/// whose keys named the number, whose duplicate flags the caller must put
/// right: every message of a conversation the number is the chat handle
/// of, a member of, or a sender in ([`messages_keyed_by`]). A merge adds
/// the same for the identity holding the `+` form, because its keys change
/// too when it becomes one of the account's own: its one-to-one becomes a
/// conversation with yourself, and a group leaves it out of its key.
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
    let mut changed = messages_keyed_by(conn, account_id, form.handle_id).await?;
    match form.existing {
        None => handles::set_plus_form(conn, account_id, form.handle_id, &form.key).await?,
        Some(into) => {
            changed.extend(messages_keyed_by(conn, account_id, into).await?);
            changed.sort_unstable();
            changed.dedup();
            merge_into(conn, account_id, form.handle_id, into, edited_contact).await?;
        }
    }
    Ok(changed)
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
) -> Result<()> {
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
    // What the with-yourself rule set aside for `from` is kept for `into`,
    // so it comes back for the one identity (#1662).
    for statement in [
        "INSERT INTO participants_set_aside (conversation_id, handle_id, name_alias)
         SELECT conversation_id, $2, name_alias FROM participants_set_aside WHERE handle_id = $1
         ON CONFLICT DO NOTHING",
        "INSERT INTO contact_group_members_set_aside (group_id, handle_id)
         SELECT group_id, $2 FROM contact_group_members_set_aside WHERE handle_id = $1
         ON CONFLICT DO NOTHING",
        "INSERT INTO import_contacts_set_aside (import_id, handle_id, reason)
         SELECT import_id, $2, reason FROM import_contacts_set_aside WHERE handle_id = $1
         ON CONFLICT DO NOTHING",
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
    Ok(())
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
        "INSERT INTO participants_set_aside (conversation_id, handle_id, name_alias)
         SELECT $2, handle_id, name_alias FROM participants_set_aside WHERE conversation_id = $1
         ON CONFLICT DO NOTHING",
    ] {
        sqlx::query(statement)
            .bind(source)
            .bind(target)
            .execute(&mut *conn)
            .await?;
    }
    // The merged conversation is in the Trash only when both were: a live
    // conversation brings the other out with it, because emptying the Trash
    // would otherwise delete its messages for good.
    let source_trashed: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM trashed_conversations WHERE conversation_id = $1)",
    )
    .bind(source)
    .fetch_one(&mut *conn)
    .await?;
    if !source_trashed {
        sqlx::query("DELETE FROM trashed_conversations WHERE conversation_id = $1")
            .bind(target)
            .execute(&mut *conn)
            .await?;
    }
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
