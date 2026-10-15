//! Contact linking and display-name merging during import.

use anyhow::Result;
use chrono::{DateTime, Utc};
use message_ir::trimmed;
use sqlx::SqliteConnection;

use super::pipeline::ImportCounts;
use crate::db::contacts;
use crate::db::import_contacts::{self, ContactReason};
use crate::db::trash;

/// The contact that owns `handle_id`, creating one when nothing owns it yet.
///
/// Every participant an import meets becomes a contact. ADR-0006: a backup is
/// an address book the person already curated, so the name it supplies goes on
/// the contact. It goes on at creation, or later if the contact has no name,
/// whatever made it: an earlier backup, an address book load, or the person. A contact
/// that already has a name is untouched, because the same number arrives
/// spelled differently across backups and the first spelling is as good as
/// the second, and a name the person typed or loaded is theirs.
///
/// A contact in the Trash is the one exception to reuse. ADR-0013: a backup
/// that still holds someone the person set aside is the person saying they
/// still talk to them, so the import makes a fresh contact from the backup,
/// as a first import would, and moves this handle and its siblings on the
/// trashed contact (the same number on another service) to it. It then
/// discards the trashed contact: each handle that contact still held leaves
/// it the one way a handle leaves a contact, so one in a conversation goes to
/// a new contact with no name and is Unknown until the backup, or the person,
/// says who it is.
///
/// Whatever the run does to the contact is recorded against `import_id`
/// (`db::import_contacts`), so the run can say afterwards which contacts it
/// created, named, or gave a handle, and why. A contact it changes is
/// stamped `now`.
pub(super) async fn ensure_contact_for_handle(
    tx: &mut SqliteConnection,
    account_id: i64,
    import_id: Option<i64>,
    handle_id: i64,
    backup_name: Option<&str>,
    counts: &mut ImportCounts,
    now: DateTime<Utc>,
) -> Result<i64> {
    let name = backup_name.and_then(trimmed).unwrap_or("");
    let trashed =
        match ensure_sibling_contact_link(tx, account_id, import_id, handle_id, now).await? {
            Some(existing) if !trash::is_contact_trashed(tx, account_id, existing).await? => {
                // An import names only a nameless contact, whatever made it;
                // `contacts::propose_name` is where that rule and its two
                // siblings live.
                if contacts::propose_name(
                    tx,
                    account_id,
                    existing,
                    name,
                    contacts::Origin::Import,
                    now,
                )
                .await?
                {
                    import_contacts::record(tx, import_id, existing, ContactReason::Named).await?;
                }
                return Ok(existing);
            }
            Some(existing) => Some(existing),
            None => None,
        };
    let contact_id =
        contacts::create_contact(tx, account_id, name, contacts::Origin::Import).await?;
    let goes = contacts::IdentityGoes::To(contact_id, contacts::Origin::Import);
    contacts::move_identity(tx, account_id, handle_id, goes).await?;
    let reason = match trashed {
        Some(trashed) => {
            for sibling in contacts::siblings_on_contact(tx, account_id, handle_id, trashed).await?
            {
                contacts::move_identity(tx, account_id, sibling, goes).await?;
            }
            for unknown in trash::discard_trashed_contact(tx, account_id, trashed).await? {
                import_contacts::record(tx, import_id, unknown, ContactReason::Created).await?;
                counts.contacts_created += 1;
            }
            ContactReason::ReplacedTrashed
        }
        None => ContactReason::Created,
    };
    import_contacts::record(tx, import_id, contact_id, reason).await?;
    counts.contacts_created += 1;
    Ok(contact_id)
}

/// If this handle has no contact but a sibling handle (same normalized value
/// and type, different platform service) is already linked, attach this handle
/// to that contact, stamped `now`, and record against `import_id` that the
/// run gave the contact a handle.
async fn ensure_sibling_contact_link(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: Option<i64>,
    handle_id: i64,
    now: DateTime<Utc>,
) -> Result<Option<i64>> {
    if let Some(existing) = contacts::contact_id_for_handle(conn, account_id, handle_id).await? {
        return Ok(Some(existing));
    }
    let Some(contact_id) =
        contacts::contact_id_of_sibling_handle(conn, account_id, handle_id).await?
    else {
        return Ok(None);
    };
    if contacts::link_handle_to_contact(
        conn,
        account_id,
        handle_id,
        contact_id,
        contacts::Origin::Import,
    )
    .await?
    {
        contacts::touch_contact(conn, account_id, contact_id, now).await?;
        import_contacts::record(conn, import_id, contact_id, ContactReason::IdentityAdded).await?;
    }
    Ok(Some(contact_id))
}

#[cfg(test)]
mod tests;
