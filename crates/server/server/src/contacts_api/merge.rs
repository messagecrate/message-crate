//! `POST /v1/contacts/{id}/merge` and `POST /v1/contacts/{id}/unmerge`:
//! Merge Contacts on the Contacts list, and its Undo
//! (`docs/architecture/rules/contacts-identities-and-messages.md`,
//! CONTACT-56). The queries are in `db::contact_merges`.

use std::collections::HashSet;

use crate::extract::{Json, Path as AxumPath};
use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::{Contact, get_contact_detail};
use crate::api_error::ApiError;
use crate::auth::{FullAccess, FullDeleteAccess};
use crate::db::contact_merges::{self, MAX_MERGED_CONTACTS, MergeRefusal, UnmergeRefusal};

/// Body for `POST /v1/contacts/{id}/merge`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MergeContactRequest {
    /// The contacts to merge into the one in the path: at least one, none
    /// of them that contact or named twice, and at most 9, so that one
    /// merge takes at most 10 contacts. Anything else is refused with
    /// `422 Unprocessable Entity`, and so is an id that names no contact of
    /// the account.
    // `max_items` takes only a literal; a test holds it to
    // `MAX_MERGED_CONTACTS - 1`.
    #[schema(min_items = 1, max_items = 9)]
    pub from: Vec<i64>,
}

/// The answer to `POST /v1/contacts/{id}/unmerge`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UnmergeContactResponse {
    /// The contact the others were merged into, as it now stands.
    pub contact: Contact,
    /// The merged contacts written back, by id. A contact with no name
    /// whose identities all moved elsewhere since the merge is left out:
    /// nothing could reach it, so it was not written back.
    pub restored: Vec<i64>,
}

/// The `from` list's problem, if it has one.
fn check_from(target_id: i64, from: &[i64]) -> Result<(), ApiError> {
    if from.is_empty() {
        return Err(ApiError::validation("from must name at least one contact"));
    }
    if from.len() + 1 > MAX_MERGED_CONTACTS {
        return Err(ApiError::validation(format!(
            "one merge takes at most {MAX_MERGED_CONTACTS} contacts"
        )));
    }
    if from.contains(&target_id) {
        return Err(ApiError::validation(
            "from names the contact the others merge into",
        ));
    }
    let mut seen = HashSet::with_capacity(from.len());
    if let Some(id) = from.iter().find(|id| !seen.insert(**id)) {
        return Err(ApiError::validation(format!(
            "from names contact {id} twice"
        )));
    }
    Ok(())
}

/// Merge other contacts into this one.
///
/// The contact in the path keeps its id, name and origin, and every contact
/// in `from` moves into it: its identities, Contact Groups and Import Run
/// records go to this contact, and it leaves every list. No conversation or
/// message changes, because they are kept by identity. The merge can be
/// undone with `POST /v1/contacts/{id}/unmerge` until the account's next
/// merge, which deletes the contacts this one merged for good, so a merge
/// needs the `delete` permission. A contact in the Trash, or one holding one
/// of the account's own identities, answers `409 Conflict`. A contact the
/// last merge took is no contact of the account's: in the path it is
/// `404 Not Found`, and in `from` `422 Unprocessable Entity`.
#[utoipa::path(
    post,
    path = "/v1/contacts/{id}/merge",
    tag = "Contacts",
    security(("session" = ["delete"])),
    params(("id" = i64, Path, description = "The contact the others merge into")),
    request_body = MergeContactRequest,
    responses(
        (status = 200, body = Contact),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn merge_contact(
    State(db): State<SqlitePool>,
    FullDeleteAccess(auth): FullDeleteAccess,
    AxumPath(target_id): AxumPath<i64>,
    Json(body): Json<MergeContactRequest>,
) -> Result<Json<Contact>, ApiError> {
    check_from(target_id, &body.from)?;
    let mut conn = db.acquire().await?;
    let mut tx = crate::db::begin_write(&mut conn).await?;
    match contact_merges::merge(
        &mut tx,
        auth.account_id,
        target_id,
        &body.from,
        chrono::Utc::now(),
    )
    .await?
    {
        Ok(()) => {}
        Err(MergeRefusal::TargetMissing) => return Err(ApiError::not_found("contact")),
        Err(MergeRefusal::SourceMissing(id)) => {
            return Err(ApiError::validation(format!("contact {id} does not exist")));
        }
        Err(MergeRefusal::Trashed(id)) => {
            return Err(ApiError::StateConflict(format!(
                "contact {id} is in the trash; restore it before merging"
            )));
        }
        Err(MergeRefusal::HoldsAccountIdentity(id)) => {
            return Err(ApiError::StateConflict(format!(
                "contact {id} holds one of the account's own identities"
            )));
        }
    }
    let contact = get_contact_detail(&mut tx, auth.account_id, target_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("contact missing after a merge")))?;
    tx.commit().await?;
    Ok(Json(contact))
}

/// Undo the account's most recent contact merge, which merged into this contact.
///
/// Each merged contact comes back under its own id with its name, its
/// Contact Groups and its Import Run records, and with each identity it held
/// that is still on this contact; one moved elsewhere since stays there.
/// This contact keeps what was its own before the merge. A contact that was
/// not the target of the account's most recent merge answers
/// `409 Conflict`. `restored` names the contacts written back.
#[utoipa::path(
    post,
    path = "/v1/contacts/{id}/unmerge",
    tag = "Contacts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "The contact the others were merged into")),
    responses(
        (status = 200, body = UnmergeContactResponse),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn unmerge_contact(
    State(db): State<SqlitePool>,
    FullAccess(auth): FullAccess,
    AxumPath(target_id): AxumPath<i64>,
) -> Result<Json<UnmergeContactResponse>, ApiError> {
    let mut conn = db.acquire().await?;
    let mut tx = crate::db::begin_write(&mut conn).await?;
    let restored =
        match contact_merges::unmerge(&mut tx, auth.account_id, target_id, chrono::Utc::now())
            .await?
        {
            Ok(restored) => restored,
            Err(UnmergeRefusal::TargetMissing) => return Err(ApiError::not_found("contact")),
            Err(UnmergeRefusal::NotTheLastMerge) => {
                return Err(ApiError::StateConflict(
                    "the account's most recent merge did not merge into this contact".into(),
                ));
            }
        };
    let contact = get_contact_detail(&mut tx, auth.account_id, target_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("contact missing after an unmerge")))?;
    tx.commit().await?;
    Ok(Json(UnmergeContactResponse { contact, restored }))
}

#[cfg(test)]
mod tests;
