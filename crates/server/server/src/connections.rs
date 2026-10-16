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
        tracing::debug!("A connection closed with an error: {error}");
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
    fn waited<T>(
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
        // `serve_one` writes the error to the log as the connection closes.
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!(
                "the client read none of the answer for {} seconds",
                stall.as_secs()
            ),
        )))
    }
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
        self.waited(cx, polled)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let polled = Pin::new(&mut self.io).poll_write_vectored(cx, bufs);
        self.waited(cx, polled)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let polled = Pin::new(&mut self.io).poll_flush(cx);
        self.waited(cx, polled)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let polled = Pin::new(&mut self.io).poll_shutdown(cx);
        self.waited(cx, polled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
    use tokio::sync::{mpsc, oneshot};

    /// The limits the tests hold the server to: the server's own, with the
    /// default body stall time.
    const LIMITS: ConnectionLimits = ConnectionLimits {
        max_connections: MAX_CONNECTIONS,
        header_read: HEADER_READ_LIMIT,
        answer_stall: Duration::from_secs(60),
    };

    /// How large `GET /big`'s answer is: far more than a connection's
    /// buffers hold, so the server can send it all only to a client that
    /// reads it.
    const BIG_ANSWER: usize = 1024 * 1024;

    /// Connections the test hands the server, in place of a socket, so a
    /// test controls each one and nothing depends on the network.
    struct Duplexes(mpsc::Receiver<DuplexStream>);

    impl Listener for Duplexes {
        type Io = DuplexStream;
        type Addr = ();

        async fn accept(&mut self) -> (DuplexStream, ()) {
            match self.0.recv().await {
                Some(io) => (io, ()),
                None => std::future::pending().await,
            }
        }

        fn local_addr(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A server on [`Duplexes`] answering `GET /ok`, the sender that opens
    /// a connection to it, and the sender that stops it.
    fn start(
        limits: ConnectionLimits,
    ) -> (
        mpsc::Sender<DuplexStream>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let app = Router::new().route("/ok", axum::routing::get(|| async { "ok" }));
        start_app(app, limits)
    }

    /// A server on [`Duplexes`] answering `GET /big` with [`BIG_ANSWER`]
    /// bytes, the sender that opens a connection to it, the sender that
    /// stops it, and a receiver that resolves once the server lets go of the
    /// answer: sent in full, or dropped with its connection.
    fn start_big(
        limits: ConnectionLimits,
    ) -> (
        mpsc::Sender<DuplexStream>,
        oneshot::Sender<()>,
        oneshot::Receiver<()>,
    ) {
        let (released_tx, released_rx) = oneshot::channel::<()>();
        let released_tx = Arc::new(std::sync::Mutex::new(Some(released_tx)));
        let app = Router::new().route(
            "/big",
            axum::routing::get(move || {
                let released = ReleasedOnDrop(released_tx.lock().unwrap().take());
                async move {
                    let chunks = (0..BIG_ANSWER / 1024).map(move |_| {
                        let _held = &released;
                        Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![b'x'; 1024]))
                    });
                    axum::body::Body::from_stream(futures_util::stream::iter(chunks))
                }
            }),
        );
        let (open, stop, _server) = start_app(app, limits);
        (open, stop, released_rx)
    }

    /// Sends on its channel when dropped, which is when the server lets go
    /// of the answer that holds it.
    struct ReleasedOnDrop(Option<oneshot::Sender<()>>);

    impl Drop for ReleasedOnDrop {
        fn drop(&mut self) {
            if let Some(released) = self.0.take() {
                let _ = released.send(());
            }
        }
    }

    /// Serve `app` on [`Duplexes`], returning the sender that opens a
    /// connection to it, and the sender that stops it.
    fn start_app(
        app: Router,
        limits: ConnectionLimits,
    ) -> (
        mpsc::Sender<DuplexStream>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let (open_tx, open_rx) = mpsc::channel(8);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server = crate::request_id::spawn_in_current_span(serve_connections(
            Duplexes(open_rx),
            app,
            limits,
            async move {
                let _ = stop_rx.await;
            },
        ));
        (open_tx, stop_tx, server)
    }

    /// Open a connection to the server `open` feeds.
    async fn connect(open: &mpsc::Sender<DuplexStream>) -> DuplexStream {
        let (client, server_side) = tokio::io::duplex(64 * 1024);
        open.send(server_side).await.unwrap();
        client
    }

    /// Send `GET /ok` on `client` and read its answer, keeping the
    /// connection open.
    async fn get_ok(client: &mut DuplexStream) {
        client
            .write_all(b"GET /ok HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        let mut answer = Vec::new();
        let mut buf = [0u8; 1024];
        while !answer.ends_with(b"\r\n\r\nok") {
            let n = client.read(&mut buf).await.unwrap();
            assert_ne!(n, 0, "the connection closed before its answer");
            answer.extend_from_slice(&buf[..n]);
        }
        assert!(answer.starts_with(b"HTTP/1.1 200 OK"), "{answer:?}");
    }

    /// Wait until the server closes `client`, failing the test if it is
    /// still open well past the header read limit, and return how long it
    /// took on the test's clock.
    async fn closed_after(client: &mut DuplexStream) -> Duration {
        let started = tokio::time::Instant::now();
        let mut buf = [0u8; 64];
        let read = tokio::time::timeout(HEADER_READ_LIMIT * 4, client.read(&mut buf))
            .await
            .expect("the server kept the connection open past the header read limit");
        assert_eq!(read.unwrap(), 0, "the server sent something");
        started.elapsed()
    }

    /// A connection that sends nothing is closed once the header read limit
    /// passes, and not before. Before #2100 it stayed open for good. Time
    /// is paused, so the limit passes as soon as nothing else can run.
    #[tokio::test(start_paused = true)]
    async fn a_connection_that_sends_no_headers_is_closed_at_the_header_read_limit() {
        let (open, _stop, _server) = start(LIMITS);
        let mut client = connect(&open).await;

        assert_eq!(closed_after(&mut client).await, HEADER_READ_LIMIT);
    }

    /// Headers sent a line at a time, each well inside the limit, are still
    /// cut off at the limit, counted from the start.
    #[tokio::test(start_paused = true)]
    async fn headers_sent_slowly_are_cut_off_at_the_header_read_limit() {
        let (open, _stop, _server) = start(LIMITS);
        let mut client = connect(&open).await;
        let started = tokio::time::Instant::now();
        client.write_all(b"GET /ok HTTP/1.1\r\n").await.unwrap();
        let trickle = async {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                if client.write_all(b"X-Slow: yes\r\n").await.is_err() {
                    break;
                }
            }
        };
        tokio::time::timeout(HEADER_READ_LIMIT * 4, trickle)
            .await
            .expect("the server kept taking headers past the header read limit");
        let mut buf = [0u8; 64];
        assert_eq!(client.read(&mut buf).await.unwrap(), 0);

        // The write after the close is the one that fails, at the first
        // 10-second step past the limit.
        assert_eq!(
            started.elapsed(),
            HEADER_READ_LIMIT + Duration::from_secs(10)
        );
    }

    /// A connection kept open after its answer is closed once it sits idle
    /// for the header read limit.
    #[tokio::test(start_paused = true)]
    async fn an_idle_connection_is_closed_at_the_header_read_limit_after_its_answer() {
        let (open, _stop, _server) = start(LIMITS);
        let mut client = connect(&open).await;
        get_ok(&mut client).await;

        assert_eq!(closed_after(&mut client).await, HEADER_READ_LIMIT);
    }

    /// With the limit of connections open, the next one is accepted only
    /// once one of them closes, and is then served.
    #[tokio::test(start_paused = true)]
    async fn a_connection_past_the_limit_waits_until_one_closes() {
        let (open, _stop, _server) = start(ConnectionLimits {
            max_connections: 2,
            ..LIMITS
        });
        let mut first = connect(&open).await;
        let mut second = connect(&open).await;
        get_ok(&mut first).await;
        get_ok(&mut second).await;
        let mut third = connect(&open).await;

        // Both open connections were taken; the third waits in the channel,
        // because the server takes a connection only once it holds a slot.
        assert_eq!(open.capacity(), open.max_capacity() - 1);

        drop(first);
        get_ok(&mut third).await;
        assert_eq!(open.capacity(), open.max_capacity());
        get_ok(&mut second).await;
    }

    /// A stop closes an idle connection at once and returns, rather than
    /// waiting for the header read limit to close it.
    #[tokio::test(start_paused = true)]
    async fn a_stop_closes_idle_connections_and_returns() {
        let (open, stop, server) = start(LIMITS);
        let mut client = connect(&open).await;
        get_ok(&mut client).await;
        let started = tokio::time::Instant::now();

        stop.send(()).unwrap();
        server.await.unwrap();

        assert_eq!(started.elapsed(), Duration::ZERO);
        let mut buf = [0u8; 64];
        assert_eq!(client.read(&mut buf).await.unwrap(), 0);
    }

    /// A client that asks for an answer and then reads none of it loses its
    /// connection once the answer has waited on it for the stall time, and
    /// the server lets go of the answer. Before #2984 the connection, and
    /// the export file or attachment the answer read from, were held for
    /// good. Time is paused, so the stall time passes as soon as nothing
    /// else can run.
    #[tokio::test(start_paused = true)]
    async fn an_answer_the_client_reads_none_of_is_dropped_at_the_stall_time() {
        let (open, _stop, released) = start_big(LIMITS);
        let mut client = connect(&open).await;
        client
            .write_all(b"GET /big HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        let started = tokio::time::Instant::now();

        tokio::time::timeout(LIMITS.answer_stall * 4, released)
            .await
            .expect("the server held an answer nobody read past the stall time")
            .unwrap();

        // The connection's buffers fill at once, so the wait starts as the
        // request arrives.
        assert_eq!(started.elapsed(), LIMITS.answer_stall);
        let mut answer = Vec::new();
        client.read_to_end(&mut answer).await.unwrap();
        assert!(answer.len() < BIG_ANSWER, "{} bytes", answer.len());
    }

    /// A client that reads an answer slowly, each read well inside the
    /// stall time, gets all of it however long it takes in all: the stall
    /// time counts from the last byte the client took.
    #[tokio::test(start_paused = true)]
    async fn an_answer_read_slowly_is_sent_in_full() {
        let (open, _stop, released) = start_big(LIMITS);
        let mut client = connect(&open).await;
        client
            .write_all(b"GET /big HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        let started = tokio::time::Instant::now();

        let mut answer = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            tokio::time::sleep(LIMITS.answer_stall - Duration::from_secs(1)).await;
            let n = client.read(&mut buf).await.unwrap();
            assert_ne!(n, 0, "the connection closed part-way through the answer");
            answer.extend_from_slice(&buf[..n]);
            if answer.ends_with(b"0\r\n\r\n") {
                break;
            }
        }

        assert!(
            answer.len() > BIG_ANSWER,
            "the answer and its framing are {} bytes",
            answer.len()
        );
        assert!(started.elapsed() > LIMITS.answer_stall * 4);
        released.await.unwrap();
    }
}
