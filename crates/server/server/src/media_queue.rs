//! The background pass that makes Thumbnails and Previews after each Import
//! Run (`docs/architecture/rules/media.md`, MEDIA-4).
//!
//! When an Import Run ends, [`queue_import_run`] adds the Assets of the
//! attachment rows it wrote to the `media_queue` table and wakes the pass; the run's
//! answer never waits for it. The pass, which `serve` starts once, takes the
//! queued Assets oldest first, makes what each needs with
//! [`crate::process_assets::process_one_asset`], and removes its row. The
//! queue is a table, so the rows a stopped server leaves are worked on when
//! it starts again, and a run of the `import` command, which has no pass of
//! its own, is worked on by the next `serve`.
//!
//! Without ffmpeg the pass makes nothing and leaves the queue as it is, so
//! the Assets are worked on once ffmpeg is there: at the next start, or
//! after the next Import Run. With ffmpeg or without, the pass records
//! whether each newly queued original is shown as it is before its next
//! conversion ([`crate::process_assets::decide_shown_as_is`]), so a photo
//! opens without waiting for the videos queued before it.
//!
//! When the server stops, [`MediaQueue::stop`] kills the ffmpeg the pass
//! runs and waits for the pass to end. The Asset it was working on stays
//! queued, and the part-made file is removed with the work directory.
//!
//! Nothing one Asset does stops the pass: ffmpeg and ffprobe are killed
//! when they run too long (`media`), and an Asset whose conversion fails or
//! panics leaves the queue like any other failure. A pass that fails or
//! panics for any other reason, such as a database that stays busy, runs
//! again after a wait ([`keep_working`]), without waiting for an Import Run.

use std::any::Any;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Result;
use futures_util::FutureExt;
use media::MediaTools;
use sqlx::SqlitePool;
use tokio::sync::Notify;

use crate::config::Config;
use crate::db::media_queue;
use crate::process_assets::ProcessAssetsStats;

/// The handle that wakes and stops the background pass. Every clone wakes
/// the same pass; one that no pass was started for wakes nothing, which is
/// how the tests see the queue before the pass runs.
#[derive(Debug, Clone, Default)]
pub(crate) struct MediaQueue {
    wake: Arc<Notify>,
    /// Set when the server stops: the conversion that runs is killed, and
    /// the pass ends.
    stop: Arc<AtomicBool>,
    /// The pass's thread, for [`MediaQueue::stop`] to wait on.
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl MediaQueue {
    /// Wake the pass. A wake that comes while it is working makes it look
    /// at the queue once more when it is done.
    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }

    /// Start the pass: it works through the queue now, which is what
    /// resumes the work a stopped server left, and again each time it is
    /// woken. It runs until [`MediaQueue::stop`].
    ///
    /// The conversions run ffmpeg and wait for it, for minutes on a long
    /// video, so the pass runs on a thread of its own, with a runtime of its
    /// own, rather than on the threads that answer requests. A plain thread
    /// and not a blocking task of the server's runtime, because the runtime
    /// waits for its blocking tasks when it shuts down, and the pass ends
    /// only when it is stopped.
    ///
    /// # Panics
    ///
    /// When the operating system cannot start a thread or a runtime, which
    /// leaves the server unable to serve anything either.
    pub(crate) fn start(&self, pool: SqlitePool, cfg: Arc<Config>, tools: Arc<dyn MediaTools>) {
        let wake = Arc::clone(&self.wake);
        let stop = Arc::clone(&self.stop);
        let thread = std::thread::Builder::new()
            .name("media-queue".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime for the pass that makes Thumbnails and Previews");
                runtime.block_on(keep_working(&wake, &stop, FIRST_RETRY, async || {
                    work_through(&pool, &cfg, tools.as_ref(), &stop).await
                }));
            })
            .expect("a thread for the pass that makes Thumbnails and Previews");
        *self.thread.lock().unwrap_or_else(PoisonError::into_inner) = Some(thread);
    }

    /// Tell the pass to stop, without waiting for it: the ffmpeg it runs is
    /// killed, and it starts nothing more. [`MediaQueue::stop`] waits.
    pub(crate) fn ask_to_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.wake.notify_one();
    }

    /// Stop the pass and wait for it to end, for a server that is stopping:
    /// the ffmpeg it runs is killed and waited for, the Asset it was working
    /// on stays queued for the next start, and its work directory, with the
    /// part-made file, is removed. Waits for nothing when no pass was
    /// started.
    pub(crate) async fn stop(&self) {
        self.ask_to_stop();
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(thread) = thread else {
            return;
        };
        let ended = crate::request_id::spawn_blocking_in_current_span(move || thread.join()).await;
        if !matches!(ended, Ok(Ok(()))) {
            tracing::warn!("The pass that makes Thumbnails and Previews did not end cleanly");
        }
    }
}

/// How long the pass waits before it runs again after it failed or
/// panicked, the first time. The wait doubles with each failure in a row,
/// up to [`LONGEST_RETRY`], and starts here again after a pass that ends
/// cleanly.
const FIRST_RETRY: Duration = Duration::from_secs(5);

/// The longest the pass waits before it runs again after failures in a
/// row.
const LONGEST_RETRY: Duration = Duration::from_secs(5 * 60);

/// Run `pass` now, then each time `wake` is notified, until `stop` is set.
///
/// A pass that fails or panics is logged and runs again after a wait, or at
/// the next wake when that comes first (#2072, #2091). Without the wait, a
/// database busy for a moment stopped the pass until the next Import Run
/// ended, which on a server imported into once was never, and a panic ended
/// the pass's thread until the server restarted. The wait starts at
/// `first_retry`, [`FIRST_RETRY`] outside tests, and doubles to
/// [`LONGEST_RETRY`], so a failure that lasts does not fill the log, and a
/// pass that ends cleanly starts it again at `first_retry`.
async fn keep_working(
    wake: &Notify,
    stop: &AtomicBool,
    first_retry: Duration,
    mut pass: impl AsyncFnMut() -> Result<ProcessAssetsStats>,
) {
    let mut retry = first_retry;
    while !stop.load(Ordering::Relaxed) {
        let failed = match AssertUnwindSafe(pass()).catch_unwind().await {
            Ok(Ok(_)) => false,
            Ok(Err(error)) => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "The pass that makes Thumbnails and Previews stopped. It starts again in {} \
                     seconds, or when the next Import Run ends",
                    retry.as_secs()
                );
                true
            }
            Err(panic) => {
                tracing::error!(
                    panic = panic_text(&*panic),
                    "The pass that makes Thumbnails and Previews panicked. It starts again in {} \
                     seconds, or when the next Import Run ends",
                    retry.as_secs()
                );
                true
            }
        };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if failed {
            tokio::select! {
                () = wake.notified() => {}
                () = tokio::time::sleep(retry) => {}
            }
            retry = (retry * 2).min(LONGEST_RETRY);
        } else {
            retry = first_retry;
            wake.notified().await;
        }
    }
}

/// What a panic said: its message when it was given one, as `panic!` and
/// `expect` give it.
fn panic_text(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic with no message")
}

/// A panic in work on one Asset, made an error so that the Asset fails like
/// any other and the pass goes on to the next.
#[derive(Debug, thiserror::Error)]
#[error("it panicked: {0}")]
struct Panicked(String);

/// Run `work`, failing with [`Panicked`] when it panics.
async fn unless_it_panics<T>(work: impl Future<Output = Result<T>>) -> Result<T> {
    AssertUnwindSafe(work)
        .catch_unwind()
        .await
        .unwrap_or_else(|panic| Err(Panicked(panic_text(&*panic).to_string()).into()))
}

/// Log that one Asset's work failed with `error`, saying `what` failed: at
/// `error` when the work panicked, which is a bug, else at `warn`.
fn log_asset_failure(asset: &media_queue::QueuedAsset, what: &str, error: &anyhow::Error) {
    if error.downcast_ref::<Panicked>().is_some() {
        tracing::error!(
            account_id = asset.account_id,
            sha256 = asset.sha256,
            error = format!("{error:#}"),
            "{what}"
        );
    } else {
        tracing::warn!(
            account_id = asset.account_id,
            sha256 = asset.sha256,
            error = format!("{error:#}"),
            "{what}"
        );
    }
}

/// Queue the Assets Import Run `import_id` of `account_id` brought and wake
/// the pass, queued at `now`. A failure is logged: the run's end is what the
/// caller answers for, and `process-assets` repairs what was not queued.
pub(crate) async fn queue_import_run(
    pool: &SqlitePool,
    queue: &MediaQueue,
    account_id: i64,
    import_id: i64,
    now: chrono::DateTime<chrono::Utc>,
) {
    let queued = match pool.acquire().await {
        Ok(mut conn) => queue_without_waking(&mut conn, account_id, import_id, now).await,
        Err(error) => {
            log_not_queued(account_id, import_id, &error.into());
            0
        }
    };
    if queued > 0 {
        queue.wake();
    }
}

/// Queue the Assets Import Run `import_id` of `account_id` brought, on
/// `conn`, without waking a pass: the `import` command runs none, and the
/// next `serve` works on them. They are queued at `now`. Answers how many
/// were queued; a failure is logged and answers none.
pub(crate) async fn queue_without_waking(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    import_id: i64,
    now: chrono::DateTime<chrono::Utc>,
) -> u64 {
    match media_queue::queue_import_run(conn, account_id, import_id, now).await {
        Ok(count) => {
            if count > 0 {
                tracing::info!(
                    account_id,
                    import_id,
                    count,
                    "Assets are queued for Thumbnails and Previews"
                );
            }
            count
        }
        Err(error) => {
            log_not_queued(account_id, import_id, &error);
            0
        }
    }
}

fn log_not_queued(account_id: i64, import_id: i64, error: &anyhow::Error) {
    tracing::warn!(
        account_id,
        import_id,
        error = format!("{error:#}"),
        "The Import Run's Assets could not be queued for Thumbnails and Previews"
    );
}

/// Record whether every browser shows each original queued in a row after
/// `after` as it is, until `stop` is set, leaving the queue as it is, and
/// answer the last row decided. Only an MP4 is opened, by ffprobe, which is
/// quick, and without ffprobe an MP4 is left as it was. An Asset whose rows
/// cannot be written, or whose decision panics, is logged, counted in
/// `stats.not_decided`, and left for its turn in the pass, which records it
/// again.
///
/// # Errors
///
/// Returns an error when the queue cannot be read.
async fn decide_queued(
    pool: &SqlitePool,
    cfg: &Config,
    tools: &dyn MediaTools,
    stop: &AtomicBool,
    after: i64,
    stats: &mut ProcessAssetsStats,
) -> Result<i64> {
    let mut decided = after;
    for asset in media_queue::queued_after(&mut *pool.acquire().await?, after).await? {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Err(error) = unless_it_panics(crate::process_assets::decide_shown_as_is(
            cfg,
            tools,
            pool,
            asset.account_id,
            Some(&asset.sha256),
        ))
        .await
        {
            stats.not_decided += 1;
            log_asset_failure(
                &asset,
                "Whether an Asset is shown as it is could not be recorded",
                &error,
            );
        }
        decided = asset.rowid;
    }
    Ok(decided)
}

/// Make the Thumbnail and Preview of every queued Asset, oldest first, until
/// the queue is empty or `stop` is set, and answer what was made. Each Asset
/// leaves the queue once it is processed, whether or not every version could
/// be made, and whether its work failed or panicked; a failure is logged,
/// why each version was not made is recorded
/// on the Asset's rows for the `/v1` Attachment to answer, and
/// `process-assets` tries it again. An Asset queued again while it was
/// worked on stays queued, and so does the one being worked on when `stop`
/// is set. Without ffmpeg nothing is made and the queue is left as it is.
///
/// Before each conversion, the pass records whether every browser shows each
/// original queued since it last looked as it is ([`decide_queued`]), so a
/// photo opens at once rather than after the videos queued before it, a
/// photo an Import Run queues while the pass works included, and on a
/// server without ffmpeg.
///
/// A connection is taken for each query and given back after it, so the
/// pass holds none of the pool's connections while ffmpeg runs.
///
/// # Errors
///
/// Returns an error when the queue cannot be read or written, or the work
/// directory cannot be made.
pub(crate) async fn work_through(
    pool: &SqlitePool,
    cfg: &Config,
    tools: &dyn MediaTools,
    stop: &AtomicBool,
) -> Result<ProcessAssetsStats> {
    work_through_with(pool, cfg, tools, stop, async |work: &Path, asset| {
        crate::process_assets::process_one_asset(
            cfg,
            tools,
            pool,
            work,
            asset.account_id,
            &asset.sha256,
            stop,
        )
        .await
    })
    .await
}

/// [`work_through`], making each Asset's versions with `make`, in the work
/// directory it is given, so a test can make one fail or panic.
async fn work_through_with(
    pool: &SqlitePool,
    cfg: &Config,
    tools: &dyn MediaTools,
    stop: &AtomicBool,
    mut make: impl AsyncFnMut(&Path, &media_queue::QueuedAsset) -> Result<ProcessAssetsStats>,
) -> Result<ProcessAssetsStats> {
    let mut stats = ProcessAssetsStats::default();
    let waiting = media_queue::count(&mut *pool.acquire().await?).await?;
    if waiting == 0 {
        return Ok(stats);
    }
    let mut decided = decide_queued(pool, cfg, tools, stop, 0, &mut stats).await?;
    if let Some(why) = tools_unavailable(&tools.find()) {
        tracing::warn!(waiting, "{why}");
        return Ok(stats);
    }
    let work = crate::process_assets::work_dir(&cfg.paths.data_dir)?;
    while !stop.load(Ordering::Relaxed)
        && let Some(asset) = media_queue::first(&mut *pool.acquire().await?).await?
    {
        // An Import Run that ended while the last Asset was converted
        // queued more, behind the videos already waiting.
        decided = decide_queued(pool, cfg, tools, stop, decided, &mut stats).await?;
        let done = unless_it_panics(make(work.path(), &asset)).await;
        if stop.load(Ordering::Relaxed) {
            // Whatever it was making is gone with the work directory, so it
            // is made at the next start.
            tracing::info!(
                account_id = asset.account_id,
                sha256 = asset.sha256,
                "The server is stopping, so the Asset stays queued"
            );
            break;
        }
        match done {
            Ok(made) => stats.add(&made),
            Err(error) => {
                stats.not_made += 1;
                log_asset_failure(
                    &asset,
                    "An Asset's Thumbnail and Preview could not be made",
                    &error,
                );
            }
        }
        media_queue::remove(&mut *pool.acquire().await?, &asset).await?;
    }
    if !stop.load(Ordering::Relaxed) {
        tracing::info!(
            thumbnails = stats.thumbnails,
            previews = stats.previews,
            dropped = stats.dropped,
            shared = stats.shared,
            not_made = stats.not_made,
            not_dropped = stats.not_dropped,
            not_decided = stats.not_decided,
            "The queued Assets are done"
        );
    }
    Ok(stats)
}

/// Why the pass makes no Thumbnail or Preview, from where ffmpeg and
/// ffprobe were looked for, or `None` when both are found in one place.
/// Two programs in two places are found but not used, so that case says
/// why rather than that either is missing.
fn tools_unavailable(tools: &Result<media::FfmpegTools>) -> Option<String> {
    match tools {
        Err(err) => Some(format!(
            "ffmpeg and ffprobe are not used, so no Thumbnail or Preview is made. {err} \
             The Assets wait in the queue until both are in one place"
        )),
        Ok(tools) => {
            let missing = tools.missing();
            if missing.is_empty() {
                return None;
            }
            let (verb, pronoun) = if missing.len() == 1 {
                ("was", "it")
            } else {
                ("were", "them")
            };
            Some(format!(
                "{} {verb} not found, so no Thumbnail or Preview is made. \
                 The Assets wait in the queue until the server finds {pronoun}",
                missing.join(" and ")
            ))
        }
    }
}

#[cfg(test)]
mod tests;
