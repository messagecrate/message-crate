//! Reading a request body: its media type, its declared length, the time
//! limits every body is held to, and reading it into memory or to a file
//! within a cap.

use std::path::Path;

use axum::http::{HeaderMap, header};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::api_error::ApiError;

/// The media type from `Content-Type` without its parameters.
pub(crate) fn content_type_base(headers: &HeaderMap) -> Option<&str> {
    let ct = headers.get(header::CONTENT_TYPE)?.to_str().ok()?;
    Some(ct.split(';').next().unwrap_or(ct).trim())
}

/// The upload's declared media type, or `None` when it is missing or the generic octet-stream.
pub(crate) fn upload_content_type(headers: &HeaderMap) -> Option<String> {
    let base = content_type_base(headers)?;
    if base.is_empty() || base.eq_ignore_ascii_case("application/octet-stream") {
        None
    } else {
        Some(base.to_string())
    }
}

/// Whether the request's `Content-Length` declares a body over `limit`
/// bytes. A request that declares no length, or one that does not read as a
/// number, is not over: its body is cut off as it is read.
pub(crate) fn declared_over(headers: &HeaderMap, limit: usize) -> bool {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|bytes| bytes > limit as u64)
}

/// How long the server waits for one request body to arrive. Every body
/// holds a connection while it arrives, an upload (an import batch, an
/// attachment, or a part of one) holds a file too, and an import batch holds
/// one of the two import slots, so a client that stops sending part-way,
/// such as a laptop that goes to sleep, or one that sends a byte now and
/// then, must not hold them for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyTimeLimits {
    /// How long a body may go with no byte arriving: `[server]
    /// body_stall_secs`.
    pub(crate) stall: std::time::Duration,
    /// How long a whole body may take to arrive, from the start of the read:
    /// `[server] body_piece_secs`.
    pub(crate) piece: std::time::Duration,
}

impl BodyTimeLimits {
    /// The limits `server` sets.
    pub fn of(server: &crate::config::ServerConfig) -> Self {
        Self {
            stall: std::time::Duration::from_secs(server.body_stall_secs),
            piece: std::time::Duration::from_secs(server.body_piece_secs),
        }
    }
}

/// Why the read of a body ended before the body did.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BodyTooSlow {
    /// Nothing arrived for [`BodyTimeLimits::stall`].
    #[error(
        "no bytes of the body arrived for {} seconds, so the server stopped reading it and stored none of it",
        .0.as_secs()
    )]
    Stalled(std::time::Duration),
    /// The body was still arriving at [`BodyTimeLimits::piece`].
    #[error(
        "the body was still arriving {} seconds after it started, so the server stopped reading it and stored none of it",
        .0.as_secs()
    )]
    Overdue(std::time::Duration),
}

/// `body`, ending with a [`BodyTooSlow`] error once
/// `limits.stall` passes with no chunk arriving, or `limits.piece` passes
/// from the start of the read. Reading it answers `408 Request Timeout`
/// (`body-stalled`) then ([`body_read_error`]).
pub(crate) fn body_time_bounded(
    body: axum::body::Body,
    limits: BodyTimeLimits,
) -> axum::body::Body {
    type BoxError = Box<dyn std::error::Error + Send + Sync>;
    let deadline = tokio::time::Instant::now() + limits.piece;
    let chunks =
        futures_util::stream::unfold(Some(body.into_data_stream()), move |stream| async move {
            let mut stream = stream?;
            let idle_end = tokio::time::Instant::now() + limits.stall;
            let (wait_until, too_slow) = if idle_end < deadline {
                (idle_end, BodyTooSlow::Stalled(limits.stall))
            } else {
                (deadline, BodyTooSlow::Overdue(limits.piece))
            };
            match tokio::time::timeout_at(wait_until, stream.next()).await {
                Ok(Some(chunk)) => Some((chunk.map_err(BoxError::from), Some(stream))),
                Ok(None) => None,
                Err(_) => Some((Err(BoxError::from(too_slow)), None)),
            }
        });
    axum::body::Body::from_stream(chunks)
}

/// True when the request body is JSON Lines (one JSON object per line).
pub(crate) fn is_jsonl_content_type(base: &str) -> bool {
    base.eq_ignore_ascii_case("application/jsonl")
        || base.eq_ignore_ascii_case("application/x-ndjson")
}

/// The problem a failed read of a request body answers. `limit_request_body`
/// wraps a body with no `Content-Length` in `Limited`, which ends the stream
/// with a `LengthLimitError` once the body passes its cap: that body is too
/// large. [`body_time_bounded`] ends it with [`BodyTooSlow`] once it
/// stalls or runs past its time: the connection failed, not the data, so it
/// answers `body-stalled`. Any other failure leaves the body unreadable.
fn body_read_error(error: axum::Error) -> ApiError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    while let Some(cause) = source {
        if cause.is::<http_body_util::LengthLimitError>() {
            return ApiError::PayloadTooLarge;
        }
        source = cause.source();
    }
    body_too_slow(&error)
        .unwrap_or_else(|| ApiError::MalformedBody(format!("failed to read body: {error}")))
}

/// The `body-stalled` problem when [`body_time_bounded`] ended the body
/// whose read failed with `error`, found anywhere in its chain of causes, so
/// a body read by Axum's own extractors answers it as well as one the
/// server reads itself.
pub(crate) fn body_too_slow(error: &(dyn std::error::Error + 'static)) -> Option<ApiError> {
    let mut source = Some(error);
    while let Some(cause) = source {
        if let Some(too_slow) = cause.downcast_ref::<BodyTooSlow>() {
            return Some(ApiError::BodyStalled(format!(
                "{too_slow}: the connection stopped delivering the body and nothing is wrong \
                 with the data, so sending the same request again can succeed"
            )));
        }
        source = cause.source();
    }
    None
}

/// Read the whole request body into memory, failing once it passes `max_bytes`.
pub(crate) async fn read_body_limited(
    body: axum::body::Body,
    max_bytes: usize,
) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(body_read_error)?;
        if out.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::PayloadTooLarge);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Drain request body without retaining it (used when asset already exists).
pub(crate) async fn discard_body(
    body: axum::body::Body,
    max_body_bytes: usize,
) -> Result<(), ApiError> {
    let mut stream = body.into_data_stream();
    let mut seen = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(body_read_error)?;
        seen = seen.saturating_add(chunk.len());
        if seen > max_body_bytes {
            return Err(ApiError::PayloadTooLarge);
        }
    }
    Ok(())
}

/// Create `dest` and its parent directories for an upload.
async fn create_dest_file(dest: &Path) -> Result<tokio::fs::File, ApiError> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("mkdir {}: {e}", parent.display())))?;
    }
    tokio::fs::File::create(dest)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("create {}: {e}", dest.display())))
}

/// Stream a request body to `dest`, failing once it passes `max_body_bytes`. Returns the bytes written.
pub(crate) async fn stream_body_to_file(
    body: axum::body::Body,
    dest: &Path,
    max_body_bytes: usize,
) -> Result<u64, ApiError> {
    let file = create_dest_file(dest).await?;
    stream_body_into(body, file, dest, max_body_bytes).await
}

/// Stream a request body into `file`, already open at `dest`, failing once
/// it passes `max_body_bytes`, and return the bytes written. The body is
/// held one chunk at a time, never whole.
pub(crate) async fn stream_body_into(
    body: axum::body::Body,
    mut file: tokio::fs::File,
    dest: &Path,
    max_body_bytes: usize,
) -> Result<u64, ApiError> {
    let mut written = 0u64;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(body_read_error)?;
        written = written.saturating_add(chunk.len() as u64);
        if written > max_body_bytes as u64 {
            return Err(ApiError::PayloadTooLarge);
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("write {}: {e}", dest.display())))?;
    }
    file.flush()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("flush {}: {e}", dest.display())))?;
    Ok(written)
}

#[cfg(test)]
mod tests;
