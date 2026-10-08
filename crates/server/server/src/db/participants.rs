//! The participants an identity change moves: the `participants` rows at
//! one of the account's identities, which `participants_set_aside` keeps
//! while the identity is linked (#1662, `imports_api::with_yourself`).
//! Reading a conversation's participants is in `db::participant_names`, and
//! an import writes them through `db::staging`.

use anyhow::Result;
use sqlx::SqliteConnection;

use crate::db::account_profile::is_account_identity_sql;

/// Move every participant of the account's conversations, one-to-one and
/// group alike, that is at one of the account's identities into
/// `participants_set_aside`, with its `name_alias`. An import never writes
/// one (`imports_api::staging`, `insert_participant`, #1093); other
/// participants stay.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn set_aside_identity_participants(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<()> {
    let at_identity = format!(
        "conversation_id IN (SELECT id FROM conversations WHERE account_id = $1)
         AND handle_id IN (SELECT h.id FROM handles h
                           WHERE h.account_id = $1 AND {})",
        is_account_identity_sql("h", "$1"),
    );
    sqlx::query(&format!(
        "INSERT INTO participants_set_aside (conversation_id, handle_id, name_alias)
         SELECT conversation_id, handle_id, name_alias FROM participants
         WHERE {at_identity}
         ON CONFLICT (conversation_id, handle_id) DO UPDATE SET name_alias = excluded.name_alias"
    ))
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(&format!("DELETE FROM participants WHERE {at_identity}"))
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Move each participant set aside in the account's conversations whose
/// identity is no longer one of the account's back into `participants`, as
/// it was. A participant an import wrote for the same seat since stays as
/// the import wrote it.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn restore_set_aside_participants(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<()> {
    let released = format!(
        "conversation_id IN (SELECT id FROM conversations WHERE account_id = $1)
         AND handle_id IN (SELECT h.id FROM handles h
                           WHERE h.account_id = $1 AND NOT {})",
        is_account_identity_sql("h", "$1"),
    );
    sqlx::query(&format!(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         SELECT conversation_id, handle_id, name_alias FROM participants_set_aside
         WHERE {released}
         ON CONFLICT (conversation_id, handle_id) DO NOTHING"
    ))
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(&format!(
        "DELETE FROM participants_set_aside WHERE {released}"
    ))
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Make `handle_id` a participant of `conversation_id`, unless it is one
/// already. Its `name_alias` is the name of the contact on the handle, when
/// that contact has one: a conversation imported while the handle was an
/// identity has no participant row to give back, and the contact's name is
/// where an import puts the backup's.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn add_chat_handle_participant(
    conn: &mut SqliteConnection,
    account_id: i64,
    conversation_id: i64,
    handle_id: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         SELECT $2, $3, NULLIF(
           (SELECT ct.preferred_name FROM contact_handles ch
            JOIN contacts ct ON ct.id = ch.contact_id AND ct.account_id = ch.account_id
            WHERE ch.account_id = $1 AND ch.handle_id = $3), '')
         WHERE true
         ON CONFLICT (conversation_id, handle_id) DO NOTHING",
    )
    .bind(account_id)
    .bind(conversation_id)
    .bind(handle_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}
