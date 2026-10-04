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

use std::sync::Arc;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use tokio::sync::Notify;

use crate::config::Config;
use crate::db::media_queue::{self, QueuedAsset};
use crate::process_assets::ProcessAssetsStats;

/// The handle that wakes the background pass. Every clone wakes the same
/// pass; one that no pass was started for wakes nothing, which is how the
/// tests see the queue before the pass runs.
#[derive(Debug, Clone, Default)]
pub(crate) struct MediaQueue {
    wake: Arc<Notify>,
}

impl MediaQueue {
    /// Wake the pass. A wake that comes while it is working makes it look
    /// at the queue once more when it is done.
    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }

    /// Start the pass: it works through the queue now, which is what
    /// resumes the work a stopped server left, and again each time it is
    /// woken. It runs until the process ends.
    ///
    /// The conversions run ffmpeg and wait for it, for minutes on a long
    /// video, so the pass runs on a thread of its own, with a runtime of its
    /// own, rather than on the threads that answer requests. A plain thread
    /// and not a blocking task of the server's runtime, because the runtime
    /// waits for its blocking tasks when it shuts down, and the pass never
    /// ends.
    ///
    /// # Panics
    ///
    /// When the operating system cannot start a thread or a runtime, which
    /// leaves the server unable to serve anything either.
    pub(crate) fn start(&self, pool: SqlitePool, cfg: Arc<Config>) {
        let wake = Arc::clone(&self.wake);
        std::thread::Builder::new()
            .name("media-queue".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime for the pass that makes Thumbnails and Previews");
                runtime.block_on(async move {
                    loop {
                        if let Err(error) = work_through(&pool, &cfg).await {
                            tracing::warn!(
                                error = format!("{error:#}"),
                                "the pass that makes Thumbnails and Previews stopped; it starts again after the next Import Run"
                            );
                        }
                        wake.notified().await;
                    }
                });
            })
            .expect("a thread for the pass that makes Thumbnails and Previews");
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
    let queued = async {
        let mut conn = pool.acquire().await?;
        media_queue::queue_import_run(&mut conn, account_id, import_id).await
    }
    .await;
    match queued {
        Ok(0) => {}
        Ok(count) => {
            tracing::info!(
                account_id,
                import_id,
                count,
                "queued Assets for Thumbnails and Previews"
            );
            queue.wake();
        }
        Err(error) => tracing::warn!(
            account_id,
            import_id,
            %error,
            "the Import Run's Assets could not be queued for Thumbnails and Previews"
        ),
    }
}

/// Make the Thumbnail and Preview of every queued Asset, oldest first, until
/// the queue is empty, and answer what was made. Each Asset leaves the queue
/// once it is processed, whether or not every version could be made; a
/// failure is logged, and `process-assets` tries it again. Without ffmpeg
/// nothing is made and the queue is left as it is.
///
/// # Errors
///
/// Returns an error when the queue cannot be read or written, or the work
/// directory cannot be made.
pub(crate) async fn work_through(pool: &SqlitePool, cfg: &Config) -> Result<ProcessAssetsStats> {
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
    let work = tempfile::TempDir::new().context("make a work directory for Thumbnails")?;
    // A connection is taken for each Asset and given back after it, so the
    // pass holds none of the pool's connections between Assets.
    loop {
        let mut conn = pool.acquire().await?;
        let Some(asset) = media_queue::first(&mut conn).await? else {
            break;
        };
        let done = process(cfg, &mut conn, work.path(), &asset).await;
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
        media_queue::remove(&mut conn, &asset).await?;
    }
    tracing::info!(
        thumbnails = stats.thumbnails,
        previews = stats.derived,
        failures = stats.errors,
        "the queued Assets are done"
    );
    Ok(stats)
}

/// Make what one queued Asset needs.
async fn process(
    cfg: &Config,
    conn: &mut sqlx::SqliteConnection,
    work_dir: &std::path::Path,
    asset: &QueuedAsset,
) -> Result<ProcessAssetsStats> {
    crate::process_assets::process_one_asset(cfg, conn, work_dir, asset.account_id, &asset.sha256)
        .await
}

#[cfg(test)]
mod tests;
