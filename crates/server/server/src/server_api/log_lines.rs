//! `GET /v1/server/log-lines`: the server's log lines, newest first, the
//! owner's alone. The files are written by `crate::logging`, and read here
//! a page at a time from the id of the last line the client holds
//! (`docs/architecture/server-log.md`).

use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::extract::{Json, Query};
use crate::logging::{LogLevel, LogLine, LogLinesQuery, log_dir, read_lines};
use crate::paging::{DEFAULT_LIST_LIMIT, page_params};
use crate::server::{ApiError, AppState, Owner};

/// Query string of `GET /v1/server/log-lines`.
#[derive(Debug, Deserialize)]
pub(crate) struct ListLogLinesQuery {
    #[serde(default)]
    pub(crate) level: Option<LogLevel>,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) after: Option<i64>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

/// A page of the server's log lines, newest first.
///
/// Not a `Page`: the log is written while it is read, so a count and an
/// offset from the newest line would move under the reader, and counting a
/// 250 MB log for every page is the cost a line's id saves
/// (`docs/architecture/http-api.md`, "Lists").
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ListLogLinesResponse {
    /// The lines, newest first.
    pub items: Vec<LogLine>,
    /// The most lines this page could hold.
    pub limit: usize,
    /// Whether older lines match too. Read them with `after` set to the id of
    /// the last line here.
    pub has_more: bool,
}

/// Read the server's log lines, newest first.
///
/// `after` reads the lines older than the line with that id, so the page
/// after this one starts at the id of its last line, and lines the server
/// writes in between do not move it. `level` keeps the lines at that level
/// and the more severe ones. `text` keeps the lines whose text, after the
/// time and the level, holds it, ignoring case. A line never holds a
/// password, a token, message text or a contact's name or identities. The
/// owner's alone, because the log is about the whole installation.
#[utoipa::path(
    get,
    path = "/v1/server/log-lines",
    tag = "Server",
    security(("session" = ["owner"])),
    params(
        ("level" = Option<LogLevel>, Query, description = "Only lines at this level or more severe: `error`, `warn` (errors too), `info`, `debug`, `trace` (every line)"),
        ("text" = Option<String>, Query, description = "Only lines whose text, after the time and the level, holds this, ignoring case"),
        ("after" = Option<i64>, Query, description = "Only lines older than the line with this id: the id of the last line of the page before"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500")
    ),
    responses(
        (status = 200, body = ListLogLinesResponse),
    )
)]
pub(crate) async fn list_log_lines(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Query(query): Query<ListLogLinesQuery>,
) -> Result<Json<ListLogLinesResponse>, ApiError> {
    let params = page_params(query.limit, None, DEFAULT_LIST_LIMIT, None)?;
    if query.after.is_some_and(|after| after < 0) {
        return Err(ApiError::validation(
            "after must be the id of a line, which is never negative",
        ));
    }
    let dir = log_dir(&state.cfg.paths.data_dir);
    let lines_query = LogLinesQuery {
        after: query.after,
        level: query.level,
        text: query.text,
        limit: params.limit,
    };
    let (items, has_more) = super::read_log("reading the server's log", move || {
        read_lines(&dir, &lines_query)
    })
    .await?;
    Ok(Json(ListLogLinesResponse {
        items,
        limit: params.limit,
        has_more,
    }))
}

#[cfg(test)]
mod tests;
