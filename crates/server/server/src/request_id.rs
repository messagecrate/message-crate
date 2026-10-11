//! One id per request, made by the server, carried on every response and
//! repeated in every problem body (`docs/architecture/http-api.md`).
//!
//! The id is set by [`layer`], the outermost layer on the router, and read by
//! [`current`] wherever a problem document is built. It travels as a task
//! local rather than a request extension because the code that builds the
//! failure body is `IntoResponse for ApiError`, which sees no request at all.
//!
//! The id also joins the request's tracing span, so every log line written
//! under the request carries it. A span does not cross `tokio::spawn` or
//! `spawn_blocking` on its own, so work a request starts in a task of its
//! own goes through [`spawn`] and [`spawn_blocking`], which carry it (#2186).
//!
//! An `x-request-id` a client sends is dropped, never kept: none of the
//! server's own clients send one, and an operator grepping the log needs ids
//! they know the server made.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use tracing::Instrument;
use tracing::instrument::WithSubscriber;

/// The header the id travels in, both ways.
pub const HEADER: HeaderName = HeaderName::from_static("x-request-id");

tokio::task_local! {
    static REQUEST_ID: String;
}

/// The id of the request being served, or `None` outside one (a handler
/// called directly from a test, a CLI command).
#[must_use]
pub fn current() -> Option<String> {
    REQUEST_ID.try_with(Clone::clone).ok()
}

/// Make the id, put it on the request so the trace span and any handler can
/// see it, serve the request under it, and put it on the response.
pub async fn layer(mut request: Request, next: Next) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let value = HeaderValue::from_str(&id).expect("a UUID is a valid header value");
    request.headers_mut().insert(HEADER, value.clone());
    let mut response = REQUEST_ID.scope(id, next.run(request)).await;
    response.headers_mut().insert(HEADER, value);
    response
}

/// `tokio::spawn`, with `future` run under the current span and subscriber,
/// so a line it writes carries the id of the request that started it.
pub(crate) fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(future.in_current_span().with_current_subscriber())
}

/// `tokio::task::spawn_blocking`, with `f` run under the current span and
/// subscriber, so a line it writes carries the id of the request that
/// started it.
pub(crate) fn spawn_blocking<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || span.in_scope(f))
    })
}
