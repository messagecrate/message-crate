//! `GET /v1/server/log-files` and `GET /v1/server/log-files/{id}`: the
//! server's log files, listed and downloaded whole, the owner's alone
//! (`docs/architecture/server-log.md`).

use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use tokio::io::AsyncReadExt as _;

use crate::extract::{Json, Path, Query};
use crate::logging::{LogFile, list_files, log_dir, log_file_path};
use crate::paging::{DEFAULT_LIST_LIMIT, Page, page_of, page_params};
use crate::server::{ApiError, AppState, Owner};

/// Query string of `GET /v1/server/log-files`: a page.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ListLogFilesQuery {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
}

/// List the server's log files, newest first.
///
/// The server writes to the newest, and starts the next before a line would
/// carry it past 50 MB. It keeps 5, and deletes the oldest when one more
/// starts. The owner's alone.
#[utoipa::path(
    get,
    path = "/v1/server/log-files",
    tag = "Server",
    security(("session" = ["owner"])),
    params(
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip")
    ),
    responses(
        (status = 200, body = Page<LogFile>),
    )
)]
pub(crate) async fn list_log_files(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Query(query): Query<ListLogFilesQuery>,
) -> Result<Json<Page<LogFile>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let dir = log_dir(&state.cfg.paths.data_dir);
    let files = super::read_log("listing the server's log", move || list_files(&dir)).await?;
    Ok(Json(page_of(files, params)))
}

/// Download one of the server's log files whole, as it is on disk.
///
/// The answer is `text/plain`, an attachment named for the file. The newest
/// file is answered as it stood when the download started; lines written
/// while it downloads are in the next one. The owner's alone.
#[utoipa::path(
    get,
    path = "/v1/server/log-files/{id}",
    tag = "Server",
    security(("session" = ["owner"])),
    params(("id" = i64, Path, description = "The file's id, from `GET /v1/server/log-files`")),
    responses(
        (
            status = 200,
            description = "The file, as an attachment named `server-<id>.log`",
            content_type = "text/plain",
            body = String,
            headers(("Content-Disposition" = String, description = "`attachment; filename=\"server-000001.log\"`, the file's name"))
        ),
    )
)]
pub(crate) async fn get_log_file(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let not_found = || ApiError::NotFound(format!("the server's log has no file {id}"));
    let (path, name) =
        log_file_path(&log_dir(&state.cfg.paths.data_dir), id).ok_or_else(not_found)?;
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(not_found()),
        Err(error) => {
            return Err(ApiError::Internal(
                anyhow::Error::from(error).context("opening a file of the server's log"),
            ));
        }
    };
    let len = file
        .metadata()
        .await
        .map_err(|error| ApiError::Internal(error.into()))?
        .len();
    // Held to the length it had now, so the body matches `Content-Length`
    // while the server goes on writing to the newest file.
    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file.take(len)));
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "text/plain; charset=utf-8".to_string(),
            ),
            (header::CONTENT_LENGTH, len.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests;
