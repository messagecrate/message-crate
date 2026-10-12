//! The Session singleton: `POST`, `GET` and `DELETE /v1/session`.
//!
//! A Session is one per logged-in account or owner. Logging in creates it,
//! reading it says which account the bearer token names, and logging out
//! ends it. The password rules it applies live in `credentials`.

use axum::extract::State;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::credentials::{MAX_PASSWORD_BYTES, normalize_username, verify_login_password};
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

/// What a login whose password was right finds under the write lock.
enum LoginOpened {
    /// The Session was opened.
    Session(CreateSessionResponse),
    /// The account is disabled, so no Session was opened.
    Disabled,
    /// The account is gone, so no Session was opened.
    Gone,
}

impl CreateSessionResponse {
    /// Open the Session for an existing account, replacing the one it had,
    /// and record the login, unless the account is disabled or gone.
    /// `username` is the one the account's row holds.
    ///
    /// The account is read under the write lock that opens the Session, so
    /// an owner disabling it during the password check, whose write ends
    /// every Session the account has, cannot be followed by a Session that
    /// outlives the disable and works again once the account is enabled
    /// (#2074).
    async fn for_existing_account(
        conn: &mut SqliteConnection,
        account_id: i64,
        username: String,
        app: Option<&ConnectingApp>,
    ) -> anyhow::Result<LoginOpened> {
        let mut tx = crate::db::begin_write(conn).await?;
        match account_profile::load_account_auth(&mut tx, account_id).await? {
            None => return Ok(LoginOpened::Gone),
            Some(auth) if auth.disabled => return Ok(LoginOpened::Disabled),
            Some(_) => {}
        }
        let token = session_tokens::open_session(&mut tx, account_id, &username, app).await?;
        account_profile::record_login(&mut tx, account_id).await?;
        tx.commit().await?;
        Ok(LoginOpened::Session(CreateSessionResponse {
            token,
            account_id,
            username,
        }))
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
const INVALID_LOGIN: &str = "invalid username or password";

/// Log in: verify a local username and password and answer the Session, a
/// `201 Created` whose `Location` is the singleton itself.
///
/// A refused login waits about a second, then answers
/// `invalid-credentials`, whatever was wrong: a username nobody holds, a
/// wrong or empty password, a disabled account. A login with the right
/// password does not wait.
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
        crate::problem::openapi::InvalidCredentials
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
    // Before the lookup, so the answer is the same whether or not the
    // username names an account.
    if req.password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::validation("password is too long"));
    }
    let app = crate::server::connecting_app(&headers);
    // `log_in` has given its connection back by the time it answers, so a
    // refusal waits holding none.
    match log_in(&state, &username, &req.password, app.as_ref()).await? {
        Some(body) => Ok(Created {
            location: "/v1/session".to_string(),
            body,
        }),
        None => Err(state.refusal_wait.refuse(INVALID_LOGIN).await),
    }
}

/// Check `username` and `password` and open the account's Session.
/// `Ok(None)` when the login is refused; the Audit Trail records why.
async fn log_in(
    state: &AppState,
    username: &str,
    password: &str,
    app: Option<&ConnectingApp>,
) -> Result<Option<CreateSessionResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let account_id = account_profile::lookup_account_by_username(&mut conn, username).await?;
    // Refused logins for usernames nobody holds are kept for a fixed time,
    // and a login is when the old ones go.
    audit_trail::trim_refused_logins(&mut conn).await?;

    // The username the account's row holds. An account deleted since the
    // lookup above is a username nobody holds by now, so it takes the same
    // branch and gets the same Audit Trail entry.
    let account = match account_id {
        Some(id) => account_profile::username_for_account(&mut conn, id)
            .await?
            .map(|stored| (id, stored)),
        None => None,
    };
    let Some((account_id, stored_username)) = account else {
        // Argon2 runs against a dummy hash, as it runs against the real one
        // below, and the connection goes back to the pool for the length of
        // the run.
        drop(conn);
        let _ = verify_login_password(None, password).await?;
        let mut conn = state.db.acquire().await?;
        audit_trail::record_refused_login(
            &mut conn,
            username,
            None,
            AuditReason::UnknownUsername,
            app,
        )
        .await?;
        return Ok(None);
    };

    // The Demo Account cannot be entered while it is being built: until the
    // build ends it holds part of its data, and a failed build removes it.
    // The login card is told there is no Demo Account, so the answer here is
    // the one for a username that does not exist.
    if account_id == account_profile::DEMO_ACCOUNT_ID && state.demo_build.is_building() {
        return Ok(None);
    }

    let account = Some((account_id, stored_username.as_str()));
    let password_hash = account_profile::load_password_hash(&mut conn, account_id).await?;
    // Argon2 runs on the blocking thread pool with no pooled connection held,
    // so a burst of logins neither stalls the async workers nor empties the
    // pool. An account changed meanwhile is read afresh below.
    drop(conn);
    let verified = verify_login_password(password_hash.as_deref(), password).await?;
    let mut conn = state.db.acquire().await?;
    if !verified {
        audit_trail::record_refused_login(
            &mut conn,
            username,
            account,
            AuditReason::WrongPassword,
            app,
        )
        .await?;
        return Ok(None);
    }

    // A disabled account gets the answer every refused login gets. Its own
    // answer would come only after the right password, so a guesser would
    // learn which guess was right, and that password works again once the
    // owner enables the account. The Audit Trail still says why.
    match CreateSessionResponse::for_existing_account(
        &mut conn,
        account_id,
        stored_username.clone(),
        app,
    )
    .await?
    {
        LoginOpened::Session(body) => Ok(Some(body)),
        LoginOpened::Disabled => {
            audit_trail::record_refused_login(
                &mut conn,
                username,
                account,
                AuditReason::AccountDisabled,
                app,
            )
            .await?;
            Ok(None)
        }
        LoginOpened::Gone => Ok(None),
    }
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
