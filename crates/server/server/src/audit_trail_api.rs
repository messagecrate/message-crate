//! The Audit Trail: `GET /v1/audit-trail`, every account's entries for the
//! owner, or one deleted account's, beside
//! `GET /v1/accounts/{id}/audit-trail` in `accounts_api`, one account's for
//! the owner and for that account. Both answer from [`audit_trail_page`], so
//! the two cannot differ. `GET /v1/audit-trail/deleted-accounts` lists the
//! deleted accounts the owner can narrow the trail to.
//!
//! The trail says who did what and when, and how much: never what a message
//! said or which conversation it was in, so the owner reads all of it
//! (`docs/adr/0008-the-owner-holds-no-messages.md`). Nobody edits or deletes
//! an entry, and an account's entries outlive it
//! (`docs/adr/0020-the-audit-trail-outlives-the-account.md`).

use axum::extract::State;
use serde::Deserialize;

use crate::db::audit_trail::{self, AuditEntry, DeletedAccount, Scope};
use crate::extract::{Json, Query};
use crate::paging::{
    DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, PageParams, page_params, page_read,
};
use crate::server::{ApiError, AppState, Owner};

/// Query string of `GET /v1/audit-trail`.
#[derive(Debug, Deserialize)]
pub(crate) struct ListAuditTrailQuery {
    /// Only one deleted account's entries and runs, by
    /// [`DeletedAccount::id`].
    #[serde(default)]
    pub(crate) deleted_account_id: Option<i64>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
}

/// Query string of `GET /v1/accounts/{id}/audit-trail`: a page.
#[derive(Debug, Deserialize)]
pub(crate) struct ListAccountAuditTrailQuery {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
}

/// Query string of `GET /v1/audit-trail/deleted-accounts`: a page, as for
/// one account's Audit Trail.
pub(crate) type ListDeletedAccountsQuery = ListAccountAuditTrailQuery;

/// The page `limit` and `offset` ask for, as every Audit Trail list reads it.
fn trail_page_params(limit: Option<usize>, offset: Option<usize>) -> Result<PageParams, ApiError> {
    page_params(limit, offset, DEFAULT_LIST_LIMIT, Some(MAX_LIST_OFFSET))
}

/// One page of the Audit Trail over `scope`, newest first, as `reader` (the
/// caller's account) is shown it: an API token's masked hint only on the
/// entries about the reader's own account. The hint is part of the secret,
/// and the owner never reads another account's secret (`http-api.md`,
/// Credentials).
pub(crate) async fn audit_trail_page(
    state: &AppState,
    scope: Scope,
    reader: i64,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Json<Page<AuditEntry>>, ApiError> {
    let params = trail_page_params(limit, offset)?;
    let mut conn = state.db.acquire().await?;
    let (mut items, total) =
        audit_trail::page(&mut conn, scope, params.limit, params.offset).await?;
    for item in &mut items {
        if item.account_id != Some(reader) {
            item.api_token_hint = None;
        }
    }
    Ok(Json(page_read(items, total, params)))
}

/// Every account's Audit Trail, newest first: logins, sessions ending,
/// refused logins, Import Runs, Export Runs, and the owner's and holders'
/// changes to accounts, including those of deleted accounts under their old
/// usernames. The owner's alone.
///
/// `deleted_account_id` narrows the list to one deleted account's entries
/// and runs, which no longer carry an account id. The ids are listed by
/// `GET /v1/audit-trail/deleted-accounts`; an id that names no deleted
/// account answers an empty page. A live account's entries are read at
/// `GET /v1/accounts/{id}/audit-trail`.
#[utoipa::path(
    get,
    path = "/v1/audit-trail",
    tag = "Audit Trail",
    security(("session" = ["owner"])),
    params(
        ("deleted_account_id" = Option<i64>, Query, description = "Only the entries and runs of the deleted account with this id, from `GET /v1/audit-trail/deleted-accounts`"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000")
    ),
    responses(
        (status = 200, body = Page<AuditEntry>),
        crate::problem::openapi::NotTheOwner
    )
)]
pub(crate) async fn list_audit_trail(
    State(state): State<AppState>,
    Owner(auth): Owner,
    Query(query): Query<ListAuditTrailQuery>,
) -> Result<Json<Page<AuditEntry>>, ApiError> {
    let scope = query
        .deleted_account_id
        .map_or(Scope::All, Scope::DeletedAccount);
    audit_trail_page(&state, scope, auth.account_id, query.limit, query.offset).await
}

/// The deleted accounts whose entries the Audit Trail keeps, by username A to
/// Z, the latest deletion first under one username. Each is one account:
/// two accounts deleted under one username are two. Owner Home offers them
/// beside the live accounts, to narrow the Audit Trail to one with
/// `GET /v1/audit-trail?deleted_account_id=`. The owner's alone.
#[utoipa::path(
    get,
    path = "/v1/audit-trail/deleted-accounts",
    tag = "Audit Trail",
    security(("session" = ["owner"])),
    params(
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000")
    ),
    responses(
        (status = 200, body = Page<DeletedAccount>),
        crate::problem::openapi::NotTheOwner
    )
)]
pub(crate) async fn list_deleted_accounts(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Query(query): Query<ListDeletedAccountsQuery>,
) -> Result<Json<Page<DeletedAccount>>, ApiError> {
    let params = trail_page_params(query.limit, query.offset)?;
    let mut conn = state.db.acquire().await?;
    let (items, total) =
        audit_trail::deleted_accounts_page(&mut conn, params.limit, params.offset).await?;
    Ok(Json(page_read(items, total, params)))
}

#[cfg(test)]
mod tests;
