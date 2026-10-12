//! The Session singleton: `POST`, `GET` and `DELETE /v1/session`.
//!
//! A Session is one per logged-in account or owner. Logging in creates it,
//! reading it says which account the bearer token names, and logging out
//! ends it. The password rules it applies live in `credentials`.

use std::time::Instant;

use axum::extract::State;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::credentials::{
    AuthRateLimits, MAX_PASSWORD_BYTES, RateBucket, normalize_username, verify_login_password,
};
use crate::db::audit_trail::{self, AuditReason};
use crate::db::session_tokens::ConnectingApp;
use crate::db::{account_profile, api_tokens, dedupe, session_tokens};
use crate::extract::Json;
use crate::server::{ApiError, AppState, AuthIdentity, Created};

/// Username and password, the body of `POST /v1/session`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateSessionRequest {
    /// Login username.
    pub username: String,
    /// Login password.
    #[serde(default)]
    pub password: String,
}

/// Session token plus the account id and username it belongs to.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreateSessionResponse {
    /// Session token to send as `Authorization: Bearer …`.
    pub token: String,
    /// Account id the session belongs to.
    pub account_id: i64,
    /// The username the account logs in with.
    pub username: String,
}

impl CreateSessionResponse {
    /// Open the Session for an existing account, replacing the one it had,
    /// and record the login. `username` is the one the account's row holds.
    async fn for_existing_account(
        conn: &mut SqliteConnection,
        account_id: i64,
        username: String,
        app: Option<&ConnectingApp>,
    ) -> anyhow::Result<CreateSessionResponse> {
        let mut tx = crate::db::begin_write(conn).await?;
        let token = session_tokens::open_session(&mut tx, account_id, &username, app).await?;
        account_profile::record_login(&mut tx, account_id).await?;
        tx.commit().await?;
        Ok(CreateSessionResponse {
            token,
            account_id,
            username,
        })
    }
}

/// The logged-in credential's account, username, and import sources.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct Session {
    /// Source ids the account's messages came from, such as `imessage` or
    /// `whatsapp`, in the order their first messages were imported, earliest
    /// first.
    sources: Vec<String>,
    /// Id of the account the credential acts for.
    account_id: i64,
    /// The username the account logs in with.
    username: String,
}

/// The Session the bearer token names: its account, username, and import
/// sources. A session token and an API token both answer, because a program
/// checking its token needs the same facts as a browser restoring a login.
/// An account deleted after its credential was checked answers
/// `401 Unauthorized`, as a credential naming no account does.
#[utoipa::path(
    get,
    path = "/v1/session",
    tag = "Session",
    security(("session" = []), ("api-token" = [])),
    responses(
        (status = 200, body = Session),
    )
)]
pub(crate) async fn get_session(
    State(state): State<AppState>,
    auth: AuthIdentity,
) -> Result<Json<Session>, ApiError> {
    let account_id = auth.account_id;
    let username = load_username(&state.db, account_id).await?;
    let sources = list_account_sources(&state.db, account_id).await?;
    Ok(Json(Session {
        sources,
        account_id,
        username,
    }))
}

/// Source ids this account has imported, oldest first.
async fn list_account_sources(pool: &SqlitePool, account_id: i64) -> Result<Vec<String>, ApiError> {
    let mut conn = pool.acquire().await?;
    Ok(dedupe::sources_in_import_order(&mut conn, account_id).await?)
}

/// Username for the credential's account. An account deleted between the
/// credential check and this read answers `401 Unauthorized`, as a credential
/// naming no account does.
async fn load_username(pool: &SqlitePool, account_id: i64) -> Result<String, ApiError> {
    let mut conn = pool.acquire().await?;
    account_profile::username_for_account(&mut conn, account_id)
        .await?
        .ok_or_else(ApiError::account_gone)
}

/// The answer to every refused login, whatever was wrong, so the answer does
/// not tell a guesser which part failed.
fn invalid_login() -> ApiError {
    ApiError::InvalidCredentials("invalid username or password".into())
}

/// Count a login as `username`, which names no account: once for the whole
/// server, then under the username. `Ok(false)` when the server's count is
/// full, and the login is to be refused unchecked.
///
/// A login the username's count refuses is given back to the server's
/// count, because a refused attempt is not counted. The username's count is
/// kept past the server's count too, so its `429` comes at the same attempt
/// as a real account's whether or not the server's count is full, and does
/// not tell a stranger which usernames name an account.
fn count_unknown_username(limits: &AuthRateLimits, username: &str) -> Result<bool, ApiError> {
    let own = RateBucket::unknown_username(username);
    match limits.count(&RateBucket::UnknownUsernames) {
        Ok(server_hit) => {
            if let Err(err) = limits.count(&own) {
                limits.give_back(&RateBucket::UnknownUsernames, server_hit)?;
                return Err(err);
            }
            Ok(true)
        }
        Err(ApiError::RateLimited { .. }) => {
            limits.count(&own)?;
            Ok(false)
        }
        Err(err) => Err(err),
    }
}

/// Log in: verify a local username and password and answer the Session, a
/// `201 Created` whose `Location` is the singleton itself.
#[utoipa::path(
    post,
    path = "/v1/session",
    tag = "Session",
    request_body = CreateSessionRequest,
    responses(
        (
            status = 201,
            description = "Logged in; the Session exists",
            body = CreateSessionResponse,
            headers(("Location" = String, description = "`/v1/session`"))
        ),
        crate::problem::openapi::InvalidCredentials,
        crate::problem::openapi::RateLimited
    )
)]
pub async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Created<CreateSessionResponse>, ApiError> {
    let username = normalize_username(&req.username);
    if username.is_empty() {
        return Err(ApiError::validation("username is required"));
    }
    // Before any count, so the answer is the same whether or not the
    // username names an account and whatever its count holds.
    if req.password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::validation("password is too long"));
    }
    let app = crate::server::connecting_app(&headers);
    let app = app.as_ref();
    let mut conn = state.db.acquire().await?;
    let account_id = account_profile::lookup_account_by_username(&mut conn, &username).await?;
    // A rate-limited attempt is turned away before anything is checked, and
    // the Audit Trail records nothing of it. Every attempt is counted before
    // its password is checked, so guesses sent at once cannot all pass the
    // limit; a login with the right password is given back below, so the
    // holder's own logins never fill the count.
    //
    // Counted per account, not per username as typed: the lookup ignores
    // case, so every spelling of a username guesses at the same password. A
    // username nobody holds is counted under that username, and once more for
    // the whole server, so a script trying a new username each time cannot
    // run Argon2 and write to the Audit Trail as fast as it likes.
    //
    // Past the server's count a login as a username nobody holds is refused
    // unchecked: no Argon2, no Audit Trail entry, no log line. Its answer is
    // the one a wrong password gets, after about as long, not `429`, because
    // a `429` that only unknown usernames get would tell a stranger which
    // usernames name an account.
    let limits = &state.auth_rate_limits;
    let counted = match account_id {
        Some(id) => {
            let bucket = RateBucket::Password(id);
            let hit = limits.count(&bucket)?;
            Some((bucket, hit))
        }
        None => {
            if !count_unknown_username(limits, &username)? {
                drop(conn);
                limits.wait_as_long_as_an_unknown_refusal().await;
                return Err(invalid_login());
            }
            None
        }
    };
    let counted_at = Instant::now();
    // Refused logins for usernames nobody holds are kept for a fixed time,
    // and a login is when the old ones go.
    audit_trail::trim_refused_logins(&mut conn).await?;

    let password = req.password.clone();

    // The username the account's row holds. An account deleted since the
    // lookup above is a username nobody holds by now, so it takes the same
    // branch, with the same timing and the same Audit Trail entry.
    let account = match account_id {
        Some(id) => account_profile::username_for_account(&mut conn, id)
            .await?
            .map(|stored| (id, stored)),
        None => None,
    };
    let Some((account_id, stored_username)) = account else {
        // The connection goes back to the pool for the length of the Argon2
        // run, as it does before the real check below.
        drop(conn);
        let _ = verify_login_password(None, &password).await?;
        let mut conn = state.db.acquire().await?;
        audit_trail::record_refused_login(
            &mut conn,
            &username,
            None,
            AuditReason::UnknownUsername,
            app,
        )
        .await?;
        limits.record_unknown_refusal(counted_at.elapsed());
        return Err(invalid_login());
    };

    // The Demo Account cannot be entered while it is being built: until the
    // build ends it holds part of its data, and a failed build removes it.
    // The login card is told there is no Demo Account, so the answer here is
    // the one for a username that does not exist.
    if account_id == account_profile::DEMO_ACCOUNT_ID && state.demo_build.is_building() {
        return Err(invalid_login());
    }

    let account = Some((account_id, stored_username.as_str()));
    let password_hash = account_profile::load_password_hash(&mut conn, account_id).await?;
    // Argon2 runs on the blocking thread pool with no pooled connection held,
    // so a burst of logins neither stalls the async workers nor empties the
    // pool. An account changed meanwhile is read afresh below.
    drop(conn);
    let verified = verify_login_password(password_hash.as_deref(), &password).await?;
    let mut conn = state.db.acquire().await?;
    if !verified {
        audit_trail::record_refused_login(
            &mut conn,
            &username,
            account,
            AuditReason::WrongPassword,
            app,
        )
        .await?;
        return Err(invalid_login());
    }

    let auth = account_profile::load_account_auth(&mut conn, account_id)
        .await?
        .ok_or_else(invalid_login)?;
    // A disabled account gets the answer every refused login gets. Its own
    // answer would come only after the right password, so a guesser would
    // learn which guess was right, and that password works again once the
    // owner enables the account. The Audit Trail still says why. The attempt
    // stays counted for the same reason: given back, it would show in the
    // count which guess was right.
    if auth.disabled {
        audit_trail::record_refused_login(
            &mut conn,
            &username,
            account,
            AuditReason::AccountDisabled,
            app,
        )
        .await?;
        return Err(invalid_login());
    }

    let body =
        CreateSessionResponse::for_existing_account(&mut conn, account_id, stored_username, app)
            .await?;
    // The holder's own login: it does not count against the account.
    if let Some((bucket, hit)) = counted {
        limits.give_back(&bucket, hit)?;
    }

    Ok(Created {
        location: "/v1/session".to_string(),
        body,
    })
}

/// Revoke the session token. Returns whether it named a Session.
async fn logout_on_conn(conn: &mut SqliteConnection, token: &str) -> anyhow::Result<bool> {
    session_tokens::revoke_session_token(conn, token).await
}

/// Log out: revoke the presented session token, ending the Session.
///
/// It takes the bearer token itself rather than a guard, so a disabled
/// account can still end its own Session. An API token is not a Session and
/// is refused; a token that names nothing is a `401`.
#[utoipa::path(
    delete,
    path = "/v1/session",
    tag = "Session",
    security(("session" = [])),
    responses(
        (status = 204, description = "Logged out"),
    )
)]
pub async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<axum::http::StatusCode, ApiError> {
    let token = crate::server::bearer_token(&headers)?;
    let mut conn = state.db.acquire().await?;
    if logout_on_conn(&mut conn, &token).await? {
        return Ok(axum::http::StatusCode::NO_CONTENT);
    }
    if api_tokens::lookup_account_for_api_token(&mut conn, &token)
        .await?
        .is_some()
    {
        return Err(ApiError::InsufficientScope(
            "an API token is not a session and cannot log out; delete it under Settings instead"
                .into(),
        ));
    }
    Err(ApiError::TokenRefused("this token names no session".into()))
}

#[cfg(test)]
mod tests;
