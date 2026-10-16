//! The server's accept loop: how many connections it holds open at once,
//! how long a connection may take over a request's headers or sit idle
//! between requests (#2100), and how long an answer may wait on a client
//! that reads none of it (#2984).
//!
//! `axum::serve` sets neither. It builds each connection without a timer,
//! and hyper only reads its header timeout with one set, so a device that
//! opened a connection and sent nothing, or sent its headers a byte at a
//! time, held a task and a file handle for as long as it liked, and any
//! number of them could. A body is held to time in the router instead
//! ([`crate::server::with_server_layers`]), with the body time limits.
//!
//! Nothing limits how long a whole answer takes to send, so an export
//! download or a large attachment is never cut off on a slow link. What is
//! limited is a write the client takes nothing of: a client that stops
//! reading part-way, such as a tab that hangs mid-download, would otherwise
//! hold its connection, and the file its answer reads from, for as long as
//! the operating system keeps the connection ([`AnswerStall`]).
//!
//! These limits keep one slow or stuck client from holding the server. They
//! do not make it safe to reach from the internet.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use axum::Router;
use axum::serve::Listener;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::request_body::BodyTimeLimits;

/// How long a connection may take to send a request's headers, counted from
/// when the connection opens or the previous answer was sent. A connection
/// that sends no request in that time is closed, so it is also how long an
/// idle connection stays open. Thirty seconds is hyper's own default, which
/// `axum::serve` left unset; a client reconnects for its next request, as it
/// does after any closed connection.
pub(crate) const HEADER_READ_LIMIT: Duration = Duration::from_secs(30);

/// How many connections the server holds open at once. With that many open,
/// it accepts the next one only once one of them closes; until then the
/// operating system keeps it waiting. It is well above what a few browser
/// tabs (six connections each) and the desktop app's Upload (64 kept open
/// at most) use together, and well under the 1024 open files a Linux
/// process gets by default, which the connections share with the database
/// and the upload files.
pub(crate) const MAX_CONNECTIONS: u32 = 256;

/// The limits [`serve_connections`] holds every connection to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConnectionLimits {
    /// How many connections are open at once at most.
    pub(crate) max_connections: u32,
    /// How long a connection may take over a request's headers, or sit idle.
    pub(crate) header_read: Duration,
    /// How long an answer may wait on a client that reads none of it.
    pub(crate) answer_stall: Duration,
}

impl ConnectionLimits {
    /// The limits the server runs with. An answer is held to the body stall
    /// time, `[server] body_stall_secs`: a client that reads none of an
    /// answer for that long is as stuck as one that sends none of a body.
    pub(crate) fn server(body_time_limits: BodyTimeLimits) -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            header_read: HEADER_READ_LIMIT,
            answer_stall: body_time_limits.stall,
        }
    }
}

#[cfg(test)]
impl ConnectionLimits {
    /// The limits the server runs with on a default config.
    pub(crate) fn of_default_config() -> Self {
        Self::server(BodyTimeLimits::of(&crate::config::ServerConfig::default()))
    }
}

/// Serve `app` over HTTP/1.1 on the connections `listener` accepts, holding
/// each to `limits`, until `stop` resolves. Then accept no more, close every
/// idle connection, let every request in flight finish, and return once the
/// last connection has closed.
pub(crate) async fn serve_connections<L: Listener>(
    mut listener: L,
    app: Router,
    limits: ConnectionLimits,
    stop: impl Future<Output = ()>,
) {
    let slots = Arc::new(Semaphore::new(limits.max_connections as usize));
    let stopping = CancellationToken::new();
    let mut stop = std::pin::pin!(stop);
    loop {
        // The slot comes before the accept, so a connection past the limit
        // waits with the operating system and holds nothing here.
        let next = async {
            let slot = Arc::clone(&slots)
                .acquire_owned()
                .await
                .expect("the connection slots are never closed");
            let (io, _addr) = listener.accept().await;
            (slot, io)
        };
        let (slot, io) = tokio::select! {
            () = &mut stop => break,
            next = next => next,
        };
        tracing::trace!("The server accepted a connection");
        let service = TowerToHyperService::new(app.clone());
        let stopping = stopping.clone();
        crate::request_id::spawn_in_current_span(async move {
            let io = AnswerStall::new(io, limits.answer_stall);
            serve_one(io, service, limits.header_read, stopping).await;
            drop(slot);
        });
    }
    drop(listener);
    stopping.cancel();
    // Every connection gives its slot back as it closes, so holding them all
    // means none is open.
    let _all = slots
        .acquire_many(limits.max_connections)
        .await
        .expect("the connection slots are never closed");
}

/// Serve one connection until it closes, or until `stopping` is canceled
/// and the request in flight on it, if any, has been answered.
async fn serve_one<I>(
    io: I,
    service: TowerToHyperService<Router>,
    header_read: Duration,
    stopping: CancellationToken,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(header_read);
    let mut conn = std::pin::pin!(builder.serve_connection(TokioIo::new(io), service));
    let ended = tokio::select! {
        ended = conn.as_mut() => ended,
        () = stopping.cancelled() => {
            conn.as_mut().graceful_shutdown();
            conn.await
        }
    };
    if let Err(error) = ended {
        // A stall close is the answer limit at work, which the person
        // running the server should be able to see at the default level.
        match answer_stalled(&error) {
            Some(stalled) => tracing::info!("The server closed a connection: {stalled}"),
            None => tracing::debug!("A connection closed with an error: {error}"),
        }
    }
}

/// A connection whose writes fail once one has waited `stall` with the
/// client taking no byte of it, which closes the connection and drops the
/// answer. The wait restarts with every byte the client takes, so an answer
/// that is still moving, however slowly, is never cut off, and a connection
/// with nothing to write is never timed: an idle one is closed by the header
/// read limit, and an answer the server is still making is not the client's
/// to wait on.
struct AnswerStall<I> {
    io: I,
    stall: Duration,
    /// When the write waiting on the client fails; `None` while no write
    /// waits.
    deadline: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<I> AnswerStall<I> {
    fn new(io: I, stall: Duration) -> Self {
        Self {
            io,
            stall,
            deadline: None,
        }
    }

    /// Pass on what a write, flush or shutdown of `io` answered: anything
    /// but a wait ends the wait, and a wait fails once it has lasted
    /// `stall`. A wait polls the deadline with the same waker as the write,
    /// so the connection is polled again when the deadline passes.
    fn held_to_stall<T>(
        &mut self,
        cx: &mut Context<'_>,
        polled: Poll<std::io::Result<T>>,
    ) -> Poll<std::io::Result<T>> {
        if polled.is_ready() {
            self.deadline = None;
            return polled;
        }
        let stall = self.stall;
        let deadline = self
            .deadline
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(stall)));
        ready!(deadline.as_mut().poll(cx));
        self.deadline = None;
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            AnswerStalled(stall),
        )))
    }
}

/// The error a write fails with in [`AnswerStall`], which `serve_one`
/// finds under hyper's error to log the close at `INFO`.
#[derive(Debug, thiserror::Error)]
#[error("the client took none of the answer for {} seconds", .0.as_secs())]
struct AnswerStalled(Duration);

/// The [`AnswerStalled`] a connection's error carries, if it closed for one.
fn answer_stalled(error: &hyper::Error) -> Option<&AnswerStalled> {
    std::error::Error::source(error)?
        .downcast_ref::<std::io::Error>()?
        .get_ref()?
        .downcast_ref::<AnswerStalled>()
}

impl<I: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for AnswerStall<I> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl<I: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for AnswerStall<I> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let polled = Pin::new(&mut self.io).poll_write(cx, buf);
        self.held_to_stall(cx, polled)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let polled = Pin::new(&mut self.io).poll_write_vectored(cx, bufs);
        self.held_to_stall(cx, polled)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let polled = Pin::new(&mut self.io).poll_flush(cx);
        self.held_to_stall(cx, polled)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let polled = Pin::new(&mut self.io).poll_shutdown(cx);
        self.held_to_stall(cx, polled)
    }
}

#[cfg(test)]
mod tests;
