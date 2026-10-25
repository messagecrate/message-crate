//! What the layers around every handler run, which `with_server_layers` in
//! [`crate::server`] puts in order: CORS, the body limits, the `Accept`
//! check, `Cache-Control`, the Content Security Policy's value, the answer to
//! a handler that panicked and the request's log line, with the request
//! classifiers they decide by and the URI a log line names.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tower_http::classify::{ClassifiedResponse, ClassifyEos, ClassifyResponse};
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use tower_http::trace::{DefaultOnResponse, OnResponse};
use tracing::Level;

use crate::api_error::ApiError;
use crate::limits::MAX_REQUEST_BODY_BYTES;
use crate::problem::Problem;
use crate::request_body::BodyTimeLimits;
use crate::request_body::{body_time_bounded, declared_over};

/// Origins the packaged desktop app runs from. A Tauri window is not a page on
/// the web, so its origin is fixed by the platform rather than chosen by
/// anyone: `tauri://localhost` on Linux and macOS, and `http(s)://tauri.localhost`
/// on Windows.
///
/// These are allowed whatever the config says. A server built from source starts
/// with `cors_origins` commented out, and the desktop app pointed at it then
/// fails in a way that reads as a network problem. The browser refuses the
/// response before any code can see it, so the app reports the server as
/// unreachable while `curl` to the same port succeeds. That sends people to
/// their firewall for a missing line of TOML.
///
/// Allowing them by default gives away nothing a listed origin does not. The
/// browser sets `Origin` itself and a page on the web cannot claim to be one of
/// these, so this widens what the desktop app can reach, not what a website can.
pub(crate) const PACKAGED_DESKTOP_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Build the Cross-Origin Resource Sharing (CORS) layer from
/// `[server] cors_origins` and each `--cors-origin`. CORS is the browser rule
/// that decides which other websites may call this API.
///
/// - a `*` anywhere in the list → fully permissive (local debugging only), with a `WARN` line
///   in the server's log as it starts, since any website a person on the
///   network visits can then call the routes a stranger may call (#2218)
/// - otherwise → exact origin allow list, always including
///   [`PACKAGED_DESKTOP_ORIGINS`]
///
/// An empty list is therefore not "no CORS" but "the desktop app and nothing
/// else", which is what an unconfigured server wants: the browser UI it serves
/// itself is same-origin and needs no header at all.
pub(crate) fn build_cors_layer(origins: &[String]) -> CorsLayer {
    if origins.iter().any(|o| o.trim() == "*") {
        tracing::warn!(
            "cors_origins in the config's [server] section, or a --cors-origin flag, lists \"*\", so any website can call this server through a visitor's browser; list the origins that need it instead"
        );
        return CorsLayer::permissive();
    }
    let mut allowed: Vec<HeaderValue> = Vec::new();
    for origin in origins
        .iter()
        .map(String::as_str)
        .chain(PACKAGED_DESKTOP_ORIGINS.iter().copied())
    {
        let trimmed = origin.trim();
        if trimmed.is_empty() {
            continue;
        }
        // A config that lists a packaged origin by hand is the common case, and
        // naming the same origin twice in the allow list helps no one.
        if let Ok(value) = trimmed.parse::<HeaderValue>()
            && !allowed.contains(&value)
        {
            allowed.push(value);
        }
    }
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed))
        .allow_methods(AllowMethods::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
}

/// `tower_http`'s `RequestBodyLimitLayer` answers a plain-text `413` itself,
/// bypassing every extractor, the moment a `Content-Length` header already
/// announces a payload over the limit. `extract::Json`'s own 413 handling
/// only ever sees a body that had to be read to discover it was too long.
/// Rewrite that one plain-text response into the `payload-too-large` problem
/// so a body over the limit answers the same way however the client declares
/// its size.
pub(crate) async fn json_body_limit_response(response: Response) -> Response {
    if response.status() != StatusCode::PAYLOAD_TOO_LARGE {
        return response;
    }
    let already_problem = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with(Problem::CONTENT_TYPE));
    if already_problem {
        return response;
    }
    ApiError::PayloadTooLarge.into_response()
}

/// Hold a request body to its cap. An attachment upload
/// ([`is_attachment_upload`]) is held to the attachment size limit, as the
/// Server Settings have it when the request arrives; every other request to
/// [`MAX_REQUEST_BODY_BYTES`]. A `Content-Length` over the cap is refused
/// before any handler runs; a body with no declared length is cut off once it
/// passes the cap. A request with a safe method carries no body the server
/// reads, so it skips the check. A part of a multipart upload
/// ([`is_upload_part`]) skips it too, because its route holds the body to the
/// part size the upload started with, and the limit as it is now would refuse
/// every remaining part of an upload the owner lowered the limit under.
///
/// Every body is held to time as well as size, with the body time limits
/// ([`BodyTimeLimits`]), so a client
/// that stops sending a body part-way, or sends a byte now and then, holds its
/// connection only so long (#2100).
/// The three uploads ([`reads_its_own_body_in_time`]) are left to hold
/// their bodies themselves, because each starts the clock when it starts
/// reading.
pub(crate) async fn limit_request_body(
    axum::extract::State(db): axum::extract::State<sqlx::SqlitePool>,
    axum::extract::State(body_time_limits): axum::extract::State<BodyTimeLimits>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    use axum::http::Method;
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) || is_upload_part(&request)
    {
        return next.run(request).await;
    }
    let limit = if is_attachment_upload(&request) {
        match crate::asset_uploads::asset_max_bytes(&db).await {
            Ok(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
            Err(error) => return ApiError::Internal(error).into_response(),
        }
    } else {
        MAX_REQUEST_BODY_BYTES
    };
    if declared_over(request.headers(), limit) {
        return ApiError::PayloadTooLarge.into_response();
    }
    let in_time = !reads_its_own_body_in_time(&request);
    let request = request.map(|body| {
        let body = axum::body::Body::new(http_body_util::Limited::new(body, limit));
        if in_time {
            body_time_bounded(body, body_time_limits)
        } else {
            body
        }
    });
    next.run(request).await
}

/// Whether `request` is one of the uploads that hold their body to the
/// body time limits themselves: an import batch, which waits for an
/// import slot before it reads, a single-request attachment upload, and a
/// part of a multipart one. Each starts the clock when it starts reading,
/// and a clock started on arrival would count the wait for a slot against
/// the body.
fn reads_its_own_body_in_time(request: &axum::extract::Request) -> bool {
    is_import_batch(request) || is_attachment_upload(request) || is_upload_part(request)
}

/// `POST /v1/imports/{id}/batches`: one batch of an import.
fn is_import_batch(request: &axum::extract::Request) -> bool {
    request.method() == axum::http::Method::POST
        && request
            .uri()
            .path()
            .strip_prefix("/v1/imports/")
            .and_then(|rest| rest.strip_suffix("/batches"))
            .is_some_and(|id| !id.is_empty() && !id.contains('/'))
}

/// Whether `path` belongs to the `/v1` interface: `/v1` itself or anything
/// under it. A router's matched path and an OpenAPI path are both full paths,
/// so the one test serves the layers and the tests that walk the document.
pub(crate) fn is_api_path(path: &str) -> bool {
    path == "/v1" || path.starts_with("/v1/")
}

/// Mark every `/v1` answer `Cache-Control: no-store` unless the route set its
/// own (`docs/architecture/rules/http-api.md`, API-69). The answers carry a
/// Session token, message text, contacts or attachments, and no cache should
/// keep them. An asset read sets `private` itself and keeps it. Only `/v1`
/// is marked: the website served by the fallback, `/health` and the OpenAPI
/// UI keep whatever caching they have.
pub(crate) async fn no_store_unless_set(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = request.uri().path();
    let is_api = is_api_path(path);
    let mut response = next.run(request).await;
    if is_api {
        response
            .headers_mut()
            .entry(header::CACHE_CONTROL)
            .or_insert(header::HeaderValue::from_static("no-store"));
    }
    response
}

/// Refuse a request whose `Accept` names nothing this route can produce
/// (`docs/architecture/rules/http-api.md`). Narrow on purpose: only when the header is present and none of
/// its members is `application/json`, `application/problem+json`,
/// `application/*` or `*/*`. A missing `Accept` is a request for JSON, which
/// is what every one of the server's own clients sends. An `Accept` sent on
/// several field lines is one comma-separated list (RFC 9110, section 5.3),
/// so every line is read, and a line that is not visible ASCII is skipped.
///
/// Applied to the `/v1` routes only, through `route_layer`; the static app,
/// `/health` and the OpenAPI UI are mounted outside it, so they keep
/// producing what they produce. Six routes answer bytes, not JSON, and are
/// let through here by path: the asset download, its preview and its
/// thumbnail stream a file's own bytes (the preview and the thumbnail under
/// `HEAD` as well), the address book export and the address book written
/// from a vCard file answer `text/csv`, and a file of the server's log
/// downloads as `text/plain`.
pub(crate) async fn require_json_acceptable(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if is_asset_download(&request)
        || is_address_book_export(&request)
        || is_log_file_download(&request)
    {
        return next.run(request).await;
    }
    let lines: Vec<&str> = request
        .headers()
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    if !lines.is_empty() {
        let accept = lines.join(", ");
        if !accepts_json(&accept) {
            return ApiError::NotAcceptable(format!(
                "this route answers application/json, and Accept was {accept}"
            ))
            .into_response();
        }
    }
    next.run(request).await
}

/// `GET /v1/assets/{sha256}`, `GET /v1/assets/{sha256}/preview` and
/// `GET /v1/assets/{sha256}/thumbnail`: the asset's own bytes, its
/// preview's or its thumbnail's, each in its own media type. A `HEAD` of the
/// preview or the thumbnail is let through too, because Axum answers it with
/// the `GET` handler and its headers describe bytes, not JSON. A `HEAD` of
/// the asset itself is not: it is `head_asset`, which answers JSON.
fn is_asset_download(request: &axum::extract::Request) -> bool {
    let method = request.method();
    let is_get = method == axum::http::Method::GET;
    let is_head = method == axum::http::Method::HEAD;
    if !is_get && !is_head {
        return false;
    }
    let Some(rest) = request.uri().path().strip_prefix("/v1/assets/") else {
        return false;
    };
    let (sha256, is_preview_or_thumbnail) = match rest
        .strip_suffix("/preview")
        .or_else(|| rest.strip_suffix("/thumbnail"))
    {
        Some(sha256) => (sha256, true),
        None => (rest, false),
    };
    !sha256.is_empty() && !sha256.contains('/') && (is_get || is_preview_or_thumbnail)
}

/// The path segments after `/v1/assets/` of a `PUT`, or `None` for any other
/// request.
fn asset_put_segments(request: &axum::extract::Request) -> Option<Vec<&str>> {
    if request.method() != axum::http::Method::PUT {
        return None;
    }
    let rest = request.uri().path().strip_prefix("/v1/assets/")?;
    Some(rest.split('/').collect())
}

/// `PUT /v1/assets/{sha256}`: the one request the attachment size limit, as
/// the Server Settings have it when the request arrives, holds.
fn is_attachment_upload(request: &axum::extract::Request) -> bool {
    asset_put_segments(request)
        .is_some_and(|segments| matches!(segments.as_slice(), [sha256] if !sha256.is_empty()))
}

/// `PUT /v1/assets/{sha256}/uploads/{upload_id}/parts/{part}`: one part of a
/// multipart upload, which its route holds to the part size in the upload's
/// manifest.
fn is_upload_part(request: &axum::extract::Request) -> bool {
    asset_put_segments(request).is_some_and(|segments| {
        matches!(
            segments.as_slice(),
            [sha256, "uploads", upload_id, "parts", part]
                if !sha256.is_empty() && !upload_id.is_empty() && !part.is_empty()
        )
    })
}

/// `POST /v1/contacts/address-book` and `POST /v1/contacts/vcard-address-book`:
/// the address book as `text/csv`.
fn is_address_book_export(request: &axum::extract::Request) -> bool {
    request.method() == axum::http::Method::POST
        && matches!(
            request.uri().path(),
            "/v1/contacts/address-book" | "/v1/contacts/vcard-address-book"
        )
}

/// `GET /v1/server/log-files/{id}`: one file of the server's log as `text/plain`.
fn is_log_file_download(request: &axum::extract::Request) -> bool {
    request.method() == axum::http::Method::GET
        && request
            .uri()
            .path()
            .strip_prefix("/v1/server/log-files/")
            .is_some_and(|id| !id.is_empty() && !id.contains('/'))
}

/// Whether an `Accept` header admits a JSON answer.
fn accepts_json(accept: &str) -> bool {
    accept
        .split(',')
        .filter_map(|member| member.split(';').next())
        .map(str::trim)
        .any(|media| {
            media == "*/*"
                || media.eq_ignore_ascii_case("application/*")
                || media.eq_ignore_ascii_case("application/json")
                || media.eq_ignore_ascii_case(Problem::CONTENT_TYPE)
        })
}

/// The query parameters whose values a log line keeps: each is a number, an
/// id or a fixed word, so none can carry a credential, message text or a
/// contact's name.
const LOGGED_QUERY_VALUES: [&str; 10] = [
    "after",
    "around",
    "before",
    "deleted_account_id",
    "level",
    "limit",
    "mode",
    "offset",
    "sort",
    "status",
];

/// A query parameter's name, as it is written in the URL, decoded as the
/// server reads every query. `None` when it cannot be read.
fn decoded_query_name(raw_name: &str) -> Option<String> {
    let uri = format!("/?{raw_name}=").parse::<axum::http::Uri>().ok()?;
    axum::extract::Query::<Vec<(String, String)>>::try_from_uri(&uri)
        .ok()?
        .0
        .into_iter()
        .next()
        .map(|(name, _)| name)
}

/// The request's URI as the log line names it: the path and the query, with
/// every value hidden but those of [`LOGGED_QUERY_VALUES`]. A `media_link` is
/// a credential that travels in the URL, because a media element can send it
/// nowhere else, and `q` is a search over what the messages say; the log
/// holds neither (`docs/architecture/rules/server-log.md`). A parameter added
/// later is hidden until it is added to the list.
///
/// A URI longer than [`LOGGED_URI_BYTES`] is cut there and ends in
/// [`LOGGED_URI_CUT`], so a request with a long path, which needs no
/// credential, cannot write a line of any length and push the rest of the
/// log off the disk (#2088).
pub(crate) fn logged_uri(uri: &axum::http::Uri) -> String {
    let mut logged = uri_with_hidden_values(uri);
    if logged.len() > LOGGED_URI_BYTES {
        logged.truncate(logged.floor_char_boundary(LOGGED_URI_BYTES - LOGGED_URI_CUT.len()));
        logged.push_str(LOGGED_URI_CUT);
    }
    logged
}

/// The most bytes of a request's URI a log line holds, the mark of a cut
/// included. A URI the server's own clients send is far shorter.
const LOGGED_URI_BYTES: usize = 2048;

/// What ends a URI [`logged_uri`] cut.
const LOGGED_URI_CUT: &str = "...[cut]";

/// [`logged_uri`] before the cut.
fn uri_with_hidden_values(uri: &axum::http::Uri) -> String {
    let Some(query) = uri.query() else {
        return uri.path().to_string();
    };
    let query = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            // The name as the server reads it, percent-decoded, so no
            // spelling of a hidden name the server accepts reaches the log.
            Some((name, _))
                if !decoded_query_name(name)
                    .is_some_and(|name| LOGGED_QUERY_VALUES.contains(&name.as_str())) =>
            {
                format!("{name}=[hidden]")
            }
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{}?{query}", uri.path())
}

/// The `Content-Security-Policy` on every answer (#2073). Each directive's
/// reason is in `docs/architecture/rules/http-api.md`, API-70.
pub(crate) const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: http: https:; \
     media-src 'self' blob: http: https:; connect-src 'self' http: https:; \
     font-src 'self' data:; object-src 'none'; base-uri 'self'; form-action 'none'; \
     frame-ancestors 'none'";

/// The answer to a request whose handler panicked: the `500 Internal Server
/// Error` problem document every internal failure answers, with the
/// request's id, so the person has an id to report. Its `ERROR` line says
/// only that the handler panicked: the panic hook (`logging::init`) has
/// already written the panic's message and place, under the request's span,
/// on the thread that panicked.
pub(crate) fn answer_panic(_panic: Box<dyn std::any::Any + Send + 'static>) -> Response {
    ApiError::Internal(anyhow::anyhow!("the request's handler panicked")).into_response()
}

/// Marks a response whose log line is written at `debug` when it succeeds:
/// a file of the website, and the answer to `GET /health`, which the web app
/// asks for every 30 seconds in each open tab. At `info`, these lines would
/// bury the requests that did something (#2274).
#[derive(Clone, Copy)]
struct Routine;

/// Mark `response` as [`Routine`].
pub(crate) async fn mark_routine(mut response: Response) -> Response {
    response.extensions_mut().insert(Routine);
    response
}

/// Write `response`'s line: at `debug` for a [`Routine`] answer that is not
/// a client or server error, and at `info` for every other. A failed probe
/// and a file the website does not have are not routine, so each keeps its
/// `info` line.
pub(crate) fn log_response(
    response: &Response,
    latency: std::time::Duration,
    span: &tracing::Span,
) {
    let status = response.status();
    let routine = response.extensions().get::<Routine>().is_some()
        && !status.is_client_error()
        && !status.is_server_error();
    let level = if routine { Level::DEBUG } else { Level::INFO };
    DefaultOnResponse::new()
        .level(level)
        .on_response(response, latency, span);
}

/// The `TraceLayer`'s classifier: a response is never a failure for its
/// status, because `ApiError::to_problem` already writes the `ERROR` line of
/// a 5xx. Only its body can fail, partway, after the status went out with
/// the response's line, such as a download that hits a disk error
/// (#2870). tower-http's own `ServerErrorsAsFailures` classifies at the
/// status and never looks at the body, so such a failure wrote no line.
#[derive(Clone, Copy, Default)]
pub(crate) struct BodyFailures;

impl ClassifyResponse for BodyFailures {
    type FailureClass = String;
    type ClassifyEos = Self;

    fn classify_response<B>(self, _response: &Response<B>) -> ClassifiedResponse<String, Self> {
        ClassifiedResponse::RequiresEos(self)
    }

    fn classify_error<E: std::fmt::Display + 'static>(self, error: &E) -> String {
        error.to_string()
    }
}

impl ClassifyEos for BodyFailures {
    type FailureClass = String;

    fn classify_eos(self, _trailers: Option<&axum::http::HeaderMap>) -> Result<(), String> {
        Ok(())
    }

    fn classify_error<E: std::fmt::Display + 'static>(self, error: &E) -> String {
        error.to_string()
    }
}

/// Write the `ERROR` line of a response whose body failed partway, under
/// the request's span, so the request's id is on it next to the `status=200`
/// line the response wrote when it started.
pub(crate) fn log_body_failure(
    error: String,
    _latency: std::time::Duration,
    _span: &tracing::Span,
) {
    tracing::error!(error = %error, "A response's body failed after its status was sent");
}

#[cfg(test)]
mod tests;
