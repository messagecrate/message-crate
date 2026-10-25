//! The trash marker tables: `trashed_conversations` and `trashed_contacts`.
//!
//! A row in either table is a soft-delete flag; the conversation or contact
//! it points at is untouched until [`delete_trashed`] or [`empty_trash`]
//! removes it for good (or [`purge_account`] and account deletion clear the
//! markers along with everything else). Both marker operations first check
//! explicitly, through [`crate::db::ownership`], that the target row belongs
//! to `account_id`, rather than inferring ownership from whether an
//! insert or delete affected a row. That is what lets [`restore`] tell "not
//! this account's id" (`false`) apart from "this account's id, and it was not
//! trashed" (`true`, a no-op): a `DELETE` that matches zero rows looks
//! identical in both cases.
//!
//! Trashing something already trashed, and restoring something not trashed,
//! both return `true`. These operations are idempotent, per the HTTP routes
//! built on top of them.
//!
//! A conversation and a contact are trashed the same way, so one pair of
//! functions covers both and [`Trashable`] carries which is meant. The marker
//! table and its id column come from that enum and nowhere else, so no part
//! of a request reaches the SQL text.
//!
//! # Permanent deletion
//!
//! Trash is the only door to permanent deletion (CONTEXT.md, "Trash").
//! [`delete_trashed`] refuses a row that is not in the trash, so nothing can
//! be destroyed without having been set aside first, and both it and
//! [`empty_trash`] run inside one transaction so a failure part-way leaves
//! the database as it was.
//!
//! A conversation is deleted outright: the row goes and the schema's cascades
//! take its messages, attachments, tapbacks, participants and tag memberships
//! with it. A contact is not deleted: deleting a trashed contact does what a
//! phone's Delete Contact does. The name and the person's edits go, the row
//! stays as an Unknown contact holding the same handles, and every
//! conversation it was in is untouched and now shows the handle. The two are
//! still one operation to the caller ("Delete" on a trash row), so they share
//! [`delete_trashed`].
//!
//! An import is the other way out of the trash. When it meets a handle that
//! belongs to a trashed contact, it makes a fresh contact from the backup, as
//! a first import would, moves that handle to it, and
//! [`discard_trashed_contact`] deletes the trashed contact outright (row,
//! Contact Group memberships and marker) after taking off each handle it
//! still held, so one in a conversation goes to a new contact with no name
//! (ADR-0013). A backup that still holds
//! the person is the person saying they still talk to them.

use anyhow::{Context as _, Result};
use sqlx::SqliteConnection;

use crate::db::attachment_versions::NamedFiles;
use crate::db::audit_trail::{self, AuditAction, AuditActor, ContactChange, Details};
use crate::db::contacts;
use crate::db::erase;
use crate::db::ownership::{owns_contact, owns_conversation};
use crate::db::sql::{SQLITE_IN_CHUNK, in_placeholders};
use crate::db::{WriteTx, begin_write};

/// Conversation `conv` is not in the trash, as a SQL condition. Every
/// statement that leaves trashed conversations out writes it with this
/// function or [`not_trashed_conversation_id`], so the lists, the counts,
/// the names and search cannot drift apart.
pub(crate) fn not_trashed_conversation(conv: &str) -> String {
    not_trashed_conversation_id(&format!("{conv}.account_id"), &format!("{conv}.id"))
}

/// The conversation with id `id_expr` in account `account_expr` is not in
/// the trash, for a row that carries a conversation id without joining
/// `conversations` (a `messages` row).
pub(crate) fn not_trashed_conversation_id(account_expr: &str, id_expr: &str) -> String {
    format!(
        "NOT EXISTS (SELECT 1 FROM trashed_conversations tc WHERE tc.account_id = {account_expr} AND tc.conversation_id = {id_expr})"
    )
}

/// Contact `ct` is not in the trash, as a SQL condition. Every statement
/// that leaves trashed contacts out writes it with this function or
/// [`not_trashed_contact_id`].
pub(crate) fn not_trashed_contact(ct: &str) -> String {
    not_trashed_contact_id(&format!("{ct}.account_id"), &format!("{ct}.id"))
}

/// The contact with id `id_expr` in account `account_expr` is not in the
/// trash, for a row that carries a contact id without joining `contacts`
/// (a `contact_handles` or a `participants` row).
pub(crate) fn not_trashed_contact_id(account_expr: &str, id_expr: &str) -> String {
    format!(
        "NOT EXISTS (SELECT 1 FROM trashed_contacts tct WHERE tct.account_id = {account_expr} AND tct.contact_id = {id_expr})"
    )
}

/// A thing that can be put in the trash, named by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trashable {
    /// A row of `conversations`, marked in `trashed_conversations`.
    Conversation(i64),
    /// A row of `contacts`, marked in `trashed_contacts`.
    Contact(i64),
}

impl Trashable {
    /// The row id, whichever kind this is.
    fn id(self) -> i64 {
        match self {
            Self::Conversation(id) | Self::Contact(id) => id,
        }
    }

    /// The marker table and the column in it that carries the row id.
    fn marker(self) -> (&'static str, &'static str) {
        match self {
            Self::Conversation(_) => ("trashed_conversations", "conversation_id"),
            Self::Contact(_) => ("trashed_contacts", "contact_id"),
        }
    }

    /// True when `account_id` owns the row this names.
    async fn is_owned(self, conn: &mut SqliteConnection, account_id: i64) -> Result<bool> {
        match self {
            Self::Conversation(id) => owns_conversation(conn, account_id, id)
                .await
                .context("check that the account holds the conversation"),
            Self::Contact(id) => owns_contact(conn, account_id, id)
                .await
                .context("check that the account holds the contact"),
        }
    }

    /// True when this row carries a trash marker for `account_id`.
    async fn is_trashed(self, conn: &mut SqliteConnection, account_id: i64) -> Result<bool> {
        let (table, id_column) = self.marker();
        let found: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT 1 FROM {table} WHERE account_id = $1 AND {id_column} = $2"
        )))
        .bind(account_id)
        .bind(self.id())
        .fetch_optional(&mut *conn)
        .await
        .context("check for a trash marker")?;
        Ok(found.is_some())
    }
}

/// Mark a conversation or contact trashed. Returns `false` when the id is not
/// `account_id`'s; `true` (a no-op) when it is already trashed.
///
/// # Errors
///
/// Returns an error naming the step when a statement fails.
pub async fn move_to_trash(
    conn: &mut SqliteConnection,
    account_id: i64,
    target: Trashable,
) -> Result<bool> {
    if !target.is_owned(conn, account_id).await? {
        return Ok(false);
    }
    let (table, id_column) = target.marker();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {table} (account_id, {id_column}) VALUES ($1, $2)
         ON CONFLICT DO NOTHING"
    )))
    .bind(account_id)
    .bind(target.id())
    .execute(&mut *conn)
    .await
    .context("insert a trash marker")?;
    Ok(true)
}

/// Remove a conversation's or contact's trash marker, if any. Returns `false`
/// when the id is not `account_id`'s; `true` (a no-op) when it was not
/// trashed.
///
/// # Errors
///
/// Returns an error naming the step when a statement fails.
pub async fn restore(
    conn: &mut SqliteConnection,
    account_id: i64,
    target: Trashable,
) -> Result<bool> {
    if !target.is_owned(conn, account_id).await? {
        return Ok(false);
    }
    let (table, id_column) = target.marker();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM {table} WHERE account_id = $1 AND {id_column} = $2"
    )))
    .bind(account_id)
    .bind(target.id())
    .execute(&mut *conn)
    .await
    .context("delete a trash marker")?;
    Ok(true)
}

/// What [`update_trashed_contacts`] did, each list in the order the request
/// named its contacts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrashedContactsChange {
    /// Contacts of `add` that are in the Trash now, those that were there
    /// already included. A contact the account does not hold is left out.
    pub in_trash: Vec<i64>,
    /// Contacts that were not in the Trash and now are: what the
    /// `contacts_trashed` entry names.
    pub trashed: Vec<i64>,
    /// Contacts that were in the Trash and now are not.
    pub restored: Vec<i64>,
}

/// Move the contacts `add` names to the Trash and take the ones `remove`
/// names out of it, in one write transaction, and record each half that
/// changed a contact in the Audit Trail at `now`: one `contacts_trashed`
/// entry and one `contacts_restored` entry, each with the count and the ids
/// of the contacts it changed (CONTACT-59).
///
/// An id the account does not hold is skipped in either list, so one contact
/// deleted since the person chose it never keeps the others where they were
/// (`docs/architecture/rules/http-api.md`, API-13). A contact already in the
/// Trash stays there and is not recorded again, and one not in it is not
/// restored. The caller keeps an id out of both lists at once.
///
/// # Errors
///
/// Returns an error naming the step when a statement fails; the transaction
/// is then rolled back.
pub async fn update_trashed_contacts(
    conn: &mut SqliteConnection,
    account_id: i64,
    add: &[i64],
    remove: &[i64],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<TrashedContactsChange> {
    let mut tx = begin_write(conn)
        .await
        .context("begin a write transaction")?;
    let mut change = TrashedContactsChange::default();
    for &id in add {
        if change.in_trash.contains(&id)
            || !owns_contact(&mut tx, account_id, id)
                .await
                .context("check that the account holds the contact")?
        {
            continue;
        }
        let inserted = sqlx::query(
            "INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(account_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("insert a contact's trash marker")?
        .rows_affected();
        change.in_trash.push(id);
        if inserted > 0 {
            change.trashed.push(id);
        }
    }
    for &id in remove {
        let deleted =
            sqlx::query("DELETE FROM trashed_contacts WHERE account_id = $1 AND contact_id = $2")
                .bind(account_id)
                .bind(id)
                .execute(&mut *tx)
                .await
                .context("delete a contact's trash marker")?
                .rows_affected();
        if deleted > 0 {
            change.restored.push(id);
        }
    }
    for (action, ids) in [
        (AuditAction::ContactsTrashed, &change.trashed),
        (AuditAction::ContactsRestored, &change.restored),
    ] {
        if ids.is_empty() {
            continue;
        }
        let moved = ContactChange {
            contact_id: None,
            contact_ids: ids,
        };
        audit_trail::record_contact_change(&mut tx, action, account_id, moved, now).await?;
    }
    tx.commit()
        .await
        .context("commit the contacts' trash markers")?;
    Ok(change)
}

/// True when `contact_id` carries `account_id`'s trash marker.
///
/// # Errors
///
/// Returns an error naming the step when the query fails.
pub async fn is_contact_trashed(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<bool> {
    Trashable::Contact(contact_id)
        .is_trashed(conn, account_id)
        .await
}

/// Delete `contact_id`, which the caller has established is `account_id`'s
/// trashed contact, for good, and answer the new contacts its identities
/// went to.
///
/// Unlike [`delete_trashed`], which forgets a contact and keeps its row, this
/// deletes the row. This is what an import does when the backup still holds
/// someone the person set aside (ADR-0013): the import first moves the
/// identity it met, with its siblings, to a fresh contact. Every identity the
/// contact still holds then leaves it through
/// [`contacts::take_identities_off`], so one in a conversation goes to a new
/// contact with no name and none is left on no contact. The Contact Group
/// memberships and the marker go with the row.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn discard_trashed_contact(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<Vec<i64>> {
    let held = contacts::identities_of_contact(conn, account_id, contact_id).await?;
    let new_contacts = contacts::take_identities_off(conn, account_id, &held)
        .await?
        .into_iter()
        .filter_map(|moved| match moved {
            contacts::IdentityMoved::NewContact(id) => Some(id),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<i64>>()
        .into_iter()
        .collect();
    contacts::delete_contact(conn, account_id, contact_id).await?;
    Ok(new_contacts)
}

/// Remove every trash marker `account_id` holds. Called when an account's
/// conversations (and, by extension, whatever they trashed) are purged, and
/// at the end of [`empty_trash`].
///
/// # Errors
///
/// Returns an error naming the step when a statement fails.
pub async fn purge_account(conn: &mut SqliteConnection, account_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM trashed_conversations WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .context("delete the account's trashed conversation markers")?;
    sqlx::query("DELETE FROM trashed_contacts WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .context("delete the account's trashed contact markers")?;
    Ok(())
}

/// What [`delete_trashed`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// The row was deleted (a conversation) or made Unknown (a contact). The
    /// files are those no remaining message references, for the caller to
    /// remove from disk once the transaction has committed.
    Deleted(Vec<OrphanedFile>),
    /// The id is not `account_id`'s, or does not exist: a 404, the same
    /// answer [`move_to_trash`] gives.
    NotOwned,
    /// The row is this account's but is not in the trash: a 409. Only a
    /// trashed row can be deleted.
    NotTrashed,
}

/// A stored attachment file that no remaining message references.
///
/// Attachments are stored by content hash under
/// `data_dir/<account>/<assets_dir>/`, one directory for every source of the
/// account, and several messages (in one conversation or across many, from
/// one source or several) can point at the same file. A file is therefore
/// reported here only after the delete has run and a lookup for the same
/// sha256 finds no attachment of the account left, from any source, in the
/// promoted tables or in staging. An original's path is relative to the
/// account's originals directory (`<assets_dir>`), a Preview's to its Previews
/// directory (`<assets_converted_dir>`); the caller joins them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrphanedFile {
    /// The original bytes: `assets_path` under the account's assets
    /// directory, plus the `.<sha256>.mime` sidecar beside it when one was
    /// written.
    Original { sha256: String, assets_path: String },
    /// A Preview or a Thumbnail: `assets_path` under the account's converted
    /// directory.
    Derived { assets_path: String },
}

/// Permanently delete a trashed conversation, or make a trashed contact
/// Unknown again, its `last_modified` set to `now`. See the module notes for
/// what each means. A conversation's Audit Trail entry is stamped `now` too.
/// What was deleted is erased from the database's files ([`crate::db::erase`]).
///
/// # Errors
///
/// Returns an error naming the step when a statement fails; the transaction is then
/// rolled back.
pub async fn delete_trashed(
    conn: &mut SqliteConnection,
    account_id: i64,
    target: Trashable,
    actor: AuditActor,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<DeleteOutcome> {
    // The checks run inside the write transaction, so a restore cannot commit
    // between the check of the Trash marker and the delete it allows.
    let mut tx = begin_write(conn)
        .await
        .context("begin a write transaction")?;
    if !target.is_owned(&mut tx, account_id).await? {
        return Ok(DeleteOutcome::NotOwned);
    }
    if !target.is_trashed(&mut tx, account_id).await? {
        return Ok(DeleteOutcome::NotTrashed);
    }
    let orphaned = match target {
        Trashable::Conversation(id) => {
            let orphaned = delete_conversations(&mut tx, account_id, &[id]).await?;
            // Which conversation it was is the holder's; the trail says only
            // that one was deleted for good. Recorded in the delete's own
            // transaction, so the two land together or not at all.
            audit_trail::record_about(
                &mut tx,
                AuditAction::ConversationDeleted,
                actor,
                account_id,
                Details::default(),
                now,
            )
            .await?;
            erase::erase_and_commit(tx).await?;
            orphaned
        }
        Trashable::Contact(id) => {
            forget_contacts(&mut tx, account_id, &[id], now).await?;
            // The contact's id, as the Audit Trail names every contact it
            // records (CONTACT-59), in the delete's own transaction.
            let deleted = ContactChange {
                contact_id: Some(id),
                contact_ids: &[],
            };
            audit_trail::record_contact_change(
                &mut tx,
                AuditAction::ContactDeleted,
                account_id,
                deleted,
                now,
            )
            .await?;
            tx.commit().await.context("commit the forgotten contact")?;
            Vec::new()
        }
    };
    erase::clear_write_ahead_log(conn).await;
    Ok(DeleteOutcome::Deleted(orphaned))
}

/// Empty the trash: delete every trashed conversation permanently and make
/// every trashed contact Unknown, its `last_modified` set to `now`, then
/// clear both marker tables and record it in the Audit Trail at `now`. One
/// transaction, so the trash is either emptied or untouched. A Trash that
/// holds nothing is not recorded, because emptying it changes nothing
/// (`docs/architecture/rules/contacts-identities-and-messages.md`,
/// CONTACT-59). What was deleted is erased from the database's files
/// ([`crate::db::erase`]).
///
/// # Errors
///
/// Returns an error naming the step when a statement fails; the transaction is then
/// rolled back.
pub async fn empty_trash(
    conn: &mut SqliteConnection,
    account_id: i64,
    actor: AuditActor,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<EmptiedTrash> {
    let mut tx = begin_write(conn)
        .await
        .context("begin a write transaction")?;
    let conversation_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT t.conversation_id
         FROM trashed_conversations t
         JOIN conversations c ON c.id = t.conversation_id AND c.account_id = t.account_id
         WHERE t.account_id = $1
         ORDER BY t.conversation_id",
    )
    .bind(account_id)
    .fetch_all(&mut *tx)
    .await
    .context("read the account's trashed conversations")?;
    let contact_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT t.contact_id
         FROM trashed_contacts t
         JOIN contacts ct ON ct.id = t.contact_id AND ct.account_id = t.account_id
         WHERE t.account_id = $1
         ORDER BY t.contact_id",
    )
    .bind(account_id)
    .fetch_all(&mut *tx)
    .await
    .context("read the account's trashed contacts")?;
    let orphaned = delete_conversations(&mut tx, account_id, &conversation_ids).await?;
    forget_contacts(&mut tx, account_id, &contact_ids, now).await?;
    // The two calls above cleared the markers for the rows they found; this
    // also drops any marker whose row is already gone, which the marker
    // tables allow because neither carries a foreign key to its target.
    purge_account(&mut tx, account_id).await?;
    if !(conversation_ids.is_empty() && contact_ids.is_empty()) {
        let count = |n: usize| Some(i64::try_from(n).unwrap_or(i64::MAX));
        audit_trail::record_about(
            &mut tx,
            AuditAction::TrashEmptied,
            actor,
            account_id,
            Details {
                conversations: count(conversation_ids.len()),
                contacts: count(contact_ids.len()),
                ..Details::default()
            },
            now,
        )
        .await?;
    }
    erase::erase_and_commit(tx).await?;
    erase::clear_write_ahead_log(conn).await;
    Ok(EmptiedTrash {
        orphaned,
        conversations: conversation_ids.len(),
        contacts: contact_ids.len(),
    })
}

/// What emptying the trash did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptiedTrash {
    /// Attachment files no remaining message references.
    pub orphaned: Vec<OrphanedFile>,
    /// Conversations deleted for good.
    pub conversations: usize,
    /// Contacts made Unknown again.
    pub contacts: usize,
}

/// Delete those of `ids` that are `account_id`'s trashed conversations, and
/// report the attachment files nothing references any more. The `DELETE`
/// checks the Trash marker itself, so a conversation restored since the
/// caller listed it stays. The schema's cascades remove messages, attachments, tapbacks,
/// earlier versions, participants and tag memberships; `duplicate_of` on a message elsewhere
/// that pointed at one of these is set NULL, so the surviving copy becomes
/// the one that shows.
async fn delete_conversations(
    tx: &mut WriteTx<'_>,
    account_id: i64,
    ids: &[i64],
) -> Result<Vec<OrphanedFile>> {
    let conn: &mut SqliteConnection = tx;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    // Each attachment's stored files, read before its message is deleted so
    // the reference check afterwards knows what to look for.
    let mut candidates: Vec<NamedFiles> = Vec::new();
    for chunk in ids.chunks(SQLITE_IN_CHUNK) {
        let placeholders = in_placeholders(1, chunk.len());
        let sql = format!(
            "SELECT DISTINCT a.sha256, a.assets_path,
                    a.preview_sha256, a.preview_assets_path,
                    a.thumbnail_sha256, a.thumbnail_assets_path
             FROM attachments a
             JOIN messages m ON m.id = a.message_id
             WHERE m.conversation_id IN ({placeholders})
               AND (a.sha256 IS NOT NULL OR a.preview_sha256 IS NOT NULL
                    OR a.thumbnail_sha256 IS NOT NULL)"
        );
        let mut q = sqlx::query_as::<_, NamedFiles>(sqlx::AssertSqlSafe(sql));
        for id in chunk {
            q = q.bind(*id);
        }
        candidates.extend(
            q.fetch_all(&mut *conn)
                .await
                .context("read the attachment files of the conversations to delete")?,
        );
    }
    for chunk in ids.chunks(SQLITE_IN_CHUNK) {
        let placeholders = in_placeholders(2, chunk.len());
        // The conversations go first, while their markers still say they are
        // in the Trash.
        for (table, id_column, trashed, step) in [
            (
                "conversations",
                "id",
                " AND id IN (SELECT conversation_id FROM trashed_conversations
                             WHERE account_id = $1)",
                "delete trashed conversations",
            ),
            (
                "trashed_conversations",
                "conversation_id",
                "",
                "delete the trash markers of the conversations deleted for good",
            ),
        ] {
            let sql = format!(
                "DELETE FROM {table}
                 WHERE account_id = $1 AND {id_column} IN ({placeholders}){trashed}"
            );
            let mut q = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(account_id);
            for id in chunk {
                q = q.bind(*id);
            }
            q.execute(&mut *conn).await.context(step)?;
        }
    }
    orphaned_files(conn, account_id, candidates).await
}

/// The files among `candidates` that no attachment of `account_id` points at
/// any more, from any source, checked one sha256 at a time against the
/// promoted and the staging attachment tables. Sorted and de-duplicated, so two
/// deleted messages sharing one file report it once.
async fn orphaned_files(
    conn: &mut SqliteConnection,
    account_id: i64,
    candidates: Vec<NamedFiles>,
) -> Result<Vec<OrphanedFile>> {
    let mut out = Vec::new();
    for NamedFiles {
        sha256,
        assets_path,
        preview_sha256,
        preview_assets_path,
        thumbnail_sha256,
        thumbnail_assets_path: thumbnail_path,
    } in candidates
    {
        if let (Some(sha256), Some(assets_path)) = (sha256, assets_path)
            && !original_is_referenced(conn, account_id, &sha256).await?
        {
            out.push(OrphanedFile::Original {
                sha256,
                assets_path,
            });
        }
        // A Preview and a Thumbnail share the converted directory, so a file
        // there goes only when no row names it as either.
        for (sha256, assets_path) in [
            (preview_sha256, preview_assets_path),
            (thumbnail_sha256, thumbnail_path),
        ] {
            if let (Some(sha256), Some(assets_path)) = (sha256, assets_path)
                && !super::attachment_versions::converted_file_is_named(conn, account_id, &sha256)
                    .await?
            {
                out.push(OrphanedFile::Derived { assets_path });
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// True when any attachment of `account_id`, from any source, promoted or in
/// staging, still names the original `sha256`. Staging is included so an
/// import that has already uploaded a file it is about to promote does not
/// lose it.
async fn original_is_referenced(
    conn: &mut SqliteConnection,
    account_id: i64,
    sha256: &str,
) -> Result<bool> {
    let sql = "SELECT 1 FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1 AND a.sha256 = $2
         UNION ALL
         SELECT 1 FROM staging_attachments sa
         JOIN staging_messages sm ON sm.id = sa.message_id
         WHERE sm.account_id = $1 AND sa.sha256 = $2
         LIMIT 1";
    let found: Option<i64> = sqlx::query_scalar(sql)
        .bind(account_id)
        .bind(sha256)
        .fetch_optional(&mut *conn)
        .await
        .context("check whether an attachment still names an original file")?;
    Ok(found.is_some())
}

/// Make `ids`, which the caller has already established are `account_id`'s
/// trashed contacts, Unknown again: the name goes, so the next import that
/// meets one of its handles may name it, its Contact Group memberships go,
/// and its trash marker goes. The row keeps its origin, because `origin`
/// records what made the row and an import names a nameless contact
/// whatever made it. The handles stay linked, so the conversations the
/// person was in keep showing them as one participant. They show by handle
/// now, since the name is blank. Their `last_modified` is set to `now`.
async fn forget_contacts(
    conn: &mut SqliteConnection,
    account_id: i64,
    ids: &[i64],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let last_modified = now.format("%Y-%m-%d %H:%M:%S").to_string();
    for chunk in ids.chunks(SQLITE_IN_CHUNK) {
        let placeholders = in_placeholders(3, chunk.len());
        let sql = format!(
            "UPDATE contacts SET preferred_name = '', last_modified = $2
             WHERE account_id = $1 AND id IN ({placeholders})"
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(account_id)
            .bind(&last_modified);
        for id in chunk {
            q = q.bind(*id);
        }
        q.execute(&mut *conn)
            .await
            .context("clear the names of trashed contacts")?;

        let placeholders = in_placeholders(2, chunk.len());
        let sql = format!(
            "DELETE FROM contact_group_members
             WHERE contact_id IN (
                 SELECT id FROM contacts WHERE account_id = $1 AND id IN ({placeholders})
             )"
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(account_id);
        for id in chunk {
            q = q.bind(*id);
        }
        q.execute(&mut *conn)
            .await
            .context("delete trashed contacts' Contact Group memberships")?;

        let sql = format!(
            "DELETE FROM trashed_contacts WHERE account_id = $1 AND contact_id IN ({placeholders})"
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(account_id);
        for id in chunk {
            q = q.bind(*id);
        }
        q.execute(&mut *conn)
            .await
            .context("delete trashed contacts' trash markers")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
