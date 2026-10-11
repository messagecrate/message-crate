//! The record of a Demo Account build that has not finished.
//!
//! One row, at id 1, in `demo_account_build`. A build writes it before its
//! first write and removes it after its last, so a row that is there when the
//! server starts belongs to a build the server stopped part-way (#1215).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::db::session_tokens::unix_secs_string;

/// How much Demo Data the Demo Account holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DemoDataSize {
    /// About 54,000 messages. A new Message Crate starts with this.
    Medium,
    /// About 613,000 messages. Building it takes about a minute.
    Large,
}

impl From<DemoDataSize> for demo_seed::DemoSize {
    fn from(size: DemoDataSize) -> Self {
        match size {
            DemoDataSize::Medium => Self::Medium,
            DemoDataSize::Large => Self::Large,
        }
    }
}

/// Record that a Demo Account build has started. A record already there is
/// replaced, since only one build runs at a time.
///
/// # Errors
///
/// Returns an error when the row cannot be written.
pub async fn begin(conn: &mut SqliteConnection) -> Result<()> {
    sqlx::query("INSERT OR REPLACE INTO demo_account_build (id, started_at) VALUES (1, $1)")
        .bind(unix_secs_string())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Remove the record: the build finished, or what it left has been removed.
///
/// # Errors
///
/// Returns an error when the row cannot be deleted.
pub async fn end(conn: &mut SqliteConnection) -> Result<()> {
    sqlx::query("DELETE FROM demo_account_build WHERE id = 1")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Whether a Demo Account build has started and not finished.
///
/// # Errors
///
/// Returns an error when the table cannot be read.
pub async fn is_unfinished(conn: &mut SqliteConnection) -> Result<bool> {
    let row: Option<i64> = sqlx::query_scalar("SELECT id FROM demo_account_build WHERE id = 1")
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
}
