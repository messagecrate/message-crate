//! One id per request, made by the server, carried on every response and
//! repeated in every problem body (`docs/architecture/http-api.md`).
//!
//! The id is set by [`layer`], the outermost layer on the router, and read by
//! [`current`] wherever a problem document is built. It travels as a task
//! local rather than a request extension because the code that builds the
//! failure body is `IntoResponse for ApiError`, which sees no request at all.
//!
//! The id also joins the request's tracing span, so every log line written
//! under the request carries it, and every Audit Trail entry the request
//! writes keeps it (`crate::db::audit_trail`), so the two are found by each
//! other. Neither a span nor a task local crosses `tokio::spawn` or
//! `spawn_blocking` on its own, so work a request starts in a task of its
//! own goes through `spawn_in_current_span` and
//! `spawn_blocking_in_current_span`, which carry both (#2186, #2276).
//! Every spawn in the crate does, request or not, and `clippy.toml` refuses
//! tokio's own calls. Outside a request there is no span and no id, and the
//! two do what tokio's calls do.
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

/// `tokio::spawn`, with `future` run under the current span, subscriber and
/// request id, so a line it writes and an Audit Trail entry it records carry
/// the id of the request that started it.
#[allow(clippy::disallowed_methods)] // the one place tokio's own call is made
pub(crate) fn spawn_in_current_span<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let future = future.in_current_span().with_current_subscriber();
    match current() {
        Some(id) => tokio::spawn(REQUEST_ID.scope(id, future)),
        None => tokio::spawn(future),
    }
}

/// `tokio::task::spawn_blocking`, with `f` run under the current span,
/// subscriber and request id, so a line it writes and an Audit Trail entry it
/// records carry the id of the request that started it.
#[allow(clippy::disallowed_methods)] // the one place tokio's own call is made
pub(crate) fn spawn_blocking_in_current_span<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let id = current();
    tokio::task::spawn_blocking(move || {
        let run = || tracing::dispatcher::with_default(&dispatch, || span.in_scope(f));
        match id {
            Some(id) => REQUEST_ID.sync_scope(id, run),
            None => run(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Work a request starts in a task or a thread of its own reads the
    /// request's id, so an Audit Trail entry it writes carries the id its log
    /// lines carry: the owner's Demo Account build deletes the old Demo
    /// Account in a task of its own (#2276).
    #[tokio::test]
    async fn spawned_work_reads_the_id_of_the_request_that_started_it() {
        let (in_task, in_thread) = REQUEST_ID
            .scope("the-request".to_string(), async {
                let task = spawn_in_current_span(async { current() });
                let thread = spawn_blocking_in_current_span(current);
                (task.await.unwrap(), thread.await.unwrap())
            })
            .await;
        assert_eq!(in_task.as_deref(), Some("the-request"));
        assert_eq!(in_thread.as_deref(), Some("the-request"));
    }

    /// Outside a request there is no id, in a spawned task or not.
    #[tokio::test]
    async fn work_outside_a_request_has_no_id() {
        assert_eq!(current(), None);
        assert_eq!(
            spawn_in_current_span(async { current() }).await.unwrap(),
            None
        );
        assert_eq!(spawn_blocking_in_current_span(current).await.unwrap(), None);
    }
}
