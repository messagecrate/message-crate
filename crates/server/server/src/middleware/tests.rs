use super::*;
use std::path::Path;
use std::sync::Arc;

use axum::Router;

use crate::logging::{LogLevel, LogLine};
use crate::problem::ProblemType;
use crate::server::{AppState, with_server_layers};
use crate::test_support::{expect_problem, test_fixture};
use tempfile::TempDir;

fn with_cors(mut state: AppState, origins: &[&str]) -> AppState {
    let mut cfg = (*state.cfg).clone();
    cfg.server.as_mut().unwrap().cors_origins = origins.iter().map(|s| (*s).to_string()).collect();
    state.cfg = Arc::new(cfg);
    state
}

async fn cors_preflight(state: AppState, origin: &str) -> reqwest::Response {
    let server = crate::test_support::serve(&state).await;
    server
        .request(reqwest::Method::OPTIONS, "/health")
        .header("Origin", origin)
        .header("Access-Control-Request-Method", "GET")
        .header("Access-Control-Request-Headers", "content-type")
        .send()
        .await
        .unwrap()
}

fn allow_origin(response: &reqwest::Response) -> Option<&str> {
    response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|value| value.to_str().ok())
}

#[tokio::test]
async fn cors_preflight_allows_packaged_desktop_and_vite_origins() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let origins = [
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        "https://tauri.localhost",
        "http://tauri.localhost",
        "tauri://localhost",
    ];
    for origin in origins {
        let response = cors_preflight(with_cors(state.clone(), &origins), origin).await;
        assert_eq!(
            allow_origin(&response),
            Some(origin),
            "preflight Origin {origin}"
        );
    }
}

/// A server built from source starts with `cors_origins` commented out. The
/// desktop app still has to reach it, so the packaged origins do not wait
/// to be configured.
#[tokio::test]
async fn cors_preflight_allows_packaged_desktop_without_configuration() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    for origin in PACKAGED_DESKTOP_ORIGINS {
        let response = cors_preflight(with_cors(state.clone(), &[]), origin).await;
        assert_eq!(
            allow_origin(&response),
            Some(*origin),
            "unconfigured preflight Origin {origin}"
        );
    }
}

/// Built in does not mean open: everything else still has to be listed.
#[tokio::test]
async fn cors_preflight_rejects_unknown_origin_without_configuration() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let response = cors_preflight(with_cors(state, &[]), "https://evil.example").await;
    assert_eq!(allow_origin(&response), None);
}

#[tokio::test]
async fn cors_preflight_rejects_unknown_origin() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let response = cors_preflight(
        with_cors(state, &["tauri://localhost"]),
        "https://evil.example",
    )
    .await;
    assert_eq!(allow_origin(&response), None);
}

/// The server log's lines while the CORS layer is built from `origins`.
fn log_lines_building_cors(origins: &[&str]) -> Vec<LogLine> {
    use crate::logging::{LogFiles, LogLinesQuery, SERVER_LOG_LIMITS, read_lines};

    let dir = TempDir::new().unwrap();
    let log_dir = dir.path().join("logs");
    let files = LogFiles::open(&log_dir, SERVER_LOG_LIMITS).unwrap();
    let origins: Vec<String> = origins.iter().map(|s| (*s).to_string()).collect();
    let _layer =
        tracing::subscriber::with_default(crate::logging::subscriber_for(files, "info"), || {
            build_cors_layer(&origins)
        });
    read_lines(
        &log_dir,
        &LogLinesQuery {
            limit: 10,
            ..LogLinesQuery::default()
        },
    )
    .unwrap()
    .0
}

/// A `*` in `cors_origins` lets any website call the server, so the server
/// says so in its log when it starts, naming the setting (#2218).
#[test]
fn a_wildcard_cors_origin_is_warned_about() {
    let lines = log_lines_building_cors(&["https://app.example", " * "]);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].level, LogLevel::Warn);
    assert!(lines[0].text.contains("cors_origins"), "{}", lines[0].text);
    assert!(lines[0].text.contains("--cors-origin"), "{}", lines[0].text);
    assert!(lines[0].text.contains("any website"), "{}", lines[0].text);
}

#[test]
fn a_listed_cors_origin_is_not_warned_about() {
    let lines = log_lines_building_cors(&["https://app.example"]);
    assert!(lines.is_empty(), "{lines:?}");
}

/// `state` serving the website from a directory of its own, which holds
/// `index.html` and nothing else.
fn with_website(mut state: AppState, website: &Path) -> AppState {
    std::fs::write(website.join("index.html"), "<!doctype html>").unwrap();
    let mut cfg = (*state.cfg).clone();
    cfg.server.as_mut().unwrap().static_dir = website.to_path_buf();
    state.cfg = Arc::new(cfg);
    state
}

/// The server log's lines, oldest first, written at `filter` while `state`'s
/// app answers a `GET` for each of `paths` in turn.
async fn log_lines_serving(state: &AppState, filter: &str, paths: &[&str]) -> Vec<LogLine> {
    use crate::logging::{LogFiles, LogLinesQuery, SERVER_LOG_LIMITS, read_lines};

    let dir = TempDir::new().unwrap();
    let log_dir = dir.path().join("logs");
    let files = LogFiles::open(&log_dir, SERVER_LOG_LIMITS).unwrap();
    {
        // The test's runtime has one thread, so the server's tasks write
        // through this thread's subscriber too.
        let _subscriber =
            tracing::subscriber::set_default(crate::logging::subscriber_for(files, filter));
        let server = crate::test_support::serve(state).await;
        for path in paths {
            server
                .get(path)
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
        }
    }
    let mut lines = read_lines(
        &log_dir,
        &LogLinesQuery {
            limit: 100,
            ..LogLinesQuery::default()
        },
    )
    .unwrap()
    .0;
    lines.reverse();
    lines
}

/// The per-response lines among `lines`: each one's level and the status it
/// names.
fn response_lines(lines: &[LogLine]) -> Vec<(LogLevel, String)> {
    lines
        .iter()
        .filter(|line| line.text.contains("finished processing request"))
        .map(|line| {
            let status = line
                .text
                .split_whitespace()
                .find_map(|word| word.strip_prefix("status="))
                .unwrap_or_else(|| panic!("no status in {}", line.text))
                .to_string();
            (line.level, status)
        })
        .collect()
}

/// A `500 Internal Server Error` writes one `ERROR` line, the one with the
/// error's chain, and its per-response line at `INFO` (#2254). The request
/// is a failed health probe, which is not routine, so its line stays at
/// `INFO` too (#2274).
#[tokio::test]
async fn a_500_writes_one_error_line_and_an_info_response_line() {
    let fixture = test_fixture().await;
    // A closed pool fails the health check's statement.
    fixture.state.db.close().await;
    let lines = log_lines_serving(&fixture.state, "info", &["/health"]).await;

    let errors: Vec<_> = lines
        .iter()
        .filter(|line| line.level == LogLevel::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{lines:#?}");
    assert!(
        errors[0]
            .text
            .contains("run the health check's statement on the database"),
        "{}",
        errors[0].text
    );
    assert_eq!(
        response_lines(&lines),
        [(LogLevel::Info, "500".to_string())],
        "{lines:#?}"
    );
}

/// A successful health probe and a file of the website are answered without
/// a line at the default `info`, because the web app probes `/health` every
/// 30 seconds in each tab. An answer from `/v1`, and a 404 from the website,
/// still write theirs (#2274).
#[tokio::test]
async fn a_successful_probe_or_website_file_writes_no_info_line() {
    let fixture = test_fixture().await;
    let website = TempDir::new().unwrap();
    let state = with_website(fixture.state.clone(), website.path());
    let lines = log_lines_serving(
        &state,
        "info",
        &["/health", "/index.html", "/", "/missing.js", "/v1/nothing"],
    )
    .await;

    assert_eq!(
        response_lines(&lines),
        [
            (LogLevel::Info, "404".to_string()),
            (LogLevel::Info, "404".to_string()),
        ],
        "{lines:#?}"
    );
}

/// The trace layer writes the lines left out at `info` at `DEBUG`, so a
/// `debug` filter still shows every response (#2274).
#[tokio::test]
async fn a_successful_probe_or_website_file_writes_a_debug_line() {
    let fixture = test_fixture().await;
    let website = TempDir::new().unwrap();
    let state = with_website(fixture.state.clone(), website.path());
    // `debug` for the response lines alone, so the debug lines of sqlx,
    // hyper and reqwest never crowd them out of the lines read back.
    let lines = log_lines_serving(
        &state,
        "info,tower_http::trace=debug",
        &["/health", "/index.html"],
    )
    .await;

    assert_eq!(
        response_lines(&lines),
        [
            (LogLevel::Debug, "200".to_string()),
            (LogLevel::Debug, "200".to_string()),
        ],
        "{lines:#?}"
    );
}

/// The OpenAPI document and the Swagger UI are wrapped by the same layers as
/// every other route: a request id and the CORS headers for an allowed origin.
/// `/docs/` is asked for the way a browser asks for a page, so the `Accept`
/// check that guards `/v1` must not refuse it.
#[tokio::test]
async fn openapi_ui_routes_carry_a_request_id_and_cors_headers() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    {
        let cfg = Arc::make_mut(&mut state.cfg);
        cfg.server.as_mut().unwrap().openapi_ui = true;
    }
    let origin = "http://localhost:5173";
    let state = with_cors(state, &[origin]);
    let server = crate::test_support::serve(&state).await;
    for path in ["/openapi.json", "/docs/"] {
        let response = server
            .get(path)
            .header("Origin", origin)
            .header(header::ACCEPT, "text/html")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(
            response.headers().contains_key(crate::request_id::HEADER),
            "{path} carries no request id"
        );
        assert_eq!(allow_origin(&response), Some(origin), "{path}");
    }
}

/// `limit_request_body` answers its own 413 the moment a `Content-Length`
/// announces an oversize body, without running any handler. That response
/// must still pass through the CORS layer, or a browser reports a CORS
/// failure instead of showing the 413 the server sent.
#[tokio::test]
async fn the_fast_413_carries_cors_headers() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    // The default test config's `cors_origins` is empty, which only
    // allows the packaged desktop origins (`build_cors_layer`) — not the
    // browser origin this test sends. Configure it explicitly so the
    // assertion below tests CORS header propagation, not the allow list.
    let state = with_cors(fixture.state.clone(), &["https://app.example"]);
    crate::test_support::store_asset_max_bytes(&state, 1024).await;

    let sha = "0".repeat(64);
    let server = crate::test_support::serve(&state).await;
    let response = server
        .put(&format!("/v1/assets/{sha}"))
        .bearer_auth(&user.token)
        .header(header::ORIGIN, "https://app.example")
        .header(header::CONTENT_TYPE, "image/png")
        // A sized body, so the limit layer answers from Content-Length alone.
        .body(vec![b'x'; 4096])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(
        response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        "the fast 413 must carry CORS headers, got: {:?}",
        response.headers()
    );
    let status = response.status();
    let text = response.text().await.unwrap();
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::PayloadTooLarge,
    );
}

/// `Accept` is checked on the `/v1` routes that produce JSON and nowhere
/// else: not on the static app, and not on the asset download.
#[tokio::test]
async fn accept_is_checked_on_v1_json_routes_only() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = fixture.state.clone();
    let server = crate::test_support::serve(&state).await;

    let refused = server
        .get("/v1/conversations")
        .bearer_auth(&user.token)
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .unwrap();
    let status = refused.status();
    let text = refused.text().await.unwrap();
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotAcceptable);

    for accept in [
        "application/json",
        "*/*",
        "text/html, */*;q=0.1",
        "application/*",
    ] {
        let allowed = server
            .get("/v1/conversations")
            .bearer_auth(&user.token)
            .header(header::ACCEPT, accept)
            .send()
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK, "Accept: {accept}");
    }

    // A browser navigation: the static app is served, or 404 without a
    // static dir here, but never refused for its Accept.
    let page = server
        .get("/")
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .unwrap();
    assert_ne!(page.status(), StatusCode::NOT_ACCEPTABLE);

    // The asset download, one of five /v1 routes that answer bytes, takes
    // any Accept; the route then refuses for its own reasons (no source
    // named), never for the header.
    let asset = server
        .get(&format!(
            "/v1/assets/{}",
            crate::test_support::fake_sha256('a')
        ))
        .bearer_auth(&user.token)
        .header(header::ACCEPT, "image/jpeg")
        .send()
        .await
        .unwrap();
    assert_ne!(asset.status(), StatusCode::NOT_ACCEPTABLE);
}

/// The attachment size limit holds the attachment uploads and nothing else.
/// An owner who sets it to 20 bytes can still log in and change the Server
/// Settings, and an account can still post an import batch of a few
/// kilobytes. Only an attachment of 21 bytes is refused. When the limit
/// capped every body, the login itself answered `413` and nothing could
/// raise the limit again.
#[tokio::test]
async fn a_small_attachment_size_limit_holds_only_the_attachment_uploads() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let owner = crate::test_support::claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let importer = crate::test_support::register_via_api(&state, "bob", "hunter2hunter2").await;
    let _: serde_json::Value = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        &owner.token,
        serde_json::json!({ "asset_max_bytes": 20 }),
    )
    .await;

    // The login body is over 20 bytes and is not an attachment. `log_in`
    // asserts the `201 Created`.
    let session = crate::test_support::log_in(&state, "keeper", "hunter2hunter2").await;
    let changed: serde_json::Value = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        session["token"].as_str().unwrap(),
        serde_json::json!({ "asset_max_bytes": 20, "public_registration": true }),
    )
    .await;
    assert_eq!(changed["asset_max_bytes"], 20);

    let (_, created): (String, serde_json::Value) = crate::test_support::post_created_json(
        &state,
        "/v1/imports",
        &importer.token,
        serde_json::json!({ "source": "whatsapp" }),
    )
    .await;
    let header = crate::test_support::conversation_header("whatsapp", "+15555550107")
        .owner("+15555550106", Some("Me"))
        .participant("+15555550107", None);
    let mut batch = header.line();
    batch.push_str(
        &crate::test_support::message_line("g-1", &"a".repeat(4096))
            .at(1_700_000_000_000)
            .sms()
            .service(ir::Transport::Whatsapp)
            .sender("+15555550107")
            .line(),
    );
    let (status, text) = crate::test_support::post_raw(
        &state,
        &format!("/v1/imports/{}/batches", created["id"]),
        &importer.token,
        "application/jsonl",
        batch,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");

    let bytes = vec![b'x'; 21];
    let sha = file_io::sha256_hex(&bytes);
    let (status, text) = crate::test_support::put_raw(
        &state,
        &format!("/v1/assets/{sha}"),
        &importer.token,
        "image/png",
        bytes,
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::PayloadTooLarge,
    );
}

/// Send `PUT path` with `Transfer-Encoding: chunked` and `body` as one chunk,
/// and return the answer's status and body. reqwest sends every body it is
/// given here with a `Content-Length`, so the request is written by hand.
async fn put_chunked(base: &str, path: &str, token: &str, body: &[u8]) -> (StatusCode, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let address = base.strip_prefix("http://").unwrap();
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let head = format!(
        "PUT {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: image/png\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
        body.len()
    );
    // The server may answer and close before it has read the whole body, so
    // a failed write is not what the test checks; the answer is.
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response).await;
    let response = String::from_utf8_lossy(&response);
    let code = response
        .split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("no status line in {response:?}"));
    let (_, text) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no end to the headers in {response:?}"));
    (
        StatusCode::from_u16(code.parse().unwrap()).unwrap(),
        text.to_string(),
    )
}

/// An attachment upload with no `Content-Length` and a body over the
/// attachment size limit answers 413, as one with a `Content-Length` does,
/// and with the same `detail`: the limit layer refuses the sized body and the
/// upload route the chunked one, and a client must not see two sentences for
/// one condition.
#[tokio::test]
async fn a_chunked_attachment_upload_over_the_limit_is_413() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    crate::test_support::store_asset_max_bytes(&fixture.state, 1024).await;
    let server = crate::test_support::serve(&fixture.state).await;
    let sha = "0".repeat(64);

    let (status, text) = put_chunked(
        server.base(),
        &format!("/v1/assets/{sha}"),
        &user.token,
        &[b'x'; 4096],
    )
    .await;
    let sized = server
        .put(&format!("/v1/assets/{sha}"))
        .bearer_auth(&user.token)
        .header(header::CONTENT_TYPE, "image/png")
        .body(vec![b'x'; 4096])
        .send()
        .await
        .unwrap();
    let sized_status = sized.status();
    let sized_text = sized.text().await.unwrap();

    let chunked = expect_problem(status, &text, ProblemType::PayloadTooLarge);
    let sized = expect_problem(sized_status, &sized_text, ProblemType::PayloadTooLarge);
    assert_eq!(
        chunked.detail.as_deref(),
        Some("the request body is too large")
    );
    assert_eq!(sized.detail, chunked.detail);
}

/// A part of a multipart upload with no `Content-Length` and a body over the
/// part size its upload started with answers 413, as one with a
/// `Content-Length` does. `limit_request_body` lets a part through uncapped,
/// so the part route is the only cap it has.
#[tokio::test]
async fn a_chunked_part_over_its_upload_part_size_is_413() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let mut state = fixture.state.clone();
    state.assets.part_size = 16;
    let server = crate::test_support::serve(&state).await;
    let sha = "0".repeat(64);
    let started: serde_json::Value = server
        .post(&format!("/v1/assets/{sha}/uploads"))
        .bearer_auth(&user.token)
        .json(&serde_json::json!({ "bytes": 40 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(started["part_size"], 16);
    let path = format!(
        "/v1/assets/{sha}/uploads/{}/parts/1",
        started["upload_id"].as_str().unwrap()
    );

    let (status, text) = put_chunked(server.base(), &path, &user.token, &[b'x'; 17]).await;

    let problem = expect_problem(status, &text, ProblemType::PayloadTooLarge);
    assert_eq!(
        problem.detail.as_deref(),
        Some("the request body is too large"),
        "a chunked part reads as a sized one does: {text}"
    );
}

/// The server's log names every request, and a media link in a URL is a
/// credential: the line keeps the path and the other parameters' names and
/// hides the link, under any spelling of its name the server reads as
/// `media_link`.
#[test]
fn a_logged_uri_hides_the_media_link() {
    for (uri, logged) in [
        (
            "/v1/assets/ab?media_link=7.1790000000.deadbeef&limit=1",
            "/v1/assets/ab?media_link=[hidden]&limit=1",
        ),
        (
            "/v1/assets/ab?limit=1&media%5Flink=7.1790000000.deadbeef",
            "/v1/assets/ab?limit=1&media%5Flink=[hidden]",
        ),
        (
            "/v1/assets/ab?media_lin%6B=7.1790000000.deadbeef",
            "/v1/assets/ab?media_lin%6B=[hidden]",
        ),
        ("/v1/messages", "/v1/messages"),
    ] {
        let uri: axum::http::Uri = uri.parse().unwrap();
        assert_eq!(logged_uri(&uri), logged);
    }
}

/// A search is a question about what the messages say, and the log never
/// holds message text or a contact's name
/// (`docs/architecture/server-log.md`). So a logged URI keeps the value of a
/// parameter only when it is a number or a fixed word (`limit`, `offset`,
/// `sort`, `status` and the like), and hides every other value: `q`, the
/// `text` filter of the log lines route, and a parameter the server does not
/// know, under any spelling the server decodes to that name.
#[test]
fn a_logged_uri_hides_a_search_and_every_value_that_is_not_a_number_or_a_fixed_word() {
    for (uri, logged) in [
        (
            "/v1/messages?q=from%3AAda%20dinner&limit=40&offset=80&sort=-date",
            "/v1/messages?q=[hidden]&limit=40&offset=80&sort=-date",
        ),
        ("/v1/contacts?%71=Ada", "/v1/contacts?%71=[hidden]"),
        (
            "/v1/server/log-lines?level=warn&text=Ada&after=4294967296",
            "/v1/server/log-lines?level=warn&text=[hidden]&after=4294967296",
        ),
        (
            "/v1/imports?status=running&unknown=secret",
            "/v1/imports?status=running&unknown=[hidden]",
        ),
        (
            "/v1/conversations/7/messages?around=12",
            "/v1/conversations/7/messages?around=12",
        ),
        ("/v1/messages?q", "/v1/messages?q"),
    ] {
        let uri: axum::http::Uri = uri.parse().unwrap();
        assert_eq!(logged_uri(&uri), logged, "{uri}");
    }
}

/// A URI longer than the limit is cut on a character boundary and marked,
/// so a request with a long path, which needs no credential, writes a line
/// of a bounded length (#2088). One at the limit is kept whole.
#[test]
fn a_logged_uri_is_cut_at_its_limit_and_marked() {
    for filler in ["a", "é"] {
        // 60 KB of path, near the most a URI holds (64 KiB); `é` is two
        // bytes, so the cut can land inside one.
        let long: axum::http::Uri = format!("/{}?q=Ada", filler.repeat(60_000 / filler.len()))
            .parse()
            .unwrap();
        let logged = logged_uri(&long);
        assert!(logged.len() <= LOGGED_URI_BYTES, "{}", logged.len());
        assert!(
            logged.len() > LOGGED_URI_BYTES - LOGGED_URI_CUT.len() - filler.len(),
            "{}",
            logged.len()
        );
        assert!(logged.ends_with(LOGGED_URI_CUT), "{logged}");
        assert!(logged.starts_with(&format!("/{filler}")), "{logged}");
    }

    let at_limit: axum::http::Uri = format!("/{}", "a".repeat(LOGGED_URI_BYTES - 1))
        .parse()
        .unwrap();
    assert_eq!(logged_uri(&at_limit), at_limit.path());
}

/// A handler that panics is answered like any other internal failure: a
/// `500 Internal Server Error` problem document carrying the request's id,
/// the same id as the `x-request-id` header, and one `ERROR` line in the
/// server's log that says the handler panicked, under that request's id
/// (#2060). Without the catch, the connection closed with no answer at all.
#[tokio::test]
async fn a_handler_that_panics_answers_a_500_problem_with_its_request_id() {
    async fn panics() -> StatusCode {
        panic!("the handler this test makes panic");
    }

    let fixture = test_fixture().await;
    let routes = Router::new().route("/v1/panics", axum::routing::get(panics));
    let (response, lines) = crate::test_support::lines_written_by("info", async {
        let server =
            crate::test_support::serve_router(with_server_layers(routes, fixture.state.clone()))
                .await;
        server
            .get("/v1/panics")
            .send()
            .await
            .expect("the server answers a request whose handler panicked")
    })
    .await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let header_id = response
        .headers()
        .get(crate::request_id::HEADER)
        .and_then(|v| v.to_str().ok())
        .expect("the answer carries its request id")
        .to_string();
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["status"], 500, "{body}");
    assert_eq!(body["type"], crate::problem::INTERNAL_TYPE, "{body}");
    assert_eq!(body["request_id"], header_id.as_str(), "{body}");

    let errors: Vec<_> = lines
        .iter()
        .filter(|line| line.level == LogLevel::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{lines:#?}");
    assert!(
        errors[0].text.contains("the request's handler panicked"),
        "{}",
        errors[0].text
    );
    assert!(errors[0].text.contains(&header_id), "{}", errors[0].text);
}

/// Every `/v1` answer that sets no `Cache-Control` of its own carries
/// `no-store`, so neither a browser's disk cache nor a proxy keeps a Session
/// token, message text or a refusal (#2295). That holds for an answer a
/// handler gives, one an extractor refuses, and one a layer outside the
/// handler gives, CORS answering a preflight included. An asset read keeps
/// the `private` it sets itself, and the website and `/health`, outside
/// `/v1`, are left alone.
#[tokio::test]
async fn every_v1_answer_without_its_own_cache_control_is_no_store() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let site = fixture.dir().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), "<title>the site</title>").unwrap();
    let mut state = fixture.state.clone();
    let mut cfg = (*state.cfg).clone();
    cfg.server.as_mut().unwrap().static_dir = site;
    state.cfg = std::sync::Arc::new(cfg);
    let origin = "http://localhost:5173";
    let state = with_cors(state, &[origin]);
    let server = crate::test_support::serve(&state).await;
    let cache_control = |response: &reqwest::Response| {
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|v| v.to_str().unwrap().to_string())
    };

    let login = server
        .post("/v1/session")
        .json(&serde_json::json!({
            "username": user.username,
            "password": crate::test_support::PASSWORD,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::CREATED);
    let login_cache_control = cache_control(&login);
    // A new login ends the Session before it, so the rest use the new token.
    let token = login.json::<serde_json::Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let session = server
        .get("/v1/session")
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(session.status(), StatusCode::OK);
    let logged_out = server.get("/v1/session").send().await.unwrap();
    assert_eq!(logged_out.status(), StatusCode::UNAUTHORIZED);
    let asset_logged_out = server
        .get(&format!("/v1/assets/{}", "a".repeat(64)))
        .send()
        .await
        .unwrap();
    assert_eq!(asset_logged_out.status(), StatusCode::UNAUTHORIZED);
    let not_acceptable = server
        .get("/v1/session")
        .bearer_auth(&token)
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .unwrap();
    assert_eq!(not_acceptable.status(), StatusCode::NOT_ACCEPTABLE);
    let undeclared_query = server
        .get("/v1/session?unknown=1")
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(undeclared_query.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let wrong_method = server
        .patch("/v1/session")
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
    let preflight = server
        .request(reqwest::Method::OPTIONS, "/v1/session")
        .header("Origin", origin)
        .header("Access-Control-Request-Method", "GET")
        .header("Access-Control-Request-Headers", "authorization")
        .send()
        .await
        .unwrap();
    assert_eq!(allow_origin(&preflight), Some(origin));
    let unknown = server.get("/v1/no-such-route").send().await.unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    for (what, response) in [
        ("session", &session),
        ("logged out", &logged_out),
        ("asset logged out", &asset_logged_out),
        ("not acceptable", &not_acceptable),
        ("undeclared query", &undeclared_query),
        ("wrong method", &wrong_method),
        ("CORS preflight", &preflight),
        ("unknown route", &unknown),
    ] {
        assert_eq!(
            cache_control(response).as_deref(),
            Some("no-store"),
            "{what}"
        );
    }

    assert_eq!(login_cache_control.as_deref(), Some("no-store"), "login");

    let website = server.get("/").send().await.unwrap();
    assert_eq!(website.status(), StatusCode::OK);
    assert_eq!(cache_control(&website), None);
    let health = server.get("/health").send().await.unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(cache_control(&health), None);
}

/// #2073: the website and every `/v1` answer carry the protective headers,
/// so a script injected into the page cannot read the Session token out of
/// browser storage and send it away, and no other site can frame the login
/// card. A `/v1` refusal answered by a layer carries them as well as a
/// handler's answer.
#[tokio::test]
async fn the_website_and_every_v1_answer_carry_the_protective_headers() {
    let (fixture, _user) = crate::test_support::fixture_with_account().await;
    let site = fixture.dir().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), "<title>the site</title>").unwrap();
    let mut state = fixture.state.clone();
    let mut cfg = (*state.cfg).clone();
    cfg.server.as_mut().unwrap().static_dir = site;
    state.cfg = std::sync::Arc::new(cfg);
    let server = crate::test_support::serve(&state).await;

    let website = server.get("/").send().await.unwrap();
    assert_eq!(website.status(), StatusCode::OK);
    let server_info = server.get("/v1/server").send().await.unwrap();
    assert_eq!(server_info.status(), StatusCode::OK);
    let logged_out = server.get("/v1/session").send().await.unwrap();
    assert_eq!(logged_out.status(), StatusCode::UNAUTHORIZED);
    let unknown = server.get("/v1/no-such-route").send().await.unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    for (what, response) in [
        ("website", &website),
        ("server", &server_info),
        ("logged out", &logged_out),
        ("unknown route", &unknown),
    ] {
        let headers = response.headers();
        let policy = headers
            .get(header::CONTENT_SECURITY_POLICY)
            .map(|v| v.to_str().unwrap());
        assert_eq!(policy, Some(CONTENT_SECURITY_POLICY), "{what}");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{what}");
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer", "{what}");
    }
    for directive in [
        "script-src 'self';",
        "object-src 'none';",
        "frame-ancestors 'none'",
    ] {
        assert!(CONTENT_SECURITY_POLICY.contains(directive), "{directive}");
    }
}
