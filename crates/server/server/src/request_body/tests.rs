use super::*;
use axum::Router;
use axum::http::StatusCode;

use crate::extract::Json;
use crate::problem::ProblemType;
use crate::server::{AppState, with_server_layers};
use crate::test_support::{expect_problem, test_fixture};
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

/// Body time limits other than the defaults, so a test shows the
/// configured limits are the ones applied.
const TEST_BODY_TIMES: BodyTimeLimits = BodyTimeLimits {
    stall: std::time::Duration::from_secs(7),
    total: std::time::Duration::from_secs(45),
};

/// The defaults are the ones `config-and-accounts.md` states, and an
/// `AppState` holds the limits its `[server]` section sets.
#[test]
fn the_body_time_limits_come_from_the_server_section() {
    let defaults = BodyTimeLimits::of(&crate::config::ServerConfig::default());
    assert_eq!(defaults.stall.as_secs(), 60);
    assert_eq!(defaults.total.as_secs(), 300);

    let set = crate::config::ServerConfig {
        body_stall_secs: 7,
        body_total_secs: 45,
        ..crate::config::ServerConfig::default()
    };
    assert_eq!(BodyTimeLimits::of(&set), TEST_BODY_TIMES);
}

/// A body that stops arriving part-way ends the read once the stall limit
/// passes, as `body-stalled` naming the limit, so an upload from a client
/// that went away never holds its import slot or its file for good. Before
/// #2897 it answered `malformed-body`, as though the data were bad. Time is
/// paused, so the limit passes as soon as nothing else can run.
#[tokio::test(start_paused = true)]
async fn a_body_that_stops_arriving_is_body_stalled_at_the_stall_limit() {
    let dir = TempDir::new().unwrap();
    let first: Result<axum::body::Bytes, std::io::Error> =
        Ok(axum::body::Bytes::from_static(b"{}\n"));
    let stalled = futures_util::stream::iter([first]).chain(futures_util::stream::pending());
    let body = body_time_bounded(axum::body::Body::from_stream(stalled), TEST_BODY_TIMES);
    let started = tokio::time::Instant::now();

    let error = stream_body_to_file(body, &dir.path().join("upload"), 1024)
        .await
        .unwrap_err();

    assert_eq!(started.elapsed(), TEST_BODY_TIMES.stall);
    assert_eq!(
        error.problem_type(),
        Some(ProblemType::BodyStalled),
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
async fn a_body_that_trickles_is_body_stalled_at_the_total_limit() {
    let dir = TempDir::new().unwrap();
    let trickle = futures_util::stream::unfold((), |()| async {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let byte: Result<axum::body::Bytes, std::io::Error> =
            Ok(axum::body::Bytes::from_static(b"\n"));
        Some((byte, ()))
    });
    let body = body_time_bounded(axum::body::Body::from_stream(trickle), TEST_BODY_TIMES);
    let started = tokio::time::Instant::now();

    let error = stream_body_to_file(body, &dir.path().join("upload"), 1024 * 1024)
        .await
        .unwrap_err();

    assert_eq!(started.elapsed(), TEST_BODY_TIMES.total);
    assert_eq!(
        error.problem_type(),
        Some(ProblemType::BodyStalled),
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
/// answer `body-stalled` the same way.
#[tokio::test(start_paused = true)]
async fn every_bounded_read_answers_body_stalled() {
    let stalled = || {
        body_time_bounded(
            axum::body::Body::from_stream(futures_util::stream::pending::<
                Result<axum::body::Bytes, std::io::Error>,
            >()),
            TEST_BODY_TIMES,
        )
    };

    let dir = TempDir::new().unwrap();
    let read = stream_body_to_file(stalled(), &dir.path().join("upload"), 1024)
        .await
        .unwrap_err();
    let discarded = discard_body(stalled(), 1024).await.unwrap_err();

    assert_eq!(
        read.problem_type(),
        Some(ProblemType::BodyStalled),
        "{read}"
    );
    assert_eq!(
        discarded.problem_type(),
        Some(ProblemType::BodyStalled),
        "{discarded}"
    );
}

/// Send `request` through the server's layers around `routes`, with the
/// test's body time limits, and return the answer's status and text, and
/// how long the answer took on the paused clock.
async fn answer_with_test_body_times(
    routes: Router<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> (StatusCode, String, std::time::Duration) {
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.body_time_limits = TEST_BODY_TIMES;
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
    let (status, bytes) = tokio::time::timeout(TEST_BODY_TIMES.total * 4, answer)
        .await
        .expect("the server was still reading the body long after every limit");
    let took = started.elapsed();
    (status, String::from_utf8(bytes.to_vec()).unwrap(), took)
}

/// A JSON body that stops arriving part-way is held to the body time
/// limits, as an upload's body is, and answers `body-stalled` at the stall limit
/// (#2100). Before, any body but an upload's could take as long as its
/// client liked, holding its connection all the while. Time is paused, so
/// the limit passes as soon as nothing else can run.
#[tokio::test]
async fn a_json_body_that_stops_arriving_is_body_stalled_at_the_stall_limit() {
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

    let (status, text, took) = answer_with_test_body_times(routes, request).await;

    // The timer wheel counts whole milliseconds, and the clock was paused
    // part-way through one.
    assert!(
        took >= TEST_BODY_TIMES.stall
            && took < TEST_BODY_TIMES.stall + std::time::Duration::from_secs(1),
        "{took:?}"
    );
    let problem = expect_problem(status, &text, ProblemType::BodyStalled);
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
            tokio::time::sleep(TEST_BODY_TIMES.total * 2).await;
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

    let (status, text, _) = answer_with_test_body_times(routes, request).await;

    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(text, "3");
}

/// The limits pass `Limited`'s error through, so a body with no declared
/// length over its cap is still too large, not stalled or malformed.
#[tokio::test]
async fn a_bounded_upload_body_over_the_cap_is_too_large() {
    let dir = TempDir::new().unwrap();
    let body = body_time_bounded(limited_chunked_body(5), TEST_BODY_TIMES);

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
