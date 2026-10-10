//! The background pass that makes Thumbnails and Previews after each Import
//! Run (`docs/architecture/media.md`, rule 4).
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
                                "The pass that makes Thumbnails and Previews stopped. It starts again after the next Import Run"
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
        let ended = tokio::task::spawn_blocking(move || thread.join()).await;
        if !matches!(ended, Ok(Ok(()))) {
            tracing::warn!("The pass that makes Thumbnails and Previews did not end cleanly");
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

fn log_not_queued(account_id: i64, import_id: i64, error: &sqlx::Error) {
    tracing::warn!(
        account_id,
        import_id,
        %error,
        "The Import Run's Assets could not be queued for Thumbnails and Previews"
    );
}

/// Record whether every browser shows each original queued in a row after
/// `after` as it is, until `stop` is set, leaving the queue as it is, and
/// answer the last row decided. Only an MP4 is opened, by ffprobe, which is
/// quick, and without ffprobe an MP4 is left as it was. An Asset whose rows
/// cannot be written is logged, counted in `stats.not_decided`, and left for
/// its turn in the pass, which records it again.
///
/// # Errors
///
/// Returns an error when the queue cannot be read.
async fn decide_queued(
    pool: &SqlitePool,
    cfg: &Config,
    stop: &AtomicBool,
    after: i64,
    stats: &mut ProcessAssetsStats,
) -> Result<i64> {
    let mut decided = after;
    for asset in media_queue::queued_after(&mut *pool.acquire().await?, after).await? {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Err(error) = crate::process_assets::decide_shown_as_is(
            cfg,
            pool,
            asset.account_id,
            Some(&asset.sha256),
        )
        .await
        {
            stats.not_decided += 1;
            tracing::warn!(
                account_id = asset.account_id,
                sha256 = asset.sha256,
                error = format!("{error:#}"),
                "Whether an Asset is shown as it is could not be recorded"
            );
        }
        decided = asset.rowid;
    }
    Ok(decided)
}

/// Make the Thumbnail and Preview of every queued Asset, oldest first, until
/// the queue is empty or `stop` is set, and answer what was made. Each Asset
/// leaves the queue once it is processed, whether or not every version could
/// be made; a failure is logged, and `process-assets` tries it again. An
/// Asset queued again while it was worked on stays queued, and so does the
/// one being worked on when `stop` is set. Without ffmpeg nothing is made
/// and the queue is left as it is.
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
    stop: &AtomicBool,
) -> Result<ProcessAssetsStats> {
    let mut stats = ProcessAssetsStats::default();
    let waiting = media_queue::count(&mut *pool.acquire().await?).await?;
    if waiting == 0 {
        return Ok(stats);
    }
    let mut decided = decide_queued(pool, cfg, stop, 0, &mut stats).await?;
    if let Some(why) = tools_unavailable(&media::ffmpeg_tools()) {
        tracing::warn!(waiting, "{why}");
        return Ok(stats);
    }
    let work = crate::process_assets::work_dir(&cfg.paths.data_dir)?;
    while !stop.load(Ordering::Relaxed)
        && let Some(asset) = media_queue::first(&mut *pool.acquire().await?).await?
    {
        // An Import Run that ended while the last Asset was converted
        // queued more, behind the videos already waiting.
        decided = decide_queued(pool, cfg, stop, decided, &mut stats).await?;
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
                "The server is stopping, so the Asset stays queued"
            );
            break;
        }
        match done {
            Ok(made) => stats.add(&made),
            Err(error) => {
                stats.not_made += 1;
                tracing::warn!(
                    account_id = asset.account_id,
                    sha256 = asset.sha256,
                    error = format!("{error:#}"),
                    "An Asset's Thumbnail and Preview could not be made"
                );
            }
        }
        media_queue::remove(&mut *pool.acquire().await?, &asset).await?;
    }
    if !stop.load(Ordering::Relaxed) {
        tracing::info!(
            thumbnails = stats.thumbnails,
            previews = stats.derived,
            removed = stats.removed,
            dropped = stats.dropped,
            shared = stats.shared,
            not_made = stats.not_made,
            not_removed = stats.not_removed,
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
