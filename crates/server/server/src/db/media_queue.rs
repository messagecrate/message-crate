//! `media_queue`: the Assets whose Thumbnail and Preview the server still has
//! to make, one row per account and fingerprint. An Import Run that ends adds
//! the Assets its messages name, and the background pass
//! ([`crate::media_queue`]) takes them oldest first and removes each when it
//! is done with it.

use sqlx::SqliteConnection;

/// One queued Asset: the account and the original's fingerprint, and the
/// row it was read from.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct QueuedAsset {
    pub account_id: i64,
    pub sha256: String,
    /// The row's `rowid`. Queuing an Asset again gives it a new row, so a
    /// pass that is still working on the old one removes only that.
    pub rowid: i64,
}

/// Queue every Asset the messages of Import Run `import_id` of `account_id`
/// name, and answer how many. An Asset already queued is queued again, at
/// the back: the pass may be working on it, and the run's new rows must not
/// be left out when that work is done.
///
/// # Errors
///
/// Returns a database error when the statement fails.
pub async fn queue_import_run(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
) -> Result<u64, sqlx::Error> {
    let queued = sqlx::query(
        "INSERT OR REPLACE INTO media_queue (account_id, sha256, queued_at)
         SELECT DISTINCT m.account_id, a.sha256, $3
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1 AND m.import_id = $2
           AND a.sha256 IS NOT NULL AND a.sha256 != ''
           AND a.assets_path IS NOT NULL AND a.assets_path != ''",
    )
    .bind(account_id)
    .bind(import_id)
    .bind(chrono::Utc::now().timestamp())
    .execute(&mut *conn)
    .await?;
    Ok(queued.rows_affected())
}

/// The Asset queued first, or `None` when the queue is empty.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn first(conn: &mut SqliteConnection) -> Result<Option<QueuedAsset>, sqlx::Error> {
    sqlx::query_as(
        "SELECT account_id, sha256, rowid FROM media_queue
         ORDER BY queued_at, rowid
         LIMIT 1",
    )
    .fetch_optional(&mut *conn)
    .await
}

/// Take `asset` off the queue, unless it was queued again since it was
/// read.
///
/// # Errors
///
/// Returns a database error when the statement fails.
pub async fn remove(conn: &mut SqliteConnection, asset: &QueuedAsset) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM media_queue WHERE account_id = $1 AND sha256 = $2 AND rowid = $3")
        .bind(asset.account_id)
        .bind(&asset.sha256)
        .bind(asset.rowid)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// How many Assets are queued.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn count(conn: &mut SqliteConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM media_queue")
        .fetch_one(&mut *conn)
        .await
}
