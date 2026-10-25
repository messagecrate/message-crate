//! Settings that belong to the whole server rather than to one account.
//!
//! One row, at id 1. A database that has never been written to has no row at
//! all, which reads the same as a row of defaults (`asset_max_bytes` at
//! 512 MiB), so nothing has to seed it.

use anyhow::{Context as _, Result};
use sqlx::SqliteConnection;

/// What the attachment size limit reads as until the administrator sets it: 512 MiB.
pub const DEFAULT_ASSET_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// The server settings the administrator controls.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
pub struct ServerSettings {
    /// The attachment size limit, in bytes: the largest asset the server
    /// accepts. 536870912 (512 MiB) until the administrator changes it.
    // The limit holds for one `PUT` and for the declared total of a
    // multipart upload alike. This row is the only place it lives.
    pub asset_max_bytes: u64,
}

impl Default for ServerSettings {
    /// What an unwritten database reads as: an attachment may be up to
    /// 512 MiB.
    fn default() -> Self {
        Self {
            asset_max_bytes: DEFAULT_ASSET_MAX_BYTES,
        }
    }
}

/// Read the server's settings, or the defaults when nothing has been written.
pub async fn load(conn: &mut SqliteConnection) -> Result<ServerSettings> {
    #[derive(sqlx::FromRow)]
    struct Row {
        asset_max_bytes: i64,
    }
    let row = sqlx::query_as::<_, Row>("SELECT asset_max_bytes FROM server_settings WHERE id = 1")
        .fetch_optional(&mut *conn)
        .await
        .context("read the server settings")?;
    Ok(
        row.map_or_else(ServerSettings::default, |row| ServerSettings {
            // The column is only ever written from a `u64` that fits.
            asset_max_bytes: u64::try_from(row.asset_max_bytes).unwrap_or(0),
        }),
    )
}

/// Set the attachment size limit, creating the settings row if this is the
/// first thing ever written to it. The caller has checked the number against
/// the part size; this stores what it is given.
///
/// # Errors
///
/// Returns an error when `bytes` does not fit the column, or the write fails.
pub async fn set_asset_max_bytes(conn: &mut SqliteConnection, bytes: u64) -> Result<()> {
    let bytes = i64::try_from(bytes)?;
    sqlx::query(
        "INSERT INTO server_settings (id, asset_max_bytes) VALUES (1, $1)
         ON CONFLICT(id) DO UPDATE SET asset_max_bytes = excluded.asset_max_bytes",
    )
    .bind(bytes)
    .execute(&mut *conn)
    .await
    .context("store the attachment size limit")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database nobody has written settings to reads the default limit,
    /// and writing the limit twice updates the one row rather than failing
    /// on its primary key. The limit is above 2 GiB, which a 32-bit column
    /// would not hold.
    #[tokio::test]
    async fn the_limit_reads_as_the_default_until_set_and_round_trips() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            load(&mut conn).await.unwrap().asset_max_bytes,
            DEFAULT_ASSET_MAX_BYTES
        );

        let four_gib = 4 * 1024 * 1024 * 1024;
        set_asset_max_bytes(&mut conn, four_gib).await.unwrap();
        assert_eq!(load(&mut conn).await.unwrap().asset_max_bytes, four_gib);
        set_asset_max_bytes(&mut conn, 1024).await.unwrap();
        assert_eq!(load(&mut conn).await.unwrap().asset_max_bytes, 1024);

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM server_settings")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(rows, 1, "the database has one settings record");
    }
}
