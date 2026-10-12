//! Passwords, usernames, and the rate limiter on attempts to present them.
//!
//! Nothing here is a route. The Session routes (`session_api`), the accounts
//! collection (`accounts_api`), claiming (`server_api`) and the owner's shell
//! commands (`owner_cli`) all hash, verify and validate through this module,
//! so a password rule is written once.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use sqlx::SqliteConnection;
use tokio::sync::Semaphore;

use crate::db::{account_profile, api_tokens, session_tokens};
use crate::server::ApiError;

/// Max password bytes accepted before hashing (creation, login, change).
pub(crate) const MAX_PASSWORD_BYTES: usize = 1024;
/// Sliding window for the routes a stranger may call with a credential.
pub(crate) const AUTH_RATE_WINDOW: Duration = Duration::from_secs(60);
/// The most hits one bucket may count within [`AUTH_RATE_WINDOW`]. The next
/// attempt is refused as `rate-limited`. Every bucket but
/// [`RateBucket::UnknownUsernames`] holds this many.
pub(crate) const AUTH_RATE_MAX: usize = 20;
/// The most logins as usernames that name no account the whole server takes
/// within [`AUTH_RATE_WINDOW`], whichever usernames they are. Well above what
/// people mistyping their usernames produce, and low enough that a script
/// trying a new username each time cannot keep Argon2 busy or fill the Audit
/// Trail.
pub(crate) const UNKNOWN_USERNAME_RATE_MAX: usize = 120;

static DUMMY_PASSWORD_HASH: OnceLock<String> = OnceLock::new();

/// One count the rate limiter keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RateBucket {
    /// Registrations, once for the whole server.
    Register,
    /// Claims, once for the whole server.
    Claim,
    /// Guesses at one account's password: a login as it, and a
    /// `current_password` sent to change the owner's password or to delete
    /// the account. Keyed by the id, so every spelling of the username counts
    /// here.
    Password(i64),
    /// Logins as one username that names no account, folded the way the
    /// account lookup folds it (`COLLATE NOCASE`, which folds ASCII letters
    /// only), so its spellings share one count as an account's do. Build it
    /// with [`RateBucket::unknown_username`].
    UnknownUsername(String),
    /// Logins as any username that names no account, once for the whole
    /// server, so a script that tries a new username each time is counted.
    UnknownUsernames,
}

impl RateBucket {
    /// The bucket for logins as `username`, which names no account.
    pub(crate) fn unknown_username(username: &str) -> Self {
        Self::UnknownUsername(username.to_ascii_lowercase())
    }

    /// The key the bucket's hits are kept under: its kind, and the account
    /// or username it counts for.
    fn key(&self) -> String {
        match self {
            Self::Password(account_id) => format!("{}:{account_id}", self.kind()),
            Self::UnknownUsername(folded) => format!("{}:{folded}", self.kind()),
            Self::Register | Self::Claim | Self::UnknownUsernames => self.kind().to_string(),
        }
    }

    /// The most hits the bucket holds in [`AUTH_RATE_WINDOW`].
    fn max(&self) -> usize {
        match self {
            Self::UnknownUsernames => UNKNOWN_USERNAME_RATE_MAX,
            _ => AUTH_RATE_MAX,
        }
    }

    /// The bucket's kind as the server's log names it. A username is never
    /// written: the log holds ids and outcomes only.
    fn kind(&self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Claim => "claim",
            Self::Password(_) => "password",
            Self::UnknownUsername(_) => "unknown_username",
            Self::UnknownUsernames => "unknown_usernames",
        }
    }

    /// The account whose password the bucket guards, for the log.
    fn account_id(&self) -> Option<i64> {
        match self {
            Self::Password(account_id) => Some(*account_id),
            _ => None,
        }
    }
}

/// One counted attempt, which [`AuthRateLimits::give_back`] takes back when
/// the attempt turned out to be the holder's own.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hit(Instant);

/// One bucket's hits inside the window, and whether the log has been told it
/// filled.
#[derive(Debug, Default)]
struct Hits {
    times: VecDeque<Instant>,
    warned: bool,
}

/// Sliding-window hit counts for the credential routes, one per
/// [`RateBucket`].
///
/// This lives on `AppState` rather than in a process-global static: a running
/// server builds exactly one state, so the limiter still spans the whole server,
/// while each test fixture gets its own counts and cannot rate-limit an unrelated
/// test running beside it in the same binary.
#[derive(Debug, Clone, Default)]
pub(crate) struct AuthRateLimits {
    buckets: Arc<Mutex<HashMap<String, Hits>>>,
    /// The time every count reads, when a test has stopped the clock.
    /// `None` reads the system clock.
    frozen_at: Option<Instant>,
    /// How long the last login as an unknown username took to refuse once
    /// counted, Argon2, the Audit Trail entry and the waits for both
    /// included, in nanoseconds. A login refused unchecked waits this long.
    unknown_refusal_nanos: Arc<AtomicU64>,
}

impl AuthRateLimits {
    /// A limiter whose clock reads `now` for good, so a test can fill a
    /// window with no wall-clock time passing.
    #[cfg(test)]
    pub(crate) fn frozen_at(now: Instant) -> Self {
        Self {
            buckets: Arc::default(),
            frozen_at: Some(now),
            unknown_refusal_nanos: Arc::default(),
        }
    }

    /// Remember how long a login as an unknown username took to refuse,
    /// from its count to its answer.
    pub(crate) fn record_unknown_refusal(&self, took: Duration) {
        let nanos = u64::try_from(took.as_nanos()).unwrap_or(u64::MAX);
        self.unknown_refusal_nanos.store(nanos, Ordering::Relaxed);
    }

    /// Wait as long as the last login as an unknown username took to refuse,
    /// without running Argon2 or writing anything. A login refused unchecked,
    /// because the server's count of them is full, then answers after about
    /// the time a checked one does, so its timing does not say the username
    /// names no account, and it costs no CPU.
    pub(crate) async fn wait_as_long_as_an_unknown_refusal(&self) {
        let nanos = self.unknown_refusal_nanos.load(Ordering::Relaxed);
        tokio::time::sleep(Duration::from_nanos(nanos)).await;
    }

    fn now(&self) -> Instant {
        self.frozen_at.unwrap_or_else(Instant::now)
    }

    /// Count one attempt in `bucket`, or refuse it as `rate-limited` when
    /// the bucket already holds its most hits inside [`AUTH_RATE_WINDOW`]. A
    /// refused attempt is not counted. The answer is the hit, for
    /// [`AuthRateLimits::give_back`].
    ///
    /// Every attempt is counted before its password is checked, so that many
    /// attempts sent at once cannot all pass while none has been counted yet.
    pub(crate) fn count(&self, bucket: &RateBucket) -> Result<Hit, ApiError> {
        self.count_at(bucket, self.now())
    }

    /// [`AuthRateLimits::count`] with the clock as an argument, so a test can
    /// move it.
    fn count_at(&self, bucket: &RateBucket, now: Instant) -> Result<Hit, ApiError> {
        let mut map = self.lock()?;
        // Forget every bucket whose newest hit is outside the window. A login
        // as an unknown username is counted under that username, so without
        // this a client spraying usernames grows the map for the life of the
        // process.
        map.retain(|_, hits| {
            hits.times
                .back()
                .is_some_and(|newest| now.duration_since(*newest) <= AUTH_RATE_WINDOW)
        });
        let hits = map.entry(bucket.key()).or_default();
        while let Some(oldest) = hits.times.front() {
            if now.duration_since(*oldest) <= AUTH_RATE_WINDOW {
                break;
            }
            hits.times.pop_front();
        }
        if hits.times.len() >= bucket.max() {
            // The oldest hit inside the window is the next to leave it; until
            // it does, every attempt is refused. Never zero: a client told to
            // wait nothing would retry at once and be refused again.
            let oldest = hits.times.front().copied().unwrap_or(now);
            let remaining = AUTH_RATE_WINDOW.saturating_sub(now.duration_since(oldest));
            let retry_after_secs = remaining.as_secs().max(1);
            // One line when the bucket fills, not one per refused attempt:
            // each refusal already leaves its own `rate-limited` line, and a
            // sustained attack must not write the rest of the log away. The
            // bucket forgets it warned once it empties.
            if !hits.warned {
                hits.warned = true;
                tracing::warn!(
                    bucket = bucket.kind(),
                    account_id = bucket.account_id(),
                    retry_after_secs,
                    "Too many authentication attempts: the rate limit refuses more until the window passes"
                );
            }
            return Err(ApiError::RateLimited { retry_after_secs });
        }
        hits.times.push_back(now);
        Ok(Hit(now))
    }

    /// Take back `hit` from `bucket`: the attempt was the holder's own, such
    /// as a login with the right password, so it does not count against the
    /// account. Only an attempt whose answer already tells the caller it
    /// succeeded is given back; giving back any other would let a guesser
    /// read from the count which guess was right.
    pub(crate) fn give_back(&self, bucket: &RateBucket, hit: Hit) -> Result<(), ApiError> {
        let mut map = self.lock()?;
        let key = bucket.key();
        let Some(hits) = map.get_mut(&key) else {
            return Ok(());
        };
        if let Some(at) = hits.times.iter().position(|time| *time == hit.0) {
            hits.times.remove(at);
        }
        if hits.times.is_empty() {
            map.remove(&key);
        }
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Hits>>, ApiError> {
        self.buckets
            .lock()
            .map_err(|_| ApiError::Internal(anyhow::anyhow!("auth rate limiter poisoned")))
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
/// threads. Without a limit, a few hundred logins at once, each as a
/// different unknown username and so each in its own rate-limit bucket,
/// would ask for gigabytes and could get the server killed. One run per core
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
    crate::server::run_blocking(task, move || {
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

/// Always run Argon2 so missing accounts cost similar to wrong passwords.
/// Passwordless accounts (NULL hash) still accept an empty password only.
/// A login as a username nobody holds calls this with no hash and ignores
/// the answer. Argon2 runs on the blocking thread pool.
pub(crate) async fn verify_login_password(
    password_hash: Option<&str>,
    password: &str,
) -> Result<bool, ApiError> {
    let (hash, password) = (password_hash.map(str::to_owned), password.to_owned());
    run_argon2("password check", move || {
        Ok(match hash.as_deref() {
            None | Some("") => {
                let _ = verify_password(dummy_password_hash(), &password);
                password.is_empty()
            }
            Some(hash) => verify_password(hash, &password),
        })
    })
    .await
}

/// Hash the owner's password, on the blocking thread pool. The owner must
/// have one, so an empty password is refused; one character is enough.
pub(crate) async fn hash_owner_password(password: &str) -> Result<String, ApiError> {
    if password.is_empty() {
        return Err(ApiError::validation("the owner must have a password"));
    }
    require_hashable(password)?;
    let password = password.to_owned();
    run_argon2("password hash", move || hash_password(&password)).await
}

/// Hash a user account's password, on the blocking thread pool. There is no
/// minimum length: an empty password is `None`, an account with no password.
pub(crate) async fn hash_user_password(password: &str) -> Result<Option<String>, ApiError> {
    if password.is_empty() {
        return Ok(None);
    }
    require_hashable(password)?;
    let password = password.to_owned();
    run_argon2("password hash", move || hash_password(&password).map(Some)).await
}

/// Refuse a password longer than Argon2 should be asked to hash.
fn require_hashable(password: &str) -> Result<(), ApiError> {
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::validation("password is too long"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Username validation
// ---------------------------------------------------------------------------

/// The username as stored: surrounding whitespace removed.
pub(crate) fn normalize_username(raw: &str) -> String {
    raw.trim().to_string()
}

/// True for 1 to 128 characters of ASCII letters, digits, `_`, `-`, or `.`.
///
/// ASCII only, because the account lookup (`COLLATE NOCASE`) and
/// [`unknown_username_bucket`] fold ASCII letters only: a letter from another
/// script would make an account whose name looks like another's, such as
/// `аdmin` with a Cyrillic `а` beside `admin`. A preferred name carries a
/// person's name in any script.
pub(crate) fn is_valid_username(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// The trimmed username, or the `validation-failed` every route that takes
/// one answers when it is malformed.
pub(crate) fn require_valid_username(raw: &str) -> Result<String, ApiError> {
    let username = normalize_username(raw);
    if !is_valid_username(&username) {
        return Err(ApiError::validation(
            "username must be 1–128 characters: letters A to Z, digits, _, - or .",
        ));
    }
    Ok(username)
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
/// record the change in the Audit Trail. All of that happens in one database
/// transaction so a failure leaves the old credentials in place. The
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
) -> Result<String> {
    let mut tx = crate::db::begin_write(conn).await?;
    account_profile::update_password_hash(&mut tx, account_id, new_hash).await?;
    api_tokens::delete_all_api_tokens(&mut tx, account_id).await?;
    let login_entry_id = session_tokens::live_login_entry(&mut tx, account_id).await?;
    let (token, expires) =
        session_tokens::rotate_account_session_token(&mut tx, account_id).await?;
    crate::db::audit_trail::record_own_password_change(
        &mut tx,
        account_id,
        login_entry_id.map(|id| (id, expires)),
    )
    .await?;
    tx.commit().await?;
    Ok(token)
}

#[cfg(test)]
mod tests;
