//! The background pass that makes Thumbnails and Previews after each Import
//! Run (`docs/architecture/media.md`, rule 4).
//!
//! When an Import Run ends, [`queue_import_run`] adds the Assets its
//! messages name to the `media_queue` table and wakes the pass; the run's
//! answer never waits for it. The pass, which `serve` starts once, takes the
//! queued Assets oldest first, makes what each needs with
//! [`crate::process_assets::process_one_asset`], and removes its row. The
//! queue is a table, so the rows a stopped server leaves are worked on when
//! it starts again, and a run of the `import` command, which has no pass of
//! its own, is worked on by the next `serve`.
//!
//! Without ffmpeg the pass makes nothing and leaves the queue as it is, so
//! the Assets are worked on once ffmpeg is there: at the next start, or
//! after the next Import Run.
//!
//! When the server stops, [`MediaQueue::stop`] kills the ffmpeg the pass
//! runs and waits for the pass to end. The Asset it was working on stays
//! queued, and the part-made file is removed with the work directory.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use anyhow::Result;
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
    pub(crate) fn start(&self, pool: SqlitePool, cfg: Arc<Config>) {
        let wake = Arc::clone(&self.wake);
        let stop = Arc::clone(&self.stop);
        let thread = std::thread::Builder::new()
            .name("media-queue".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime for the pass that makes Thumbnails and Previews");
                runtime.block_on(async move {
                    while !stop.load(Ordering::Relaxed) {
                        if let Err(error) = work_through(&pool, &cfg, &stop).await {
                            tracing::warn!(
                                error = format!("{error:#}"),
                                "the pass that makes Thumbnails and Previews stopped; it starts again after the next Import Run"
                            );
                        }
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        wake.notified().await;
                    }
                });
            })
            .expect("a thread for the pass that makes Thumbnails and Previews");
        *self.thread.lock().unwrap_or_else(PoisonError::into_inner) = Some(thread);
    }

    /// Stop the pass and wait for it to end, for a server that is stopping:
    /// the ffmpeg it runs is killed and waited for, the Asset it was working
    /// on stays queued for the next start, and its work directory, with the
    /// part-made file, is removed. Waits for nothing when no pass was
    /// started.
    pub(crate) async fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.wake.notify_one();
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(thread) = thread else {
            return;
        };
        let ended = tokio::task::spawn_blocking(move || thread.join()).await;
        if !matches!(ended, Ok(Ok(()))) {
            tracing::warn!("the pass that makes Thumbnails and Previews did not end cleanly");
        }
    }
}

/// Queue the Assets Import Run `import_id` of `account_id` brought and wake
/// the pass. A failure is logged: the run's end is what the caller answers
/// for, and `process-assets` repairs what was not queued.
pub(crate) async fn queue_import_run(
    pool: &SqlitePool,
    queue: &MediaQueue,
    account_id: i64,
    import_id: i64,
) {
    let queued = match pool.acquire().await {
        Ok(mut conn) => queue_without_waking(&mut conn, account_id, import_id).await,
        Err(error) => {
            log_not_queued(account_id, import_id, &error);
            0
        }
    };
    if queued > 0 {
        queue.wake();
    }
}

/// Queue the Assets Import Run `import_id` of `account_id` brought, on
/// `conn`, without waking a pass: the `import` command runs none, and the
/// next `serve` works on them. Answers how many were queued; a failure is
/// logged and answers none.
pub(crate) async fn queue_without_waking(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    import_id: i64,
) -> u64 {
    match media_queue::queue_import_run(conn, account_id, import_id).await {
        Ok(count) => {
            if count > 0 {
                tracing::info!(
                    account_id,
                    import_id,
                    count,
                    "queued Assets for Thumbnails and Previews"
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

fn log_not_queued(account_id: i64, import_id: i64, error: &sqlx::Error) {
    tracing::warn!(
        account_id,
        import_id,
        %error,
        "the Import Run's Assets could not be queued for Thumbnails and Previews"
    );
}

/// Make the Thumbnail and Preview of every queued Asset, oldest first, until
/// the queue is empty or `stop` is set, and answer what was made. Each Asset
/// leaves the queue once it is processed, whether or not every version could
/// be made; a failure is logged, and `process-assets` tries it again. An
/// Asset queued again while it was worked on stays queued, and so does the
/// one being worked on when `stop` is set. Without ffmpeg nothing is made
/// and the queue is left as it is.
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
    stop: &AtomicBool,
) -> Result<ProcessAssetsStats> {
    let mut stats = ProcessAssetsStats::default();
    let waiting = media_queue::count(&mut *pool.acquire().await?).await?;
    if waiting == 0 {
        return Ok(stats);
    }
    if !media::ffmpeg_available() {
        tracing::warn!(
            waiting,
            "ffmpeg was not found, so no Thumbnail or Preview is made; the Assets wait in the queue until the server finds it"
        );
        return Ok(stats);
    }
    let work = crate::process_assets::work_dir(&cfg.paths.data_dir)?;
    while !stop.load(Ordering::Relaxed)
        && let Some(asset) = media_queue::first(&mut *pool.acquire().await?).await?
    {
        let done = crate::process_assets::process_one_asset(
            cfg,
            pool,
            work.path(),
            asset.account_id,
            &asset.sha256,
            stop,
        )
        .await;
        if stop.load(Ordering::Relaxed) {
            // Whatever it was making is gone with the work directory, so it
            // is made at the next start.
            tracing::info!(
                account_id = asset.account_id,
                sha256 = asset.sha256,
                "the server is stopping; the Asset stays queued"
            );
            break;
        }
        match done {
            Ok(made) => stats.add(&made),
            Err(error) => {
                stats.errors += 1;
                tracing::warn!(
                    account_id = asset.account_id,
                    sha256 = asset.sha256,
                    error = format!("{error:#}"),
                    "an Asset's Thumbnail and Preview could not be made"
                );
            }
        }
        media_queue::remove(&mut *pool.acquire().await?, &asset).await?;
    }
    if !stop.load(Ordering::Relaxed) {
        tracing::info!(
            thumbnails = stats.thumbnails,
            previews = stats.derived,
            failures = stats.errors,
            "the queued Assets are done"
        );
    }
    Ok(stats)
}

#[cfg(test)]
mod tests;
