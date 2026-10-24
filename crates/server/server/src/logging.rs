//! Where the server's diagnostics go.
//!
//! Every runtime message, an internal error behind a 500, a warning the
//! importer could not act on, the request log, goes through `tracing` and
//! lands on stderr through one subscriber installed by [`init`]. `RUST_LOG`
//! picks the level (`info` when unset); the CLI subcommands keep printing
//! their own progress to stdout, which is their output, not a log, and the
//! work they share with `serve` is told which through
//! `crate::progress::Progress`.
//!
//! `serve` also writes every line to the server's log, rotating files in the
//! Data Directory's `logs` directory ([`write_files_in`], [`LogFiles`]), which
//! the owner reads through `GET /v1/server/log-lines` and
//! `GET /v1/server/log-files`. Why files, why 5 of 50 MB and why the owner:
//! `docs/architecture/rules/server-log.md`.

mod files;
mod read;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;

pub(crate) use files::log_file_path;
pub use files::{LogFiles, LogLimits, SERVER_LOG_LIMITS};
pub(crate) use read::{LogFile, LogLevel, LogLine, LogLinesQuery, list_files, read_lines};

/// The directory of the server's log inside a Data Directory.
pub fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

/// The files the process-wide subscriber writes to, once `serve` names them.
/// Empty before then, and in every other subcommand, whose lines go to
/// stderr alone.
static SERVER_LOG: RwLock<Option<LogFiles>> = RwLock::new(None);

/// Install the subscriber: stderr, and the server's log once
/// [`write_files_in`] opens it. Call once, before any work; a second call is
/// ignored so tests that build a server in-process do not panic. A unit
/// test never calls it: the log capture of unit tests installs its own
/// process-wide subscriber.
///
/// The call that installs the subscriber also installs the panic hook
/// (`log_panics`), so a panic on any thread of the process reaches the
/// log as well.
///
/// Neither layer reports a write it could not make. A layer that does
/// reports it with `eprintln!`, which panics when stderr is a pipe with no
/// reader, as it is once the desktop app that started the server has
/// crashed (#1934). Every line would then panic, and the panic hook's own
/// line would panic inside the hook, which aborts the process.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(std::io::stderr().is_terminal())
                .with_target(false)
                .log_internal_errors(false),
        )
        .with(file_layer(FileTarget::Process));
    if tracing::subscriber::set_global_default(subscriber).is_ok() {
        log_panics();
    }
}

/// Send every panic of the process through `tracing` at `ERROR`, with its
/// message and the file and line it happened at, in place of Rust's default
/// hook, which writes to stderr alone. A panic in a request handler, the
/// media pass or any other thread of `serve` then leaves a line in the
/// server's log, under the span it happened in, so a handler's panic carries
/// its request's id (#2086).
///
/// The default hook still runs when `RUST_BACKTRACE` asks for a backtrace,
/// because only it writes one, and when `RUST_LOG` leaves out the `ERROR`
/// line, so a panic always reaches stderr or the log.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let logged = tracing::event_enabled!(tracing::Level::ERROR);
        tracing::error!(panic = %info, "A thread of the server panicked");
        if !logged || std::env::var_os("RUST_BACKTRACE").is_some_and(|value| value != "0") {
            default(info);
        }
    }));
}

/// Write why the process ends with `error`. Once `serve` has opened the
/// server's log, the line goes there at `ERROR`, with the error's whole
/// chain, as well as to stderr, so the owner can read why a Message Crate
/// would not start or why it stopped (#2079). Before then, in every other
/// subcommand, and when `RUST_LOG` leaves out the `ERROR` line, it goes to
/// stderr alone, as `fn main() -> Result<()>` would write it, so the reason
/// always reaches stderr.
pub fn write_failure(error: &anyhow::Error) {
    let writes_files = SERVER_LOG.read().is_ok_and(|slot| slot.is_some());
    if writes_files && tracing::event_enabled!(tracing::Level::ERROR) {
        tracing::error!(
            error = %crate::api_error::error_chain(error),
            "The server stopped with an error"
        );
    } else {
        eprintln!("Error: {error:?}");
    }
}

/// Open the server's log in `data_dir` and send every line from here on to it
/// as well as to stderr. `serve` calls it before it touches the database, so
/// a database that fails to open is in the log ([`write_failure`]).
///
/// # Errors
///
/// Returns an error when the `logs` directory cannot be made or its newest
/// file cannot be opened.
pub fn write_files_in(data_dir: &Path) -> std::io::Result<LogFiles> {
    let files = LogFiles::open(&log_dir(data_dir), SERVER_LOG_LIMITS)?;
    if let Ok(mut slot) = SERVER_LOG.write() {
        *slot = Some(files.clone());
    }
    Ok(files)
}

/// A subscriber that writes, at `filter`, to `files` alone, formatted as the
/// server's log is. For a test that reads what a run left in the log. Its
/// first call also installs [`AskEachTime`] process-wide, through
/// [`install_ask_each_time`].
#[cfg(test)]
pub(crate) fn subscriber_for(
    files: LogFiles,
    filter: &str,
) -> impl tracing::Subscriber + Send + Sync {
    install_ask_each_time();
    tracing_subscriber::registry()
        .with(EnvFilter::new(filter))
        .with(file_layer(FileTarget::Files(files)))
}

/// Install [`AskEachTime`] as the process-wide subscriber, once.
///
/// A test installs its log capture as its thread's default, beside tests on
/// other threads that install none. `tracing` caches, once per process,
/// whether any subscriber wants each `tracing::warn!` and the like. While
/// exactly one subscriber exists in the process, it fills that cache by
/// asking only the subscriber of the thread that first reaches the line. A
/// test thread with no subscriber then marks the line as wanted by no one,
/// and the test whose subscriber does want it reads an empty log (#2824).
/// With a process-wide subscriber, at least two subscribers exist whenever
/// a test captures the log, so `tracing` asks every one of them.
#[cfg(test)]
fn install_ask_each_time() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        tracing::subscriber::set_global_default(AskEachTime)
            .expect("no unit test calls `init` or installs a process-wide subscriber");
    });
}

/// The process-wide subscriber of a test run: it records nothing, and
/// answers "ask me each time" for every line, so no line's cached answer is
/// ever "no one wants it" and each line asks the subscriber of the thread
/// it runs on.
#[cfg(test)]
struct AskEachTime;

#[cfg(test)]
impl tracing::Subscriber for AskEachTime {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        false
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, _: &tracing::Event<'_>) {}

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// The server's log layer: the stderr format without color, one event per
/// line.
fn file_layer<S>(target: FileTarget) -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    tracing_subscriber::fmt::layer()
        .with_writer(target)
        .with_ansi(false)
        .with_target(false)
        // A write a full disk refuses is not reported on stderr, which may
        // be a pipe with no reader (`init`).
        .log_internal_errors(false)
}

/// Which files the server's log layer writes to.
#[derive(Clone)]
enum FileTarget {
    /// Whatever [`write_files_in`] opened for this process, or nowhere.
    Process,
    /// These files.
    #[cfg(test)]
    Files(LogFiles),
}

impl<'a> MakeWriter<'a> for FileTarget {
    type Writer = EventWriter;

    fn make_writer(&'a self) -> Self::Writer {
        EventWriter(match self {
            Self::Process => SERVER_LOG.read().ok().and_then(|slot| slot.clone()),
            #[cfg(test)]
            Self::Files(files) => Some(files.clone()),
        })
    }
}

/// Hands one formatted event to the files. `tracing_subscriber`'s `fmt`
/// layer formats an event whole and writes it in one `write_all`, so each
/// call here is one event.
struct EventWriter(Option<LogFiles>);

impl std::io::Write for EventWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(files) = &self.0 {
            files.write_event(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
