//! Saved searches stored in `saved_searches`.
//!
//! Rows are addressed by id in the path rather than by name in the body, the
//! same way Contact Groups and Message Tags are. A saved search carries a name
//! and a query that are edited together, so name-addressing would use the
//! changing field as the key.

use crate::extract::{Json, Path, Query};
use crate::paging::{DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, PageQuery, page_of, page_params};
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::db::saved_searches::{self, SavedSearch, SavedSearchKind};
use crate::server::{ApiError, AppState, Created, FullAccess};

/// The name and query of a new saved search.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateSavedSearchRequest {
    /// Name of the Saved Search, which the server trims. A name that is
    /// blank or over 80 characters is refused with `422 Unprocessable
    /// Entity`. A name another of the account's Saved Searches has, in any
    /// letter case, is refused with `409 Conflict`.
    name: String,
    /// The query in the search language, trimmed and stored as written. The
    /// server does not check it when storing it, so a query a list cannot
    /// run is refused when it runs. A blank query is refused with `422
    /// Unprocessable Entity`.
    query: String,
}

/// A saved search's new name and query.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct UpdateSavedSearchRequest {
    /// The Saved Search's new name, under the rules a new one's name
    /// follows. A change to the letter case of its own name is allowed.
    name: String,
    /// The new query in the search language, under the rules a new one's
    /// query follows.
    query: String,
}

/// List the account's saved searches, A–Z.
#[utoipa::path(
    get,
    path = "/v1/saved-searches",
    tag = "Saved searches",
    security(("session" = [])),
    params(
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset, max 50000")
    ),
    responses(
        (status = 200, body = crate::paging::Page<SavedSearch>),
    )
)]
pub(crate) async fn list_saved_searches(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<SavedSearch>>, ApiError> {
    let params = page_params(
        query.limit,
        query.offset,
        DEFAULT_LIST_LIMIT,
        Some(MAX_LIST_OFFSET),
    )?;
    let mut conn = state.db.acquire().await?;
    let rows = saved_searches::list(&mut conn, auth.account_id).await?;
    Ok(Json(page_of(rows, params)))
}

/// Create a saved search: `201 Created`, `Location: /v1/saved-searches/{id}`,
/// and the row.
#[utoipa::path(
    post,
    path = "/v1/saved-searches",
    tag = "Saved searches",
    security(("session" = [])),
    request_body = CreateSavedSearchRequest,
    responses(
        (
            status = 201,
            body = SavedSearch,
            headers(("Location" = String, description = "Path of the new saved search"))
        ),
        crate::problem::openapi::NameTaken
    )
)]
pub(crate) async fn create_saved_search(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Json(body): Json<CreateSavedSearchRequest>,
) -> Result<Created<SavedSearch>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let row = saved_searches::create(
        &mut conn,
        auth.account_id,
        &body.name,
        &body.query,
        SavedSearchKind::Manual,
    )
    .await?;
    Ok(Created {
        location: format!("/v1/saved-searches/{}", row.id),
        body: row,
    })
}

/// One saved search, as the list shows it.
#[utoipa::path(
    get,
    path = "/v1/saved-searches/{id}",
    tag = "Saved searches",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Saved search id")),
    responses(
        (status = 200, body = SavedSearch),
    )
)]
pub(crate) async fn get_saved_search(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Path(id): Path<i64>,
) -> Result<Json<SavedSearch>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let row = saved_searches::get(&mut conn, auth.account_id, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("saved search not found".into()))?;
    Ok(Json(row))
}

/// Replace a saved search's name and query, and return it.
#[utoipa::path(
    patch,
    path = "/v1/saved-searches/{id}",
    tag = "Saved searches",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Saved search id")),
    request_body = UpdateSavedSearchRequest,
    responses(
        (status = 200, body = SavedSearch),
        crate::problem::openapi::NameTaken
    )
)]
pub(crate) async fn update_saved_search(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Path(id): Path<i64>,
    Json(body): Json<UpdateSavedSearchRequest>,
) -> Result<Json<SavedSearch>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let row =
        saved_searches::update(&mut conn, auth.account_id, id, &body.name, &body.query).await?;
    Ok(Json(row))
}

/// Delete a saved search.
///
/// Deleting an import-created saved search removes the shortcut only. The
/// `imports` row it pointed at is the account's permanent record of that
/// run and is never touched here.
#[utoipa::path(
    delete,
    path = "/v1/saved-searches/{id}",
    tag = "Saved searches",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Saved search id")),
    responses(
        (status = 204, description = "Saved search deleted"),
    )
)]
pub(crate) async fn delete_saved_search(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    saved_searches::delete(&mut conn, auth.account_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::problem::ProblemType;
    use crate::test_support::{
        delete_status, expect_problem, fixture_with_account, get_json, get_raw, get_status,
        patch_json, post_created_json, register_via_api,
    };

    #[tokio::test]
    async fn saved_searches_list_as_items_and_each_write_answers_the_row_or_204() {
        let (fixture, user) = fixture_with_account().await;
        let state = fixture.state.clone();

        let (location, created): (String, serde_json::Value) = post_created_json(
            &state,
            "/v1/saved-searches",
            &user.token,
            serde_json::json!({ "name": "Family", "query": "group:Family" }),
        )
        .await;
        assert_eq!(created["name"], "Family");
        assert!(created["id"].is_i64());
        assert!(created.get("savedSearch").is_none() && created.get("savedSearches").is_none());
        let id = created["id"].as_i64().unwrap();
        assert_eq!(location, format!("/v1/saved-searches/{id}"));

        let renamed: serde_json::Value = patch_json(
            &state,
            &format!("/v1/saved-searches/{id}"),
            &user.token,
            serde_json::json!({ "name": "Kin", "query": "group:Family" }),
        )
        .await;
        assert_eq!(renamed["name"], "Kin");

        let list: serde_json::Value = get_json(&state, "/v1/saved-searches", &user.token).await;
        assert_eq!(list["items"][0]["name"], "Kin");
        assert!(list.get("savedSearches").is_none());

        let status = delete_status(&state, &format!("/v1/saved-searches/{id}"), &user.token).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let list: serde_json::Value = get_json(&state, "/v1/saved-searches", &user.token).await;
        assert_eq!(list["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn a_curated_list_is_a_page_like_every_other_list() {
        let (fixture, user) = fixture_with_account().await;
        let state = fixture.state.clone();
        for name in ["Anna", "Bess", "Cleo"] {
            let _: (String, serde_json::Value) = post_created_json(
                &state,
                "/v1/saved-searches",
                &user.token,
                serde_json::json!({ "name": name, "query": "from:me" }),
            )
            .await;
        }

        let page: serde_json::Value =
            get_json(&state, "/v1/saved-searches?limit=2&offset=1", &user.token).await;
        assert_eq!(
            page["total"],
            serde_json::json!(3),
            "the whole set, not the page"
        );
        assert_eq!(page["limit"], serde_json::json!(2));
        assert_eq!(page["offset"], serde_json::json!(1));
        let names: Vec<&str> = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Bess", "Cleo"]);

        let past_the_end: serde_json::Value =
            get_json(&state, "/v1/saved-searches?offset=99", &user.token).await;
        assert_eq!(past_the_end["items"].as_array().unwrap().len(), 0);
        assert_eq!(past_the_end["total"], serde_json::json!(3));
    }

    #[tokio::test]
    async fn a_saved_search_is_read_at_its_location_as_its_list_shows_it() {
        let (fixture, user) = fixture_with_account().await;
        let state = fixture.state.clone();
        let (location, created): (String, serde_json::Value) = post_created_json(
            &state,
            "/v1/saved-searches",
            &user.token,
            serde_json::json!({ "name": " Family ", "query": " group:Family " }),
        )
        .await;

        let read: serde_json::Value = get_json(&state, &location, &user.token).await;
        let list: serde_json::Value = get_json(&state, "/v1/saved-searches", &user.token).await;
        assert_eq!(read, list["items"][0], "GET {location}");
        assert_eq!(read, created, "GET {location}");
    }

    #[tokio::test]
    async fn another_accounts_saved_search_answers_404_to_get() {
        let (fixture, user) = fixture_with_account().await;
        let state = fixture.state.clone();
        let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
        let (location, _): (String, serde_json::Value) = post_created_json(
            &state,
            "/v1/saved-searches",
            &bob.token,
            serde_json::json!({ "name": "Family", "query": "group:Family" }),
        )
        .await;

        let (status, text) = get_raw(&state, &location, &user.token).await;
        expect_problem(status, &text, ProblemType::NotFound);
        assert_eq!(
            get_status(&state, &location, &bob.token).await,
            StatusCode::OK
        );
    }
}
