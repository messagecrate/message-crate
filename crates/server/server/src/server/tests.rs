use super::*;
use crate::extract::{Json, Path as AxumPath};
use crate::imports_api::ImportMode;
use crate::imports_api::{
    CompleteImportRequest, CreateImportRequest, DiscardImportRequest, ImportIssueRequest,
    UpdateImportRequest, complete_import, create_import, discard_import, get_import, list_imports,
    update_import,
};
use crate::logging::{LogLevel, LogLine};
use crate::problem::ProblemType;
use crate::test_support::{
    ImportingAccount, expect_problem, expect_problem_for, http_client, test_fixture,
};
use axum::extract::State;
use tempfile::TempDir;

#[test]
fn jsonl_content_type_accepts_x_ndjson() {
    assert!(is_jsonl_content_type("application/x-ndjson"));
    assert!(is_jsonl_content_type("application/jsonl"));
    assert!(is_jsonl_content_type("Application/X-NDJSON"));
    assert!(!is_jsonl_content_type("multipart/form-data"));
    assert!(!is_jsonl_content_type("application/json"));
}

#[tokio::test]
async fn run_blocking_answers_a_panicked_task_as_an_internal_error_naming_it() {
    let answer = run_blocking("upload read", || -> Result<(), ApiError> { panic!("boom") }).await;
    let Err(ApiError::Internal(err)) = answer else {
        panic!("expected an internal error, got {answer:?}");
    };
    assert!(err.to_string().starts_with("upload read task: "), "{err}");
}

#[test]
fn error_chain_keeps_every_context_layer() {
    let err = anyhow::Error::new(std::io::Error::other("disk full"))
        .context("write staging row")
        .context("stage conversation chat-1");
    assert_eq!(
        error_chain(&err),
        "stage conversation chat-1: write staging row: disk full"
    );
}

/// `sqlx::Error::Database` says "error returned from database:" and the
/// database's message, and gives that message again as its source, so the
/// chain names it once (#2675).
#[tokio::test]
async fn error_chain_names_a_database_message_once() {
    let fixture = test_fixture().await;
    let err = sqlx::query("SELECT * FROM no_such_table")
        .execute(&fixture.state.db)
        .await
        .unwrap_err();
    assert!(matches!(err, sqlx::Error::Database(_)), "{err:?}");
    let err = anyhow::Error::new(err).context("read the attachments of a page of messages");
    assert_eq!(
        error_chain(&err),
        "read the attachments of a page of messages: error returned from database: \
         (code: 1) no such table: no_such_table"
    );
}

/// A cause whose words only happen to end the step's phrase still says
/// something new, so the chain keeps it.
#[test]
fn error_chain_keeps_a_cause_that_ends_the_step_by_chance() {
    let err = anyhow::anyhow!("conversation").context("stage conversation");
    assert_eq!(error_chain(&err), "stage conversation: conversation");
}

#[test]
fn internal_error_keeps_the_chain_and_answers_500() {
    let err = ApiError::from(anyhow::anyhow!("disk full").context("stage conversation"));
    let ApiError::Internal(inner) = &err else {
        panic!("expected Internal, got {err:?}");
    };
    assert_eq!(format!("{inner:#}"), "stage conversation: disk full");
    assert_eq!(
        err.into_response().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

const TEST_ACCOUNT: i64 = 7;

#[tokio::test]
async fn api_token_cannot_exceed_its_owner() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    sqlx::query("UPDATE accounts SET can_import = 0 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();
    let created =
        api_tokens::create_api_token(&mut conn, TEST_ACCOUNT, "tool", Permissions::all(), None)
            .await
            .unwrap();

    let identity = resolve_auth_on_conn(&mut conn, &created.token, None)
        .await
        .unwrap();

    assert!(
        !identity.permissions().import,
        "the account lost import, so its token must not have it"
    );
    assert!(identity.permissions().export);
}

/// The schema is made sure of when the pool opens, and never by a request:
/// a database another program stamped with another fingerprint while this
/// server serves it is not rebuilt by the next request, which would empty
/// it from inside a request (#2064).
#[tokio::test]
async fn a_request_never_rebuilds_the_database() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let token = session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let other = crate::db::schema::SCHEMA_FINGERPRINT ^ 1;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA user_version = {other}"
    )))
    .execute(&mut *conn)
    .await
    .unwrap();

    resolve_auth_on_conn(&mut conn, &token, None).await.unwrap();

    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(accounts, 1, "the request left the database as it was");
    let stamped: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(stamped, other);
}

#[tokio::test]
async fn disabling_an_account_kills_its_live_session() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let token = session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();

    // The token works while the account is active.
    resolve_auth_on_conn(&mut conn, &token, None).await.unwrap();

    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();

    let err = resolve_auth_on_conn(&mut conn, &token, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApiError::AccountDisabled(_)),
        "a disabled account's existing token must stop working, got {err:?}"
    );
}

#[tokio::test]
async fn disabling_an_account_kills_its_live_api_token() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let created =
        api_tokens::create_api_token(&mut conn, TEST_ACCOUNT, "tool", Permissions::all(), None)
            .await
            .unwrap();
    let token = created.token;

    // The API token works while the account is active.
    resolve_auth_on_conn(&mut conn, &token, None).await.unwrap();

    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();

    let err = resolve_auth_on_conn(&mut conn, &token, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApiError::AccountDisabled(_)),
        "a disabled account's existing API token must stop working, got {err:?}"
    );
}

fn auth_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    headers
}

/// The account's running Import Run through `GET /v1/imports?status=running`,
/// as the desktop app finds it.
async fn running_import(
    state: &AppState,
    token: &str,
) -> Option<crate::imports_api::ImportRunSummary> {
    list_imports(
        State(state.clone()),
        import_access(state, token).await,
        crate::extract::Query(crate::imports_api::ListImportsQuery {
            sort: None,
            status: Some("running".into()),
            limit: None,
            offset: None,
        }),
    )
    .await
    .unwrap()
    .0
    .items
    .into_iter()
    .next()
}

/// Resolve the token the way the `ImportAccess` extractor would, for
/// tests that call import handlers directly instead of over HTTP.
async fn import_access(state: &AppState, token: &str) -> ImportAccess {
    let auth = resolve_auth(&auth_headers(token), state).await.unwrap();
    require_import_access(&auth).unwrap();
    ImportAccess(auth)
}

async fn get_path(state: AppState, path: &str) -> reqwest::Response {
    let server = crate::test_support::serve(&state).await;
    server.get(path).send().await.unwrap()
}

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

#[tokio::test]
async fn openapi_ui_off_does_not_serve_spec() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    assert!(!state.cfg.require_server().unwrap().openapi_ui);
    let response = get_path(state, "/openapi.json").await;
    assert_ne!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        "application/json"
    );
}

#[tokio::test]
async fn openapi_ui_on_serves_spec_without_token() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    {
        let cfg = Arc::make_mut(&mut state.cfg);
        cfg.server.as_mut().unwrap().openapi_ui = true;
    }
    let response = get_path(state, "/openapi.json").await;
    assert_eq!(response.status(), StatusCode::OK);
    let v: serde_json::Value = response.json().await.unwrap();
    assert!(v["openapi"].as_str().unwrap().starts_with("3."));
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

#[tokio::test]
async fn imports_complete_and_detail_surface_timings_and_issues() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let body = CompleteImportRequest {
        status: "completed".into(),
        bytes_uploaded: Some(100),
        duration_ms: Some(48_000),
        parse_ms: Some(18_000),
        attachments_ms: Some(22_000),
        prepare_ms: Some(4_000),
        upload_ms: Some(8_000),
        summary: Some(serde_json::json!({
            "parse": { "messages": 10 },
            "convert": { "files": 2 }
        })),
        issues: vec![
            ImportIssueRequest {
                kind: message_crate_api_types::RunIssueKind::Skip,
                stage: crate::db::imports::ImportIssueStage::Media,
                item: "photo.heic".into(),
                reason: "convert failed".into(),
            },
            ImportIssueRequest {
                kind: message_crate_api_types::RunIssueKind::Error,
                stage: crate::db::imports::ImportIssueStage::Upload,
                item: "archive.zip".into(),
                reason: "upload failed".into(),
            },
        ],
        notes: Vec::new(),
    };

    let response = complete_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap();
    assert_eq!(response.0.run.status.as_str(), "completed");
    // Counted from what the run holds, which is nothing here.
    assert_eq!(response.0.run.message_count, 0);
    assert_eq!(response.0.run.attachment_count, 0);
    assert_eq!(response.0.run.bytes_uploaded, 100);

    let detail = get_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
    )
    .await
    .unwrap();
    let value = detail.0;
    assert_eq!(value.run.id, import_id);
    assert_eq!(value.run.duration_ms, Some(48_000));
    assert_eq!(value.run.parse_ms, Some(18_000));
    assert_eq!(value.run.attachments_ms, Some(22_000));
    assert_eq!(value.run.prepare_ms, Some(4_000));
    assert_eq!(value.run.upload_ms, Some(8_000));
    assert_eq!(value.run.summary["parse"]["messages"], 10);
    assert_eq!(value.issues.len(), 2);
    assert_eq!(
        value.issues[0].kind,
        message_crate_api_types::RunIssueKind::Skip
    );
    assert_eq!(
        value.issues[0].stage,
        crate::db::imports::ImportIssueStage::Media
    );
    assert_eq!(
        value.issues[1].kind,
        message_crate_api_types::RunIssueKind::Error
    );
    assert_eq!(
        value.issues[1].stage,
        crate::db::imports::ImportIssueStage::Upload
    );
}

#[tokio::test]
async fn imports_complete_stores_completed_with_issues_status() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let body = CompleteImportRequest {
        status: "completed_with_issues".into(),
        bytes_uploaded: Some(100),
        duration_ms: None,
        parse_ms: None,
        attachments_ms: None,
        prepare_ms: None,
        upload_ms: None,
        summary: None,
        issues: Vec::new(),
        notes: Vec::new(),
    };
    let response = complete_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap();
    assert_eq!(response.0.run.status.as_str(), "completed_with_issues");
}

#[tokio::test]
async fn imports_complete_rejects_unknown_status() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let body = CompleteImportRequest {
        status: "victorious".into(),
        bytes_uploaded: None,
        duration_ms: None,
        parse_ms: None,
        attachments_ms: None,
        prepare_ms: None,
        upload_ms: None,
        summary: None,
        issues: Vec::new(),
        notes: Vec::new(),
    };
    let err = complete_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ApiError::ValidationFailed(_)));

    // The session is untouched.
    let mut conn = state.db.acquire().await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(status, "running");
}

/// A note belongs in the request's `notes`: sent as an issue, it is refused
/// before the run is written.
#[tokio::test]
async fn imports_complete_refuses_a_note_as_an_issue_before_db_write() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let body = CompleteImportRequest {
        status: "completed".into(),
        bytes_uploaded: Some(100),
        duration_ms: Some(48_000),
        parse_ms: Some(18_000),
        attachments_ms: Some(22_000),
        prepare_ms: Some(4_000),
        upload_ms: Some(8_000),
        summary: None,
        issues: vec![ImportIssueRequest {
            kind: message_crate_api_types::RunIssueKind::Note,
            stage: crate::db::imports::ImportIssueStage::Upload,
            item: "archive.zip".into(),
            reason: "not allowed".into(),
        }],
        notes: Vec::new(),
    };

    let err = complete_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap_err();

    match err {
        ApiError::ValidationFailed(errors) => {
            assert!(errors[0].contains("invalid import issue kind"));
        }
        other => panic!("expected validation-failed, got {other:?}"),
    }

    let status: String = sqlx::query_scalar("SELECT status FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(status, "running");
}

#[tokio::test]
async fn imports_get_handler_returns_not_found_for_missing_import() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let err = get_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id + 1),
    )
    .await
    .unwrap_err();

    match err {
        ApiError::NotFound(msg) => {
            assert!(msg.contains("import"));
            assert!(msg.contains("not found"));
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[tokio::test]
async fn active_import_run_is_empty_then_reports_the_live_one() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;

    let body = CreateImportRequest {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: Some("message-crate".into()),
        stage: Some(crate::db::imports::ImportStage::Parse),
        run_dir: Some("/home/u/message-crate/staging-260830".into()),
        device_id: Some("device-a".into()),
        form: Some(serde_json::json!({ "source": "imessage-ios" })),
        source_fingerprint: Some(serde_json::json!({ "size_bytes": 42 })),
        source_identities: None,
        phone_country: None,
    };
    // The fixture already started an Import Run; discard it so this one can start.
    let _ = discard_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(DiscardImportRequest {
            issues: Vec::new(),
            notes: Vec::new(),
        }),
    )
    .await
    .unwrap();

    let created = create_import(
        State(state.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(session.id, created.body.id);
    assert_eq!(session.stage, Some(crate::db::imports::ImportStage::Parse));
    assert_eq!(
        session.run_dir.as_deref(),
        Some("/home/u/message-crate/staging-260830")
    );
    assert_eq!(session.device_id.as_deref(), Some("device-a"));
    assert_eq!(session.form["source"], "imessage-ios");
}

/// A stored form snapshot never carries credentials, whatever the
/// client posts: the row outlives the run, and the secret must not.
#[tokio::test]
async fn a_stored_form_snapshot_drops_credentials() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let _ = discard_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(DiscardImportRequest {
            issues: Vec::new(),
            notes: Vec::new(),
        }),
    )
    .await
    .unwrap();

    let body = CreateImportRequest {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        stage: None,
        run_dir: None,
        device_id: None,
        // A client that has not learned the rule.
        form: Some(serde_json::json!({
            "source": "imessage-ios",
            "backupPassword": "hunter2",
            "whatsappKey": "0123456789abcdef",
        })),
        source_fingerprint: None,
        source_identities: None,
        phone_country: None,
    };
    let _ = create_import(
        State(state.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(
        session.form["source"], "imessage-ios",
        "the rest of the snapshot is kept"
    );
    assert!(
        session.form.get("backupPassword").is_none(),
        "backupPassword was stored: {}",
        session.form
    );
    assert!(
        session.form.get("whatsappKey").is_none(),
        "whatsappKey was stored: {}",
        session.form
    );
}

/// The identity list a client read from the backup rides on the Import Run
/// so a resumed Staging Review can show it without re-reading the backup.
#[tokio::test]
async fn imports_create_stores_source_identities() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let _ = discard_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(DiscardImportRequest {
            issues: Vec::new(),
            notes: Vec::new(),
        }),
    )
    .await
    .unwrap();

    let body = CreateImportRequest {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        stage: None,
        run_dir: None,
        device_id: None,
        form: None,
        source_fingerprint: None,
        source_identities: Some(serde_json::json!(["+15555550110", "owner@example.com"])),
        phone_country: None,
    };
    let _ = create_import(
        State(state.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(
        session.source_identities,
        serde_json::json!(["+15555550110", "owner@example.com"])
    );
}

#[tokio::test]
async fn a_second_import_run_is_refused_with_conflict() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, .. } = fixture.account_with_running_import().await;
    let body = CreateImportRequest {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        stage: None,
        run_dir: None,
        device_id: None,
        form: None,
        source_fingerprint: None,
        source_identities: None,
        phone_country: None,
    };
    let err = create_import(
        State(state.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap_err();
    let ApiError::StateConflict(message) = &err else {
        panic!("expected StateConflict, got {err:?}");
    };
    // The 409 has to name the way out: a stranded session is resumed or
    // discarded from the desktop app's Import screen, or discarded with
    // `message-crate-server imports discard`.
    assert!(
        message.contains("Import in the desktop app"),
        "the conflict names how to clear the session: {message}"
    );
}

#[tokio::test]
async fn stage_endpoint_advances() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;

    let _ = update_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(UpdateImportRequest {
            stage: crate::db::imports::ImportStage::Write,
            summary: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        running_import(&state, &token).await.unwrap().stage,
        Some(crate::db::imports::ImportStage::Write)
    );
}

#[tokio::test]
async fn discard_frees_the_slot() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let _ = discard_import(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(DiscardImportRequest {
            issues: Vec::new(),
            notes: Vec::new(),
        }),
    )
    .await
    .unwrap();
    assert!(running_import(&state, &token).await.is_none());
}

/// `/v1/contacts/{id}` takes an `i64`, and three literal routes sit beside
/// it: `summaries`, `unmatched-identities`, and `address-book`. This test
/// covers the first two. Each is a `POST`, and editing a contact is a `PATCH`, so if the `{id}` route ever swallowed one of them
/// the request would come back 405 (no `POST` on `/v1/contacts/{id}`)
/// instead of reaching its own handler. Each assertion below distinguishes
/// "matched my route and rejected my body" from "matched the wrong route".
#[tokio::test]
async fn literal_contact_routes_are_not_captured_by_the_id_route() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let user =
        crate::test_support::register_via_api(&state, "contact-routes", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::get_status(&state, "/v1/contacts", &user.token).await,
        StatusCode::OK
    );

    // A real id still reaches the detail handler: an unknown contact is its
    // 404, not a 400 from a failed `i64` path parse.
    let (status, text) =
        crate::test_support::get_raw(&state, "/v1/contacts/999999", &user.token).await;
    expect_problem(status, &text, ProblemType::NotFound);

    for path in [
        "/v1/contacts/summaries",
        "/v1/contacts/unmatched-identities",
    ] {
        let status =
            crate::test_support::post_status(&state, path, &user.token, serde_json::json!({}))
                .await;
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{path} was captured by /v1/contacts/{{id}}"
        );
    }
}

/// The `ImportAccess` extractor guards `GET /v1/imports`: with
/// `can_import` off, the endpoint refuses; turned back on, it succeeds.
#[tokio::test]
async fn import_endpoint_honors_can_import_flag() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let owner =
        crate::test_support::claim_as_owner(&state, "import-guard-keeper", "hunter2hunter2").await;
    let user =
        crate::test_support::register_via_api(&state, "import-guard-user", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_import": false }),
        )
        .await,
        StatusCode::OK
    );
    // can_import=false refuses GET /v1/imports.
    let (status, text) = crate::test_support::get_raw(&state, "/v1/imports", &user.token).await;
    expect_problem(status, &text, ProblemType::InsufficientScope);

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_import": true }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/imports", &user.token).await,
        StatusCode::OK,
        "can_import=true must allow GET /v1/imports"
    );
}

/// The `ExportAccess` extractor guards `GET /v1/exports`: with `can_export`
/// off, the endpoint refuses; turned back on, it succeeds.
#[tokio::test]
async fn export_endpoint_honors_can_export_flag() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let owner =
        crate::test_support::claim_as_owner(&state, "export-guard-keeper", "hunter2hunter2").await;
    let user =
        crate::test_support::register_via_api(&state, "export-guard-user", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": false }),
        )
        .await,
        StatusCode::OK
    );
    // can_export=false refuses GET /v1/exports.
    let (status, text) = crate::test_support::get_raw(&state, "/v1/exports", &user.token).await;
    expect_problem(status, &text, ProblemType::InsufficientScope);

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": true }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/exports", &user.token).await,
        StatusCode::OK,
        "can_export=true must allow GET /v1/exports"
    );
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

/// Every response carries an id the server made, a failure repeats it in the
/// body, and an id the client sends is dropped rather than kept.
#[tokio::test]
async fn every_response_carries_a_server_made_request_id_and_a_problem_repeats_it() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = fixture.state.clone();
    let server = crate::test_support::serve(&state).await;

    let ok = server
        .get("/v1/conversations")
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let id = ok.headers()[crate::request_id::HEADER]
        .to_str()
        .unwrap()
        .to_string();
    assert!(uuid::Uuid::parse_str(&id).is_ok(), "not a UUID: {id}");

    let failed = server
        .get("/v1/conversations/abc/sources")
        .bearer_auth(&user.token)
        .header(crate::request_id::HEADER, "chosen-by-the-client")
        .send()
        .await
        .unwrap();
    let header = failed.headers()[crate::request_id::HEADER]
        .to_str()
        .unwrap()
        .to_string();
    assert_ne!(header, "chosen-by-the-client");
    assert_eq!(
        failed.headers()[header::CONTENT_TYPE],
        crate::problem::Problem::CONTENT_TYPE
    );
    let status = failed.status();
    let text = failed.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(problem.request_id.as_deref(), Some(header.as_str()));
    assert!(problem.errors.is_some_and(|e| !e.is_empty()), "{text}");
}

/// A wrong password is `invalid-credentials` at `401 Unauthorized`, with no
/// `Retry-After`: the server waited before answering, so there is nothing to
/// wait for.
#[tokio::test]
async fn a_wrong_password_is_401_invalid_credentials() {
    let (fixture, _) = crate::test_support::fixture_with_account().await;
    let state = fixture.state.clone();
    let server = crate::test_support::serve(&state).await;
    let wrong = server
        .post("/v1/session")
        .json(&serde_json::json!({ "username": "alice", "password": "not-it-at-all" }))
        .send()
        .await
        .unwrap();
    assert!(!wrong.headers().contains_key(header::RETRY_AFTER));
    let status = wrong.status();
    let text = wrong.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::InvalidCredentials,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some("invalid username or password")
    );
}

/// Every `/v1` operation in the document refuses a query parameter it does
/// not declare, and accepts the ones it does: walked over the whole document
/// once, as `docs/architecture/http-api.md` asks of a rule every route
/// follows, so a new route is covered without anyone remembering it.
#[tokio::test]
async fn every_operation_refuses_a_query_parameter_it_does_not_declare() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let server = crate::test_support::serve(&state).await;
    let spec: serde_json::Value =
        serde_json::from_str(&crate::openapi::dump_openapi_json()).unwrap();

    let mut checked = 0;
    for (template, item) in spec["paths"].as_object().unwrap() {
        if !template.starts_with("/v1/") {
            continue;
        }
        // Any segment matches a route; the refusal comes before the handler
        // reads it, so the value never matters.
        let path = template
            .split('/')
            .map(|seg| if seg.starts_with('{') { "1" } else { seg })
            .collect::<Vec<_>>()
            .join("/");
        for (method, op) in item.as_object().unwrap() {
            let method = reqwest::Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let declared: Vec<&str> = op["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["in"] == "query")
                .filter_map(|p| p["name"].as_str())
                .collect();
            let response = server
                .request(method.clone(), &format!("{path}?no_such_parameter=1"))
                .send()
                .await
                .unwrap();
            let status = response.status();
            let text = response.text().await.unwrap();
            if method == reqwest::Method::HEAD {
                // A HEAD answer has no body to read a problem from.
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "HEAD {template}");
            } else {
                let problem = crate::test_support::expect_problem(
                    status,
                    &text,
                    crate::problem::ProblemType::ValidationFailed,
                );
                let errors = problem.errors.unwrap().join(" ");
                assert!(
                    errors.contains("no_such_parameter")
                        && declared.iter().all(|name| errors.contains(name)),
                    "{method} {template} must name the parameter and the ones it takes: {errors}"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 60, "walked only {checked} operations");
}

/// Two cases the walk above cannot reach. A path segment's name is not a
/// query parameter the route takes, and a `HEAD` the `GET` handler answers is
/// held to the `GET` operation's parameters.
#[tokio::test]
async fn a_path_parameters_name_and_a_head_request_are_held_to_the_declared_query() {
    let fixture = crate::test_support::test_fixture().await;
    let server = crate::test_support::serve(&fixture.state).await;

    let response = server.get("/v1/conversations/1?id=1").send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(
        problem.errors.unwrap(),
        ["unknown query parameter 'id'; this route takes no query parameters"]
    );

    let response = server
        .head("/v1/conversations?no_such_parameter=1")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
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

/// `/health` answers a probe that accepts only text, as a container health
/// check or a load balancer may send (#1221): it is outside `/v1`, so its
/// `Accept` is not checked.
#[tokio::test]
async fn health_answers_a_probe_that_accepts_only_text() {
    let fixture = crate::test_support::test_fixture().await;
    let server = crate::test_support::serve(&fixture.state).await;

    let response = server
        .get("/health")
        .header(header::ACCEPT, "text/plain")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "ok\n");
}

/// `/health` fails as a `/v1` call does when the database cannot be
/// reached, so a probe does not read a server as up while every `/v1` call
/// fails (#2202).
#[tokio::test]
async fn health_fails_when_the_database_cannot_be_reached() {
    let fixture = crate::test_support::test_fixture().await;
    let server = crate::test_support::serve(&fixture.state).await;
    fixture.state.db.close().await;

    let response = server.get("/health").send().await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
}

/// `/health` fails within its time limit when every pool connection is held,
/// rather than waiting for one as long as a `/v1` call would (#2202). No
/// connection is given back while it waits, so the answer does not depend on
/// how fast the machine is.
#[tokio::test]
async fn health_fails_when_every_pool_connection_is_held() {
    let fixture = crate::test_support::test_fixture().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let mut held = Vec::new();
    for _ in 0..fixture.state.db.options().get_max_connections() {
        held.push(fixture.state.db.acquire().await.unwrap());
    }

    let response = server.get("/health").send().await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    drop(held);
}

#[test]
fn every_api_error_answers_the_status_its_problem_type_declares() {
    let cases: Vec<(ApiError, StatusCode)> = vec![
        (
            ApiError::ValidationFailed(vec!["x".into()]),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (ApiError::MalformedBody("x".into()), StatusCode::BAD_REQUEST),
        (
            ApiError::UnsupportedMediaType("x".into()),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (ApiError::PayloadTooLarge, StatusCode::PAYLOAD_TOO_LARGE),
        (
            ApiError::InvalidCredentials("x".into()),
            StatusCode::UNAUTHORIZED,
        ),
        (
            ApiError::AuthenticationRequired("x".into()),
            StatusCode::UNAUTHORIZED,
        ),
        (ApiError::TokenRefused("x".into()), StatusCode::UNAUTHORIZED),
        (ApiError::UsernameTaken("x".into()), StatusCode::CONFLICT),
        (ApiError::NameTaken("x".into()), StatusCode::CONFLICT),
        (
            ApiError::DemoAccountProtected("x".into()),
            StatusCode::FORBIDDEN,
        ),
        (ApiError::NotTheOwner("x".into()), StatusCode::FORBIDDEN),
        (
            ApiError::RegistrationClosed("x".into()),
            StatusCode::FORBIDDEN,
        ),
        (
            ApiError::InsufficientScope("x".into()),
            StatusCode::FORBIDDEN,
        ),
        (ApiError::AccountDisabled("x".into()), StatusCode::FORBIDDEN),
        (
            ApiError::SearchQueryInvalid {
                detail: "x".into(),
                word: None,
                did_you_mean: None,
            },
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (ApiError::StateConflict("x".into()), StatusCode::CONFLICT),
        (
            ApiError::AssetUploadInvalid("x".into()),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (ApiError::NotFound("x".into()), StatusCode::NOT_FOUND),
        (
            ApiError::MethodNotAllowed("x".into()),
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            ApiError::NotAcceptable("x".into()),
            StatusCode::NOT_ACCEPTABLE,
        ),
        (
            ApiError::ServerStopping("x".into()),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            ApiError::from(sqlx::Error::PoolTimedOut),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            ApiError::Internal(anyhow::anyhow!("x")),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ];
    for (error, status) in cases {
        assert_eq!(error.status(), status, "{error:?}");
    }
}

/// Every `401` carries a `WWW-Authenticate` challenge, as RFC 9110, section
/// 15.5.2, requires, and names `invalid_token` (RFC 6750) only when the
/// request sent a Bearer token that was refused. No other status carries one.
#[test]
fn every_401_carries_a_bearer_challenge_and_nothing_else_does() {
    let challenge = |error: ApiError| {
        let response = error.into_response();
        response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .map(|v| (response.status(), v.to_str().unwrap().to_string()))
    };
    for error in [
        ApiError::AuthenticationRequired("x".into()),
        ApiError::InvalidCredentials("x".into()),
        ApiError::MediaLinkInvalid("x".into()),
    ] {
        assert_eq!(
            challenge(error),
            Some((StatusCode::UNAUTHORIZED, BEARER_CHALLENGE.to_string()))
        );
    }
    assert_eq!(
        challenge(ApiError::TokenRefused("x".into())),
        Some((
            StatusCode::UNAUTHORIZED,
            REFUSED_TOKEN_CHALLENGE.to_string()
        ))
    );
    assert_eq!(
        challenge(ApiError::account_gone()).map(|(_, value)| value),
        Some(REFUSED_TOKEN_CHALLENGE.to_string()),
        "a token whose account is gone was sent and refused"
    );
    for error in [
        ApiError::InsufficientScope("x".into()),
        ApiError::AccountDisabled("x".into()),
        ApiError::NotFound("x".into()),
    ] {
        assert_eq!(challenge(error), None);
    }
}

/// A request refused because another request to the same upload holds its
/// lock found the upload busy, not wrong: `409 state-conflict`, so a client
/// sends it again rather than blaming its bytes.
#[test]
fn an_asset_upload_held_by_another_request_is_a_state_conflict() {
    let error = ApiError::from(crate::assets_api::AssetError::Locked);
    assert_eq!(error.problem_type(), Some(ProblemType::StateConflict));
    assert_eq!(error.status(), StatusCode::CONFLICT);
}

#[test]
fn every_api_error_displays_its_detail_sentence() {
    assert_eq!(
        ApiError::ValidationFailed(vec!["name is required".into(), "name is too long".into()])
            .to_string(),
        "name is required; name is too long"
    );
    assert_eq!(
        ApiError::SearchQueryInvalid {
            detail: "unknown word: frm".into(),
            word: Some("frm"),
            did_you_mean: Some("from"),
        }
        .to_string(),
        "unknown word: frm"
    );
    assert_eq!(
        ApiError::not_found("API token").to_string(),
        "API token not found"
    );
    assert_eq!(
        ApiError::Internal(anyhow::anyhow!("disk full").context("stage conversation")).to_string(),
        "stage conversation: disk full"
    );
}

/// A variant that carries only a sentence answers with that sentence as the
/// `detail`, and displays the same sentence.
#[test]
fn every_message_variant_answers_its_sentence_as_the_detail() {
    let variants: Vec<fn(String) -> ApiError> = vec![
        ApiError::MalformedBody,
        ApiError::UnsupportedMediaType,
        ApiError::InvalidCredentials,
        ApiError::AuthenticationRequired,
        ApiError::TokenRefused,
        ApiError::UsernameTaken,
        ApiError::NameTaken,
        ApiError::DemoAccountProtected,
        ApiError::NotTheOwner,
        ApiError::RegistrationClosed,
        ApiError::InsufficientScope,
        ApiError::AccountDisabled,
        ApiError::StateConflict,
        ApiError::AssetUploadInvalid,
        ApiError::NotFound,
        ApiError::MethodNotAllowed,
        ApiError::NotAcceptable,
        ApiError::MediaLinkInvalid,
    ];
    for variant in variants {
        let error = variant("the sentence".into());
        assert_eq!(
            error.to_problem().detail.as_deref(),
            Some("the sentence"),
            "{error:?}"
        );
        assert_eq!(error.to_string(), "the sentence", "{error:?}");
    }
}

/// A request that waited for a pool connection until sqlx gave up is not a
/// bug in the server: it answers `503 Service Unavailable` with
/// `Retry-After`, whether the error arrives as it is or under the step that
/// hit it (#2084, #2124).
#[test]
fn a_pool_timeout_answers_database_busy_with_retry_after() {
    for error in [
        ApiError::from(sqlx::Error::PoolTimedOut),
        ApiError::from(anyhow::Error::new(sqlx::Error::PoolTimedOut).context("read the account")),
    ] {
        let problem = error.to_problem();
        assert_eq!(problem.kind, ProblemType::DatabaseBusy.url(), "{error:?}");
        assert_eq!(problem.status, 503);
        assert_eq!(
            problem.retry_after,
            Some(crate::problem::DATABASE_BUSY_RETRY_AFTER_SECS)
        );
        assert_eq!(
            problem.detail.as_deref(),
            Some("the database is busy, so try again in 5 seconds")
        );
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "5");
    }
}

/// A busy database leaves one `WARN` line, not an `ERROR`, and the line
/// names the step that met the lock, so a lock that never clears can be
/// traced (#2084).
#[test]
fn a_busy_database_leaves_a_warning_naming_the_step() {
    use crate::logging::{LogFiles, LogLinesQuery, SERVER_LOG_LIMITS, read_lines};

    let dir = TempDir::new().unwrap();
    let log_dir = dir.path().join("logs");
    let files = LogFiles::open(&log_dir, SERVER_LOG_LIMITS).unwrap();
    let error =
        ApiError::from(anyhow::Error::new(sqlx::Error::PoolTimedOut).context("read the account"));
    tracing::subscriber::with_default(crate::logging::subscriber_for(files, "info"), || {
        error.to_problem()
    });
    let lines = read_lines(
        &log_dir,
        &LogLinesQuery {
            limit: 10,
            ..LogLinesQuery::default()
        },
    )
    .unwrap()
    .0;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].level, LogLevel::Warn);
    assert!(lines[0].text.contains("database-busy"), "{}", lines[0].text);
    assert!(
        lines[0].text.contains("read the account"),
        "{}",
        lines[0].text
    );
}

/// A write that meets another connection's write lock until the busy
/// timeout runs out answers `503 Service Unavailable` (`database-busy`) with
/// `Retry-After`, not `500 Internal Server Error` (#2084). The pool's busy
/// timeout is zero, so SQLite answers `SQLITE_BUSY` at once and the test
/// waits on nothing.
#[tokio::test]
async fn a_write_that_meets_a_held_write_lock_answers_database_busy() {
    let fixture =
        crate::test_support::test_fixture_with_busy_timeout(std::time::Duration::ZERO).await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "alice", "hunter2hunter2").await;
    let server = crate::test_support::serve(&fixture.state).await;
    let mut writer = fixture.conn().await;
    sqlx::query(crate::db::engine::BEGIN_IMMEDIATE_SQL)
        .execute(&mut *writer)
        .await
        .unwrap();

    let response = server
        .post("/v1/saved-searches")
        .bearer_auth(&account.token)
        .json(&serde_json::json!({"name": "Ada", "query": "ada"}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.headers()[header::RETRY_AFTER], "5");
    let status = response.status();
    let text = response.text().await.unwrap();
    let problem = expect_problem(status, &text, ProblemType::DatabaseBusy);
    assert_eq!(problem.retry_after, Some(5));
    sqlx::query("ROLLBACK").execute(&mut *writer).await.unwrap();
}

#[test]
fn a_sqlx_error_becomes_an_internal_error_and_keeps_its_message() {
    let error = ApiError::from(sqlx::Error::RowNotFound);

    assert!(matches!(error, ApiError::Internal(_)), "{error:?}");
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        error.to_string(),
        "no rows returned by a query that expected to return at least one row"
    );
}

#[tokio::test]
async fn discard_body_drains_a_body_within_the_cap() {
    let body = axum::body::Body::from(vec![7u8; 1024]);

    let drained = discard_body(body, 1024).await;

    assert!(drained.is_ok(), "{drained:?}");
}

#[tokio::test]
async fn discard_body_refuses_a_body_over_the_cap() {
    let body = axum::body::Body::from(vec![7u8; 1025]);

    let error = discard_body(body, 1024).await.unwrap_err();

    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error.to_string(), "the request body is too large");
}

#[tokio::test]
async fn discard_body_reports_a_stream_that_fails_midway() {
    let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = vec![
        Ok(axum::body::Bytes::from_static(b"abc")),
        Err(std::io::Error::other("connection reset")),
    ];
    let body = axum::body::Body::from_stream(futures_util::stream::iter(chunks));

    let error = discard_body(body, 1024).await.unwrap_err();

    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error.to_string(), "failed to read body: connection reset");
}

/// A body sent in pieces passes the cap between two pieces without ever
/// standing exactly on it, as an upload with no declared length does.
#[tokio::test]
async fn a_body_streamed_to_a_file_is_refused_once_its_chunks_pass_the_cap() {
    let dir = TempDir::new().unwrap();
    let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = vec![
        Ok(axum::body::Bytes::from_static(b"four")),
        Ok(axum::body::Bytes::from_static(b"more")),
        Ok(axum::body::Bytes::from_static(b"last")),
    ];
    let body = axum::body::Body::from_stream(futures_util::stream::iter(chunks));

    let error = stream_body_to_file(body, &dir.path().join("upload"), 5)
        .await
        .unwrap_err();

    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

// ---------------------------------------------------------------------------
// The app a session connects with
// ---------------------------------------------------------------------------

fn app_headers(app: &str, version: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(APP_HEADER, app.parse().unwrap());
    headers.insert(APP_VERSION_HEADER, version.parse().unwrap());
    headers
}

#[test]
fn a_request_names_its_app_with_two_headers() {
    let app = connecting_app(&app_headers("desktop", "0.9.0+343fe0d8")).unwrap();
    assert_eq!(app.kind, session_tokens::AppKind::Desktop);
    assert_eq!(app.build, "0.9.0+343fe0d8");
}

#[test]
fn a_request_that_names_no_app_or_names_it_badly_records_nothing() {
    assert_eq!(connecting_app(&HeaderMap::new()), None);
    assert_eq!(connecting_app(&app_headers("toaster", "0.9.0")), None);
    assert_eq!(connecting_app(&app_headers("website", "")), None);
    assert_eq!(
        connecting_app(&app_headers("website", &"9".repeat(65))),
        None
    );
    let mut only_the_app = HeaderMap::new();
    only_the_app.insert(APP_HEADER, "website".parse().unwrap());
    assert_eq!(connecting_app(&only_the_app), None);
}

#[tokio::test]
async fn a_session_records_the_app_it_connects_with() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let token = session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();

    // A request that names no app is served and leaves the row as it was.
    resolve_auth_on_conn(&mut conn, &token, None).await.unwrap();
    assert_eq!(
        session_tokens::connecting_app_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap(),
        None
    );

    let desktop = session_tokens::ConnectingApp {
        kind: session_tokens::AppKind::Desktop,
        build: "0.9.0+343fe0d8".into(),
    };
    resolve_auth_on_conn(&mut conn, &token, Some(&desktop))
        .await
        .unwrap();
    assert_eq!(
        session_tokens::connecting_app_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap(),
        Some(desktop)
    );

    // The same session from an updated app: the row follows, no new login.
    let website = session_tokens::ConnectingApp {
        kind: session_tokens::AppKind::Website,
        build: "0.10.0".into(),
    };
    resolve_auth_on_conn(&mut conn, &token, Some(&website))
        .await
        .unwrap();
    assert_eq!(
        session_tokens::connecting_app_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap(),
        Some(website)
    );
}

/// An API token belongs to a program, not to either app, and has no session
/// row to write to.
#[tokio::test]
async fn an_api_token_records_no_app() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let created =
        api_tokens::create_api_token(&mut conn, TEST_ACCOUNT, "tool", Permissions::all(), None)
            .await
            .unwrap();

    let desktop = session_tokens::ConnectingApp {
        kind: session_tokens::AppKind::Desktop,
        build: "0.9.0".into(),
    };
    resolve_auth_on_conn(&mut conn, &created.token, Some(&desktop))
        .await
        .unwrap();

    assert_eq!(
        session_tokens::connecting_app_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap(),
        None
    );
}

/// Make every UPDATE of `table` fail, as it does when another connection
/// holds SQLite's write lock past `busy_timeout`. Reads still succeed.
async fn fail_updates_of(conn: &mut SqliteConnection, table: &str) {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TEMP TRIGGER fail_update_{table} BEFORE UPDATE ON main.{table}
         BEGIN SELECT RAISE(ABORT, 'database is locked'); END"
    )))
    .execute(&mut *conn)
    .await
    .unwrap();
}

/// A failed `last_accessed_at` write does not fail the request (#1189).
#[tokio::test]
async fn an_api_token_works_when_its_last_used_time_cannot_be_written() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let created =
        api_tokens::create_api_token(&mut conn, TEST_ACCOUNT, "tool", Permissions::all(), None)
            .await
            .unwrap();
    fail_updates_of(&mut conn, "account_api_tokens").await;

    let identity = resolve_auth_on_conn(&mut conn, &created.token, None)
        .await
        .expect("the token is still accepted");

    assert_eq!(identity.account_id, TEST_ACCOUNT);
}

/// A failed write of the connecting app does not fail the request (#1189).
#[tokio::test]
async fn a_session_works_when_its_connecting_app_cannot_be_written() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let token = session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    fail_updates_of(&mut conn, "account_session_tokens").await;
    let desktop = session_tokens::ConnectingApp {
        kind: session_tokens::AppKind::Desktop,
        build: "0.10.0".into(),
    };

    let identity = resolve_auth_on_conn(&mut conn, &token, Some(&desktop))
        .await
        .expect("the session is still accepted");

    assert_eq!(identity.account_id, TEST_ACCOUNT);
}

/// The website is served from the directory the config names, so a server
/// started somewhere other than beside a `static` directory, as the desktop
/// app's is (#970), still has its website.
#[tokio::test]
async fn the_website_is_served_from_the_configured_directory() {
    let fixture = crate::test_support::test_fixture().await;
    let site = fixture.dir().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), "<title>the site</title>").unwrap();
    let mut state = fixture.state.clone();
    let mut cfg = (*state.cfg).clone();
    cfg.server.as_mut().unwrap().static_dir = site;
    state.cfg = std::sync::Arc::new(cfg);

    let (status, body) = crate::test_support::get_raw(&state, "/", "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "<title>the site</title>");
}

/// The `Bearer` scheme matches in any case, because RFC 7235, section 2.1,
/// makes authentication schemes case-insensitive (#1211).
#[tokio::test]
async fn a_lower_case_bearer_scheme_is_accepted() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let response = server
        .get("/v1/session")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("bearer {}", user.token),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
}

/// A valid token under another scheme is still refused, so matching the
/// scheme without regard to case does not accept any word before the token.
#[tokio::test]
async fn a_valid_token_under_another_scheme_answers_401() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let response = server
        .get("/v1/session")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Basic {}", user.token),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 401);
}

/// No API token acts as the owner (ADR 0008). The route that issues tokens
/// refuses the owner, but a token row on the owner's account can still
/// exist: written by hand, or carried over in a restored database. A request
/// with it answers the same `401 Unauthorized` as a token the server never
/// issued, on a route the owner's session reaches, on one an import reaches,
/// and on logging out, and leaves no record of a use on the token.
#[tokio::test]
async fn an_api_token_on_the_owners_account_answers_401_like_an_unknown_token() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    crate::test_support::claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let created = {
        let mut conn = fixture.conn().await;
        api_tokens::create_api_token(
            &mut conn,
            account_profile::OWNER_ACCOUNT_ID,
            "by hand",
            Permissions::all(),
            None,
        )
        .await
        .unwrap()
    };

    let send = async |method: &str, path: &str, token: &str| match method {
        "GET" => crate::test_support::get_raw(&state, path, token).await,
        "DELETE" => crate::test_support::delete_raw(&state, path, token).await,
        other => panic!("no helper for {other}"),
    };
    for (method, path) in [
        ("GET", "/v1/session"),
        ("GET", "/v1/accounts"),
        ("GET", "/v1/imports"),
        ("DELETE", "/v1/session"),
    ] {
        let (status, text) = send(method, path, &created.token).await;
        let owners = expect_problem_for(
            &format!("{method} {path} with a token on the owner's account"),
            status,
            &text,
            ProblemType::AuthenticationRequired,
        );
        let (status, text) = send(method, path, "mc-api-never-issued").await;
        let unknown = expect_problem(status, &text, ProblemType::AuthenticationRequired);
        assert_eq!(owners.detail, unknown.detail, "{method} {path}");
    }

    let last_used: Option<String> =
        sqlx::query_scalar("SELECT last_accessed_at FROM account_api_tokens WHERE id = $1")
            .bind(created.id)
            .fetch_one(&mut *fixture.conn().await)
            .await
            .unwrap();
    assert_eq!(last_used, None, "a refused token records no use");
}

/// `docker stop` and a service manager send SIGTERM. The server must drain a
/// request in flight and then exit, as it does on Ctrl-C (#1218). Every step
/// is awaited without a time limit, so a hang is left to the test runner and
/// a slow machine cannot fail it (#2975).
#[cfg(unix)]
#[tokio::test]
async fn sigterm_drains_the_request_in_flight_then_stops_the_server() {
    use tokio::signal::unix::{SignalKind, signal};
    use tokio::sync::{Notify, oneshot};

    // Installed before the signal is sent, so a server that ignores SIGTERM
    // leaves the test waiting rather than killing the test process.
    let _sigterm_caught = signal(SignalKind::terminate()).unwrap();

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let app = Router::new().route(
        "/slow",
        axum::routing::get({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move || async move {
                entered.notify_one();
                release.notified().await;
                "done"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // The server's own stop signal, which says when it has seen SIGTERM.
    let (stopping_tx, stopping_rx) = oneshot::channel::<()>();
    // A drain limit that never passes, so only the request's end lets the
    // server stop.
    let server = crate::request_id::spawn_in_current_span(serve_until_shutdown(
        listener,
        app,
        async move {
            shutdown_signal(None).await;
            let _ = stopping_tx.send(());
        },
        || async {},
        std::future::pending::<()>,
    ));

    let request = crate::request_id::spawn_in_current_span(async move {
        http_client()
            .get(format!("http://{addr}/slow"))
            .send()
            .await
            .unwrap()
    });
    // The server polls its stop signal before it accepts a connection, so
    // once the request is in the handler, the server is listening for
    // SIGTERM.
    entered.notified().await;

    let status = std::process::Command::new("kill")
        .args(["-TERM", &std::process::id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    stopping_rx.await.expect("the server saw SIGTERM");

    // The request is still in flight, so the server must still be running.
    assert!(
        !server.is_finished(),
        "the server stopped before the request in flight finished"
    );

    release.notify_one();
    let response = request.await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "done");

    let stopped = server.await.unwrap();
    assert!(matches!(stopped, Stopped::Drained), "{stopped:?}");
}

/// A request that does not finish holds a stopping server only until the
/// drain limit passes, and the server's log then names it (#2122). The limit
/// is a future the test resolves, so nothing here waits on the clock.
#[tokio::test]
async fn a_stopping_server_gives_up_on_a_request_still_running_at_the_drain_limit() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::{Notify, oneshot};

    let in_flight = crate::in_flight::InFlight::default();
    let entered = Arc::new(Notify::new());
    let app = Router::new()
        .route(
            "/never",
            axum::routing::post({
                let entered = Arc::clone(&entered);
                move || async move {
                    entered.notify_one();
                    std::future::pending::<&'static str>().await
                }
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            in_flight.clone(),
            crate::in_flight::track,
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (limit_tx, limit_rx) = oneshot::channel::<()>();
    let limit_started = Arc::new(AtomicBool::new(false));
    let server = crate::request_id::spawn_in_current_span(serve_until_shutdown(
        listener,
        app,
        async move {
            let _ = stop_rx.await;
        },
        || async {},
        {
            let limit_started = Arc::clone(&limit_started);
            move || {
                limit_started.store(true, Ordering::SeqCst);
                async move {
                    let _ = limit_rx.await;
                }
            }
        },
    ));

    let request = crate::request_id::spawn_in_current_span(async move {
        http_client()
            .post(format!("http://{addr}/never"))
            .header(crate::request_id::HEADER, "req-never")
            .send()
            .await
    });
    entered.notified().await;
    assert!(
        !limit_started.load(Ordering::SeqCst),
        "the drain limit counts from the stop, not from the start"
    );

    stop_tx.send(()).unwrap();
    limit_tx.send(()).unwrap();
    let stopped = server.await.unwrap();
    assert!(matches!(stopped, Stopped::GaveUp), "{stopped:?}");
    assert!(limit_started.load(Ordering::SeqCst));

    let lines = log_lines_of(|| stopped.log(&in_flight));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines.iter().all(|line| line.level == LogLevel::Warn),
        "{lines:?}"
    );
    assert!(
        lines[0].text.contains("stopped waiting 30 seconds")
            && lines[0].text.contains("Requests still running: 1."),
        "{}",
        lines[0].text
    );
    assert!(
        lines[1].text.contains("request req-never, POST /never"),
        "{}",
        lines[1].text
    );
    request.abort();
}

/// The work the stop signal starts, such as a Demo Account build's cleanup,
/// holds the stop within the same limit as the requests in flight, so no
/// work after the limit can outlast Docker's stop time (#2122).
#[tokio::test]
async fn a_stopping_server_gives_up_on_its_own_work_at_the_drain_limit() {
    use tokio::sync::oneshot;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (limit_tx, limit_rx) = oneshot::channel::<()>();
    let (work_tx, work_rx) = oneshot::channel::<()>();
    let server = crate::request_id::spawn_in_current_span(serve_until_shutdown(
        listener,
        Router::new(),
        async move {
            let _ = stop_rx.await;
        },
        move || async move {
            let _ = work_tx.send(());
            std::future::pending::<()>().await;
        },
        move || async move {
            let _ = limit_rx.await;
        },
    ));

    stop_tx.send(()).unwrap();
    work_rx.await.expect("the stop signal started the work");
    limit_tx.send(()).unwrap();
    let stopped = server.await.unwrap();
    assert!(matches!(stopped, Stopped::GaveUp), "{stopped:?}");
}

/// A stop with no request in flight still waits for the work the signal
/// started, and then counts as drained.
#[tokio::test]
async fn a_stopping_server_waits_for_its_own_work() {
    use tokio::sync::oneshot;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (started_tx, started_rx) = oneshot::channel::<()>();
    let (finish_tx, finish_rx) = oneshot::channel::<()>();
    let server = crate::request_id::spawn_in_current_span(serve_until_shutdown(
        listener,
        Router::new(),
        async move {
            let _ = stop_rx.await;
        },
        move || async move {
            let _ = started_tx.send(());
            let _ = finish_rx.await;
        },
        std::future::pending::<()>,
    ));

    stop_tx.send(()).unwrap();
    started_rx.await.expect("the stop signal started the work");
    assert!(
        !server.is_finished(),
        "the server stopped before the work the signal started ended"
    );
    finish_tx.send(()).unwrap();
    let stopped = server.await.unwrap();
    assert!(matches!(stopped, Stopped::Drained), "{stopped:?}");
}

/// The server log's lines, oldest first, while `f` runs.
fn log_lines_of(f: impl FnOnce()) -> Vec<LogLine> {
    use crate::logging::{LogFiles, LogLinesQuery, SERVER_LOG_LIMITS, read_lines};

    let dir = TempDir::new().unwrap();
    let log_dir = dir.path().join("logs");
    let files = LogFiles::open(&log_dir, SERVER_LOG_LIMITS).unwrap();
    tracing::subscriber::with_default(crate::logging::subscriber_for(files, "info"), f);
    let mut lines = read_lines(
        &log_dir,
        &LogLinesQuery {
            limit: 10,
            ..LogLinesQuery::default()
        },
    )
    .unwrap()
    .0;
    lines.reverse();
    lines
}

/// `docker stop` kills a container that has not exited when its stop time
/// is up, so every way the documentation starts a Message Crate in Docker
/// must give the server longer than its own drain limit (#2122): a kill
/// would cut off what the limit lets finish, and leave no line in the log
/// saying why.
#[test]
fn docker_stop_waits_longer_than_the_drain() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"))
    };
    let limit = STOP_DRAIN_LIMIT.as_secs();
    let longer = |seconds: &str, place: &str| {
        let seconds: u64 = seconds
            .parse()
            .unwrap_or_else(|_| panic!("{place}: {seconds:?} is not a whole number of seconds"));
        assert!(
            seconds > limit,
            "{place} gives the server {seconds} seconds to stop, not more than its {limit}-second drain limit"
        );
    };

    for compose in ["docker/compose.yml", "docker/compose.release.yml"] {
        let text = read(compose);
        let periods: Vec<&str> = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix("stop_grace_period:"))
            .map(|value| value.trim().trim_end_matches('s'))
            .collect();
        assert_eq!(
            periods.len(),
            1,
            "{compose} must set stop_grace_period once, in seconds"
        );
        longer(periods[0], compose);
    }

    let docs = "docs/src/content/docs/docs/user/features/owner";
    let mut started = 0;
    for page in [
        "run-with-docker.md",
        "run-on-another-machine.md",
        "update.md",
    ] {
        let path = format!("{docs}/{page}");
        let text = read(&path);
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            let place = format!("{path}:{}", at + 1);
            if line.starts_with("docker run -d ") {
                started += 1;
                let timeout = lines[at..]
                    .iter()
                    .take_while(|line| !line.starts_with("```"))
                    .find_map(|line| line.trim().strip_prefix("--stop-timeout "))
                    .unwrap_or_else(|| panic!("{place} starts a server without --stop-timeout"));
                longer(timeout.trim_end_matches('\\').trim(), &place);
            } else if let Some(rest) = line.strip_prefix("docker stop ") {
                let timeout = rest
                    .strip_prefix("-t ")
                    .and_then(|rest| rest.split_whitespace().next())
                    .unwrap_or_else(|| panic!("{place} stops the server without -t"));
                longer(timeout, &place);
            }
        }
    }
    assert_eq!(
        started, 3,
        "the pages that start a server in Docker moved, or one was added"
    );
}

/// Every `docker run` command in the user guide that starts a server carries
/// the four limits both Compose files set (#2655): no Linux capability, no
/// privilege gain, a read-only root filesystem, and `/tmp` in memory. The
/// three pages carry the command separately, so this keeps each copy from
/// losing a limit. A command is found wherever it starts on a line and
/// however it is split across lines, and only one that runs the Message
/// Crate image counts.
#[test]
fn user_guide_docker_run_commands_limit_the_container() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let limits = [
        "--cap-drop ALL",
        "--security-opt no-new-privileges:true",
        "--read-only",
        "--tmpfs /tmp",
    ];
    let docs = "docs/src/content/docs/docs/user/features/owner";
    let mut started = 0;
    for page in [
        "run-with-docker.md",
        "run-on-another-machine.md",
        "update.md",
    ] {
        let path = format!("{docs}/{page}");
        let text = std::fs::read_to_string(root.join(&path))
            .unwrap_or_else(|e| panic!("read {path}: {e}"));
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("docker run ") {
                continue;
            }
            // The command runs on while a line ends in `\`.
            let mut words: Vec<&str> = Vec::new();
            for part in &lines[at..] {
                let part = part.trim();
                words.extend(part.trim_end_matches('\\').split_whitespace());
                if !part.ends_with('\\') {
                    break;
                }
            }
            if !words
                .iter()
                .any(|word| word.starts_with("bitrealm/message-crate"))
            {
                continue;
            }
            started += 1;
            let command = words.join(" ");
            for limit in limits {
                assert!(
                    command.contains(&format!(" {limit} ")),
                    "{path}:{} starts a server without {limit}",
                    at + 1
                );
            }
        }
    }
    assert_eq!(
        started, 3,
        "the pages that start a server in Docker moved, or one was added"
    );
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
            .service(message_ir::IrService::Whatsapp)
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

// ---------------------------------------------------------------------------
// A body with no declared length over its cap (#1219)
// ---------------------------------------------------------------------------

/// A body of 8 bytes in two chunks, wrapped in `Limited` at `cap` the way
/// `limit_request_body` wraps a body with no `Content-Length`.
fn limited_chunked_body(cap: usize) -> axum::body::Body {
    let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = vec![
        Ok(axum::body::Bytes::from_static(b"four")),
        Ok(axum::body::Bytes::from_static(b"more")),
    ];
    let inner = axum::body::Body::from_stream(futures_util::stream::iter(chunks));
    axum::body::Body::new(http_body_util::Limited::new(inner, cap))
}

/// `Limited` cuts the body off at the same cap the reader holds it to, so the
/// reader sees `Limited`'s error before its own size check runs. That error
/// means the body is too large, not that it is malformed.
#[tokio::test]
async fn a_limited_body_streamed_to_a_file_over_the_cap_is_too_large() {
    let dir = TempDir::new().unwrap();

    let error = stream_body_to_file(limited_chunked_body(5), &dir.path().join("upload"), 5)
        .await
        .unwrap_err();

    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE, "{error}");
}

/// Upload time limits other than the defaults, so a test shows the
/// configured limits are the ones applied.
const TEST_UPLOAD_TIMES: UploadTimeLimits = UploadTimeLimits {
    stall: std::time::Duration::from_secs(7),
    piece: std::time::Duration::from_secs(45),
};

/// The defaults are the ones `config-and-accounts.md` states, and an
/// `AppState` holds the limits its `[server]` section sets.
#[test]
fn the_upload_time_limits_come_from_the_server_section() {
    let defaults = UploadTimeLimits::of(&crate::config::ServerConfig::default());
    assert_eq!(defaults.stall.as_secs(), 60);
    assert_eq!(defaults.piece.as_secs(), 300);

    let set = crate::config::ServerConfig {
        upload_stall_secs: 7,
        upload_piece_secs: 45,
        ..crate::config::ServerConfig::default()
    };
    assert_eq!(UploadTimeLimits::of(&set), TEST_UPLOAD_TIMES);
}

/// A body that stops arriving part-way ends the read once the stall limit
/// passes, as `upload-stalled` naming the limit, so an upload from a client
/// that went away never holds its import slot or its file for good. Before
/// #2897 it answered `malformed-body`, as though the data were bad. Time is
/// paused, so the limit passes as soon as nothing else can run.
#[tokio::test(start_paused = true)]
async fn a_body_that_stops_arriving_is_upload_stalled_at_the_stall_limit() {
    let dir = TempDir::new().unwrap();
    let first: Result<axum::body::Bytes, std::io::Error> =
        Ok(axum::body::Bytes::from_static(b"{}\n"));
    let stalled = futures_util::stream::iter([first]).chain(futures_util::stream::pending());
    let body = upload_body_bounded(axum::body::Body::from_stream(stalled), TEST_UPLOAD_TIMES);
    let started = tokio::time::Instant::now();

    let error = stream_body_to_file(body, &dir.path().join("upload"), 1024)
        .await
        .unwrap_err();

    assert_eq!(started.elapsed(), TEST_UPLOAD_TIMES.stall);
    assert_eq!(
        error.problem_type(),
        Some(ProblemType::UploadStalled),
        "{error}"
    );
    assert_eq!(error.status(), StatusCode::REQUEST_TIMEOUT, "{error}");
    let text = error.to_string();
    assert!(
        text.contains("no bytes of the body arrived for 7 seconds"),
        "{text}"
    );
    assert!(
        text.contains("sending the same request again can succeed"),
        "{text}"
    );
}

/// A body that sends a byte every 5 seconds never meets the stall limit,
/// but ends at the limit for the whole body, so a client that trickles
/// cannot hold an import slot for good either. Time is paused, so each wait
/// passes as soon as nothing else can run.
#[tokio::test(start_paused = true)]
async fn a_body_that_trickles_is_upload_stalled_at_the_piece_limit() {
    let dir = TempDir::new().unwrap();
    let trickle = futures_util::stream::unfold((), |()| async {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let byte: Result<axum::body::Bytes, std::io::Error> =
            Ok(axum::body::Bytes::from_static(b"\n"));
        Some((byte, ()))
    });
    let body = upload_body_bounded(axum::body::Body::from_stream(trickle), TEST_UPLOAD_TIMES);
    let started = tokio::time::Instant::now();

    let error = stream_body_to_file(body, &dir.path().join("upload"), 1024 * 1024)
        .await
        .unwrap_err();

    assert_eq!(started.elapsed(), TEST_UPLOAD_TIMES.piece);
    assert_eq!(
        error.problem_type(),
        Some(ProblemType::UploadStalled),
        "{error}"
    );
    let text = error.to_string();
    assert!(
        text.contains("the body was still arriving 45 seconds after it started"),
        "{text}"
    );
}

/// A body streamed to a file, as an attachment and a multipart part are,
/// and one read and dropped, as an attachment the server already holds is,
/// answer `upload-stalled` the same way.
#[tokio::test(start_paused = true)]
async fn every_upload_read_answers_upload_stalled() {
    let stalled = || {
        upload_body_bounded(
            axum::body::Body::from_stream(futures_util::stream::pending::<
                Result<axum::body::Bytes, std::io::Error>,
            >()),
            TEST_UPLOAD_TIMES,
        )
    };

    let dir = TempDir::new().unwrap();
    let read = stream_body_to_file(stalled(), &dir.path().join("upload"), 1024)
        .await
        .unwrap_err();
    let discarded = discard_body(stalled(), 1024).await.unwrap_err();

    assert_eq!(
        read.problem_type(),
        Some(ProblemType::UploadStalled),
        "{read}"
    );
    assert_eq!(
        discarded.problem_type(),
        Some(ProblemType::UploadStalled),
        "{discarded}"
    );
}

/// Send `request` through the server's layers around `routes`, with the
/// test's upload time limits, and return the answer's status and text, and
/// how long the answer took on the paused clock.
async fn answer_with_test_upload_times(
    routes: Router<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> (StatusCode, String, std::time::Duration) {
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.upload_time_limits = TEST_UPLOAD_TIMES;
    let app = with_server_layers(routes, state);
    // Paused only now, so building the fixture ran on a clock that moves.
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let answer = async {
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        (
            status,
            response.into_body().collect().await.unwrap().to_bytes(),
        )
    };
    let (status, bytes) = tokio::time::timeout(TEST_UPLOAD_TIMES.piece * 4, answer)
        .await
        .expect("the server was still reading the body long after every limit");
    let took = started.elapsed();
    (status, String::from_utf8(bytes.to_vec()).unwrap(), took)
}

/// A JSON body that stops arriving part-way is held to the upload time
/// limits as an upload's is, and answers `upload-stalled` at the stall limit
/// (#2100). Before, any body but an upload's could take as long as its
/// client liked, holding its connection all the while. Time is paused, so
/// the limit passes as soon as nothing else can run.
#[tokio::test]
async fn a_json_body_that_stops_arriving_is_upload_stalled_at_the_stall_limit() {
    let routes = Router::new().route(
        "/v1/echo",
        axum::routing::post(|Json(value): Json<serde_json::Value>| async move { Json(value) }),
    );
    let first: Result<axum::body::Bytes, std::io::Error> =
        Ok(axum::body::Bytes::from_static(b"{\"name\": "));
    let stalled = futures_util::stream::iter([first]).chain(futures_util::stream::pending());
    let request = axum::http::Request::post("/v1/echo")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from_stream(stalled))
        .unwrap();

    let (status, text, took) = answer_with_test_upload_times(routes, request).await;

    // The timer wheel counts whole milliseconds, and the clock was paused
    // part-way through one.
    assert!(
        took >= TEST_UPLOAD_TIMES.stall
            && took < TEST_UPLOAD_TIMES.stall + std::time::Duration::from_secs(1),
        "{took:?}"
    );
    let problem = expect_problem(status, &text, ProblemType::UploadStalled);
    assert!(
        problem
            .detail
            .is_some_and(|detail| detail.contains("no bytes of the body arrived for 7 seconds")),
        "{text}"
    );
}

/// An import batch holds its own body to time, from when it starts reading,
/// because it waits for an import slot first. A clock the layers started
/// when the request arrived would count that wait against the body, and
/// refuse a batch that waited longer than the limit for a whole body.
#[tokio::test]
async fn an_import_batch_body_is_timed_from_its_own_read_not_its_arrival() {
    let routes = Router::new().route(
        "/v1/imports/{id}/batches",
        axum::routing::post(|request: axum::extract::Request| async move {
            tokio::time::sleep(TEST_UPLOAD_TIMES.piece * 2).await;
            read_body_limited(request.into_body(), 1024)
                .await
                .map(|body| body.len().to_string())
        }),
    );
    // The batch's one line arrives a second after the read asks for it.
    let line = futures_util::stream::once(async {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"{}\n"))
    });
    let request = axum::http::Request::post("/v1/imports/7/batches")
        .header(header::CONTENT_TYPE, "application/jsonl")
        .body(axum::body::Body::from_stream(line))
        .unwrap();

    let (status, text, _) = answer_with_test_upload_times(routes, request).await;

    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(text, "3");
}

/// The limits pass `Limited`'s error through, so a body with no declared
/// length over its cap is still too large, not stalled or malformed.
#[tokio::test]
async fn a_bounded_upload_body_over_the_cap_is_too_large() {
    let dir = TempDir::new().unwrap();
    let body = upload_body_bounded(limited_chunked_body(5), TEST_UPLOAD_TIMES);

    let error = stream_body_to_file(body, &dir.path().join("upload"), 5)
        .await
        .unwrap_err();

    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE, "{error}");
}

#[tokio::test]
async fn a_limited_body_read_into_memory_over_the_cap_is_too_large() {
    let error = read_body_limited(limited_chunked_body(5), 5)
        .await
        .unwrap_err();

    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE, "{error}");
}

#[tokio::test]
async fn a_limited_body_discarded_over_the_cap_is_too_large() {
    let error = discard_body(limited_chunked_body(5), 5).await.unwrap_err();

    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE, "{error}");
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
    state.asset_part_size = 16;
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

/// The Demo Account's refusal is one sentence in lowercase with no final
/// period, the form `docs/architecture/http-api.md`, "Failures", gives every
/// `detail`, and keeps "Demo Account" capitalized (#2817). Any other account
/// passes.
#[test]
fn the_demo_account_refusal_is_one_lowercase_sentence() {
    let demo = account_profile::DEMO_ACCOUNT_ID;
    let refusal = refuse_for_demo_account(demo, "password cannot be set")
        .expect_err("the Demo Account is refused");
    let ApiError::DemoAccountProtected(detail) = refusal else {
        panic!("not demo-account-protected: {refusal:?}");
    };
    assert_eq!(detail, "the Demo Account's password cannot be set");
    assert!(refuse_for_demo_account(demo + 1, "password cannot be set").is_ok());
}
