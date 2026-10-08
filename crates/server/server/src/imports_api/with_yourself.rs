//! The with-yourself rule, run again when an account's identity list changes.
//!
//! An import decides that a one-to-one conversation is one the holder has
//! with themselves when its chat handle is one of the account's identities
//! (`staging.rs`, `FileStaging::stage`): it writes no participant and makes
//! no contact for the holder. The reads ask the same question of the
//! identities the account has now (`db::conversations::is_with_yourself_sql`).
//! Linking or removing an identity changes the answer for conversations
//! already imported, so the identities change runs the rule over them again,
//! in the same transaction, and the stored participants and contacts agree
//! with what the reads show (#1662). Messages are not rewritten.

use anyhow::Result;
use sqlx::SqliteConnection;

use super::ImportCounts;
use super::contact_name::ensure_contact_for_handle;
use crate::db::account_profile::is_account_identity_sql;
use crate::db::conversations::is_with_yourself_sql;

/// The ids of the account's conversations with yourself, by the identities
/// it has now.
///
/// # Errors
///
/// Returns an error when the query fails.
pub(crate) async fn conversations_with_yourself(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT c.id FROM conversations c
         WHERE c.account_id = $1 AND {}
         ORDER BY c.id",
        is_with_yourself_sql("c")
    ))
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?)
}

/// Bring the account's conversations into line with its identity list, after
/// a change to it. `before` is [`conversations_with_yourself`] as it was
/// before the change.
///
/// Each conversation that is with yourself now drops its participants and
/// the contact an import made for the holder, as an import would never have
/// written them. Each that was with yourself before and is not now gets its
/// participant back from its chat handle, and the chat handle and the
/// senders of its received rows get a contact, as an import writes them.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub(crate) async fn follow_identities(
    conn: &mut SqliteConnection,
    account_id: i64,
    before: &[i64],
) -> Result<()> {
    let now = conversations_with_yourself(conn, account_id).await?;
    for &conversation_id in &now {
        now_with_yourself(conn, account_id, conversation_id).await?;
    }
    for &conversation_id in before.iter().filter(|id| !now.contains(id)) {
        no_longer_with_yourself(conn, account_id, conversation_id).await?;
    }
    Ok(())
}

/// Drop the participants of a conversation with yourself, and delete each
/// contact an import made for the holder there that nothing else refers to.
/// The holder's handles are the chat handle, its participants and the
/// senders of its received rows that are the account's identities.
async fn now_with_yourself(
    conn: &mut SqliteConnection,
    account_id: i64,
    conversation_id: i64,
) -> Result<()> {
    let holder_contacts: Vec<i64> = sqlx::query_scalar(&format!(
        "SELECT DISTINCT ch.contact_id
         FROM contact_handles ch
         JOIN contacts ct ON ct.id = ch.contact_id AND ct.account_id = ch.account_id
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND ct.origin = 'import'
           AND {holder}
           AND (h.id = (SELECT chat_handle_id FROM conversations WHERE id = $2)
                OR h.id IN (SELECT handle_id FROM participants WHERE conversation_id = $2)
                OR h.id IN (SELECT sender_handle_id FROM messages
                            WHERE conversation_id = $2 AND is_from_me = 0))
         ORDER BY ch.contact_id",
        holder = is_account_identity_sql("h", "$1"),
    ))
    .bind(account_id)
    .bind(conversation_id)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM participants WHERE conversation_id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await?;
    for contact_id in holder_contacts {
        delete_unless_referenced(conn, account_id, contact_id).await?;
    }
    Ok(())
}

/// Delete the holder's `contact_id` unless something outside the
/// conversations with yourself refers to it: an identity on it that is not
/// one of the account's, an identity on it that is a participant, a sender
/// or a reactor, or a one-to-one chat handle, in any other conversation or
/// in an import being staged, or a Contact Group the person made.
async fn delete_unless_referenced(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<()> {
    let referenced: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS (
           SELECT 1 FROM contact_handles ch
           JOIN handles h ON h.id = ch.handle_id
           WHERE ch.account_id = $1 AND ch.contact_id = $2
             AND (NOT {holder}
                  OR EXISTS (SELECT 1 FROM participants WHERE handle_id = h.id)
                  OR EXISTS (SELECT 1 FROM staging_participants WHERE handle_id = h.id)
                  OR EXISTS (SELECT 1 FROM staging_messages
                             WHERE account_id = $1 AND sender_handle_id = h.id)
                  OR EXISTS (SELECT 1 FROM staging_tapbacks WHERE sender_handle_id = h.id)
                  OR EXISTS (SELECT 1 FROM staging_conversations
                             WHERE account_id = $1 AND chat_handle_id = h.id)
                  OR EXISTS (SELECT 1 FROM messages m
                             JOIN conversations c ON c.id = m.conversation_id
                             WHERE m.account_id = $1 AND m.sender_handle_id = h.id
                               AND NOT {with_yourself})
                  OR EXISTS (SELECT 1 FROM tapbacks t
                             JOIN messages m ON m.id = t.message_id
                             JOIN conversations c ON c.id = m.conversation_id
                             WHERE t.sender_handle_id = h.id AND NOT {with_yourself})
                  OR EXISTS (SELECT 1 FROM conversations c
                             WHERE c.account_id = $1 AND c.chat_handle_id = h.id
                               AND c.conversation_type = 'individual' COLLATE NOCASE
                               AND NOT {with_yourself})))
         OR EXISTS (SELECT 1 FROM contact_group_members gm
                    JOIN contact_groups g ON g.id = gm.group_id
                    WHERE gm.contact_id = $2 AND g.kind = 'manual')",
        holder = is_account_identity_sql("h", "$1"),
        with_yourself = is_with_yourself_sql("c"),
    ))
    .bind(account_id)
    .bind(contact_id)
    .fetch_one(&mut *conn)
    .await?;
    if referenced {
        return Ok(());
    }
    // The identities themselves stay, on no contact, as the holder's
    // always are.
    crate::db::contacts::delete_contact(conn, account_id, contact_id).await
}

/// Give a conversation that is no longer with yourself what an import of it
/// would have written: its chat handle as its participant, when it has none,
/// and a contact for the chat handle and for each sender of its received
/// rows that is not one of the account's identities.
async fn no_longer_with_yourself(
    conn: &mut SqliteConnection,
    account_id: i64,
    conversation_id: i64,
) -> Result<()> {
    let chat_handle_id: i64 =
        sqlx::query_scalar("SELECT chat_handle_id FROM conversations WHERE id = $1")
            .bind(conversation_id)
            .fetch_one(&mut *conn)
            .await?;
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id)
         SELECT $1, $2
         WHERE NOT EXISTS (SELECT 1 FROM participants WHERE conversation_id = $1)",
    )
    .bind(conversation_id)
    .bind(chat_handle_id)
    .execute(&mut *conn)
    .await?;
    let senders: Vec<i64> = sqlx::query_scalar(&format!(
        "SELECT DISTINCT h.id FROM messages m
         JOIN handles h ON h.id = m.sender_handle_id
         WHERE m.conversation_id = $2 AND m.is_from_me = 0 AND h.id <> $3
           AND NOT {holder}
         ORDER BY h.id",
        holder = is_account_identity_sql("h", "$1"),
    ))
    .bind(account_id)
    .bind(conversation_id)
    .bind(chat_handle_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut counts = ImportCounts::default();
    for handle_id in std::iter::once(chat_handle_id).chain(senders) {
        ensure_contact_for_handle(conn, account_id, None, handle_id, None, &mut counts).await?;
    }
    Ok(())
}
