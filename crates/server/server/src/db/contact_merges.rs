//! Merging contacts and undoing the merge: the `contact_merges` tables
//! (`docs/architecture/rules/contacts-identities-and-messages.md`,
//! CONTACT-56).
//!
//! A merge makes two or more contacts one. The target keeps its id, name and
//! origin; every other contact's identities, Contact Group memberships and
//! Import Run records move to it, and its own row leaves `contacts`. Nothing
//! keyed by an identity changes, so no conversation, participant, message or
//! content key moves, and no duplicate pass runs.
//!
//! What the merge took is written to the `contact_merge_*` tables first, so
//! [`unmerge`] can put it back: each merged contact's row as it was, the
//! identities it held, the Contact Groups it was in, and its Import Run
//! records, with what the target had before. An account keeps one such
//! record, its most recent merge. The next merge drops it, and the contacts
//! it took are then gone for good.
//!
//! A merged contact is deleted from `contacts` rather than marked, so no
//! reader of contacts needs a second filter beside the trash's: it is in no
//! list, search or export, and its page answers `404 Not Found`, because
//! there is no row. `contacts.id` is `AUTOINCREMENT`, so its id is never
//! handed to another contact, and an undo writes it back under the same id.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::SqliteConnection;

use crate::db::contacts::{delete_contact, delete_if_empty, touch_contact};
use crate::db::import_contacts::ContactReason;
use crate::db::trash::not_trashed_contact;

/// Most contacts one merge takes, the target among them.
///
/// A real merge is two to four rows of one person: the iPhone contact, the
/// Android one, a WhatsApp number, an email address. A larger selection is
/// almost always a slip of Shift + click or Select all, and refusing it beats
/// an undo that lasts only until the next merge.
pub const MAX_MERGED_CONTACTS: usize = 10;

/// Why a merge was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeRefusal {
    /// The target is not one of the account's contacts.
    TargetMissing,
    /// A contact in `from` is not one of the account's contacts.
    SourceMissing(i64),
    /// The contact is in the Trash.
    Trashed(i64),
    /// The contact holds one of the account's own identities, which leaves
    /// it as the account's identities change (CONTACT-48), not by a merge.
    HoldsAccountIdentity(i64),
}

/// Why an undo of a merge was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnmergeRefusal {
    /// The target is not one of the account's contacts.
    TargetMissing,
    /// The account's most recent merge did not merge into this contact, or
    /// the account has none.
    NotTheLastMerge,
}

/// What [`contact_state`] reads of one contact.
#[derive(sqlx::FromRow)]
struct ContactState {
    trashed: bool,
    holds_account_identity: bool,
}

/// The contact's place in the Trash and whether it holds one of the
/// account's own identities, or `None` when it is not the account's.
async fn contact_state(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<Option<ContactState>> {
    sqlx::query_as::<_, ContactState>(sqlx::AssertSqlSafe(format!(
        "SELECT NOT ({not_trashed}) AS trashed,
                EXISTS (SELECT 1 FROM contact_handles ch
                        JOIN handles h ON h.id = ch.handle_id
                        WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id
                          AND {own}) AS holds_account_identity
         FROM contacts ct
         WHERE ct.account_id = $1 AND ct.id = $2",
        not_trashed = not_trashed_contact("ct"),
        own = crate::db::account_profile::is_account_identity_sql("h", "ct.account_id"),
    )))
    .bind(account_id)
    .bind(contact_id)
    .fetch_optional(&mut *conn)
    .await
    .context("read a contact for a merge")
}

/// Merge each contact in `from` into `target_id`, and keep what moved so
/// [`unmerge`] can put it back. The account's previous merge is dropped,
/// and the contacts it took go for good.
///
/// The caller runs this in a write transaction and has checked `from`: not
/// empty, without `target_id`, without repeats, and at most
/// [`MAX_MERGED_CONTACTS`] with the target.
///
/// # Errors
///
/// Returns an error when a statement fails. The inner `Err` says why the
/// merge was refused, and nothing has changed.
pub async fn merge(
    conn: &mut SqliteConnection,
    account_id: i64,
    target_id: i64,
    from: &[i64],
    now: DateTime<Utc>,
) -> Result<Result<(), MergeRefusal>> {
    match contact_state(conn, account_id, target_id).await? {
        None => return Ok(Err(MergeRefusal::TargetMissing)),
        Some(state) if state.trashed => return Ok(Err(MergeRefusal::Trashed(target_id))),
        Some(state) if state.holds_account_identity => {
            return Ok(Err(MergeRefusal::HoldsAccountIdentity(target_id)));
        }
        Some(_) => {}
    }
    for &id in from {
        match contact_state(conn, account_id, id).await? {
            None => return Ok(Err(MergeRefusal::SourceMissing(id))),
            Some(state) if state.trashed => return Ok(Err(MergeRefusal::Trashed(id))),
            Some(state) if state.holds_account_identity => {
                return Ok(Err(MergeRefusal::HoldsAccountIdentity(id)));
            }
            Some(_) => {}
        }
    }

    // The previous merge becomes permanent: its record goes, and with it
    // the only copy of the contacts it took.
    sqlx::query("DELETE FROM contact_merges WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .context("drop the account's previous contact merge")?;
    let merged_at = now.format("%Y-%m-%d %H:%M:%S").to_string();
    sqlx::query(
        "INSERT INTO contact_merges (account_id, target_id, merged_at) VALUES ($1, $2, $3)",
    )
    .bind(account_id)
    .bind(target_id)
    .bind(&merged_at)
    .execute(&mut *conn)
    .await
    .context("record a contact merge")?;

    // What the target had before anything moved, so an undo can tell what
    // the merge gave it from what was its own.
    let target_groups: Vec<i64> =
        sqlx::query_scalar("SELECT group_id FROM contact_group_members WHERE contact_id = $1")
            .bind(target_id)
            .fetch_all(&mut *conn)
            .await
            .context("read the target's Contact Groups")?;
    let target_reasons = import_reasons(conn, target_id).await?;
    let mut reasons = target_reasons.clone();

    for &source in from {
        record_source(conn, account_id, source).await?;
        // Every identity the source holds moves to the target as it is.
        sqlx::query(
            "INSERT INTO contact_merge_identities (account_id, contact_id, handle_id, origin)
             SELECT account_id, contact_id, handle_id, origin FROM contact_handles
             WHERE account_id = $1 AND contact_id = $2",
        )
        .bind(account_id)
        .bind(source)
        .execute(&mut *conn)
        .await
        .context("record a merged contact's identities")?;
        sqlx::query(
            "UPDATE contact_handles SET contact_id = $3 WHERE account_id = $1 AND contact_id = $2",
        )
        .bind(account_id)
        .bind(source)
        .bind(target_id)
        .execute(&mut *conn)
        .await
        .context("move a merged contact's identities to the target")?;

        // Its Contact Groups: a group both were in stays one row.
        let groups: Vec<i64> =
            sqlx::query_scalar("SELECT group_id FROM contact_group_members WHERE contact_id = $1")
                .bind(source)
                .fetch_all(&mut *conn)
                .await
                .context("read a merged contact's Contact Groups")?;
        for group_id in groups {
            sqlx::query(
                "INSERT INTO contact_merge_groups (account_id, contact_id, group_id, target_had)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(account_id)
            .bind(source)
            .bind(group_id)
            .bind(target_groups.contains(&group_id))
            .execute(&mut *conn)
            .await
            .context("record a merged contact's Contact Group")?;
            sqlx::query(
                "INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)
                 ON CONFLICT (contact_id, group_id) DO NOTHING",
            )
            .bind(target_id)
            .bind(group_id)
            .execute(&mut *conn)
            .await
            .context("put the target in a merged contact's Contact Group")?;
        }

        // Its Import Run records. A run with a record of both keeps the
        // more consequential reason, as one run touching one contact twice
        // does (`ContactReason`).
        for (import_id, reason) in import_reasons(conn, source).await? {
            sqlx::query(
                "INSERT INTO contact_merge_imports
                   (account_id, contact_id, import_id, reason, target_reason)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(account_id)
            .bind(source)
            .bind(import_id)
            .bind(reason.as_str())
            .bind(target_reasons.get(&import_id).map(|r| r.as_str()))
            .execute(&mut *conn)
            .await
            .context("record a merged contact's Import Run record")?;
            let kept = reasons
                .get(&import_id)
                .map_or(reason, |&held| held.min(reason));
            reasons.insert(import_id, kept);
            set_import_reason(conn, import_id, target_id, kept).await?;
        }

        // The cascade takes the source's memberships and Import Run records,
        // which the target now has.
        delete_contact(conn, account_id, source).await?;
    }
    touch_contact(conn, account_id, target_id, now).await?;
    Ok(Ok(()))
}

/// Undo the account's most recent merge, which merged into `target_id`:
/// write each merged contact back under its id, give it back the identities
/// it held that are still on the target (one the person moved elsewhere
/// since stays there), its Contact Groups and its Import Run records, and
/// take back from the target what only the merge gave it. Answers the
/// contacts written back. The caller runs this in a write transaction.
///
/// # Errors
///
/// Returns an error when a statement fails. The inner `Err` says why the
/// undo was refused, and nothing has changed.
pub async fn unmerge(
    conn: &mut SqliteConnection,
    account_id: i64,
    target_id: i64,
    now: DateTime<Utc>,
) -> Result<Result<Vec<i64>, UnmergeRefusal>> {
    if contact_state(conn, account_id, target_id).await?.is_none() {
        return Ok(Err(UnmergeRefusal::TargetMissing));
    }
    let last_target: Option<i64> =
        sqlx::query_scalar("SELECT target_id FROM contact_merges WHERE account_id = $1")
            .bind(account_id)
            .fetch_optional(&mut *conn)
            .await
            .context("read the account's last contact merge")?;
    if last_target != Some(target_id) {
        return Ok(Err(UnmergeRefusal::NotTheLastMerge));
    }

    let sources: Vec<i64> = sqlx::query_scalar(
        "INSERT INTO contacts (id, account_id, preferred_name, origin, created_at, last_modified)
         SELECT contact_id, account_id, preferred_name, origin, created_at, last_modified
         FROM contact_merge_contacts WHERE account_id = $1
         ORDER BY contact_id
         RETURNING id",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .context("write the merged contacts back")?;

    sqlx::query(
        "UPDATE contact_handles
         SET contact_id = mi.contact_id, origin = mi.origin
         FROM contact_merge_identities mi
         WHERE mi.account_id = $1
           AND contact_handles.account_id = $1
           AND contact_handles.handle_id = mi.handle_id
           AND contact_handles.contact_id = $2",
    )
    .bind(account_id)
    .bind(target_id)
    .execute(&mut *conn)
    .await
    .context("give the merged contacts back their identities")?;

    sqlx::query(
        "INSERT INTO contact_group_members (contact_id, group_id)
         SELECT contact_id, group_id FROM contact_merge_groups WHERE account_id = $1
         ON CONFLICT (contact_id, group_id) DO NOTHING",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await
    .context("put the merged contacts back in their Contact Groups")?;
    sqlx::query(
        "DELETE FROM contact_group_members
         WHERE contact_id = $2
           AND group_id IN (SELECT group_id FROM contact_merge_groups
                            WHERE account_id = $1 AND target_had = 0)",
    )
    .bind(account_id)
    .bind(target_id)
    .execute(&mut *conn)
    .await
    .context("take the target out of the Contact Groups only the merge gave it")?;

    sqlx::query(
        "INSERT INTO import_contacts (import_id, contact_id, reason)
         SELECT import_id, contact_id, reason FROM contact_merge_imports WHERE account_id = $1",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await
    .context("give the merged contacts back their Import Run records")?;
    #[derive(sqlx::FromRow)]
    struct TargetRecord {
        import_id: i64,
        target_reason: Option<String>,
    }
    let records = sqlx::query_as::<_, TargetRecord>(
        "SELECT DISTINCT import_id, target_reason FROM contact_merge_imports
         WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .context("read the target's Import Run records from before the merge")?;
    for TargetRecord {
        import_id,
        target_reason,
    } in records
    {
        match target_reason.as_deref().and_then(ContactReason::parse) {
            Some(reason) => set_import_reason(conn, import_id, target_id, reason).await?,
            None => {
                sqlx::query("DELETE FROM import_contacts WHERE import_id = $1 AND contact_id = $2")
                    .bind(import_id)
                    .bind(target_id)
                    .execute(&mut *conn)
                    .await
                    .context("take back the Import Run record only the merge gave the target")?;
            }
        }
    }

    sqlx::query("DELETE FROM contact_merges WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .context("drop the undone contact merge")?;
    touch_contact(conn, account_id, target_id, now).await?;

    // A contact with no name whose identities all moved elsewhere since the
    // merge would come back with nothing to show, so it stays gone.
    let mut back = Vec::with_capacity(sources.len());
    for id in sources {
        if !delete_if_empty(conn, account_id, id).await? {
            back.push(id);
        }
    }
    Ok(Ok(back))
}

/// Copy `contact_id`'s row into `contact_merge_contacts`.
async fn record_source(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO contact_merge_contacts
           (account_id, contact_id, preferred_name, origin, created_at, last_modified)
         SELECT account_id, id, preferred_name, origin, created_at, last_modified
         FROM contacts WHERE account_id = $1 AND id = $2",
    )
    .bind(account_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .context("record a merged contact")?;
    Ok(())
}

/// Each Import Run's record of `contact_id`, by run.
async fn import_reasons(
    conn: &mut SqliteConnection,
    contact_id: i64,
) -> Result<HashMap<i64, ContactReason>> {
    #[derive(sqlx::FromRow)]
    struct Record {
        import_id: i64,
        reason: String,
    }
    let rows = sqlx::query_as::<_, Record>(
        "SELECT import_id, reason FROM import_contacts WHERE contact_id = $1",
    )
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await
    .context("read a contact's Import Run records")?;
    Ok(rows
        .into_iter()
        .filter_map(|Record { import_id, reason }| {
            ContactReason::parse(&reason).map(|reason| (import_id, reason))
        })
        .collect())
}

/// Set the reason of `import_id`'s record of `contact_id`, writing the
/// record when there is none.
async fn set_import_reason(
    conn: &mut SqliteConnection,
    import_id: i64,
    contact_id: i64,
    reason: ContactReason,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO import_contacts (import_id, contact_id, reason) VALUES ($1, $2, $3)
         ON CONFLICT (import_id, contact_id) DO UPDATE SET reason = excluded.reason",
    )
    .bind(import_id)
    .bind(contact_id)
    .bind(reason.as_str())
    .execute(&mut *conn)
    .await
    .context("write an Import Run's record of a contact")?;
    Ok(())
}
