//! Empty Trash (`DELETE /v1/trash`), and moving contacts to and out of the
//! Trash together (`PATCH /v1/trash/contacts`).
//!
//! The database work lives in [`crate::db::trash`], and removing the files
//! it reported as unreferenced lives in [`crate::asset_store`], which
//! `DELETE /v1/conversations/{id}` shares.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::api_error::ApiError;
use crate::asset_store;
use crate::auth::{FullAccess, FullDeleteAccess};
use crate::config::Config;
use crate::db::audit_trail::AuditActor;
use crate::db::trash::{self, TrashContactsOutcome};
use crate::extract::Json;

/// Most contact ids one `PATCH /v1/trash/contacts` names, `add` and
/// `remove` together. The Audit Trail entry it writes names each contact it
/// moved, so the cap bounds the entry. The web app sends a larger Delete
/// Contacts in parts of this size.
pub(crate) const MAX_TRASHED_CONTACT_IDS: usize = 10_000;

/// Body for `PATCH /v1/trash/contacts`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct UpdateTrashedContactsRequest {
    /// Contacts to move to the Trash. An id the account does not hold is
    /// refused with `422 Unprocessable Entity`, and then nothing changes. A
    /// contact already in the Trash stays there. Empty when left out.
    #[serde(default)]
    pub(crate) add: Vec<i64>,
    /// Contacts to take out of the Trash. An id that names no contact in the
    /// account's Trash is ignored. Empty when left out. `add` and `remove`
    /// together name at least one contact and at most 10000, and no contact
    /// in both.
    #[serde(default)]
    pub(crate) remove: Vec<i64>,
}

/// What `PATCH /v1/trash/contacts` changed.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct UpdateTrashedContactsResponse {
    /// Contacts the request moved to the Trash, in the order `add` named
    /// them, leaving out those already there.
    pub(crate) trashed: Vec<i64>,
    /// Contacts the request took out of the Trash, in the order `remove`
    /// named them, leaving out those that were not in it.
    pub(crate) restored: Vec<i64>,
}

/// Move contacts to the Trash and take contacts out of it, in one request.
///
/// Delete Contacts on the Contacts list sends its contacts as `add`, and its
/// Undo sends them back as `remove`. One request is one action, and the
/// Audit Trail records it as one `contacts_trashed` entry and one
/// `contacts_restored` entry, each with the count and ids of the contacts it
/// changed; a request that changes no contact records nothing.
#[utoipa::path(
    patch,
    path = "/v1/trash/contacts",
    tag = "Trash",
    security(("session" = [])),
    request_body = UpdateTrashedContactsRequest,
    responses(
        (status = 200, body = UpdateTrashedContactsResponse),
    )
)]
pub(crate) async fn update_trashed_contacts(
    State(db): State<SqlitePool>,
    FullAccess(auth): FullAccess,
    Json(body): Json<UpdateTrashedContactsRequest>,
) -> Result<Json<UpdateTrashedContactsResponse>, ApiError> {
    if body.add.is_empty() && body.remove.is_empty() {
        return Err(ApiError::validation(
            "add or remove must name at least one contact",
        ));
    }
    if body.add.len() + body.remove.len() > MAX_TRASHED_CONTACT_IDS {
        return Err(ApiError::validation(format!(
            "at most {MAX_TRASHED_CONTACT_IDS} contact ids"
        )));
    }
    let removing: std::collections::HashSet<i64> = body.remove.iter().copied().collect();
    if let Some(both) = body.add.iter().find(|id| removing.contains(id)) {
        return Err(ApiError::validation(format!(
            "contact {both} is in both add and remove"
        )));
    }
    let mut conn = db.acquire().await?;
    match trash::update_trashed_contacts(
        &mut conn,
        auth.account_id,
        &body.add,
        &body.remove,
        chrono::Utc::now(),
    )
    .await?
    {
        TrashContactsOutcome::Changed(change) => Ok(Json(UpdateTrashedContactsResponse {
            trashed: change.trashed,
            restored: change.restored,
        })),
        TrashContactsOutcome::NotHeld(id) => {
            Err(ApiError::validation(format!("contact {id} not found")))
        }
    }
}

/// Empty the trash. Every trashed conversation is deleted for good, with
/// its messages and any attachment file no other message uses, and every
/// trashed contact loses its name and details and becomes Unknown, its
/// conversations untouched. Trash is the only door to permanent deletion;
/// this is the door for everything in it at once.
#[utoipa::path(
    delete,
    path = "/v1/trash",
    tag = "Trash",
    security(("session" = ["delete"])),
    responses(
        (status = 204, description = "Trash emptied"),
    )
)]
pub(crate) async fn delete_trash(
    State(db): State<SqlitePool>,
    State(cfg): State<Arc<Config>>,
    FullDeleteAccess(auth): FullDeleteAccess,
) -> Result<StatusCode, ApiError> {
    let unreferenced = {
        let mut conn = db.acquire().await?;
        let emptied = trash::empty_trash(
            &mut conn,
            auth.account_id,
            AuditActor::Holder,
            chrono::Utc::now(),
        )
        .await?;
        emptied.orphaned
    };
    asset_store::remove_unreferenced(&db, Arc::clone(&cfg), auth.account_id, unreferenced).await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
