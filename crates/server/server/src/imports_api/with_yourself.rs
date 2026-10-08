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
//! What a link drops is set aside rather than lost, so removing the identity
//! again gives it back: the participant rows in `participants_set_aside`,
//! and the deleted holder contact's import run records and import run
//! Contact Group memberships under its identities.
//!
//! The statements are in `db::participants`, `db::contacts`,
//! `db::conversations` and `db::handles`; this module only puts them in
//! order. [`before_identity_change`] reads what the change is measured
//! against, and [`follow_identities`] takes that by value once the
//! identities have changed.

use anyhow::Result;
use sqlx::SqliteConnection;

use super::ImportCounts;
use super::contact_name::ensure_contact_for_handle;
use crate::db::{contacts, conversations, handles, participants};

/// What an identity change is measured against: the account's
/// conversations with yourself and its identity handles before the change.
/// Only [`before_identity_change`] makes one.
#[must_use = "pass it to follow_identities once the identities have changed"]
pub(crate) struct WithYourselfBefore {
    account_id: i64,
    conversations: Vec<i64>,
    identity_handles: Vec<i64>,
}

/// Read the account's conversations with yourself and its identity handles,
/// before its identity list changes.
///
/// # Errors
///
/// Returns an error when a query fails.
pub(crate) async fn before_identity_change(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<WithYourselfBefore> {
    Ok(WithYourselfBefore {
        account_id,
        conversations: conversations::with_yourself_ids(conn, account_id).await?,
        identity_handles: handles::identity_handle_ids(conn, account_id).await?,
    })
}

/// Bring the account's conversations into line with its identity list, after
/// a change to it.
///
/// For an identity removed: each participant set aside at it comes back as
/// it was. Each of its handles the conversations hold as a participant, a
/// one-to-one chat handle or a received row's sender gets a contact when it
/// has none, with the import run records and groups set aside for it. Each
/// conversation that was with yourself and is not now gets its chat handle
/// as a participant when it has none, named as its contact is.
///
/// For an identity linked: every participant at one of the account's
/// identities is set aside, in one-to-one and group conversations alike, and
/// each contact an import made for the holder and nobody touched is deleted,
/// unless something else refers to it
/// ([`contacts::delete_untouched_holder_contact`]). Other participants stay.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub(crate) async fn follow_identities(
    conn: &mut SqliteConnection,
    before: WithYourselfBefore,
) -> Result<()> {
    let WithYourselfBefore {
        account_id,
        conversations: with_yourself_before,
        identity_handles,
    } = before;

    participants::restore_set_aside_participants(conn, account_id).await?;
    let identity_handles_now = handles::identity_handle_ids(conn, account_id).await?;
    let mut counts = ImportCounts::default();
    for handle_id in identity_handles
        .into_iter()
        .filter(|id| !identity_handles_now.contains(id))
    {
        // The run records and groups set aside for the handle go to its
        // contact: the one it has, else the one an import would make for
        // it. With neither they are forgotten, so a later link starts over.
        let contact_id = match contacts::contact_id_for_handle(conn, account_id, handle_id).await? {
            Some(contact_id) => Some(contact_id),
            None if handles::is_met_in_conversations(conn, account_id, handle_id).await? => Some(
                ensure_contact_for_handle(conn, account_id, None, handle_id, None, &mut counts)
                    .await?,
            ),
            None => None,
        };
        match contact_id {
            Some(contact_id) => {
                contacts::take_back_set_aside(conn, account_id, handle_id, contact_id).await?;
            }
            None => contacts::forget_set_aside(conn, account_id, handle_id).await?,
        }
    }
    let with_yourself_now = conversations::with_yourself_ids(conn, account_id).await?;
    for conversation_id in with_yourself_before
        .into_iter()
        .filter(|id| !with_yourself_now.contains(id))
    {
        // A conversation the change joined to another is gone: its messages
        // and participants are the other one's now.
        let Some(chat_handle_id) =
            conversations::chat_handle_id(conn, account_id, conversation_id).await?
        else {
            continue;
        };
        participants::add_chat_handle_participant(
            conn,
            account_id,
            conversation_id,
            chat_handle_id,
        )
        .await?;
    }

    let holder_contacts = contacts::holder_contacts_in_conversations(conn, account_id).await?;
    participants::set_aside_identity_participants(conn, account_id).await?;
    for contact_id in holder_contacts {
        contacts::delete_untouched_holder_contact(conn, account_id, contact_id).await?;
    }
    Ok(())
}
