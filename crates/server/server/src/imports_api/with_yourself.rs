//! The with-yourself rule, run again when an account's identity list changes.
//!
//! An import decides that a one-to-one conversation is one the holder has
//! with themselves when its chat handle is one of the account's identities
//! (`staging.rs`, `FileStaging::stage`): it makes no contact for the holder.
//! It writes no participant at one of the account's identities in any
//! conversation (`insert_participant`, #1093). The reads ask the same
//! question of the identities the account has now
//! (`db::conversations::is_with_yourself_sql`). Linking or removing an
//! identity changes the answer for conversations already imported, so the
//! identities change runs the rule over them again, in the same transaction
//! (#1662). Messages are not rewritten.
//!
//! The statements are in `db::conversations` and `db::contacts`; this module
//! only puts them in order. [`before_identity_change`] reads what the change
//! is measured against, and [`follow_identities`] takes that by value once
//! the identities have changed.

use anyhow::Result;
use sqlx::SqliteConnection;

use super::ImportCounts;
use super::contact_name::ensure_contact_for_handle;
use crate::db::{contacts, conversations};

/// The account's conversations with yourself before its identity list
/// changes. Only [`before_identity_change`] makes one.
#[must_use = "pass it to follow_identities once the identities have changed"]
pub(crate) struct Before {
    account_id: i64,
    with_yourself: Vec<i64>,
}

/// Read the account's conversations with yourself, before its identity list
/// changes.
///
/// # Errors
///
/// Returns an error when the query fails.
pub(crate) async fn before_identity_change(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Before> {
    Ok(Before {
        account_id,
        with_yourself: conversations::with_yourself_ids(conn, account_id).await?,
    })
}

/// Bring the account's conversations into line with its identity list, after
/// a change to it.
///
/// Every participant at one of the account's identities is dropped, in
/// one-to-one and group conversations alike, and each contact on one that an
/// import made and nobody touched goes with it, unless something else refers
/// to it ([`contacts::delete_untouched_holder_contact`]). Other participants
/// stay.
///
/// Each conversation that was with yourself and is not now gets its chat
/// handle back as a participant, the one participant its rows can give back,
/// named as its contact is. The chat handle and each sender of its received
/// rows that is not one of the account's identities get a contact when they
/// have none.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub(crate) async fn follow_identities(conn: &mut SqliteConnection, before: Before) -> Result<()> {
    let Before {
        account_id,
        with_yourself,
    } = before;
    let holder_contacts = contacts::on_holder_identities_in_conversations(conn, account_id).await?;
    conversations::drop_identity_participants(conn, account_id).await?;
    for contact_id in holder_contacts {
        contacts::delete_untouched_holder_contact(conn, account_id, contact_id).await?;
    }
    let now = conversations::with_yourself_ids(conn, account_id).await?;
    for conversation_id in with_yourself.into_iter().filter(|id| !now.contains(id)) {
        no_longer_with_yourself(conn, account_id, conversation_id).await?;
    }
    Ok(())
}

/// Give a conversation that is no longer with yourself its chat handle as a
/// participant, and a contact to the chat handle and to each sender of its
/// received rows that has none.
async fn no_longer_with_yourself(
    conn: &mut SqliteConnection,
    account_id: i64,
    conversation_id: i64,
) -> Result<()> {
    let (chat_handle_id, senders) =
        conversations::chat_handle_and_received_senders(conn, account_id, conversation_id).await?;
    let mut counts = ImportCounts::default();
    for handle_id in std::iter::once(chat_handle_id).chain(senders) {
        // A contact the holder's identity kept, named or in the Trash, stays
        // as it is: no import is replacing it.
        if contacts::contact_id_for_handle(conn, account_id, handle_id)
            .await?
            .is_none()
        {
            ensure_contact_for_handle(conn, account_id, None, handle_id, None, &mut counts).await?;
        }
    }
    conversations::restore_participant(conn, account_id, conversation_id, chat_handle_id).await
}
