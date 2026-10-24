//! Who a request is: the Bearer credential resolved to an account and what it
//! may do, the `require_*` guards each route runs, and the extractors that
//! run a guard before a handler can start.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use api_types::{APP_HEADER, APP_VERSION_HEADER};
use axum::extract::FromRef;
use axum::http::{HeaderMap, header};
use sqlx::{SqliteConnection, SqlitePool};

use crate::api_error::ApiError;
use crate::db::account_profile;
use crate::db::api_tokens;
use crate::db::audit_trail::CredentialUsed;
use crate::db::permissions::Permissions;
use crate::db::session_tokens;

/// What a Bearer credential is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthCapability {
    /// Logged-in session on an ordinary account. Carries the account's own
    /// permissions.
    Session {
        /// What the account may do.
        permissions: Permissions,
    },
    /// Logged-in owner. Carries no permissions at all, so every guard
    /// that asks for one refuses it and the owner cannot reach message data.
    /// See `docs/adr/0008-the-owner-holds-no-messages.md`.
    Owner,
    /// Named API token. Already intersected with its owner's permissions.
    ApiToken(Permissions),
}

/// Authenticated account from a session token or named API token.
#[derive(Debug, Clone)]
pub struct AuthIdentity {
    /// The authenticated account.
    pub account_id: i64,
    /// What this credential is allowed to do.
    pub capability: AuthCapability,
    /// The credential as the Audit Trail records it on a run it starts: a
    /// Session and its app, or an API token's label and hint.
    pub credential: crate::db::audit_trail::CredentialUsed,
}

impl AuthIdentity {
    /// What this credential may do, account and token already intersected.
    /// The owner has nothing: holding no messages is what the owner is.
    pub fn permissions(&self) -> Permissions {
        match self.capability {
            AuthCapability::Session { permissions } | AuthCapability::ApiToken(permissions) => {
                permissions
            }
            AuthCapability::Owner => Permissions::none(),
        }
    }

    /// True only for the logged-in owner. An API token can never be the
    /// owner, because no token resolves to [`AuthCapability::Owner`].
    pub fn is_owner(&self) -> bool {
        matches!(self.capability, AuthCapability::Owner)
    }

    /// True when the credential is a logged-in session on an ordinary account:
    /// not a token, and not the owner.
    pub fn is_session(&self) -> bool {
        matches!(self.capability, AuthCapability::Session { .. })
    }

    /// True when a person logged in, whether as the owner or on an
    /// ordinary account. False for every API token.
    pub fn is_logged_in(&self) -> bool {
        self.is_session() || self.is_owner()
    }
}

/// Reject API tokens and the owner's session on routes that require a
/// logged-in session on an ordinary account.
///
/// # Errors
///
/// Returns `403 Forbidden` when the credential is a named API token or the
/// owner's session.
pub fn require_full_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_session() {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "this endpoint requires a logged-in session; use an API token only for import/export"
            .into(),
    ))
}

/// Reject anything that is not the logged-in owner.
///
/// # Errors
///
/// Returns forbidden for ordinary sessions and for every API token.
pub fn require_owner(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_owner() {
        return Ok(());
    }
    Err(ApiError::NotTheOwner(
        "this endpoint requires the owner's session".into(),
    ))
}

/// Allow any logged-in person, owner or ordinary account, and reject
/// API tokens. The guard for the routes under `/v1/accounts/{id}`, where a
/// handler then decides whether the caller is the owner or the account
/// itself; the owner needs them for its own row as much as anyone.
///
/// # Errors
///
/// Returns forbidden when the credential is a named API token.
pub fn require_logged_in(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_logged_in() {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "this endpoint requires a logged-in session; use an API token only for import/export"
            .into(),
    ))
}

/// Refuse an act the Demo Account is never open to, whoever asks.
///
/// The Demo Account has no password, so anyone at the login card can enter
/// it. Its limits are therefore fixed here, by its id, whatever its
/// permission row says, and are not settings the owner can change
/// (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
///
/// # Errors
///
/// Returns `demo-account-protected` when `target` is the Demo Account. `what`
/// finishes the `detail` "the Demo Account's ...", one sentence in lowercase
/// with no final period (`docs/architecture/rules/http-api.md`, API-53). What
/// a caller can do about it is on the problem type's page.
///
/// One refusal of the Demo Account is not built here: `delete_account`
/// answers "the Demo Account cannot delete itself" on its own. The owner may
/// delete the Demo Account, so that refusal depends on who asks, and this
/// function refuses whoever asks.
pub fn refuse_for_demo_account(target: i64, what: &str) -> Result<(), ApiError> {
    if account_profile::is_demo_account(target) {
        return Err(ApiError::DemoAccountProtected(format!(
            "the Demo Account's {what}"
        )));
    }
    Ok(())
}

/// Allow a credential that may import. The Demo Account never may, whatever
/// its permission row says: an import would put real messages into an
/// account anyone can enter. It is refused every import route, a read of an
/// Import Run included, because it never has one to read.
///
/// # Errors
///
/// Returns `demo-account-protected` for the Demo Account, and forbidden when
/// import is not permitted.
pub fn require_import_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    refuse_for_demo_account(
        auth.account_id,
        "imports are closed, because its messages come only from its seed",
    )?;
    if auth.permissions().import {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "import is not permitted".into(),
    ))
}

/// Allow a credential that may export.
///
/// # Errors
///
/// Returns forbidden when export is not permitted.
pub fn require_export_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.permissions().export {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "export is not permitted".into(),
    ))
}

/// Allow reading an attachment's bytes: a logged-in session, or an API token
/// that may export.
///
/// A person looking at a conversation is not exporting it, so a session
/// reads its own account's attachments whatever its export permission is.
/// An API token has no screen to show them on; fetching bytes with one is
/// taking them out, which is what the export permission decides.
///
/// # Errors
///
/// Returns forbidden for an API token that may not export.
pub fn require_asset_read_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_session() {
        return Ok(());
    }
    require_export_access(auth)
}

/// Allow a credential that may import or export, for asset probes.
///
/// # Errors
///
/// Returns forbidden when neither is permitted.
pub fn require_import_or_export_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    let p = auth.permissions();
    if p.import || p.export {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "this credential cannot access assets".into(),
    ))
}

/// Allow a credential that may destroy message data. The Demo Account never
/// may, whatever its permission row says: one visitor would empty it for the
/// next.
///
/// # Errors
///
/// Returns `demo-account-protected` for the Demo Account, and forbidden when
/// deletion is not permitted.
pub fn require_delete_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    refuse_for_demo_account(auth.account_id, "data cannot be deleted for good")?;
    if auth.permissions().delete {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "deleting messages is not permitted for this account".into(),
    ))
}

/// Allow a logged-in session that may destroy message data: the guard for
/// permanent deletion out of the trash. Both halves matter. Trash is a GUI
/// affair, so an API token is refused the way every trash route refuses it,
/// and the account must hold the `delete` permission. The Demo Account is
/// refused by its id in [`require_delete_access`], so it uses the trash and
/// deletes nothing for good.
///
/// # Errors
///
/// Returns forbidden when the credential is an API token,
/// `demo-account-protected` for a Demo Account session, and forbidden when
/// the account may not delete.
pub fn require_full_delete_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    require_full_access(auth)?;
    require_delete_access(auth)
}

/// Extract the Bearer credential: handlers take `auth: AuthIdentity` (or one
/// of the capability wrappers below) instead of hand-rolling the
/// `resolve_auth` + `require_*` preamble. Rejections are the same
/// [`ApiError`] responses the preamble produced.
impl<S> axum::extract::FromRequestParts<S> for AuthIdentity
where
    S: Send + Sync,
    SqlitePool: FromRef<S>,
    ExpiryMoves: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        resolve_auth(
            &parts.headers,
            &SqlitePool::from_ref(state),
            &ExpiryMoves::from_ref(state),
        )
        .await
    }
}

/// `auth: Option<AuthIdentity>`, for the one route a stranger and the owner
/// share, `POST /v1/accounts`: `None` when no `Authorization` header was
/// sent, the resolved credential when one was, and the usual `401` or `403`
/// when the header names nothing usable. A bad credential is never quietly
/// treated as no credential.
impl<S> axum::extract::OptionalFromRequestParts<S> for AuthIdentity
where
    S: Send + Sync,
    SqlitePool: FromRef<S>,
    ExpiryMoves: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        if parts.headers.get(header::AUTHORIZATION).is_none() {
            return Ok(None);
        }
        resolve_auth(
            &parts.headers,
            &SqlitePool::from_ref(state),
            &ExpiryMoves::from_ref(state),
        )
        .await
        .map(Some)
    }
}

/// Define a newtype extractor that resolves the Bearer credential and runs one
/// `require_*` capability check, so a route cannot compile without its guard.
macro_rules! auth_guard {
    ($(#[$doc:meta])* $name:ident, $check:path) => {
        $(#[$doc])*
        pub struct $name(pub AuthIdentity);

        impl<S> axum::extract::FromRequestParts<S> for $name
        where
            S: Send + Sync,
            SqlitePool: FromRef<S>,
            ExpiryMoves: FromRef<S>,
        {
            type Rejection = ApiError;

            async fn from_request_parts(
                parts: &mut axum::http::request::Parts,
                state: &S,
            ) -> Result<Self, Self::Rejection> {
                let auth = resolve_auth(
            &parts.headers,
            &SqlitePool::from_ref(state),
            &ExpiryMoves::from_ref(state),
        ).await?;
                $check(&auth)?;
                Ok(Self(auth))
            }
        }
    };
}

auth_guard!(
    /// Logged-in session on an ordinary account (API tokens and the owner
    /// rejected); wraps [`require_full_access`].
    FullAccess,
    require_full_access
);
auth_guard!(
    /// Logged-in owner; wraps [`require_owner`].
    Owner,
    require_owner
);
auth_guard!(
    /// Any logged-in person, owner or ordinary account; wraps
    /// [`require_logged_in`].
    LoggedIn,
    require_logged_in
);
auth_guard!(
    /// Credential that may import; wraps [`require_import_access`].
    ImportAccess,
    require_import_access
);
auth_guard!(
    /// Credential that may export; wraps [`require_export_access`].
    ExportAccess,
    require_export_access
);
auth_guard!(
    /// Credential that may import or export, for asset probes; wraps
    /// [`require_import_or_export_access`].
    ImportOrExportAccess,
    require_import_or_export_access
);
auth_guard!(
    /// Logged-in session whose account may destroy message data; wraps
    /// [`require_full_delete_access`].
    FullDeleteAccess,
    require_full_delete_access
);

/// Read the Bearer token from `Authorization`.
///
/// # Errors
///
/// Returns unauthorized when the header is missing or not a Bearer value.
pub fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(ApiError::AuthenticationRequired(
            "missing Authorization: Bearer <token>".into(),
        ));
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::AuthenticationRequired("invalid Authorization header".into()))?;
    // RFC 7235, section 2.1: the scheme matches without regard to case.
    let Some(token) = value
        .split_once(' ')
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token)
    else {
        return Err(ApiError::AuthenticationRequired(
            "Authorization must be Bearer <token>".into(),
        ));
    };
    let token = token.trim();
    if token.is_empty() {
        return Err(ApiError::AuthenticationRequired("empty API token".into()));
    }
    Ok(token.to_string())
}

/// Resolve a session token or named API token to an account.
///
/// # Errors
///
/// Returns unauthorized when the token is missing or invalid.
pub async fn resolve_auth(
    headers: &HeaderMap,
    db: &SqlitePool,
    expiry_moves: &ExpiryMoves,
) -> Result<AuthIdentity, ApiError> {
    let token = bearer_token(headers)?;
    // Always look up against SQLite so rotate/delete in Settings takes effect
    // without restarting serve (no process-local token cache).
    let mut conn = db.acquire().await?;
    let (identity, expiry_move) = resolve_auth_and_expiry_move(
        &mut conn,
        &token,
        connecting_app(headers).as_ref(),
        chrono::Utc::now(),
    )
    .await?;
    // Written apart from the request, which never waits for the write lock.
    if let Some(expiry_move) = expiry_move {
        expiry_moves.spawn(db, expiry_move);
    }
    Ok(identity)
}

/// Writes the moves of Sessions' expiries apart from the requests that made
/// them due, one at a time. A sub-state of the server's state, so every
/// request shares the one that is running.
#[derive(Debug, Clone, Default)]
pub struct ExpiryMoves(Arc<AtomicBool>);

impl ExpiryMoves {
    /// Write `expiry_move` in a task of its own, on a connection of its own
    /// from `pool`, so the request that made it due is served without
    /// waiting for the write lock. While a move runs, another is dropped,
    /// and a later request makes it again, so an import holding the write
    /// lock keeps at most one connection waiting on it. A move the busy
    /// timeout or the pool's acquire timeout ends is dropped without a word;
    /// any other failure is logged. Either way the Session expires when it
    /// would have without the move.
    pub(crate) fn spawn(&self, pool: &SqlitePool, expiry_move: session_tokens::ExpiryMove) {
        let Some(running) = MoveRunning::claim(&self.0) else {
            return;
        };
        let pool = pool.clone();
        crate::request_id::spawn_in_current_span(async move {
            let _running = running;
            let moved = async {
                let mut conn = pool.acquire().await.context("acquire a connection")?;
                session_tokens::move_session_expiry(&mut conn, &expiry_move).await
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
}

/// Holds a flag set while a move runs, and clears it when dropped, so a move
/// that fails, panics or is canceled lets the next one start.
struct MoveRunning(Arc<AtomicBool>);

impl MoveRunning {
    /// Set `running`, or `None` when it is set already.
    fn claim(running: &Arc<AtomicBool>) -> Option<Self> {
        (!running.swap(true, Ordering::AcqRel)).then(|| Self(Arc::clone(running)))
    }
}

impl Drop for MoveRunning {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Longest Build the server records. A real one is under thirty characters.
const MAX_APP_BUILD_LEN: usize = 64;

/// The app a request says it comes from. `None` unless both headers are
/// present and well formed: curl, Swagger UI and a script send neither, and
/// are served without anything being recorded.
pub(crate) fn connecting_app(headers: &HeaderMap) -> Option<session_tokens::ConnectingApp> {
    let kind = session_tokens::AppKind::parse(headers.get(APP_HEADER)?.to_str().ok()?)?;
    let build = headers.get(APP_VERSION_HEADER)?.to_str().ok()?.trim();
    let well_formed = !build.is_empty()
        && build.len() <= MAX_APP_BUILD_LEN
        && build.chars().all(|c| c.is_ascii_graphic());
    well_formed.then(|| session_tokens::ConnectingApp {
        kind,
        build: build.to_string(),
    })
}

/// Credential-specific bit not yet folded into `AuthCapability`: a session
/// carries no extra state, an API token carries its own (pre-intersection)
/// permissions. Both kinds load `AccountAuth` the same way so the disabled
/// check in [`resolve_auth_and_expiry_move`] runs exactly once.
enum Credential {
    Session,
    ApiToken(Permissions),
}

/// [`resolve_auth_and_expiry_move`] without the move, for a test that
/// resolves a credential on a connection of its own.
///
/// # Errors
///
/// As [`resolve_auth_and_expiry_move`].
#[cfg(test)]
pub(crate) async fn resolve_auth_on_conn(
    conn: &mut SqliteConnection,
    token: &str,
    app: Option<&session_tokens::ConnectingApp>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<AuthIdentity, ApiError> {
    resolve_auth_and_expiry_move(conn, token, app, now)
        .await
        .map(|(identity, _)| identity)
}

/// Resolve a Bearer credential on an existing connection, as it stands at
/// `now`: a Session or an API token that has expired by then is refused.
/// Also answers the move of a Session's expiry the request made due, for
/// the caller to write ([`ExpiryMoves::spawn`]): `None` for an API token,
/// and for a Session whose move would be too small to write.
///
/// `app` is the app the request named, if it named one. It is recorded on a
/// session and ignored for an API token, which belongs to a program rather
/// than to either app. It never decides whether the request is served.
///
/// # Errors
///
/// Unauthorized when the token matches nothing, which includes an API token
/// on the owner's account (`api_tokens::lookup_account_for_api_token` refuses
/// it); forbidden when the account is disabled.
async fn resolve_auth_and_expiry_move(
    conn: &mut SqliteConnection,
    token: &str,
    app: Option<&session_tokens::ConnectingApp>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(AuthIdentity, Option<session_tokens::ExpiryMove>), ApiError> {
    let mut expiry_move = None;
    let resolved = if let Some(session) =
        session_tokens::lookup_session(&mut *conn, token, now).await?
    {
        if let Some(app) = app {
            // A record, not a check: a failed write still serves the
            // request (#1189).
            if let Err(err) = session_tokens::record_connecting_app(&mut *conn, &session, app).await
            {
                tracing::warn!(
                    account_id = session.account_id,
                    error = %format!("{err:#}"),
                    "The connecting app could not be recorded"
                );
            }
        }
        expiry_move = session.expiry_move;
        // The app this request named, or the one the session last recorded.
        let used = CredentialUsed::Session(app.cloned().or(session.app));
        Some((session.account_id, Credential::Session, used))
    } else {
        api_tokens::lookup_account_for_api_token(&mut *conn, token, now)
            .await?
            .map(|tok| {
                let used = CredentialUsed::ApiToken {
                    label: tok.label,
                    hint: tok.token_hint,
                };
                (tok.account_id, Credential::ApiToken(tok.permissions), used)
            })
    };

    let Some((account_id, credential, used)) = resolved else {
        return Err(ApiError::TokenRefused("invalid API token".into()));
    };

    let auth = account_profile::load_account_auth(&mut *conn, account_id)
        .await?
        .ok_or_else(ApiError::account_gone)?;
    if auth.disabled {
        return Err(ApiError::AccountDisabled("this account is disabled".into()));
    }

    // A session on the owner's account resolves to `Owner`, which carries no
    // permissions. An API token never does, whichever account issued it, so
    // no token can reach what the owner reaches.
    let capability = match credential {
        Credential::Session if account_profile::is_server_owner(account_id) => {
            AuthCapability::Owner
        }
        Credential::Session => AuthCapability::Session {
            permissions: auth.permissions,
        },
        Credential::ApiToken(tok_permissions) => {
            AuthCapability::ApiToken(auth.permissions.intersect(tok_permissions))
        }
    };

    Ok((
        AuthIdentity {
            account_id,
            capability,
            credential: used,
        },
        expiry_move,
    ))
}

/// `Ok` while `account_id` still has its row, else
/// [`ApiError::account_gone`]. A route that starts a run for the account its
/// credential named calls it inside its write transaction: the credential
/// proved the row existed when the request arrived, and an account deleted
/// since gets no run and no row put back for it, which would carry no
/// password (#2061).
///
/// # Errors
///
/// [`ApiError::account_gone`] when the row is gone, or an internal error
/// when the statement fails.
pub(crate) async fn require_account_row(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<(), ApiError> {
    if account_profile::account_exists(conn, account_id).await? {
        Ok(())
    } else {
        Err(ApiError::account_gone())
    }
}

/// The account an import or asset route writes to: the one the credential
/// names, always.
///
/// A route never takes an `account=` parameter. A session or an API token
/// belongs to exactly one account, so a parameter could only repeat it or
/// contradict it, and the rules document says the credential names the
/// account (`docs/architecture/rules/http-api.md`, "Credentials and reach").
pub(crate) fn resolve_import_account(auth: &AuthIdentity) -> i64 {
    auth.account_id
}

#[cfg(test)]
mod tests;
