use super::*;
use crate::logging::{LogLevel, LogLine};
use crate::test_support::http_client;
use tempfile::TempDir;

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
        crate::connections::ConnectionLimits::of_default_config(),
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
    stopping_rx
        .await
        .expect("the stop signal was dropped before the server saw SIGTERM");

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
        crate::connections::ConnectionLimits::of_default_config(),
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
        crate::connections::ConnectionLimits::of_default_config(),
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
        crate::connections::ConnectionLimits::of_default_config(),
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

    let docs = "docs/src/content/docs/docs/user/features/admin";
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
    let docs = "docs/src/content/docs/docs/user/features/admin";
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
