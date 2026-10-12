//! Typed retry classification for the server HTTP paths.

use std::io;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use message_crate_api_types::Problem;

use crate::AuthError;

/// Whether a failure is likely to succeed on retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryKind {
    /// Worth retrying: network, timeout, 5xx, or anything unrecognized.
    Transient,
    /// Will fail the same way again, or already succeeded: auth, 4xx, a 2xx
    /// whose body could not be read, missing local files.
    Permanent,
}

/// An HTTP-status failure with its human-readable message.
///
/// `Display` prints only the message, so error text stays exactly what the
/// call site wrote; the status travels typed for [`classify_retry`], and the
/// server's problem document, when the body was one, travels for a caller
/// that reads one of its members.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct HttpError {
    status: u16,
    message: String,
    problem: Option<Box<Problem>>,
}

impl HttpError {
    /// Build a status-tagged error that displays `message` verbatim.
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            problem: None,
        }
    }

    /// Keep the problem document the server answered with.
    #[must_use]
    pub fn with_problem(mut self, problem: Problem) -> Self {
        self.problem = Some(Box::new(problem));
        self
    }

    /// The problem document the server answered with, when the body was one.
    #[must_use]
    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_deref()
    }
}

/// Whether `error` is the server refusing the session token (`401
/// Unauthorized`): the session expired or was ended. Asking again with the
/// same token gets the same answer, so a run that sees one stops.
pub fn is_session_refused(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<HttpError>()
        .is_some_and(|http| http.status == 401)
        || matches!(
            error.downcast_ref::<AuthError>(),
            Some(AuthError::Unauthorized)
        )
}

/// Classify an error for [`with_retries`].
///
/// Checks, in order: [`HttpError`] (2xx and 4xx permanent), [`AuthError`]
/// (auth and 4xx permanent, transport transient), `reqwest::Error` status (4xx
/// permanent), `std::io::Error` kind (`NotFound` permanent). Anything
/// unrecognized is transient, matching the historical default.
///
/// A [`HttpError`] with a 2xx status is an answer the client could not
/// read. The server has already done the work, so sending the request again
/// would repeat a write it committed.
pub fn classify_retry(error: &anyhow::Error) -> RetryKind {
    if let Some(http) = error.downcast_ref::<HttpError>() {
        return if (200..300).contains(&http.status) || (400..500).contains(&http.status) {
            RetryKind::Permanent
        } else {
            RetryKind::Transient
        };
    }
    if let Some(auth) = error.downcast_ref::<AuthError>() {
        return match auth {
            AuthError::Unauthorized
            | AuthError::Forbidden { .. }
            | AuthError::ApiNotFound { .. }
            | AuthError::Rejected { .. } => RetryKind::Permanent,
            AuthError::HttpStatus { status, .. } if (400..500).contains(status) => {
                RetryKind::Permanent
            }
            _ => RetryKind::Transient,
        };
    }
    if let Some(reqwest) = error.downcast_ref::<reqwest::Error>() {
        return match reqwest.status() {
            Some(status) if (400..500).contains(&status.as_u16()) => RetryKind::Permanent,
            _ => RetryKind::Transient,
        };
    }
    if let Some(io) = error.downcast_ref::<io::Error>() {
        return if io.kind() == io::ErrorKind::NotFound {
            RetryKind::Permanent
        } else {
            RetryKind::Transient
        };
    }
    RetryKind::Transient
}

/// Why a request failed because the server was not there to take it, rather
/// than because of what the request carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerAway {
    /// The server answered `server-stopping`: it was told to stop.
    Stopping,
    /// No answer came (the connection was refused, dropped, or timed out),
    /// or the answer was `502 Bad Gateway`, `503 Service Unavailable` or
    /// `504 Gateway Timeout`, which a server or the proxy before it gives
    /// when it cannot take requests.
    Unavailable,
}

/// The problem type a server told to stop answers with.
const SERVER_STOPPING: &str = "server-stopping";

/// Whether `error` says the server was not there to take the request, and
/// why. Every request fails the same way until the server is back, so a run
/// that sees one stops rather than recording each request it could not
/// send as failed. A `500 Internal Server Error` is not one: the server took
/// the request and failed on it, and the next request may succeed.
pub fn server_away(error: &anyhow::Error) -> Option<ServerAway> {
    if let Some(http) = error.downcast_ref::<HttpError>() {
        if http
            .problem()
            .is_some_and(|problem| problem.slug() == Some(SERVER_STOPPING))
        {
            return Some(ServerAway::Stopping);
        }
        return matches!(http.status, 502..=504).then_some(ServerAway::Unavailable);
    }
    // A reqwest error with no status is a request that got no answer, or
    // whose answer broke off. One that could not be built never left.
    error
        .downcast_ref::<reqwest::Error>()
        .filter(|reqwest| reqwest.status().is_none() && !reqwest.is_builder())
        .map(|_| ServerAway::Unavailable)
}

/// The least the last retry waits before it tries again, so a server or a
/// network that is away for several seconds, such as Wi-Fi reconnecting or
/// a server restarting, is back before the request is given up.
pub const LAST_RETRY_WAIT: Duration = Duration::from_secs(15);

/// How long the `attempt`th try (counted from 1) waits before the next one,
/// out of `max_retries` retries, with `jitter` in [0.0, 1.0).
///
/// Each wait doubles from half a second, with up to a quarter more for
/// jitter. The wait before the last retry is at least [`LAST_RETRY_WAIT`].
fn retry_wait(attempt: u32, max_retries: u32, jitter: f64) -> Duration {
    let base_ms = 500u64.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1)));
    let jitter_ms = (base_ms / 4).min(5000);
    let wait_ms = base_ms + (jitter_ms / 2) + (jitter_ms as f64 * jitter) as u64;
    let wait = Duration::from_millis(wait_ms.min(30_000));
    if attempt == max_retries {
        wait.max(LAST_RETRY_WAIT)
    } else {
        wait
    }
}

/// Run `op` again on transient failures, with backoff, up to `max_retries`
/// extra tries.
///
/// # Errors
///
/// Returns the last error from `op` when retries are exhausted or the error is
/// permanent.
pub fn with_retries<T, F>(max_retries: u32, op: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    retry_with_waits(
        max_retries,
        |attempt| {
            thread::sleep(retry_wait(attempt, max_retries, rand_factor()));
        },
        op,
    )
}

/// [`with_retries`], with `wait(attempt)` called before each retry in place
/// of the sleep, so a test can see the waits without taking them.
fn retry_with_waits<T>(
    max_retries: u32,
    mut wait: impl FnMut(u32),
    mut op: impl FnMut() -> Result<T>,
) -> Result<T> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt > max_retries || classify_retry(&e) == RetryKind::Permanent {
                    return Err(e);
                }
                wait(attempt);
            }
        }
    }
}

/// Clock-derived factor in [0.0, 1.0) for retry jitter: the sub-second
/// nanoseconds of the current time, modulo 1000.
fn rand_factor() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    (nanos % 1000) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    fn classified(kind: RetryKind) -> bool {
        kind == RetryKind::Permanent
    }

    #[test]
    fn http_status_errors_are_permanent_for_4xx() {
        let e = anyhow::Error::from(HttpError::new(
            404,
            "Asset check failed (HTTP 404 Not Found)",
        ));
        assert!(classified(classify_retry(&e)));
        let e = anyhow::Error::from(HttpError::new(
            413,
            "Import Run 7 batch rejected: HTTP 413 Payload Too Large",
        ));
        assert!(classified(classify_retry(&e)));
        let e = anyhow::Error::from(HttpError::new(
            401,
            "the server did not accept this session",
        ));
        assert!(classified(classify_retry(&e)));
    }

    #[test]
    fn only_a_401_is_a_refused_session() {
        let refused = anyhow::Error::from(crate::session_refused("Import Run 7 batch"));
        assert!(is_session_refused(&refused));
        assert!(is_session_refused(
            &refused.context("POST /v1/imports/7/batches")
        ));
        assert!(is_session_refused(&anyhow::Error::from(
            AuthError::Unauthorized
        )));
        assert!(!is_session_refused(&anyhow::Error::from(HttpError::new(
            403,
            "the account may not import"
        ))));
        assert!(!is_session_refused(&anyhow!("connection reset")));
    }

    #[test]
    fn an_unreadable_2xx_is_permanent_because_the_server_did_the_work() {
        let e = anyhow::Error::from(HttpError::new(
            200,
            "could not read the server's answer to Import Run 7 batch",
        ));
        assert!(classified(classify_retry(&e)));
    }

    #[test]
    fn http_status_errors_are_transient_for_5xx() {
        let e = anyhow::Error::from(HttpError::new(
            503,
            "Asset upload part 1 failed (HTTP 503 Service Unavailable)",
        ));
        assert!(!classified(classify_retry(&e)));
    }

    #[test]
    fn auth_failures_are_permanent() {
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::Unauthorized
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::Forbidden {
                status: 403,
                body: "username does not match API key".into(),
            }
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::ApiNotFound {
                status: 404,
                body: "missing".into(),
            }
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::Rejected {
                message: "bad token".into(),
            }
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::HttpStatus {
                status: 418,
                body: "teapot".into(),
            }
        ))));
    }

    #[test]
    fn auth_transport_failures_are_transient() {
        assert!(!classified(classify_retry(&anyhow::Error::from(
            AuthError::Network {
                url: "https://v".into(),
                detail: "dns".into(),
            }
        ))));
        assert!(!classified(classify_retry(&anyhow::Error::from(
            AuthError::ServerError {
                status: 503,
                body: "busy".into(),
            }
        ))));
        assert!(!classified(classify_retry(&anyhow::Error::from(
            AuthError::HttpStatus {
                status: 503,
                body: "busy".into(),
            }
        ))));
    }

    #[test]
    fn io_not_found_is_permanent_other_io_is_transient() {
        let e = anyhow::Error::from(io::Error::new(io::ErrorKind::NotFound, "no such file"));
        assert!(classified(classify_retry(&e)));
        let e = anyhow::Error::from(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        assert!(!classified(classify_retry(&e)));
    }

    #[test]
    fn unrecognized_errors_are_transient() {
        assert!(!classified(classify_retry(&anyhow!("something odd"))));
    }

    /// Run `op` through [`retry_with_waits`] with `max_retries`, and return
    /// its result with the attempts each wait came after.
    fn retried<T>(max_retries: u32, op: impl FnMut() -> Result<T>) -> (Result<T>, Vec<u32>) {
        let mut waits = Vec::new();
        let result = retry_with_waits(max_retries, |attempt| waits.push(attempt), op);
        (result, waits)
    }

    #[test]
    fn with_retries_gives_up_on_permanent_immediately() {
        let mut calls = 0;
        let (result, waits) = retried(3, || -> Result<u32> {
            calls += 1;
            Err(anyhow::Error::from(HttpError::new(404, "gone")))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
        assert!(waits.is_empty());
    }

    #[test]
    fn with_retries_retries_transient_then_succeeds() {
        let mut calls = 0;
        let (result, waits) = retried(2, || -> Result<u32> {
            calls += 1;
            if calls < 2 {
                Err(anyhow::Error::from(io::Error::other("flaky")))
            } else {
                Ok(42)
            }
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls, 2);
        assert_eq!(waits, vec![1]);
    }

    /// `max_retries` counts extra tries: one retry means two calls in all.
    #[test]
    fn with_retries_makes_one_extra_call_per_retry() {
        let mut calls = 0;
        let (result, waits) = retried(1, || -> Result<u32> {
            calls += 1;
            Err(anyhow::Error::from(io::Error::other("flaky")))
        });
        assert!(result.is_err());
        assert_eq!(calls, 2);
        assert_eq!(waits, vec![1]);
    }

    #[test]
    fn with_retries_gives_up_when_exhausted() {
        let mut calls = 0;
        let (result, waits) = retried(0, || -> Result<u32> {
            calls += 1;
            Err(anyhow::Error::from(io::Error::other("flaky")))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
        assert!(waits.is_empty());
    }

    /// The waits double from half a second, and the one before the last
    /// retry is long enough for a server or network away for several
    /// seconds to come back (#2092): three retries used to give up after
    /// about four seconds in all.
    #[test]
    fn the_wait_before_the_last_retry_outlasts_a_short_outage() {
        let waits: Vec<Duration> = (1..=3).map(|attempt| retry_wait(attempt, 3, 0.0)).collect();
        assert_eq!(
            waits,
            vec![
                Duration::from_millis(562),
                Duration::from_millis(1125),
                LAST_RETRY_WAIT,
            ]
        );
        assert!(waits.iter().sum::<Duration>() >= Duration::from_secs(15));
        // With one retry, that retry is the last.
        assert_eq!(retry_wait(1, 1, 0.0), LAST_RETRY_WAIT);
        // A wait longer than the floor keeps its own length.
        assert_eq!(retry_wait(7, 7, 0.0), Duration::from_secs(30));
    }

    /// A problem document with type `slug` and `status`, as the server
    /// answers.
    fn problem(slug: &str, status: u16) -> Problem {
        serde_json::from_value(serde_json::json!({
            "type": format!("https://messagecrate.app/docs/developer/reference/errors/{slug}"),
            "title": slug,
            "status": status,
        }))
        .unwrap()
    }

    #[test]
    fn a_server_told_to_stop_is_away_because_it_is_stopping() {
        let stopping = anyhow::Error::from(
            HttpError::new(503, "Import Run 7 batch failed")
                .with_problem(problem(SERVER_STOPPING, 503)),
        )
        .context("POST /v1/imports/7/batches");
        assert_eq!(server_away(&stopping), Some(ServerAway::Stopping));
    }

    #[test]
    fn a_gateway_or_unavailable_answer_is_a_server_away() {
        for status in [502, 503, 504] {
            let error = anyhow::Error::from(HttpError::new(status, "batch failed"));
            assert_eq!(
                server_away(&error),
                Some(ServerAway::Unavailable),
                "{status}"
            );
        }
        let busy = anyhow::Error::from(
            HttpError::new(503, "batch failed").with_problem(problem("database-busy", 503)),
        );
        assert_eq!(server_away(&busy), Some(ServerAway::Unavailable));
    }

    /// The server took the request and failed on it, or refused it: the
    /// next request may go through, so the run goes on.
    #[test]
    fn an_answer_about_the_request_is_not_a_server_away() {
        for status in [200, 400, 401, 404, 413, 500, 501] {
            let error = anyhow::Error::from(HttpError::new(status, "batch failed"));
            assert_eq!(server_away(&error), None, "{status}");
        }
        assert_eq!(server_away(&anyhow!("something odd")), None);
        assert_eq!(
            server_away(&anyhow::Error::from(io::Error::other("disk"))),
            None
        );
    }

    #[test]
    fn a_refused_connection_is_a_server_away() {
        // A port that was just free and is not listened on refuses at once.
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let refused = reqwest::blocking::Client::new()
            .get(format!("http://{address}/v1/session"))
            .send()
            .unwrap_err();
        let error = anyhow::Error::from(refused).context("Import Run 7 batch failed");
        assert_eq!(server_away(&error), Some(ServerAway::Unavailable));
    }
}
