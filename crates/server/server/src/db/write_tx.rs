//! The server's one kind of transaction: a write transaction that holds
//! SQLite's write lock from its first statement.
//!
//! A deferred transaction (`BEGIN`) that reads and then writes takes its
//! snapshot at the read and asks for the write lock at the first write. When
//! another connection commits in between, SQLite refuses that write with
//! `SQLITE_BUSY_SNAPSHOT`, because the snapshot the transaction read is no
//! longer the newest. The server runs in WAL mode, and `busy_timeout` does
//! not retry that error, so the request fails. `BEGIN IMMEDIATE` takes the
//! write lock before the first read: a second writer waits on the busy
//! timeout instead, and every read inside the transaction sees the state the
//! writes act on.
//!
//! [`WriteTx`] is the only way to hold one. Only [`begin_write`] makes it, and
//! `sqlx::Connection::begin` is refused by Clippy (`clippy.toml` beside the
//! crate's `Cargo.toml`), so a deferred transaction cannot come back. A
//! function that reads and then writes takes `&mut WriteTx`, so a caller
//! cannot reach it from a bare connection.

use std::ops::{Deref, DerefMut};

use sqlx::{Connection, Sqlite, SqliteConnection, Transaction};

use crate::db::engine::BEGIN_IMMEDIATE_SQL;

/// A transaction begun with `BEGIN IMMEDIATE`. It dereferences to the
/// connection, so a query runs on `&mut *tx` (or `&mut **tx` through a
/// `&mut WriteTx`), and a helper that takes `&mut SqliteConnection` takes
/// `&mut tx`. Dropped without [`WriteTx::commit`], it rolls back.
pub struct WriteTx<'c>(Transaction<'c, Sqlite>);

/// Begin a write transaction on `conn`, waiting on the busy timeout while
/// another connection holds the write lock, and bring the connection's copy
/// of the schema up to date before the first statement runs.
///
/// `BEGIN IMMEDIATE` does not check the schema, so a connection that idled
/// while another changed it (a promote drops and re-creates the full-text
/// search triggers) still holds the old copy. When the transaction's first
/// statement fires a full-text search trigger on such a connection, SQLite
/// connects to `messages_fts` while that statement is being prepared, and
/// cannot reload the schema until it is prepared: the connection to
/// `messages_fts` fails, and the statement with it, as "no such table"
/// (#1600). [`SCHEMA_CHECK_SQL`] reads the schema table (`sqlite_master`),
/// which makes SQLite compare its copy with the file and reload it. Nothing
/// can change the schema after that while the write lock is held.
///
/// # Errors
///
/// Returns the database error when the lock is not granted within the busy
/// timeout, or when `conn` is already inside a transaction.
pub async fn begin_write(conn: &mut SqliteConnection) -> sqlx::Result<WriteTx<'_>> {
    let mut tx = conn.begin_with(BEGIN_IMMEDIATE_SQL).await?;
    sqlx::query(SCHEMA_CHECK_SQL).execute(&mut *tx).await?;
    Ok(WriteTx(tx))
}

/// A statement that reads the schema table and nothing else, so running it
/// reloads a connection's copy of the schema when the file's is newer.
const SCHEMA_CHECK_SQL: &str = "SELECT 1 FROM sqlite_master LIMIT 1";

impl WriteTx<'_> {
    /// Commit the transaction and release the write lock.
    ///
    /// # Errors
    ///
    /// Returns the database error when the commit fails; nothing is written.
    pub async fn commit(self) -> sqlx::Result<()> {
        self.0.commit().await
    }
}

impl Deref for WriteTx<'_> {
    type Target = SqliteConnection;

    fn deref(&self) -> &SqliteConnection {
        &self.0
    }
}

impl DerefMut for WriteTx<'_> {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        &mut self.0
    }
}

/// Run `op` while `other`, a write transaction on a second connection, is
/// still open, and commit `other` once `op` has had time to read and reach
/// its first write. A test of a read-then-write uses it to land another
/// write between the two, which is the order that breaks one.
///
/// The second given to `op` covers a password hash before its first
/// statement. An `op` that has not reached its read by then sees the other
/// write already committed, and the test passes without testing the race,
/// so the wait errs long.
#[cfg(test)]
pub(crate) async fn commit_during<F: std::future::Future>(other: WriteTx<'_>, op: F) -> F::Output {
    let (out, committed) = tokio::join!(op, async {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        other.commit().await
    });
    committed.expect("the second connection's write commits");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    /// A connection that sat idle while another changed the schema begins a
    /// write transaction and runs the delete `wipe_demo_account` runs first
    /// (`delete_account_messages_batch`). Preparing it loads the full-text
    /// search trigger on `messages`, which connects to `messages_fts`; no
    /// row needs to match. The delete goes through (#1600).
    #[tokio::test]
    async fn a_write_after_another_connection_changed_the_schema_sees_the_new_schema() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut other = pool.acquire().await.expect("first connection");
        schema::ensure_schema(&mut other)
            .await
            .expect("create the schema");
        // A second connection that loads the schema and never uses the
        // full-text search table, so it has not connected to it.
        let mut stale = pool.acquire().await.expect("second connection");
        sqlx::query("SELECT count(*) FROM accounts")
            .execute(&mut *stale)
            .await
            .expect("read before the schema changes");

        // What a promote does on the first connection while the second idles.
        let mut tx = begin_write(&mut other).await.expect("begin on the first");
        schema::drop_messages_fts_triggers(&mut tx)
            .await
            .expect("drop triggers");
        schema::install_messages_fts_triggers(&mut tx)
            .await
            .expect("install triggers");
        tx.commit().await.expect("commit the schema change");

        let mut tx = begin_write(&mut stale).await.expect("begin on the second");
        sqlx::query(
            "DELETE FROM messages WHERE id IN (SELECT id FROM messages WHERE account_id = 1 LIMIT 500)",
        )
        .execute(&mut *tx)
        .await
        .expect("delete after the schema changed");
        tx.commit().await.expect("commit the delete");
    }
}
