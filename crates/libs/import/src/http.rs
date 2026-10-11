//! HTTP helpers for attachment upload and JSON Lines message import.
//!
//! JSON Lines means one JSON object per line. Calls are blocking so they can
//! run on worker threads without an async runtime. Login lives in
//! [`message_crate_http::auth_check`]; the session type here is
//! [`message_crate_http::HttpSession`].

use message_crate_api_types::ImportMode;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use message_crate_http::{
    HttpError, error_sentence, looks_like_html, ok_json, read_body, session_refused, trim_base_url,
};
use reqwest::Method;
use serde::Deserialize;

pub use message_crate_http::HttpSession;

use crate::run::Session;

/// The server's `Asset`: the answer to `HEAD` or `PUT /v1/assets/{sha256}`
/// and to completing a multipart upload. Only `already_present` is read.
#[derive(Debug, Deserialize)]
pub struct Asset {
    #[serde(default)]
    pub already_present: bool,
}

/// The answer to `POST /v1/imports/{id}/batches`. Only the counts the
/// report needs are read.
#[derive(Debug, Deserialize)]
pub struct CreateImportBatchResponse {
    #[serde(default)]
    pub messages: u64,
    #[serde(default)]
    pub messages_appended: u64,
    #[serde(default)]
    pub messages_deduped: u64,
}

/// One attachment to upload: where it is on disk, what it is, and the size
/// above which it goes up in parts.
pub(crate) struct AssetUpload<'a> {
    pub sha256: &'a str,
    pub file: &'a Path,
    pub mime: Option<&'a str>,
    /// Files larger than this use multipart upload (typically [`crate::run::MAX_PROXY_BODY_BYTES`]).
    pub multipart_threshold: usize,
}

/// How an Import Run ended, for `/v1/imports/{id}/complete`.
pub(crate) struct ImportOutcome<'a> {
    /// `completed`, `completed_with_issues`, or `failed`.
    pub status: &'a str,
    pub bytes_uploaded: u64,
}

/// The answer to `POST /v1/imports`: the new Import Run's id.
#[derive(Debug, Deserialize)]
struct CreateImportResponse {
    id: i64,
}

/// The Import Run `POST /v1/imports/{id}/complete` answers. Only `id` is read: it
/// names the run the server closed.
#[derive(Debug, Deserialize)]
struct ImportRun {
    id: i64,
}

/// The answer to `POST /v1/assets/{sha256}/uploads`.
#[derive(Debug, Deserialize)]
struct CreateAssetUploadResponse {
    #[serde(default)]
    upload_id: Option<String>,
    #[serde(default)]
    part_size: Option<usize>,
    #[serde(default)]
    already_present: bool,
}

/// True for HTTP 413 or a proxy HTML page that says the body was too large.
fn looks_like_payload_too_large(status: reqwest::StatusCode, body: &str) -> bool {
    status.as_u16() == 413
        || body.contains("413 Payload Too Large")
        || body.contains("413 Request Entity Too Large")
        || (looks_like_html(body) && body.to_ascii_lowercase().contains("payload too large"))
}

/// Human-readable 413 error that names the request kind and optional byte size.
fn payload_too_large_message(kind: &str, bytes: Option<usize>) -> String {
    let size = bytes
        .map(|n| format!(" (request was {n} bytes)"))
        .unwrap_or_default();
    format!(
        "{kind} rejected: HTTP 413 Payload Too Large{size}. \
         Cloudflare Free/Pro caps proxied uploads at ~100 MB. \
         message-crate-import chunks message imports under 64 MiB and large assets via multipart; \
         if this still fails, raise nginx client_max_body_size for /v1 (need ≥100m for 64 MiB parts) \
         or tunnel to the server on :8080."
    )
}

/// Build `{base}/v1/assets/...` with extra path segments (percent-encoded).
/// An attachment is addressed by its SHA-256 alone. The account is not a
/// parameter, because the session token names it.
fn asset_url(base_url: &str, segments: &[&str]) -> Result<reqwest::Url> {
    let base = trim_base_url(base_url);
    let mut url =
        reqwest::Url::parse(base).with_context(|| format!("invalid server address {base}"))?;
    url.path_segments_mut()
        .map_err(|()| anyhow!("invalid server address {base}"))?
        .pop_if_empty()
        .extend(["v1", "assets"].into_iter().chain(segments.iter().copied()));
    Ok(url)
}

impl Session {
    /// `/v1/assets/...` URL under this session's account.
    fn asset_url(&self, segments: &[&str]) -> Result<reqwest::Url> {
        asset_url(&self.url, segments)
    }

    /// Whether the server already holds the attachment with this digest:
    /// `true` for a 2xx, `false` for a 404. A HEAD reply carries no body, so
    /// the status is the whole answer: the problem type the server chose
    /// cannot be read, and each error message names every cause the
    /// reference lists for its status.
    ///
    /// # Errors
    ///
    /// Returns an error when the server does not accept the session
    /// (`401 Unauthorized`, because it expired or was ended), refuses the account
    /// (`403 Forbidden`: disabled, or neither import nor export), or the
    /// request fails in any other way.
    pub(crate) fn head_asset(&self, sha256: &str) -> Result<bool> {
        let what = "Asset check";
        let url = self.asset_url(&[sha256])?;
        let response = self
            .http
            .request_url(Method::HEAD, url, &self.token)
            .timeout(Duration::from_secs(15))
            .send()
            .with_context(|| format!("{what} failed"))?;
        let status = response.status();
        match status.as_u16() {
            404 => return Ok(false),
            401 => {
                return Err(session_refused(what).into());
            }
            403 => {
                return Err(HttpError::new(
                    403,
                    "The server refused this account access to attachments (403 Forbidden): \
                     the account is disabled, or it may neither import nor export.",
                )
                .into());
            }
            _ => {}
        }
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            return Err(HttpError::new(
                status.as_u16(),
                format!("{what} failed (HTTP {status}): {}", error_sentence(&text)),
            )
            .into());
        }
        Ok(true)
    }

    /// Upload one attachment: in one PUT, or in parts when the file is larger
    /// than its threshold.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or the server rejects it;
    /// a 413 says how large a body the server accepts.
    pub(crate) fn put_asset(&self, asset: &AssetUpload<'_>) -> Result<Asset> {
        let what = "Asset upload";
        let file_len = std::fs::metadata(asset.file)
            .with_context(|| format!("stat {}", asset.file.display()))?
            .len();
        if file_len > asset.multipart_threshold as u64 {
            return self.put_asset_multipart(asset, file_len);
        }

        let url = self.asset_url(&[asset.sha256])?;
        let bytes =
            std::fs::read(asset.file).with_context(|| format!("read {}", asset.file.display()))?;
        let content_type = asset
            .mime
            .filter(|mime| !mime.is_empty())
            .unwrap_or("application/octet-stream");
        let response = self
            .http
            .request_url(Method::PUT, url, &self.token)
            .timeout(Duration::from_secs(600))
            .header("Content-Type", content_type)
            .body(bytes)
            .send()
            .with_context(|| format!("{what} failed"))?;
        let (status, text) = read_body(what, response)?;
        if looks_like_payload_too_large(status, &text) {
            return Err(HttpError::new(
                413,
                payload_too_large_message(what, Some(file_len as usize)),
            )
            .into());
        }
        ok_json::<Asset>(what, status, &text)
    }

    /// Upload in parts: open a multipart upload, send each part, complete
    /// it. Any failure after the upload opened, reading the file included,
    /// aborts the upload on the server, which counts every open upload
    /// against the account's limit until it ends.
    fn put_asset_multipart(&self, asset: &AssetUpload<'_>, file_len: u64) -> Result<Asset> {
        let Some(upload) = MultipartUpload::start(self, asset, file_len)? else {
            return Ok(Asset {
                already_present: true,
            });
        };
        let sent = Self::send_parts_and_complete(&upload, asset.file, file_len);
        if sent.is_err() {
            upload.abort();
        }
        sent
    }

    /// Read `file` a part at a time, send each part of `upload`, and complete it.
    fn send_parts_and_complete(
        upload: &MultipartUpload<'_>,
        file: &Path,
        file_len: u64,
    ) -> Result<Asset> {
        let mut reader = File::open(file).with_context(|| format!("open {}", file.display()))?;
        let mut part: u32 = 1;
        let mut remaining = file_len;
        while remaining > 0 {
            let this_len = remaining.min(upload.part_size as u64) as usize;
            let mut buf = vec![0u8; this_len];
            reader
                .read_exact(&mut buf)
                .with_context(|| format!("read part {part} from {}", file.display()))?;
            upload.send_part(part, buf)?;
            remaining -= this_len as u64;
            part += 1;
        }
        upload.complete()
    }

    /// POST one JSON Lines batch into the Import Run at
    /// `/v1/imports/{id}/batches`. The run's row says the source and the
    /// mode; the request carries only the body.
    ///
    /// # Errors
    ///
    /// Returns a 413 before sending when the body is over the proxy limit,
    /// and the server's error otherwise.
    pub(crate) fn post_import(
        &self,
        import_id: i64,
        ndjson: Vec<u8>,
    ) -> Result<CreateImportBatchResponse> {
        let what = format!("Import Run {import_id} batch");
        let body_len = ndjson.len();
        if body_len > crate::run::MAX_PROXY_BODY_BYTES {
            return Err(
                HttpError::new(413, payload_too_large_message(&what, Some(body_len))).into(),
            );
        }
        let path = format!("/v1/imports/{import_id}/batches");
        let response = self
            .http
            .server_request(Method::POST, &self.url, &path, &self.token)
            .timeout(Duration::from_secs(600))
            .header("Content-Type", "application/jsonl")
            .body(ndjson)
            .send()
            .with_context(|| format!("{what} failed"))?;
        let (status, text) = read_body(&what, response)?;
        if looks_like_payload_too_large(status, &text) {
            return Err(
                HttpError::new(413, payload_too_large_message(&what, Some(body_len))).into(),
            );
        }
        ok_json::<CreateImportBatchResponse>(&what, status, &text)
    }

    /// Create an Import Run on the server and return its id. Every batch is
    /// posted into it; the bearer token names the account.
    ///
    /// # Errors
    ///
    /// Returns an error when the server refuses, which includes an account
    /// that already has a running Import Run.
    pub(crate) fn start_import(
        &self,
        source: &str,
        mode: ImportMode,
        tool: Option<&str>,
        phone_country: Option<&str>,
    ) -> Result<i64> {
        let what = "Import Run start";
        let mut body = serde_json::json!({
            "source": source,
            "mode": mode,
        });
        if let Some(tool) = tool {
            body["tool"] = serde_json::Value::String(tool.to_string());
        }
        if let Some(phone_country) = phone_country {
            body["phone_country"] = serde_json::Value::String(phone_country.to_string());
        }
        let response = self
            .http
            .server_request(Method::POST, &self.url, "/v1/imports", &self.token)
            .timeout(Duration::from_secs(60))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .with_context(|| format!("{what} failed"))?;
        let (status, text) = read_body(what, response)?;
        let parsed: CreateImportResponse = ok_json(what, status, &text)?;
        Ok(parsed.id)
    }

    /// Record how the Import Run ended.
    ///
    /// # Errors
    ///
    /// Returns an error when the server refuses.
    pub(crate) fn complete_import(
        &self,
        import_id: i64,
        outcome: &ImportOutcome<'_>,
    ) -> Result<()> {
        let what = format!("Import Run {import_id} completion");
        let body = serde_json::json!({
            "status": outcome.status,
            "bytes_uploaded": outcome.bytes_uploaded,
        });
        let response = self
            .http
            .server_request(
                Method::POST,
                &self.url,
                &format!("/v1/imports/{import_id}/complete"),
                &self.token,
            )
            .timeout(Duration::from_secs(60))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .with_context(|| format!("{what} failed"))?;
        let (status, text) = read_body(&what, response)?;
        let closed: ImportRun = ok_json(&what, status, &text)?;
        if closed.id != import_id {
            return Err(anyhow!(
                "the server closed Import Run {} when asked to complete Import Run {import_id}",
                closed.id
            ));
        }
        Ok(())
    }
}

/// A multipart upload the server has opened for one attachment.
struct MultipartUpload<'a> {
    session: &'a Session,
    sha256: &'a str,
    upload_id: String,
    /// Bytes per part, as the server asked.
    part_size: usize,
}

impl<'a> MultipartUpload<'a> {
    /// Open the upload. `None` when the server says it already has the file.
    ///
    /// # Errors
    ///
    /// Returns an error when the server refuses or its reply lacks an upload
    /// id or part size.
    fn start(session: &'a Session, asset: &AssetUpload<'a>, file_len: u64) -> Result<Option<Self>> {
        let what = "Asset upload start";
        let start_url = session.asset_url(&[asset.sha256, "uploads"])?;
        let mut start_body = serde_json::json!({ "bytes": file_len });
        if let Some(mime) = asset.mime.filter(|m| !m.is_empty()) {
            start_body["mime"] = serde_json::Value::String(mime.to_string());
        }
        let response = session
            .http
            .request_url(Method::POST, start_url, &session.token)
            .timeout(Duration::from_secs(30))
            .header("Content-Type", "application/json")
            .json(&start_body)
            .send()
            .with_context(|| format!("{what} failed"))?;
        let (status, text) = read_body(what, response)?;
        if looks_like_payload_too_large(status, &text) {
            return Err(HttpError::new(413, payload_too_large_message(what, None)).into());
        }
        let started: CreateAssetUploadResponse = ok_json(what, status, &text)?;
        if started.already_present {
            return Ok(None);
        }
        let upload_id = started
            .upload_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("the server's answer to {what} has no upload id"))?;
        let part_size = started
            .part_size
            .filter(|&n| n > 0)
            .ok_or_else(|| anyhow!("the server's answer to {what} has no part size"))?;
        Ok(Some(Self {
            session,
            sha256: asset.sha256,
            upload_id,
            part_size,
        }))
    }

    /// This upload's URL with `tail` appended: `parts/N`, `complete`, or nothing.
    fn url(&self, tail: &[&str]) -> Result<reqwest::Url> {
        let mut segments = vec![self.sha256, "uploads", self.upload_id.as_str()];
        segments.extend_from_slice(tail);
        self.session.asset_url(&segments)
    }

    /// PUT one part.
    ///
    /// # Errors
    ///
    /// Names the part and, for a 413, its size.
    fn send_part(&self, part: u32, buf: Vec<u8>) -> Result<()> {
        let part_len = buf.len();
        let what = format!("Asset upload part {part}");
        let part_url = self.url(&["parts", &part.to_string()])?;
        let response = self
            .session
            .http
            .request_url(Method::PUT, part_url, &self.session.token)
            .timeout(Duration::from_secs(600))
            .header("Content-Type", "application/octet-stream")
            .body(buf)
            .send()
            .with_context(|| format!("{what} failed"))?;
        let status = response.status();
        let text = response.text().unwrap_or_default();
        if looks_like_payload_too_large(status, &text) {
            return Err(
                HttpError::new(413, payload_too_large_message(&what, Some(part_len))).into(),
            );
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(session_refused(&what).into());
        }
        if !status.is_success() {
            return Err(HttpError::new(
                status.as_u16(),
                format!("{what} failed (HTTP {status}): {}", error_sentence(&text)),
            )
            .into());
        }
        Ok(())
    }

    /// Tell the server every part is in and read its reply.
    fn complete(&self) -> Result<Asset> {
        let what = "Asset upload completion";
        let complete_url = self.url(&["complete"])?;
        let response = self
            .session
            .http
            .request_url(Method::POST, complete_url, &self.session.token)
            .timeout(Duration::from_secs(600))
            .send()
            .with_context(|| format!("{what} failed"))?;
        let (status, text) = read_body(what, response)?;
        ok_json::<Asset>(what, status, &text)
    }

    /// Drop the upload on the server. Best effort: a failed abort only leaves
    /// a stale upload for the server to expire.
    fn abort(&self) {
        let Ok(url) = self.url(&[]) else {
            return;
        };
        let _ = self
            .session
            .http
            .request_url(Method::DELETE, url, &self.session.token)
            .timeout(Duration::from_secs(30))
            .send();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use message_crate_http::AuthInfo;

    const DIGEST: &str = "aa11";

    fn session(url: String) -> Session {
        Session {
            http: HttpSession::new().unwrap(),
            url,
            token: "mc-user-test".into(),
            username: "alice".into(),
            auth: AuthInfo {
                account_id: 1,
                username: "alice".into(),
            },
            stop: Default::default(),
            refused: Default::default(),
        }
    }

    /// The message `head_asset` gives when the server answers `HEAD` with `status`.
    fn head_asset_error(status: u16) -> String {
        let server = MockServer::start();
        let _head = server.mock(|when, then| {
            when.method("HEAD").path(format!("/v1/assets/{DIGEST}"));
            then.status(status);
        });
        session(server.base_url())
            .head_asset(DIGEST)
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn head_asset_403_names_the_documented_causes_and_no_username() {
        let message = head_asset_error(403);
        assert!(message.contains("403 Forbidden"), "got {message}");
        assert!(message.contains("disabled"), "got {message}");
        assert!(
            message.contains("neither import nor export"),
            "got {message}"
        );
        assert!(!message.contains("username"), "got {message}");
        assert!(!message.contains("API key"), "got {message}");
    }

    #[test]
    fn head_asset_401_says_to_log_in_again_and_names_no_api_key() {
        let message = head_asset_error(401);
        assert!(message.contains("401 Unauthorized"), "got {message}");
        assert!(message.contains("Log in again"), "got {message}");
        assert!(!message.contains("API key"), "got {message}");
    }

    /// A session that expires during a multipart upload answers 401 to a
    /// part, and the message names the part and says to log in again.
    #[test]
    fn send_part_401_names_the_part_and_says_to_log_in_again() {
        let server = MockServer::start();
        let _part = server.mock(|when, then| {
            when.method("PUT")
                .path(format!("/v1/assets/{DIGEST}/uploads/up-1/parts/2"));
            then.status(401);
        });
        let session = session(server.base_url());
        let upload = MultipartUpload {
            session: &session,
            sha256: DIGEST,
            upload_id: "up-1".into(),
            part_size: 4,
        };
        let err = upload.send_part(2, vec![0; 4]).unwrap_err();
        let message = err.to_string();
        assert!(
            message.starts_with("Asset upload part 2 failed."),
            "got {message}"
        );
        assert!(message.contains("Log in again"), "got {message}");
        assert_eq!(
            message_crate_http::classify_retry(&err),
            message_crate_http::RetryKind::Permanent
        );
    }

    /// A file that is shorter than it was when the upload opened (truncated
    /// after it was staged) fails to read, and the upload is aborted on the
    /// server, so it does not stay open against the account's limit.
    #[test]
    fn a_multipart_upload_whose_file_cannot_be_read_is_aborted() {
        let server = MockServer::start();
        let _start = server.mock(|when, then| {
            when.method(POST)
                .path(format!("/v1/assets/{DIGEST}/uploads"));
            then.status(201)
                .json_body(serde_json::json!({ "upload_id": "up-1", "part_size": 4 }));
        });
        let abort = server.mock(|when, then| {
            when.method(DELETE)
                .path(format!("/v1/assets/{DIGEST}/uploads/up-1"));
            then.status(204);
        });
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("short.bin");
        std::fs::write(&file, b"abc").unwrap();
        let asset = AssetUpload {
            sha256: DIGEST,
            file: &file,
            mime: None,
            multipart_threshold: 1,
        };

        let err = session(server.base_url())
            .put_asset_multipart(&asset, 8)
            .unwrap_err();
        assert!(err.to_string().contains("read part 1"), "{err:#}");
        abort.assert();
    }

    /// A server that answers the completion of one Import Run with another
    /// has not closed the run the Upload holds, so the completion fails and
    /// names both runs.
    #[test]
    fn complete_import_fails_when_the_server_closes_another_run() {
        let server = MockServer::start();
        let _complete = server.mock(|when, then| {
            when.method(POST).path("/v1/imports/7/complete");
            then.status(200).json_body(serde_json::json!({ "id": 9 }));
        });
        let err = session(server.base_url())
            .complete_import(
                7,
                &ImportOutcome {
                    status: "completed",
                    bytes_uploaded: 0,
                },
            )
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "the server closed Import Run 9 when asked to complete Import Run 7"
        );
    }

    #[test]
    fn payload_too_large_mentions_64_mib_import_chunks() {
        let msg = payload_too_large_message("Import Run 42 batch", Some(10));
        assert!(
            msg.contains("imports under 64 MiB"),
            "413 help must name the import chunk size, got {msg}"
        );
    }

    #[test]
    fn asset_url_encodes_segments_and_names_no_source() {
        let url = asset_url("http://127.0.0.1:8080/", &["abc123", "uploads", "up 1"]).unwrap();
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:8080/v1/assets/abc123/uploads/up%201"
        );
    }
}
