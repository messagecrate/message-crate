//! The fixture a server test starts from: an empty database with the schema
//! applied, the temp directory it lives in, and the accounts a test adds to
//! it.

use tempfile::TempDir;

use super::{LoggedInAccount, add_account_and_log_in, insert_account_with_id, log_in};
use crate::server::AppState;

/// A server state plus its temp directory. Drop the `TempDir` last.
pub struct TestFixture {
    /// Keeps the temp directory alive for the test's lifetime.
    tmp: TempDir,
    /// The server state every helper drives.
    pub state: AppState,
}

/// An empty database with schema applied, holding the administrator's row
/// with no password and no other account.
pub async fn test_fixture() -> TestFixture {
    fixture_on(crate::db::engine::test_pool().await).await
}

/// [`test_fixture`] whose database connections wait `busy_timeout` for
/// another connection's write lock, so a test can meet `SQLITE_BUSY` at once.
pub async fn test_fixture_with_busy_timeout(busy_timeout: std::time::Duration) -> TestFixture {
    fixture_on(crate::db::engine::test_pool_with_busy_timeout(busy_timeout).await).await
}

/// A pool on a new database in a temp directory, with the schema applied
/// as [`crate::open_db::OpenDb`] applies it when the server opens its pool:
/// no request makes sure of the schema, so a test that writes through a
/// bare pool starts here. Drop the `TempDir` last.
pub async fn schema_pool() -> (sqlx::SqlitePool, TempDir) {
    let (pool, tmp) = crate::db::engine::test_pool().await;
    {
        let mut conn = pool.acquire().await.unwrap();
        crate::db::schema::ensure_schema(&mut conn).await.unwrap();
    }
    (pool, tmp)
}

/// The fixture on `pool`, which lives in `tmp`, with the schema applied.
async fn fixture_on((pool, tmp): (sqlx::SqlitePool, TempDir)) -> TestFixture {
    {
        let mut conn = pool.acquire().await.unwrap();
        crate::db::schema::ensure_schema(&mut conn).await.unwrap();
    }
    let state = crate::server::test_app_state(pool, tmp.path());
    TestFixture { tmp, state }
}

/// The password every account the fixtures add is given.
pub const PASSWORD: &str = "hunter2hunter2";

/// A fixture with one account in it, `alice`, created as the administrator
/// creates one and logged in: the fixture a route test starts from.
pub async fn fixture_with_account() -> (TestFixture, LoggedInAccount) {
    let fixture = test_fixture().await;
    let account = add_account_and_log_in(&fixture.state, "alice", PASSWORD).await;
    (fixture, account)
}

impl TestFixture {
    /// Turn off `account_id`'s `delete` permission, as the administrator
    /// would, for a test of what an account without it is refused.
    pub async fn turn_off_delete(&self, account_id: i64) {
        let mut conn = self.conn().await;
        crate::db::account_profile::set_account_flags(
            &mut conn,
            account_id,
            crate::db::account_profile::AccountFlags {
                can_delete: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    /// A connection from this fixture's pool, for a test that seeds or asserts
    /// with SQL directly.
    pub async fn conn(&self) -> sqlx::pool::PoolConnection<sqlx::Sqlite> {
        self.state.db.acquire().await.unwrap()
    }

    /// The fixture's temp directory, for a test that needs a real path on disk.
    pub fn dir(&self) -> &std::path::Path {
        self.tmp.path()
    }

    /// Insert an `accounts` row with a chosen id, for a test that asserts on
    /// the id itself. Returns the id it was given, so a caller can bind the
    /// result rather than repeat the literal.
    pub async fn account_with_id(&self, id: i64, username: &str) -> i64 {
        let mut conn = self.conn().await;
        insert_account_with_id(&mut conn, id, username).await
    }

    /// Insert the Demo Account at its fixed id, with the schema's default row:
    /// every permission on, so whatever refuses it does so by its id
    /// (ADR 0016). Returns its id.
    pub async fn demo_account(&self) -> i64 {
        use crate::db::account_profile::{DEMO_ACCOUNT_ID, DEMO_USERNAME};
        self.account_with_id(DEMO_ACCOUNT_ID, DEMO_USERNAME).await
    }

    /// The Demo Account at its fixed id, logged in with its empty password,
    /// with a row that grants every permission. A test uses it to show that
    /// the server refuses the Demo Account by its id, whatever the row says
    /// (ADR 0016). Returns its id and session token.
    pub async fn demo_account_session(&self) -> (i64, String) {
        let id = self.demo_account().await;
        let token =
            log_in(&self.state, crate::db::account_profile::DEMO_USERNAME, "").await["token"]
                .as_str()
                .unwrap()
                .to_string();
        (id, token)
    }

    /// Insert an `accounts` row under the id the database hands out, for a
    /// test that only needs an account to exist.
    pub async fn account(&self, username: &str) -> i64 {
        let mut conn = self.conn().await;
        crate::db::account_profile::insert_account(&mut conn, username, None, None)
            .await
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_fixture_makes_an_account_with_the_id_a_test_asks_for() {
        let fixture = test_fixture().await;
        let id = fixture.account_with_id(101, "alice").await;
        assert_eq!(id, 101);

        let mut conn = fixture.conn().await;
        let username: String = sqlx::query_scalar("SELECT username FROM accounts WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(username, "alice");

        let other = fixture.account("bob").await;
        assert_ne!(other, id, "each account must get its own id");
    }
}
