use super::*;
use crate::logging::LogLevel;
use crate::problem::ProblemType;
use crate::test_support::{expect_problem, test_fixture};
use tempfile::TempDir;

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
        (ApiError::NotTheAdmin("x".into()), StatusCode::FORBIDDEN),
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
        ApiError::NotTheAdmin,
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
        crate::test_support::add_account_and_log_in(&fixture.state, "alice", "hunter2hunter2")
            .await;
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
