//! The requests the server is serving at this moment, so a stopping server
//! that gives up waiting for them can name each one in its log (#2122).
//!
//! [`track`] is a layer on every request ([`crate::server::with_server_layers`]).
//! It records the request as it arrives and forgets it once the answer's
//! body has been sent or dropped, not when the handler returns: an export
//! download or a media file is still being sent long after its handler
//! returned, and the stop waits for it all the same.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes, HttpBody};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

/// The requests in flight. Cloning shares the same record.
#[derive(Debug, Clone, Default)]
pub(crate) struct InFlight(Arc<Shared>);

#[derive(Debug, Default)]
struct Shared {
    next: AtomicU64,
    /// A `std` mutex: it is held for one map operation, never across an
    /// `.await`, and the guard's `Drop` cannot await.
    requests: Mutex<BTreeMap<u64, Entry>>,
}

#[derive(Debug, Clone)]
struct Entry {
    request_id: String,
    method: String,
    uri: String,
    started: Instant,
}

/// One request the server was still serving when it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunningRequest {
    /// The id on the request's log lines, or empty for a request that came
    /// in under no id (a test's bare router).
    pub(crate) request_id: String,
    pub(crate) method: String,
    /// The path and query as the log writes them, values hidden
    /// ([`crate::server::logged_uri`]).
    pub(crate) uri: String,
    pub(crate) running_for: Duration,
}

impl InFlight {
    /// The requests in flight, the longest-running first.
    pub(crate) fn running(&self) -> Vec<RunningRequest> {
        let requests = self
            .0
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        requests
            .values()
            .map(|entry| RunningRequest {
                request_id: entry.request_id.clone(),
                method: entry.method.clone(),
                uri: entry.uri.clone(),
                running_for: entry.started.elapsed(),
            })
            .collect()
    }

    fn start(&self, entry: Entry) -> Guard {
        let key = self.0.next.fetch_add(1, Ordering::Relaxed);
        self.0
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, entry);
        Guard {
            in_flight: self.clone(),
            key,
        }
    }
}

/// Forgets its request when dropped.
#[derive(Debug)]
struct Guard {
    in_flight: InFlight,
    key: u64,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.in_flight
            .0
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

/// Record `request` in `in_flight` until its answer's body has been sent.
pub(crate) async fn track(
    State(in_flight): State<InFlight>,
    request: Request,
    next: Next,
) -> Response {
    let guard = in_flight.start(Entry {
        request_id: request
            .headers()
            .get(crate::request_id::HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned(),
        method: request.method().to_string(),
        uri: crate::server::logged_uri(request.uri()),
        started: Instant::now(),
    });
    let response = next.run(request).await;
    response.map(|inner| Body::new(Tracked { inner, guard }))
}

/// An answer's body, holding its request's [`Guard`] until the body ends or
/// is dropped.
struct Tracked {
    inner: Body,
    guard: Guard,
}

impl HttpBody for Tracked {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.get_mut().inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

impl std::fmt::Debug for Tracked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tracked")
            .field("guard", &self.guard)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    fn app(
        in_flight: &InFlight,
        body: impl Fn() -> Body + Clone + Send + Sync + 'static,
    ) -> Router {
        Router::new()
            .route(
                "/thing",
                axum::routing::get(move || {
                    let body = body.clone();
                    async move { Response::new(body()) }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                in_flight.clone(),
                track,
            ))
    }

    /// A request is in flight until its body has been read to the end, not
    /// only until its handler returned, so a download still being sent is
    /// named at the stop.
    #[tokio::test]
    async fn a_request_is_in_flight_until_its_body_is_sent() {
        let in_flight = InFlight::default();
        let (tx, rx) = tokio::sync::oneshot::channel::<Bytes>();
        let rx = Arc::new(Mutex::new(Some(rx)));
        let app = app(&in_flight, move || {
            let rx = rx.lock().unwrap().take().unwrap();
            Body::from_stream(futures_util::stream::once(async move {
                rx.await.map_err(std::io::Error::other)
            }))
        });

        let response = app
            .oneshot(
                Request::get("/thing?token=secret")
                    .header(crate::request_id::HEADER, "req-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let running = in_flight.running();
        assert_eq!(running.len(), 1, "the handler returned, the body is unsent");
        assert_eq!(running[0].request_id, "req-1");
        assert_eq!(running[0].method, "GET");
        assert!(
            running[0].uri.starts_with("/thing?token=") && !running[0].uri.contains("secret"),
            "the query's values are hidden as in the log: {}",
            running[0].uri
        );

        tx.send(Bytes::from_static(b"done")).unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"done");
        assert!(in_flight.running().is_empty(), "the body has been sent");
    }

    /// A client that goes away drops the body unsent, and the request is
    /// forgotten with it.
    #[tokio::test]
    async fn a_dropped_answer_forgets_its_request() {
        let in_flight = InFlight::default();
        let app = app(&in_flight, || Body::from("whole"));
        let response = app
            .oneshot(Request::get("/thing").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(in_flight.running().len(), 1);
        drop(response);
        assert!(in_flight.running().is_empty());
    }
}
