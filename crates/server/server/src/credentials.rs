//! Passwords, usernames, and the wait before a refused one is answered.
//!
//! Nothing here is a route. The Session routes (`session_api`), the accounts
//! collection (`accounts_api`), setup (`server_api`) and the administrator's
//! shell command (`admin_cli`) all hash, verify and validate through this module,
//! so a password rule is written once.

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Result;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use sqlx::SqliteConnection;
use tokio::sync::Semaphore;

use crate::api_error::ApiError;
use crate::db::{account_profile, api_tokens, session_tokens};
use crate::text_caps::{MAX_USERNAME_CHARS, capped_text, capped_untrimmed};

/// Most characters in a password, counted as a username's are: in
/// characters, not bytes. Checked at account creation, at login, and when a
/// password is changed or sent to confirm a change.
///
/// Argon2 reduces a password to a fixed size before its costly work, so the
/// cap is not for Argon2's sake: it keeps a client from sending a huge body
/// as a password. NIST SP 800-63B asks a verifier to allow at least 64
/// characters; 128 leaves room for any generated password or passphrase.
pub(crate) const MAX_PASSWORD_CHARS: usize = 128;
/// How long the server waits before it answers a refused credential: a login
/// it refuses, or a wrong `current_password`.
///
/// The wait is basic common sense, not protection from the internet: a wrong
/// password costs a moment before the next try. Defending a Message Crate
/// reachable from the internet is the job of the proxy in front of it.
pub(crate) const REFUSAL_WAIT: Duration = Duration::from_secs(1);

static DUMMY_PASSWORD_HASH: OnceLock<String> = OnceLock::new();

/// The wait before the answer to every refused credential. `serve` sleeps
/// [`REFUSAL_WAIT`]; the test harness counts each wait and returns at once,
/// so no test sleeps or reads the clock.
#[derive(Debug, Clone, Default)]
pub(crate) enum RefusalWait {
    /// Sleep [`REFUSAL_WAIT`].
    #[default]
    Sleep,
    /// Count the wait and return at once.
    #[cfg(test)]
    Counted(Arc<AtomicUsize>),
}

impl RefusalWait {
    /// A wait that counts itself and returns at once, for tests.
    #[cfg(test)]
    pub(crate) fn counted() -> Self {
        Self::Counted(Arc::default())
    }

    /// How many times a counted wait has run. Always 0 for a sleeping one.
    #[cfg(test)]
    pub(crate) fn waits(&self) -> usize {
        match self {
            Self::Sleep => 0,
            Self::Counted(waits) => waits.load(Ordering::SeqCst),
        }
    }

    /// Wait, then answer `invalid-credentials` with `detail`. Every refused
    /// credential is answered through here, so each one waits the same.
    ///
    /// The caller holds no pooled database connection across this call, so
    /// refusals waiting their turn to answer do not empty the pool.
    pub(crate) async fn refuse(&self, detail: &str) -> ApiError {
        match self {
            Self::Sleep => tokio::time::sleep(REFUSAL_WAIT).await,
            #[cfg(test)]
            Self::Counted(waits) => {
                waits.fetch_add(1, Ordering::SeqCst);
            }
        }
        ApiError::InvalidCredentials(detail.to_string())
    }
}

// ---------------------------------------------------------------------------
// Password helpers
// ---------------------------------------------------------------------------

/// Hash a plaintext password with argon2id.
///
/// # Errors
///
/// Returns an error when the password cannot be hashed.
pub(crate) fn hash_password(password: &str) -> Result<String> {
    // argon2 0.6 generates the salt itself, from the system RNG, and sizes it
    // to the algorithm's recommendation. That replaces a hand-rolled 16-byte
    // fill and base64 encode, which is not code worth owning on an auth path.
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| anyhow::anyhow!("password hash failed: {e}"))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against an argon2 hash.
pub(crate) fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// The permits for Argon2 runs, one per core the process may use.
///
/// Each run takes about 19 MiB, and the blocking thread pool allows 512
/// threads. Without a limit, a few hundred logins at once would ask for
/// gigabytes and could get the server killed. One run per core
/// keeps every core busy, and the runs waiting for a permit take no Argon2
/// memory.
fn argon2_permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Arc::new(Semaphore::new(cores))
    })
}

/// Run `f`, an Argon2 hash or verify, on the blocking thread pool, once one
/// of [`argon2_permits`] is free.
///
/// Argon2 is slow on purpose. If it ran on an async worker, each hash would
/// hold that worker for its whole run, and a burst of logins or password
/// changes would stall every other request scheduled on the workers. Callers
/// hold no database connection or transaction across the await, so none
/// waits on Argon2 either.
///
/// A caller gives its connection back to the pool before the run and takes
/// one again after it. This function cannot do that for it, because callers
/// hold connections differently. A login drops its connection and gets a new
/// one. A password change gets one for each statement.
async fn run_argon2<T: Send + 'static>(
    task: &'static str,
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    run_argon2_under(argon2_permits(), task, f).await
}

/// [`run_argon2`] under the given permits, so a test can count them.
///
/// The permit moves into the blocking closure, so it is given back when the
/// run ends, not when the request does: a client that disconnects mid-run
/// cannot start another run while the first still uses its memory.
async fn run_argon2_under<T: Send + 'static>(
    permits: &Arc<Semaphore>,
    task: &'static str,
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    let permit = Arc::clone(permits)
        .acquire_owned()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{task}: {e}")))?;
    crate::api_error::run_blocking(task, move || {
        let result = f();
        drop(permit);
        result
    })
    .await
}

/// True when `password` matches the stored hash.
///
/// A missing or empty hash means the account has no password, so only an empty
/// password is accepted. Otherwise argon2 is used, on the blocking thread pool.
pub(crate) async fn passwords_match(
    password_hash: Option<&str>,
    password: &str,
) -> Result<bool, ApiError> {
    match password_hash {
        None | Some("") => Ok(password.is_empty()),
        Some(hash) => {
            let (hash, password) = (hash.to_owned(), password.to_owned());
            run_argon2("password check", move || {
                Ok(verify_password(&hash, &password))
            })
            .await
        }
    }
}

/// A real argon2 hash used only so missing-account logins take similar time.
fn dummy_password_hash() -> &'static str {
    DUMMY_PASSWORD_HASH.get_or_init(|| {
        hash_password("timing-equalization-dummy-password").expect("dummy password hash")
    })
}

/// Check `password` against `password_hash`. A `None` or empty hash means
/// the account has no password: it accepts an empty password only when
/// `no_password_login` is true, which the login sets for every account but
/// the administrator. The administrator must have a password, so before
/// setup its NULL hash is no way in. Argon2 runs against a dummy hash in that
/// branch too, so the time a refusal takes does not say whether the account
/// has a password. A login as a username nobody holds calls this with no hash
/// and ignores the answer. Argon2 runs on the blocking thread pool.
pub(crate) async fn verify_login_password(
    password_hash: Option<&str>,
    password: &str,
    no_password_login: bool,
) -> Result<bool, ApiError> {
    let (hash, password) = (password_hash.map(str::to_owned), password.to_owned());
    run_argon2("password check", move || {
        Ok(match hash.as_deref() {
            None | Some("") => {
                let _ = verify_password(dummy_password_hash(), &password);
                no_password_login && password.is_empty()
            }
            Some(hash) => verify_password(hash, &password),
        })
    })
    .await
}

/// Hash the administrator's password, on the blocking thread pool. The
/// administrator must have one, so an empty password is refused; one
/// character is enough.
pub(crate) async fn hash_admin_password(password: &str) -> Result<String, ApiError> {
    if password.is_empty() {
        return Err(ApiError::validation(
            "the administrator must have a password",
        ));
    }
    require_password_within_cap("password", password)?;
    let password = password.to_owned();
    run_argon2("password hash", move || hash_password(&password)).await
}

/// Hash a user account's password, on the blocking thread pool. There is no
/// minimum length: an empty password is `None`, an account with no password.
pub(crate) async fn hash_user_password(password: &str) -> Result<Option<String>, ApiError> {
    if password.is_empty() {
        return Ok(None);
    }
    require_password_within_cap("password", password)?;
    let password = password.to_owned();
    run_argon2("password hash", move || hash_password(&password).map(Some)).await
}

/// Refuse a password over [`MAX_PASSWORD_CHARS`], naming `field`, the body
/// field that sent it, in the wording of every length cap
/// ([`capped_untrimmed`]). The password is not trimmed: a space at either
/// end is part of it.
///
/// # Errors
///
/// `validation-failed` naming `field` and the cap when `password` is over it.
pub(crate) fn require_password_within_cap(field: &str, password: &str) -> Result<(), ApiError> {
    capped_untrimmed(field, password, MAX_PASSWORD_CHARS)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Username validation
// ---------------------------------------------------------------------------

/// The username as stored: surrounding whitespace removed.
pub(crate) fn normalize_username(raw: &str) -> String {
    raw.trim().to_string()
}

/// The trimmed username, or the `validation-failed` every route that takes
/// one answers when it is malformed: over [`MAX_USERNAME_CHARS`] characters
/// in the wording of every length cap ([`capped_text`]), or blank or holding
/// a character other than an ASCII letter, a digit, `_`, `-` or `.`.
///
/// ASCII only, because the account lookup (`COLLATE NOCASE`) folds ASCII
/// letters only: a letter from another script would make an account whose
/// name looks like another's, such as `аdmin` with a Cyrillic `а` beside
/// `admin`. A preferred name carries a person's name in any script.
pub(crate) fn require_valid_username(raw: &str) -> Result<String, ApiError> {
    let username = capped_text("username", raw, MAX_USERNAME_CHARS)?;
    if username.is_empty()
        || !username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(ApiError::validation(
            "username must be one or more letters A to Z, digits, _, - or .",
        ));
    }
    Ok(username.to_string())
}

/// Refuse a username another account already has, and the Demo Account's
/// username whether or not the Demo Account exists: the build writes the
/// Demo Account under that name, so an account holding it would make every
/// later build fail. No caller creates the Demo Account itself.
///
/// # Errors
///
/// A `409` naming the username when it is taken. A failed lookup is a `500`.
pub(crate) async fn require_username_free(
    conn: &mut SqliteConnection,
    username: &str,
) -> Result<(), ApiError> {
    if username
        .trim()
        .eq_ignore_ascii_case(account_profile::DEMO_USERNAME)
    {
        return Err(ApiError::UsernameTaken(format!(
            "username already taken: {username} belongs to the Demo Account"
        )));
    }
    if account_profile::lookup_account_by_username(conn, username)
        .await
        .map_err(ApiError::Internal)?
        .is_some()
    {
        return Err(ApiError::UsernameTaken(format!(
            "username already taken: {username}"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Changing one's own password
// ---------------------------------------------------------------------------

/// Store `new_hash`, drop named API tokens, issue a fresh session token, and
/// record the change in the Audit Trail at `now`. All of that happens in one
/// database transaction so a failure leaves the old credentials in place. The
/// logged-in session is the credential: the current password is not asked
/// for.
///
/// # Errors
///
/// Fails when a database read or write fails.
pub(crate) async fn change_password_on_conn(
    conn: &mut SqliteConnection,
    account_id: i64,
    new_hash: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String> {
    let mut tx = crate::db::begin_write(conn).await?;
    account_profile::update_password_hash(&mut tx, account_id, new_hash).await?;
    api_tokens::delete_all_api_tokens(&mut tx, account_id).await?;
    let login_entry_id = session_tokens::live_login_entry(&mut tx, account_id, now).await?;
    let (token, expires) =
        session_tokens::rotate_account_session_token(&mut tx, account_id, now).await?;
    crate::db::audit_trail::record_own_password_change(
        &mut tx,
        account_id,
        login_entry_id.map(|id| (id, expires)),
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(token)
}

#[cfg(test)]
mod tests;
