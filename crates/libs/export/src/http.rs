//! HTTP helpers for an Export Run: creating it, paging its messages, closing
//! it, and fetching Assets.
//!
//! Calls are blocking so they can run on worker threads without an async
//! runtime. The session type is [`message_crate_http::HttpSession`].
//!
//! The shapes are `message-crate-api-types`, the same definitions the server
//! serializes from. This file used to mirror them by hand, and three defects
//! shipped because the mirror and the server drifted apart with nothing to
//! notice.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use message_crate_http::{HttpError, error_sentence, ok_json, session_refused, trim_base_url};
use reqwest::Method;
use sha2::{Digest, Sha256};

use crate::part_file::write_asset;

use message_crate_api_types::{ExportRun, ExportScope, Message, Page};

pub use message_crate_http::HttpSession;

/// The body of `POST /v1/exports`.
#[derive(Debug, serde::Serialize)]
struct CreateExportBody<'a> {
    scope: &'a ExportScope,
    tool: &'a str,
}

/// `POST /v1/exports`: record a run for `scope` and return it with the
/// counts the server computed.
///
/// # Errors
///
/// Returns an error when the request fails, the server refuses the scope, or
/// the body is not a run.
pub fn create_export(
    http: &HttpSession,
    base_url: &str,
    token: &str,
    scope: &ExportScope,
    tool: &str,
) -> Result<ExportRun> {
    let what = "Export Run start";
    let body = serde_json::to_vec(&CreateExportBody { scope, tool })?;
    let response = http
        .server_request(Method::POST, base_url, "/v1/exports", token)
        .header("Content-Type", "application/json")
        .body(body)
        .timeout(Duration::from_secs(120))
        .send()
        .with_context(|| format!("{what} failed"))?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    ok_json(what, status, &text)
}

/// Arguments for [`export_messages`].
pub(crate) struct ExportMessagesArgs<'a> {
    pub base_url: &'a str,
    pub token: &'a str,
    /// The run whose messages are paged.
    pub export_id: i64,
    pub limit: usize,
    pub offset: usize,
}

/// Fetch one `Page<Message>` from `GET /v1/exports/{id}/messages`.
///
/// # Errors
///
/// Returns an error when the request fails or the body is not valid JSON.
pub fn export_messages(http: &HttpSession, args: ExportMessagesArgs<'_>) -> Result<Page<Message>> {
    let ExportMessagesArgs {
        base_url,
        token,
        export_id,
        limit,
        offset,
    } = args;
    let what = format!("Export Run {export_id} page");
    let path = format!("/v1/exports/{export_id}/messages");
    let response = http
        .server_request(Method::GET, base_url, &path, token)
        .query(&[("limit", limit.to_string()), ("offset", offset.to_string())])
        .timeout(Duration::from_secs(120))
        .send()
        .with_context(|| format!("{what} failed"))?;

    let status = response.status();
    let body = response.text().unwrap_or_default();
    ok_json(&what, status, &body)
}

/// How an Export ends its run on the server: with every file written, or
/// given up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseAction {
    Complete,
    Cancel,
}

impl CloseAction {
    /// The last segment of the route that closes the run this way.
    fn path_segment(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Cancel => "cancel",
        }
    }

    /// The request's name in an error, after "Export Run 7".
    fn noun(self) -> &'static str {
        match self {
            Self::Complete => "completion",
            Self::Cancel => "cancellation",
        }
    }
}

/// `POST /v1/exports/{id}/complete` or `.../cancel`: close the run and
/// return it as it now stands.
///
/// # Errors
///
/// Returns an error when the request fails, the run is already closed, or
/// the body is not a run.
pub fn close_export(
    http: &HttpSession,
    base_url: &str,
    token: &str,
    export_id: i64,
    action: CloseAction,
) -> Result<ExportRun> {
    let what = format!("Export Run {export_id} {}", action.noun());
    let path = format!("/v1/exports/{export_id}/{}", action.path_segment());
    let response = http
        .server_request(Method::POST, base_url, &path, token)
        .timeout(Duration::from_secs(120))
        .send()
        .with_context(|| format!("{what} failed"))?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    ok_json(&what, status, &text)
}

/// How long one Asset fetch may take, from the request to its last byte.
/// An Asset can be a video of several hundred megabytes, so five minutes
/// leaves a slow link room to finish one; the other calls here move one
/// page of JSON and allow 120 seconds.
const ASSET_READ_TIMEOUT: Duration = Duration::from_secs(300);

/// Fetch one Asset by its SHA-256 fingerprint to `dest`.
///
/// Bytes are written to the Asset's own temporary file beside `dest` first
/// ([`write_asset`]) and hashed as they are written. The file is renamed
/// into place only when their SHA-256 is `sha256`, so neither a crash nor an
/// answer that is not the Asset leaves a file at the destination, and
/// two Assets fetched at once never write one temporary file.
///
/// # Errors
///
/// Returns an error when the fingerprint is not 64 hex characters, the server
/// returns 404 or another failure, the bytes' SHA-256 is not `sha256`, or the
/// file cannot be written. The temporary file is removed on every error after
/// it was created. Each error after the fingerprint check names the Asset.
///
/// Returns the number of bytes written to `dest`.
pub fn fetch_asset(
    http: &HttpSession,
    base_url: &str,
    token: &str,
    sha256: &str,
    dest: &Path,
) -> Result<u64> {
    // Validate sha256 is a 64-char hex string before putting it in the URL.
    let sha_clean = sha256.trim();
    if sha_clean.len() != 64 || !sha_clean.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid SHA-256 digest for an Asset: {sha256}");
    }
    let what = format!("Asset {sha_clean} fetch");
    let fetch_failed = || format!("{what} failed");
    let base = trim_base_url(base_url);
    // The fingerprint alone names the Asset, and the token names the
    // account; the route takes no query.
    let url = reqwest::Url::parse(&format!("{base}/v1/assets/{sha_clean}"))
        .with_context(|| format!("invalid server address {base}"))?;

    let mut response = http
        .request_url(Method::GET, url, token)
        .timeout(ASSET_READ_TIMEOUT)
        .send()
        .with_context(fetch_failed)?;

    let status = response.status();
    if status.as_u16() == 401 {
        return Err(session_refused(&what).into());
    }
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(HttpError::new(
            status.as_u16(),
            format!("{what} failed (HTTP {status}): {}", error_sentence(&body)),
        )
        .into());
    }

    let mut len = 0;
    let written = write_asset(dest, sha_clean, |out| {
        let (digest, written) = write_hashed(&mut response, out)
            .with_context(|| format!("write {}", dest.display()))?;
        len = written;
        // A `200 OK` is not proof of the attachment: an access proxy whose
        // session has expired redirects to its login page, which answers
        // `200 OK` with HTML. Only bytes whose SHA-256 is the one asked for
        // are kept, so a later Export fetches the attachment again instead
        // of skipping it.
        if !digest.eq_ignore_ascii_case(sha_clean) {
            return Err(HttpError::new(
                status.as_u16(),
                format!("the server's answer to {what} is bytes whose SHA-256 is {digest}"),
            )
            .into());
        }
        Ok(())
    });
    // The answer that is not the Asset already names the fetch.
    written.map_err(|error| {
        if error.is::<HttpError>() {
            error
        } else {
            error.context(fetch_failed())
        }
    })?;
    Ok(len)
}

/// Copy `body` into `out` and return the lowercase hex SHA-256 of the bytes
/// written, and how many there were.
fn write_hashed(body: &mut impl Read, out: &mut dyn Write) -> Result<(String, u64)> {
    let mut writer = HashingWriter {
        inner: out,
        hasher: Sha256::new(),
    };
    let len = std::io::copy(body, &mut writer)?;
    Ok((hex::encode(writer.hasher.finalize()), len))
}

/// A writer that hashes every byte it passes on to `inner`.
struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fingerprint becomes part of the request path, so anything that is
    /// not exactly 64 hex digits is refused before a request is made.
    #[test]
    fn a_fingerprint_that_is_not_64_hex_digits_is_refused_before_any_request() {
        let server = httpmock::MockServer::start();
        let any_request = server.mock(|_when, then| {
            then.status(200).body("bytes");
        });
        let http = HttpSession::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("asset.bin");

        for bad in ["a".repeat(63), "z".repeat(64), "abc123".to_string()] {
            let err = fetch_asset(&http, &server.base_url(), "mc_test", &bad, &dest)
                .expect_err("a bad fingerprint is an error");
            assert!(
                err.to_string().contains("invalid SHA-256 digest"),
                "{bad}: {err}"
            );
        }
        assert_eq!(any_request.calls(), 0);
        assert!(!dest.exists());
    }

    /// A session that expires mid-run answers 401 to a fetch, and the
    /// message says to log in again.
    #[test]
    fn a_401_fetch_says_to_log_in_again() {
        let server = httpmock::MockServer::start();
        let digest = "a".repeat(64);
        server.mock(|when, then| {
            when.method("GET").path(format!("/v1/assets/{digest}"));
            then.status(401);
        });
        let http = HttpSession::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("asset.bin");

        let err = fetch_asset(&http, &server.base_url(), "mc_test", &digest, &dest)
            .expect_err("a 401 is an error");
        let message = err.to_string();
        assert!(
            message.starts_with(&format!("Asset {digest} fetch failed.")),
            "{message}"
        );
        assert!(message.contains("Log in again"), "{message}");
        assert!(!dest.exists());
    }

    /// A request that never reaches the server leads with the request's
    /// label, not the route, and keeps the connection's own error beneath it.
    #[test]
    fn a_request_that_cannot_connect_leads_with_its_label() {
        // A port the system just handed out and nobody holds any more refuses
        // the connection at once.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let http = HttpSession::new().unwrap();
        let err = create_export(
            &http,
            &format!("http://127.0.0.1:{port}"),
            "mc_test",
            &ExportScope::Everything,
            "message-crate-export",
        )
        .expect_err("nothing listens on that port");
        assert_eq!(err.to_string(), "Export Run start failed");
        assert!(err.source().is_some(), "the cause stays beneath the label");
    }

    /// A refused page names the Export Run and the page once.
    #[test]
    fn a_refused_page_names_the_export_run() {
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method("GET").path("/v1/exports/7/messages");
            then.status(500)
                .header("content-type", "application/problem+json")
                .json_body(serde_json::json!({
                    "type": "about:blank",
                    "title": "Internal error",
                    "status": 500,
                    "detail": "the database is locked"
                }));
        });
        let http = HttpSession::new().unwrap();
        let err = export_messages(
            &http,
            ExportMessagesArgs {
                base_url: &server.base_url(),
                token: "mc_test",
                export_id: 7,
                limit: 2,
                offset: 0,
            },
        )
        .expect_err("a 500 is an error");
        assert_eq!(
            err.to_string(),
            "Export Run 7 page failed (HTTP 500 Internal Server Error): the database is locked"
        );
    }

    /// A cancel goes to the cancel route and a refusal names it a cancellation.
    #[test]
    fn a_refused_cancel_names_the_cancellation() {
        let server = httpmock::MockServer::start();
        let cancel = server.mock(|when, then| {
            when.method("POST").path("/v1/exports/7/cancel");
            then.status(409)
                .header("content-type", "application/problem+json")
                .json_body(serde_json::json!({
                    "type": "about:blank",
                    "title": "Conflict",
                    "status": 409,
                    "detail": "the run is already closed"
                }));
        });
        let http = HttpSession::new().unwrap();
        let err = close_export(&http, &server.base_url(), "mc_test", 7, CloseAction::Cancel)
            .expect_err("a 409 is an error");
        assert_eq!(cancel.calls(), 1);
        assert_eq!(
            err.to_string(),
            "Export Run 7 cancellation failed (HTTP 409 Conflict): the run is already closed"
        );
    }

    #[test]
    fn the_create_body_carries_the_scope_as_given_and_the_tool() {
        let scope = ExportScope::Query {
            list: message_crate_api_types::ExportQueryList::Conversations,
            q: "from:me".into(),
        };
        let body = serde_json::to_value(CreateExportBody {
            scope: &scope,
            tool: "message-crate-export",
        })
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "scope": { "kind": "query", "list": "conversations", "q": "from:me" }, "tool": "message-crate-export" })
        );
    }

    #[test]
    fn a_page_parses_without_an_ok_flag_and_a_failure_body_yields_its_sentence() {
        let page: Page<Message> =
            serde_json::from_str(r#"{"items":[],"total":7,"limit":500,"offset":0}"#).unwrap();
        assert_eq!((page.items.len(), page.total), (0, 7));
        assert_eq!(
            error_sentence(
                r#"{"type":"https://messagecrate.app/docs/developer/reference/errors/validation-failed","title":"Validation failed","status":422,"errors":["limit exceeds maximum of 500"]}"#
            ),
            "limit exceeds maximum of 500"
        );
        assert_eq!(error_sentence("<html>"), "<html>");
    }
}
