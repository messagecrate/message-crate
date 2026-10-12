//! An Import Run driven over HTTP one step at a time, as an Upload drives
//! it: [`start_import_run`], [`post_import_batch`] for each batch, then
//! [`complete_import_run`]. A test that checks something between two steps,
//! or uploads files before the batch that names them, calls the steps it
//! needs and does its own work between them.

use super::{post_created_json, post_json, post_raw};
use crate::server::AppState;

/// Start an Import Run as `token`'s account, `POST /v1/imports` with
/// `request` as the body, and answer the run's id with the body the
/// `201 Created` carried.
pub async fn start_import_run(
    state: &AppState,
    token: &str,
    request: serde_json::Value,
) -> (i64, serde_json::Value) {
    let (_, created): (String, serde_json::Value) =
        post_created_json(state, "/v1/imports", token, request).await;
    let id = created["id"]
        .as_i64()
        .unwrap_or_else(|| panic!("a started Import Run has an id: {created}"));
    (id, created)
}

/// Post `body` as a batch of Import Run `run`, which must answer `200 OK`.
pub async fn post_import_batch(state: &AppState, token: &str, run: i64, body: String) {
    let (status, text) = post_raw(
        state,
        &format!("/v1/imports/{run}/batches"),
        token,
        "application/jsonl",
        body,
    )
    .await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "batch of run {run}: {text}"
    );
}

/// Complete Import Run `run`, so its account may start another, and answer
/// the run as the route returned it, which must say `completed`.
pub async fn complete_import_run(state: &AppState, token: &str, run: i64) -> serde_json::Value {
    let completed: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{run}/complete"),
        token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
    assert_eq!(completed["status"], "completed", "{completed}");
    completed
}
