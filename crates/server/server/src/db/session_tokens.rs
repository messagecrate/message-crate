//! GUI session Bearer tokens (`mc-user-…`); one per account, rotates on login.
//! Opening and ending a Session writes its entries in the Audit Trail.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
pub use message_crate_api_types::AppKind;
use rand::TryRng;
use sqlx::SqliteConnection;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::db::audit_trail::{self, AuditActor, AuditReason};
use crate::db::unix_secs;

const TOKEN_ALPHANUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// How long a Session lasts unused (7 days). A login sets its expiry this
/// far ahead, and each use moves it this far ahead again, up to
/// [`SESSION_MAX_SECS`] after the login.
pub const SESSION_IDLE_SECS: u64 = 7 * 24 * 60 * 60;

/// The longest a Session lasts however much it is used (30 days), counted
/// from the login, or from the password change that renewed it.
pub const SESSION_MAX_SECS: u64 = 30 * 24 * 60 * 60;

/// The least a use moves a Session's expiry before the move is written (1
/// hour), so a Session in use writes at most once an hour, not on every
/// request.
const SESSION_EXTEND_STEP_SECS: u64 = 60 * 60;

/// The expiry a Session made at `created_at`, expiring at `expires_at`, moves
/// to when it is used at `now`: the earlier of `now` plus
/// [`SESSION_IDLE_SECS`] and `created_at` plus [`SESSION_MAX_SECS`]. `None`
/// when that is less than [`SESSION_EXTEND_STEP_SECS`] later than
/// `expires_at`, so nothing is written. All three are Unix seconds.
fn extended_expiry(created_at: u64, expires_at: u64, now: u64) -> Option<u64> {
    let next = now
        .saturating_add(SESSION_IDLE_SECS)
        .min(created_at.saturating_add(SESSION_MAX_SECS));
    (next >= expires_at.saturating_add(SESSION_EXTEND_STEP_SECS)).then_some(next)
}

/// Generate a new GUI session token (`mc-user-` + 32 alphanumeric characters).
///
/// # Errors
///
/// Returns an error when random bytes cannot be generated.
pub fn generate_session_token() -> Result<String> {
    generate_prefixed_token("mc-user-")
}

/// Characters after a token's prefix.
const TOKEN_SUFFIX_LEN: usize = 32;

/// Bytes below this map to a character: 248, the largest multiple of 62 a
/// byte holds, so each character stands for exactly four byte values.
const ACCEPTED_BYTES: usize = 256 / TOKEN_ALPHANUM.len() * TOKEN_ALPHANUM.len();

/// A random token of `TOKEN_SUFFIX_LEN` characters after `prefix`, from the
/// OS random source.
pub(crate) fn generate_prefixed_token(prefix: &str) -> Result<String> {
    prefixed_token_from(prefix, fill_random)
}

/// A token of `TOKEN_SUFFIX_LEN` characters after `prefix`, reading bytes
/// from `fill` and drawing again for every byte `token_char` refuses, so
/// each character is equally likely.
fn prefixed_token_from(
    prefix: &str,
    mut fill: impl FnMut(&mut [u8]) -> Result<()>,
) -> Result<String> {
    let mut suffix = String::with_capacity(TOKEN_SUFFIX_LEN);
    let mut buf = [0u8; TOKEN_SUFFIX_LEN];
    while suffix.len() < TOKEN_SUFFIX_LEN {
        fill(&mut buf)?;
        let wanted = TOKEN_SUFFIX_LEN - suffix.len();
        suffix.extend(buf.iter().filter_map(|&b| token_char(b)).take(wanted));
    }
    Ok(format!("{prefix}{suffix}"))
}

/// The character `byte` stands for, or `None` for a byte of 248 or more,
/// which is drawn again. Taking `% 62` of every byte would give the first
/// eight characters five byte values each and the rest four.
fn token_char(byte: u8) -> Option<char> {
    let byte = usize::from(byte);
    (byte < ACCEPTED_BYTES).then(|| char::from(TOKEN_ALPHANUM[byte % TOKEN_ALPHANUM.len()]))
}

/// SHA-256 hex fingerprint of a plaintext token (stored in DB; used for Bearer lookup).
pub fn hash_api_token(token: &str) -> String {
    file_io::sha256_hex(token.as_bytes())
}

/// Fill `buf` from the OS random source, refusing an all-zero result.
fn fill_random(buf: &mut [u8]) -> Result<()> {
    rand::rngs::SysRng
        .try_fill_bytes(buf)
        .map_err(|e| anyhow::anyhow!("secure random unavailable: {e}"))?;
    if buf.iter().all(|&b| b == 0) {
        bail!("secure random returned an empty entropy buffer");
    }
    Ok(())
}

/// The `created_at` and `expires_at` a Session row stores, as Unix seconds.
struct SessionTimes {
    created_at: u64,
    expires_at: u64,
}

impl SessionTimes {
    /// A Session made at `now` that lasts `ttl_secs`.
    fn at(now: DateTime<Utc>, ttl_secs: u64) -> Self {
        let created_at = unix_secs(now);
        Self {
            created_at,
            expires_at: created_at.saturating_add(ttl_secs),
        }
    }
}

/// The app a request says it comes from, and the Build that app reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectingApp {
    /// Desktop app or website.
    pub kind: AppKind,
    /// The app's Build, such as `0.9.0+343fe0d8`.
    pub build: String,
}

impl ConnectingApp {
    /// Pair a stored kind and Build; `None` unless both are present and the
    /// kind is one the server knows.
    fn from_columns(kind: Option<String>, build: Option<String>) -> Option<Self> {
        let kind = AppKind::parse(kind.as_deref()?)?;
        Some(Self {
            kind,
            build: build?,
        })
    }
}

/// A live session: whose it is, and the app last recorded on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The account the session logs in.
    pub account_id: i64,
    /// `None` until a request names its app.
    pub app: Option<ConnectingApp>,
    /// The move of its expiry the lookup that found it made due, for
    /// [`spawn_expiry_move`] to write. `None` when the move would be less
    /// than [`SESSION_EXTEND_STEP_SECS`].
    pub expiry_move: Option<ExpiryMove>,
}

/// The row `lookup_session` reads.
#[derive(sqlx::FromRow)]
struct SessionRow {
    account_id: i64,
    /// Unix seconds, as text.
    created_at: String,
    /// Unix seconds, as text.
    expires_at: String,
    app_kind: Option<String>,
    app_build: Option<String>,
    login_entry_id: Option<i64>,
}

/// Look up the session this Bearer names (by hash) that is live at `now`.
/// Expired rows are removed. The session found carries the move of its
/// expiry this use is due, if any ([`Session::expiry_move`]); the lookup
/// writes nothing for it.
///
/// # Errors
///
/// Returns an error when the lookup or delete fails.
pub async fn lookup_session(
    conn: &mut SqliteConnection,
    token: &str,
    now: DateTime<Utc>,
) -> Result<Option<Session>> {
    let token_hash = hash_api_token(token);
    let found: Option<SessionRow> = sqlx::query_as(
        "SELECT account_id, created_at, expires_at, app_kind, app_build, login_entry_id \
         FROM account_session_tokens WHERE token_hash = $1",
    )
    .bind(token_hash.as_str())
    .fetch_optional(&mut *conn)
    .await
    .context("look up the Session a token names")?;
    let Some(row) = found else {
        return Ok(None);
    };
    let now = unix_secs(now);
    // An `expires_at` that does not parse counts as expired.
    let Some(expires_at) = row
        .expires_at
        .parse::<u64>()
        .ok()
        .filter(|&expires| expires > now)
    else {
        let _ = sqlx::query("DELETE FROM account_session_tokens WHERE token_hash = $1")
            .bind(token_hash.as_str())
            .execute(&mut *conn)
            .await;
        return Ok(None);
    };
    // A `created_at` that does not parse leaves the expiry where it is.
    let expiry_move = row
        .created_at
        .parse::<u64>()
        .ok()
        .and_then(|created| extended_expiry(created, expires_at, now))
        .map(|expires_at| ExpiryMove {
            token_hash,
            login_entry_id: row.login_entry_id,
            expires_at,
        });
    Ok(Some(Session {
        account_id: row.account_id,
        app: ConnectingApp::from_columns(row.app_kind, row.app_build),
        expiry_move,
    }))
}

/// A move of a Session's expiry that a use made due: the Session, by the
/// hash of its token, its `logged_in` entry, and the new expiry in Unix
/// seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiryMove {
    token_hash: String,
    login_entry_id: Option<i64>,
    expires_at: u64,
}

/// Write `expiry_move`: the Session's new expiry, and the same expiry for
/// the Audit Trail under its `logged_in` entry, when a login made it. Both
/// writes happen or neither does. A Session ended or replaced since the
/// lookup matches no row, and its `logged_in` entry already has the entry
/// that ended it, so the Audit Trail reads nothing from the record.
///
/// # Errors
///
/// Returns an error when a write fails, including `SQLITE_BUSY` once the
/// busy timeout runs out.
pub async fn move_session_expiry(
    conn: &mut SqliteConnection,
    expiry_move: &ExpiryMove,
) -> Result<()> {
    let mut tx = crate::db::begin_write(conn)
        .await
        .context("begin a write transaction")?;
    sqlx::query("UPDATE account_session_tokens SET expires_at = $1 WHERE token_hash = $2")
        .bind(expiry_move.expires_at.to_string())
        .bind(expiry_move.token_hash.as_str())
        .execute(&mut *tx)
        .await
        .context("move the Session's expiry")?;
    if let Some(login_entry_id) = expiry_move.login_entry_id {
        audit_trail::record_session_extended(&mut tx, login_entry_id, expiry_move.expires_at)
            .await?;
    }
    tx.commit()
        .await
        .context("commit the Session's new expiry")?;
    Ok(())
}

/// Whether a move of a Session's expiry is running in the background.
static EXPIRY_MOVE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Holds `running` set while a move runs, and clears it when dropped, so a
/// move that fails, panics or is cancelled lets the next one start.
struct MoveRunning<'a>(&'a AtomicBool);

impl<'a> MoveRunning<'a> {
    /// Set `running`, or `None` when it is set already.
    fn claim(running: &'a AtomicBool) -> Option<Self> {
        (!running.swap(true, Ordering::AcqRel)).then_some(Self(running))
    }
}

impl Drop for MoveRunning<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Write `expiry_move` in a task of its own, on a connection of its own from
/// `pool`, so the request that made it due is served without waiting for
/// the write lock. One move runs at a time: while one runs, another is
/// dropped, and a later request makes it again. So an import holding the
/// write lock keeps at most one connection waiting on it. A move the busy
/// timeout ends is dropped without a word; any other failure is logged.
/// Either way the Session expires when it would have without the move.
pub fn spawn_expiry_move(pool: &sqlx::SqlitePool, expiry_move: ExpiryMove) {
    let Some(running) = MoveRunning::claim(&EXPIRY_MOVE_RUNNING) else {
        return;
    };
    let pool = pool.clone();
    crate::request_id::spawn_in_current_span(async move {
        let _running = running;
        let moved = async {
            let mut conn = pool.acquire().await.context("acquire a connection")?;
            move_session_expiry(&mut conn, &expiry_move).await
        }
        .await;
        if let Err(err) = moved
            && !crate::api_error::is_database_busy(&err)
        {
            tracing::warn!(
                error = %format!("{err:#}"),
                "The Session's expiry could not be moved"
            );
        }
    });
}

/// The hash of `account_id`'s Session token, or `None` when the account
/// has no Session or it has expired by `now`. A media link is signed over it, so the
/// link ends when this Session does: by logout, a new login, a password
/// change, or its expiry.
///
/// # Errors
///
/// Returns an error when the lookup fails.
pub async fn live_session_hash(
    conn: &mut SqliteConnection,
    account_id: i64,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let found: Option<(String, String)> = sqlx::query_as(
        "SELECT token_hash, expires_at FROM account_session_tokens WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await
    .context("read the account's Session token hash")?;
    let now = unix_secs(now);
    Ok(found.and_then(|(token_hash, expires_at)| {
        expires_at
            .parse::<u64>()
            .is_ok_and(|expires| expires > now)
            .then_some(token_hash)
    }))
}

/// Record the app a session's request came from, when it is not the one
/// already recorded. In practice that is one write at login and one after an
/// app update; every other request finds the same value and writes nothing.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn record_connecting_app(
    conn: &mut SqliteConnection,
    session: &Session,
    app: &ConnectingApp,
) -> Result<()> {
    if session.app.as_ref() == Some(app) {
        return Ok(());
    }
    sqlx::query(
        "UPDATE account_session_tokens SET app_kind = $1, app_build = $2 WHERE account_id = $3",
    )
    .bind(app.kind.as_str())
    .bind(app.build.as_str())
    .bind(session.account_id)
    .execute(&mut *conn)
    .await
    .context("record the connecting app on the Session")?;
    Ok(())
}

/// The app recorded on an account's session, or `None` when the account has
/// no session or no request has named an app.
///
/// # Errors
///
/// Returns an error when the lookup fails.
pub async fn connecting_app_for_account(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<ConnectingApp>> {
    let found: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT app_kind, app_build FROM account_session_tokens WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await
    .context("read the app recorded on the account's Session")?;
    Ok(found.and_then(|(kind, build)| ConnectingApp::from_columns(kind, build)))
}

/// Replace the account's session token with a new one made at `now` and
/// return the plaintext once, with the Unix second it now expires at. The Session
/// carries on: a password change renews it, and the caller records the
/// renewed expiry in the Audit Trail.
///
/// One upsert, never a lookup and then an insert: two logins at once both
/// find no row, and the second insert would break the `account_id` primary
/// key. The later login's token replaces the earlier one's.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the write fails.
pub async fn rotate_account_session_token(
    conn: &mut SqliteConnection,
    account_id: i64,
    now: DateTime<Utc>,
) -> Result<(String, u64)> {
    let token = generate_session_token()?;
    let token_hash = hash_api_token(&token);
    let SessionTimes {
        created_at,
        expires_at: expires,
    } = SessionTimes::at(now, SESSION_IDLE_SECS);
    sqlx::query(
        r"
        INSERT INTO account_session_tokens (account_id, token_hash, created_at, expires_at)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT(account_id) DO UPDATE SET
            token_hash = excluded.token_hash,
            created_at = excluded.created_at,
            expires_at = excluded.expires_at
        ",
    )
    .bind(account_id)
    .bind(token_hash)
    .bind(created_at.to_string())
    .bind(expires.to_string())
    .execute(&mut *conn)
    .await
    .context("replace the account's Session token")?;
    Ok((token, expires))
}

/// The Audit Trail's `logged_in` entry for the account's session, when a
/// login made it and it has not expired by `now`. An expired row stays until its
/// token is presented again, and its end is read from its expiry, so ending
/// it again would rewrite when it ended.
pub(crate) async fn live_login_entry(
    conn: &mut SqliteConnection,
    account_id: i64,
    now: DateTime<Utc>,
) -> Result<Option<i64>> {
    let found: Option<(Option<i64>, String)> = sqlx::query_as(
        "SELECT login_entry_id, expires_at FROM account_session_tokens WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await
    .context("read the login entry of the account's Session")?;
    let now = unix_secs(now);
    // An `expires_at` that does not parse counts as expired, as in `lookup_session`.
    Ok(found.and_then(|(login_entry_id, expires_at)| {
        login_entry_id.filter(|_| expires_at.parse::<u64>().is_ok_and(|expires| expires > now))
    }))
}

/// Log in: open the account's one Session and return its token once.
///
/// A Session the account already had is replaced, and the Audit Trail says
/// so: a login on the desktop ends the website's. The new Session's
/// `logged_in` entry names the app the request named, and the session row
/// records that app from the start. Called by every route that opens a
/// Session for a person: login, claiming the Message Crate, and registering.
/// The Session and the entries it writes are stamped `now`, and the Session
/// expires [`SESSION_IDLE_SECS`] after it unless it is used.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or a write fails.
pub async fn open_session(
    conn: &mut SqliteConnection,
    account_id: i64,
    username: &str,
    app: Option<&ConnectingApp>,
    now: DateTime<Utc>,
) -> Result<String> {
    let actor = AuditActor::logged_in_as(account_id);
    if let Some(previous) = live_login_entry(conn, account_id, now).await? {
        audit_trail::record_session_end(conn, previous, AuditReason::Replaced, actor, now).await?;
    }
    let token = generate_session_token()?;
    let token_hash = hash_api_token(&token);
    let SessionTimes {
        created_at,
        expires_at: expires,
    } = SessionTimes::at(now, SESSION_IDLE_SECS);
    let login_entry_id =
        audit_trail::record_login(conn, (account_id, username), app, expires, now).await?;
    sqlx::query(
        r"
        INSERT INTO account_session_tokens
            (account_id, token_hash, created_at, expires_at, app_kind, app_build, login_entry_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT(account_id) DO UPDATE SET
            token_hash = excluded.token_hash,
            created_at = excluded.created_at,
            expires_at = excluded.expires_at,
            app_kind = excluded.app_kind,
            app_build = excluded.app_build,
            login_entry_id = excluded.login_entry_id
        ",
    )
    .bind(account_id)
    .bind(token_hash)
    .bind(created_at.to_string())
    .bind(expires.to_string())
    .bind(app.map(|app| app.kind.as_str()))
    .bind(app.map(|app| app.build.as_str()))
    .bind(login_entry_id)
    .execute(&mut *conn)
    .await
    .context("store the account's new Session")?;
    Ok(token)
}

/// Create a session token for an account, made at `now`, and return the
/// plaintext. No login made it, so the Audit Trail holds no entry for it: the tests and
/// the credential matrix use it to stand a Session up directly.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the insert fails.
#[cfg(test)]
pub async fn insert_account_session_token(
    conn: &mut SqliteConnection,
    account_id: i64,
    now: DateTime<Utc>,
) -> Result<String> {
    insert_account_session_token_with_ttl(conn, account_id, SESSION_IDLE_SECS, now).await
}

/// Create a session token made at `now` that lasts `ttl_secs`, and return
/// the plaintext.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the insert fails.
#[cfg(test)]
pub async fn insert_account_session_token_with_ttl(
    conn: &mut SqliteConnection,
    account_id: i64,
    ttl_secs: u64,
    now: DateTime<Utc>,
) -> Result<String> {
    let token = generate_session_token()?;
    let token_hash = hash_api_token(&token);
    let SessionTimes {
        created_at,
        expires_at,
    } = SessionTimes::at(now, ttl_secs);
    sqlx::query(
        "INSERT INTO account_session_tokens (account_id, token_hash, created_at, expires_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(account_id)
    .bind(token_hash)
    .bind(created_at.to_string())
    .bind(expires_at.to_string())
    .execute(&mut *conn)
    .await
    .context("insert a Session token")?;
    Ok(token)
}

/// Log out: end the Session the presented token names, and record that it
/// ended. Returns whether the token named a Session. The lookup, the record
/// and the delete run in one write transaction, so a login replacing the
/// Session meanwhile, or a second logout with the same token, cannot record
/// its end twice. The end is stamped `now`.
///
/// # Errors
///
/// Returns an error when the lookup, the record or the delete fails.
pub async fn revoke_session_token(
    conn: &mut SqliteConnection,
    token: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let token_hash = hash_api_token(token);
    let mut tx = crate::db::begin_write(conn)
        .await
        .context("begin a write transaction")?;
    let found: Option<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT account_id, login_entry_id FROM account_session_tokens WHERE token_hash = $1",
    )
    .bind(token_hash.as_str())
    .fetch_optional(&mut *tx)
    .await
    .context("look up the Session a token names")?;
    let Some((account_id, login_entry_id)) = found else {
        return Ok(false);
    };
    if let Some(login_entry_id) = login_entry_id {
        audit_trail::record_session_end(
            &mut tx,
            login_entry_id,
            AuditReason::LoggedOut,
            AuditActor::logged_in_as(account_id),
            now,
        )
        .await?;
    }
    sqlx::query("DELETE FROM account_session_tokens WHERE token_hash = $1")
        .bind(token_hash)
        .execute(&mut *tx)
        .await
        .context("delete the Session")?;
    tx.commit().await.context("commit the logout")?;
    Ok(true)
}

/// End the account's Session, if it has one, and record it as `revoked` by
/// `actor`. Used by `reset-owner-password`, the shell command for an owner
/// who has lost their password, so the old password's login ends with it;
/// by deleting an account; by the owner disabling an account, so enabling it
/// again does not bring back the old token; and by a Demo Account rebuild.
/// The owner setting another account's password through `/v1` does not call
/// this: that sets the password and nothing more, and the account's session
/// carries on.
///
/// It reads the Session and then writes, so it takes the caller's write
/// transaction, and the end is recorded with whatever else the caller does,
/// stamped `now`.
///
/// # Errors
///
/// Returns an error when the record or the delete fails.
pub async fn revoke_account_sessions(
    tx: &mut crate::db::WriteTx<'_>,
    account_id: i64,
    actor: AuditActor,
    now: DateTime<Utc>,
) -> Result<()> {
    if let Some(login_entry_id) = live_login_entry(tx, account_id, now).await? {
        audit_trail::record_session_end(tx, login_entry_id, AuditReason::Revoked, actor, now)
            .await?;
    }
    sqlx::query("DELETE FROM account_session_tokens WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut **tx)
        .await
        .context("delete the account's Session")?;
    Ok(())
}

/// Look up the session `token` names at `now` and write the move of its
/// expiry the lookup made due, as a request and the move it spawns do, for
/// a test to wait on both.
#[cfg(test)]
pub(crate) async fn use_session_at(
    conn: &mut SqliteConnection,
    token: &str,
    now: DateTime<Utc>,
) -> Result<Option<Session>> {
    let session = lookup_session(conn, token, now).await?;
    if let Some(expiry_move) = session.as_ref().and_then(|s| s.expiry_move.as_ref()) {
        move_session_expiry(conn, expiry_move).await?;
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-answer vectors, not a round trip.
    ///
    /// Every session and API token in every existing database is stored as this
    /// hash and looked up by it, so changing the algorithm logs everyone out
    /// and invalidates every issued API token at once. A test that only
    /// checked the length and determinism would pass after such a change —
    /// any 64-character hex hash is stable and 64 characters long. These
    /// digests are SHA-256 of the exact bytes shown, reproducible with
    /// `printf 'mc-user-abc' | sha256sum`.
    #[test]
    fn hash_is_sha256_of_the_token_bytes() {
        for (token, expected) in [
            (
                "mc-user-abc",
                "2f149722de58d39a2c4c28c6889a5bcdfeef221933624142cc2339411007a7a8",
            ),
            (
                "mc-tok-abc123",
                "892e92b80959400565df97dd4983106d1c7d61ecf6814a4694f8d878d0745cc1",
            ),
            (
                "",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
        ] {
            assert_eq!(
                hash_api_token(token),
                expected,
                "the stored hash of {token:?} changed; every session and API \
                 token in every existing database is looked up by this digest"
            );
        }
        assert_ne!(hash_api_token("mc-user-abc"), hash_api_token("mc-user-xyz"));
    }

    #[test]
    fn generate_prefixed_token_uses_os_entropy() {
        let a = generate_prefixed_token("mc-user-").unwrap();
        let b = generate_prefixed_token("mc-user-").unwrap();
        assert!(a.starts_with("mc-user-"));
        assert_eq!(a.len(), "mc-user-".len() + TOKEN_SUFFIX_LEN);
        assert_ne!(a, b);
    }

    /// Every letter and digit stands for the same number of byte values, and
    /// the bytes left over are refused. Mapping all 256 with `% 62` gave
    /// `A` to `H` five bytes each and the rest four, so those eight came up
    /// a quarter more often in every Session and API Token.
    #[test]
    fn every_token_character_stands_for_four_byte_values() {
        let mut counts = std::collections::HashMap::<char, usize>::new();
        for byte in 0..=u8::MAX {
            if let Some(c) = token_char(byte) {
                *counts.entry(c).or_default() += 1;
            }
        }
        assert_eq!(counts.len(), TOKEN_ALPHANUM.len());
        for &symbol in TOKEN_ALPHANUM {
            assert_eq!(counts[&char::from(symbol)], 4, "{}", char::from(symbol));
        }
        for byte in (0..=u8::MAX).filter(|&b| usize::from(b) >= ACCEPTED_BYTES) {
            assert_eq!(token_char(byte), None, "byte {byte} must be drawn again");
        }
    }

    /// Refused bytes are skipped, never mapped, and the source is read
    /// again until the token has all its characters.
    #[test]
    fn refused_bytes_are_drawn_again() {
        // Each read fills one buffer of `TOKEN_SUFFIX_LEN` bytes: first all
        // refused but the last two, which are the first two characters.
        let refused = vec![u8::MAX; TOKEN_SUFFIX_LEN - 2];
        let mut batches = [
            [refused, vec![0, 1]].concat(),
            (2..).take(TOKEN_SUFFIX_LEN).collect::<Vec<u8>>(),
        ]
        .into_iter();
        let token = prefixed_token_from("mc-user-", |buf| {
            buf.copy_from_slice(&batches.next().expect("a third read"));
            Ok(())
        })
        .unwrap();
        let expected: String = TOKEN_ALPHANUM[..TOKEN_SUFFIX_LEN]
            .iter()
            .map(|&b| char::from(b))
            .collect();
        assert_eq!(token, format!("mc-user-{expected}"));
    }

    use crate::test_support::fixed_instant;

    /// The Unix second `expires_at` holds for account 7's session.
    async fn stored_expiry(conn: &mut SqliteConnection) -> u64 {
        let expires: String = sqlx::query_scalar(
            "SELECT expires_at FROM account_session_tokens WHERE account_id = 7",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        expires.parse().unwrap()
    }

    #[tokio::test]
    async fn lookup_rejects_expired_session() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        let token = insert_account_session_token(&mut conn, 7, fixed_instant())
            .await
            .unwrap();
        // Force expiry into the past.
        sqlx::query("UPDATE account_session_tokens SET expires_at = '1'")
            .execute(&mut *conn)
            .await
            .unwrap();
        assert!(
            lookup_session(&mut conn, &token, fixed_instant())
                .await
                .unwrap()
                .is_none()
        );
    }

    /// The lookup compares the expiry with the instant it is given: the
    /// session is live up to the second before it expires, and refused from
    /// that second on, and the refused row is removed.
    #[tokio::test]
    async fn lookup_rejects_a_session_from_the_second_it_expires() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        let token = insert_account_session_token_with_ttl(&mut conn, 7, 3600, fixed_instant())
            .await
            .unwrap();
        let expiry = fixed_instant() + chrono::Duration::hours(1);

        let last_second = expiry - chrono::Duration::seconds(1);
        assert!(
            lookup_session(&mut conn, &token, last_second)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            live_session_hash(&mut conn, 7, last_second)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(live_session_hash(&mut conn, 7, expiry).await.unwrap(), None);
        assert!(
            lookup_session(&mut conn, &token, expiry)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            lookup_session(&mut conn, &token, last_second)
                .await
                .unwrap()
                .is_none(),
            "the expired row was removed"
        );
    }

    /// A login is good for 7 days unused from the instant it was made,
    /// and the row's `created_at` is that instant: one login reads one clock.
    #[tokio::test]
    async fn a_session_expires_seven_days_after_login() {
        const SEVEN_DAYS_SECS: u64 = 604_800;
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        open_session(&mut conn, 7, "alice", None, fixed_instant())
            .await
            .unwrap();

        let login = unix_secs(fixed_instant());
        let created: String = sqlx::query_scalar(
            "SELECT created_at FROM account_session_tokens WHERE account_id = 7",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(created, login.to_string());
        assert_eq!(stored_expiry(&mut conn).await, login + SEVEN_DAYS_SECS);
    }

    /// A password change renews the Session for 7 days from the instant
    /// of the change.
    #[tokio::test]
    async fn a_rotated_session_expires_seven_days_after_the_rotation() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        insert_account_session_token(&mut conn, 7, fixed_instant())
            .await
            .unwrap();
        let later = fixed_instant() + chrono::Duration::days(3);
        let (_token, expires) = rotate_account_session_token(&mut conn, 7, later)
            .await
            .unwrap();
        assert_eq!(expires, unix_secs(later) + SESSION_IDLE_SECS);
        assert_eq!(stored_expiry(&mut conn).await, expires);
    }

    /// A use moves the expiry 7 days past the use, stopping at 30
    /// days after the login, and only by a step of an hour or more.
    #[test]
    fn a_use_moves_the_expiry_by_an_hour_or_more_and_never_past_the_cap() {
        const DAY: u64 = 86_400;
        let login = 1_000_000;
        let first_expiry = login + 7 * DAY;
        assert_eq!(
            extended_expiry(login, first_expiry, login + 3 * DAY),
            Some(login + 10 * DAY),
            "a use 3 days in lasts 7 days from then"
        );
        assert_eq!(
            extended_expiry(login, first_expiry, login + 3599),
            None,
            "a move of less than an hour is not written"
        );
        assert_eq!(
            extended_expiry(login, first_expiry, login + 3600),
            Some(first_expiry + 3600),
            "a move of an hour is written"
        );
        assert_eq!(
            extended_expiry(login, login + 28 * DAY, login + 27 * DAY),
            Some(login + 30 * DAY),
            "the move stops at 30 days after the login"
        );
        assert_eq!(
            extended_expiry(login, login + 30 * DAY, login + 29 * DAY),
            None,
            "a Session at its cap does not move"
        );
    }

    /// A Session used before it runs out lasts 7 days from that use, and
    /// the Audit Trail is told the new expiry. A use within the hour after
    /// writes nothing.
    #[tokio::test]
    async fn using_a_session_moves_its_expiry_seven_days_past_the_use() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        let token = open_session(&mut conn, 7, "alice", None, fixed_instant())
            .await
            .unwrap();
        let used = fixed_instant() + chrono::Duration::days(6);
        assert!(
            use_session_at(&mut conn, &token, used)
                .await
                .unwrap()
                .is_some()
        );
        let moved = unix_secs(used) + SESSION_IDLE_SECS;
        assert_eq!(stored_expiry(&mut conn).await, moved);
        let recorded: String = sqlx::query_scalar("SELECT expires_at FROM session_extensions")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(
            recorded,
            (used + chrono::Duration::days(7)).to_rfc3339(),
            "the Audit Trail reads the new expiry"
        );

        let soon_after = used + chrono::Duration::minutes(59);
        assert!(
            use_session_at(&mut conn, &token, soon_after)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            stored_expiry(&mut conn).await,
            moved,
            "no write within the hour"
        );

        // Past the first 7 days, the Session the use moved is still live.
        let past_first_expiry = fixed_instant() + chrono::Duration::days(8);
        assert!(
            use_session_at(&mut conn, &token, past_first_expiry)
                .await
                .unwrap()
                .is_some()
        );
    }

    /// A lookup writes nothing for the move it makes due: it reports it, so
    /// the request it serves never waits for the write lock, and the move
    /// is written apart.
    #[tokio::test]
    async fn a_lookup_reports_the_move_and_writes_nothing() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        let token = open_session(&mut conn, 7, "alice", None, fixed_instant())
            .await
            .unwrap();
        let first_expiry = stored_expiry(&mut conn).await;

        let used = fixed_instant() + chrono::Duration::days(3);
        let session = lookup_session(&mut conn, &token, used)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored_expiry(&mut conn).await, first_expiry);

        let expiry_move = session.expiry_move.expect("a move is due");
        move_session_expiry(&mut conn, &expiry_move).await.unwrap();
        assert_eq!(
            stored_expiry(&mut conn).await,
            unix_secs(used) + SESSION_IDLE_SECS
        );
    }

    /// One move runs at a time: a second is refused while the first runs,
    /// and allowed once the first has ended however it ended.
    #[test]
    fn one_move_runs_at_a_time() {
        let running = AtomicBool::new(false);
        let first = MoveRunning::claim(&running).expect("nothing runs yet");
        assert!(MoveRunning::claim(&running).is_none(), "one is running");
        drop(first);
        assert!(MoveRunning::claim(&running).is_some(), "the first ended");
    }

    /// A Session left unused for 7 days is refused.
    #[tokio::test]
    async fn a_session_unused_for_seven_days_is_refused() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        let token = open_session(&mut conn, 7, "alice", None, fixed_instant())
            .await
            .unwrap();
        let used = fixed_instant() + chrono::Duration::days(2);
        assert!(
            use_session_at(&mut conn, &token, used)
                .await
                .unwrap()
                .is_some()
        );
        let idle = used + chrono::Duration::days(7);
        assert!(
            use_session_at(&mut conn, &token, idle)
                .await
                .unwrap()
                .is_none()
        );
    }

    /// A Session used every day still ends 30 days after the login.
    #[tokio::test]
    async fn a_session_in_daily_use_ends_thirty_days_after_login() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        let token = open_session(&mut conn, 7, "alice", None, fixed_instant())
            .await
            .unwrap();
        for day in 1..30 {
            let used = fixed_instant() + chrono::Duration::days(day);
            assert!(
                use_session_at(&mut conn, &token, used)
                    .await
                    .unwrap()
                    .is_some(),
                "live on day {day}"
            );
        }
        let cap = fixed_instant() + chrono::Duration::days(30);
        assert_eq!(stored_expiry(&mut conn).await, unix_secs(cap));
        assert!(
            use_session_at(&mut conn, &token, cap)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn insert_session_with_ttl_sets_expires_at() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        let token = insert_account_session_token_with_ttl(&mut conn, 7, 120, fixed_instant())
            .await
            .unwrap();
        assert!(token.starts_with("mc-user-"));
        assert_eq!(
            stored_expiry(&mut conn).await,
            unix_secs(fixed_instant()) + 120
        );
    }

    #[tokio::test]
    async fn revoke_session_token_removes_row() {
        let (pool, _dir) = crate::test_support::schema_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        crate::test_support::insert_account_with_id(&mut conn, 7, "alice").await;
        let token = insert_account_session_token(&mut conn, 7, fixed_instant())
            .await
            .unwrap();
        assert!(
            lookup_session(&mut conn, &token, fixed_instant())
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            revoke_session_token(&mut conn, &token, fixed_instant())
                .await
                .unwrap()
        );
        assert!(
            lookup_session(&mut conn, &token, fixed_instant())
                .await
                .unwrap()
                .is_none()
        );
    }
}
