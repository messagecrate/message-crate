//! Starting the server and stopping it: [`run`] opens the database, starts
//! the background work and listens, and a stop drains the requests in flight
//! for up to [`STOP_DRAIN_LIMIT`].

use std::sync::Arc;

use axum::Router;

use crate::config::Config;
use crate::exit_with_parent::ParentWatch;
use crate::open_db::OpenDb;
use crate::request_body::UploadTimeLimits;
use crate::server::{AppState, http_app};

/// Start the HTTP server. It runs until Ctrl-C or SIGTERM, or, when
/// `exit_with_parent` names a process, until that process is gone.
///
/// # Errors
///
/// Returns an error when the website directory and the Data Directory or the
/// database overlap, the database cannot be opened, the operation lock
/// cannot be taken, or the listener cannot bind.
pub async fn run(cfg: Config, exit_with_parent: Option<u32>) -> anyhow::Result<()> {
    // First, so the process is watched from the moment the server starts,
    // through a first start's Demo Account build.
    let exit_with_parent = exit_with_parent.map(ParentWatch::start);
    let server = cfg.require_server()?.clone();
    // Before anything is written to the Data Directory, so a layout that
    // would publish it never gets a log or a database there (#2175).
    cfg.require_static_dir_apart()?;
    let bind = server.bind.clone();
    // Before the database is opened, so a database that fails to open is in
    // the server's log as well as on stderr: from here on, `main` writes the
    // error this returns through `tracing` (`logging::write_failure`), and
    // every start-up line below goes through it too.
    let log = crate::logging::write_files_in(&cfg.paths.data_dir).map_err(|error| {
        anyhow::Error::from(error).context(format!(
            "could not open the server's log in {}",
            crate::logging::log_dir(&cfg.paths.data_dir).display()
        ))
    })?;
    tracing::info!("The server's log is in {}", log.dir().display());
    let operation_lock = crate::operation_lock::acquire_for_serve(&cfg.paths.db)?;

    // Every new Message Crate starts with the Demo Account: seed first, then
    // listen, so the first page a person loads already offers it (#971).
    if crate::reset_demo::database_is_new(&cfg).await? {
        crate::operation_lock::clear_ready(&cfg.paths.db)?;
        crate::reset_demo::seed_new_database(&cfg).await;
    }
    let opened = OpenDb::create_or_open_holding(cfg, &operation_lock).await?;
    crate::operation_lock::mark_ready(&opened.cfg.paths.db)?;
    let mode = crate::db::engine::journal_mode_or_warn(&opened.db)
        .await
        .map_or_else(|| "unknown".to_owned(), |mode| mode.to_string());
    tracing::info!(
        "The database at {} is open, in journal mode {mode}",
        opened.cfg.paths.db.display()
    );
    let state = AppState::new(
        opened,
        server.asset_part_size,
        UploadTimeLimits::of(&server),
    );
    if crate::server_api::recover_stopped_demo_build(&state).await? {
        tracing::warn!(
            "The server stopped during a Demo Account build, so the part-built Demo Account was removed"
        );
    }
    // Uploads a stopped server left open, which no upload start sweeps (#2261).
    crate::asset_store::sweep_incoming_at_start(
        &state.db,
        &state.cfg.paths,
        state.upload_time_limits.stall,
    )
    .await;
    // Import batch bodies a stopped server left, before any batch can start.
    crate::imports_api::batch_bodies::sweep_at_start(&state.cfg.paths.data_dir);
    // Works through what earlier Import Runs queued, a server stopped
    // part-way included, then waits for the next run to end.
    state.media_queue.start(
        state.db.clone(),
        Arc::clone(&state.cfg),
        Arc::clone(&state.media_tools),
    );
    // Clears a write-ahead log that a read kept from being cleared after a
    // delete for good (#2108).
    crate::db::erase::keep_retrying(state.db.clone());
    // Closes Export Runs no page was read from for an hour, a server stopped
    // part-way included (#2080).
    crate::db::exports::keep_closing_idle_runs(state.db.clone());
    // Reported as they stand now; each upload reads them again. Any stored
    // limit starts the server: a part is never larger than the limit.
    let upload_limits = state.assets.upload_limits(&state.db).await?;
    tracing::info!(
        "An attachment may be up to {} MiB, and is uploaded in parts of {} MiB",
        upload_limits.max_bytes / message_ir::MIB,
        upload_limits.part_size as u64 / message_ir::MIB
    );

    let demo_build = state.demo.build.clone();
    let media_queue = state.media_queue.clone();
    let on_signal = state.on_stop_signal();
    let in_flight = state.in_flight.clone();
    let app = http_app(state);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|error| {
            anyhow::Error::from(error).context(format!("could not listen on {bind}"))
        })?;
    // The address the listener holds, not the one it was given, so a bind to
    // port 0 names the port the operating system chose. A plain line on
    // stderr, not a `tracing` one, because the desktop app and the tests
    // read it there by its start (`LISTENING_LINE`). A failed write is
    // dropped rather than panicking, as `eprintln!` would: the desktop app
    // reads stderr through a pipe, and an app that crashes takes the pipe's
    // read end with it (#1934).
    {
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr(),
            "{}http://{}",
            message_crate_serve_protocol::LISTENING_LINE,
            listener.local_addr()?
        );
    }
    tracing::info!(
        "`message-crate-server dump-openapi` lists every route, and [server] openapi_ui = true serves the API reference at /docs"
    );
    // A Demo Account build the owner started would otherwise end part-way
    // when the process exits (#1215), and an ffmpeg the pass started would
    // go on converting after it (#1729). Both are stopped as the signal
    // comes, and the stop waits for them alongside the requests in flight,
    // within the same limit, so nothing after the limit can outlast Docker's
    // stop time. A build cut off at the limit is removed at the next start.
    let stop_work = move || {
        let demo_build = demo_build.clone();
        let media_queue = media_queue.clone();
        async move {
            tokio::join!(demo_build.stop(), media_queue.stop());
        }
    };
    let on_signal = move || {
        on_signal();
        stop_work()
    };
    serve_until_shutdown(
        listener,
        app,
        shutdown_signal(exit_with_parent),
        on_signal,
        || tokio::time::sleep(STOP_DRAIN_LIMIT),
    )
    .await
    .log(&in_flight);
    tracing::info!("The server has stopped");
    Ok(())
}

/// How long a stopping server waits for the requests in flight, a Demo
/// Account build's cleanup and the Thumbnail and Preview pass, before it
/// stops without them. An import batch whose import has started is the
/// longest of them, and one cut off is rolled back by SQLite and sent again
/// by the Upload, so the limit costs a batch's work and never its data.
///
/// Without a limit the stop waited as long as the longest request, and
/// `docker stop` killed the server after Docker's 10 seconds with no line in
/// the log saying why (#2122). `stop_grace_period` in both Compose files and
/// `--stop-timeout` in every documented `docker run` give the server 10
/// seconds more than this, so Docker never has to kill it. The server's
/// tests hold them to that (`docker_stop_waits_longer_than_the_drain`).
pub(crate) const STOP_DRAIN_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// How a stopping server's wait for its work ended.
#[derive(Debug)]
enum Stopped {
    /// Every request in flight, and the work `on_signal` started, finished.
    Drained,
    /// The drain limit passed with requests, or the work `on_signal`
    /// started, still running.
    GaveUp,
}

impl Stopped {
    /// When the server gave up, write a `WARN` line saying so and one naming
    /// each request it stopped without. A server that drained writes
    /// nothing here.
    fn log(self, in_flight: &crate::in_flight::InFlight) {
        match self {
            Self::Drained => {}
            Self::GaveUp => {
                let running = in_flight.running();
                tracing::warn!(
                    "The server stopped waiting {} seconds after it was told to stop, and exits without the work still running. Requests still running: {}. An import batch among the requests is rolled back, and the Upload sends it again",
                    STOP_DRAIN_LIMIT.as_secs(),
                    running.len()
                );
                for request in running {
                    tracing::warn!(
                        "Still running after {} seconds: request {}, {} {}",
                        request.running_for.as_secs(),
                        request.request_id,
                        request.method,
                        request.uri
                    );
                }
            }
        }
    }
}

/// Serve `app` on `listener` until `stop` resolves, then stop accepting
/// connections and return once the requests in flight and the work
/// `on_signal` started have finished, or once the future `drain_limit`
/// makes resolves, whichever comes first.
///
/// `on_signal` runs as `stop` resolves, before the requests drain, and
/// returns the work that ends alongside them. `serve` passes
/// [`AppState::on_stop_signal`] followed by the stop of the Demo Account
/// build and of the Thumbnail and Preview pass. `drain_limit` is called at
/// that moment too, so the limit counts from the stop. `serve` passes a
/// sleep of [`STOP_DRAIN_LIMIT`], and a test a future it resolves itself.
/// What is still running when the limit passes is left to end with the
/// process (`main` shuts the runtime down without waiting for it).
///
/// Every connection is held to [`crate::connections::ConnectionLimits::SERVER`]:
/// how many are open at once, and how long one may take over its headers or
/// sit idle (#2100).
async fn serve_until_shutdown<W, D>(
    listener: tokio::net::TcpListener,
    app: Router,
    stop: impl Future<Output = ()> + Send + 'static,
    on_signal: impl FnOnce() -> W + Send + 'static,
    drain_limit: impl FnOnce() -> D,
) -> Stopped
where
    W: Future<Output = ()> + Send + 'static,
    D: Future<Output = ()>,
{
    let (work_tx, work_rx) = tokio::sync::oneshot::channel::<W>();
    let (signaled_tx, signaled_rx) = tokio::sync::oneshot::channel::<()>();
    let served = crate::connections::serve_connections(
        listener,
        app,
        crate::connections::ConnectionLimits::SERVER,
        async move {
            stop.await;
            let _ = work_tx.send(on_signal());
            let _ = signaled_tx.send(());
        },
    );
    let work = async move {
        if let Ok(work) = work_rx.await {
            work.await;
        }
    };
    let finished = async move {
        tokio::join!(served, work);
    };
    // `serve_connections` returns only after `stop` resolved, and `stop`
    // sends on both channels before it resolves, so the receive below waits
    // for the stop and is never refused.
    let limit_passed = async move {
        let _ = signaled_rx.await;
        drain_limit().await;
    };
    tokio::select! {
        () = finished => Stopped::Drained,
        () = limit_passed => Stopped::GaveUp,
    }
}

/// Resolve on Ctrl-C, or on SIGTERM on Unix, or once the process
/// `exit_with_parent` names is gone, so
/// [`crate::connections::serve_connections`] drains in-flight requests, for
/// up to [`STOP_DRAIN_LIMIT`], before exiting. A crashed desktop app ends
/// its server this way (#1934).
async fn shutdown_signal(exit_with_parent: Option<ParentWatch>) {
    let parent_gone = async {
        match exit_with_parent {
            Some(watch) => watch.gone().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        () = stop_requested() => {}
        () = parent_gone => {}
    }
    tracing::info!(
        "The server is shutting down. It waits up to {} seconds for the requests in flight",
        STOP_DRAIN_LIMIT.as_secs()
    );
}

/// Resolve on Ctrl-C, or on SIGTERM on Unix. `docker stop` and a service
/// manager send SIGTERM, not Ctrl-C (#1218).
pub(crate) async fn stop_requested() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            // Without a SIGTERM handler the process still stops on SIGTERM,
            // only without draining; Ctrl-C keeps working.
            Err(e) => {
                tracing::warn!(
                    "SIGTERM cannot be caught, so it ends this process at once, without finishing the work in progress: {e}"
                );
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

#[cfg(test)]
mod tests;
