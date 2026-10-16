use super::*;
use crate::config::ServerConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::{mpsc, oneshot};

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
    let (open, _stop, _server) = start(ConnectionLimits::of_default_config());
    let mut client = connect(&open).await;

    assert_eq!(closed_after(&mut client).await, HEADER_READ_LIMIT);
}

/// Headers sent a line at a time, each well inside the limit, are still
/// cut off at the limit, counted from the start.
#[tokio::test(start_paused = true)]
async fn headers_sent_slowly_are_cut_off_at_the_header_read_limit() {
    let (open, _stop, _server) = start(ConnectionLimits::of_default_config());
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
    let (open, _stop, _server) = start(ConnectionLimits::of_default_config());
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
        ..ConnectionLimits::of_default_config()
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
    let (open, stop, server) = start(ConnectionLimits::of_default_config());
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
    // A stall time other than the default, so the test also shows the
    // answer is held to the configured `body_stall_secs`.
    let stall = Duration::from_secs(7);
    let limits = ConnectionLimits::server(BodyTimeLimits::of(&ServerConfig {
        body_stall_secs: stall.as_secs(),
        ..ServerConfig::default()
    }));
    let (open, _stop, released) = start_big(limits);
    let mut client = connect(&open).await;
    client
        .write_all(b"GET /big HTTP/1.1\r\nHost: test\r\n\r\n")
        .await
        .unwrap();
    let started = tokio::time::Instant::now();

    tokio::time::timeout(stall * 4, released)
        .await
        .expect("the server held an answer nobody read past the stall time")
        .unwrap();

    // The connection's buffers fill at once, so the wait starts as the
    // request arrives.
    assert_eq!(started.elapsed(), stall);
    let mut answer = Vec::new();
    client.read_to_end(&mut answer).await.unwrap();
    assert!(answer.len() < BIG_ANSWER, "{} bytes", answer.len());
}

/// A client that reads an answer slowly, each read well inside the
/// stall time, gets all of it however long it takes in all: the stall
/// time counts from the last byte the client took.
#[tokio::test(start_paused = true)]
async fn an_answer_read_slowly_is_sent_in_full() {
    let limits = ConnectionLimits::of_default_config();
    let (open, _stop, released) = start_big(limits);
    let mut client = connect(&open).await;
    client
        .write_all(b"GET /big HTTP/1.1\r\nHost: test\r\n\r\n")
        .await
        .unwrap();
    let started = tokio::time::Instant::now();

    let mut answer = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        tokio::time::sleep(limits.answer_stall - Duration::from_secs(1)).await;
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
    assert!(started.elapsed() > limits.answer_stall * 4);
    released.await.unwrap();
}
