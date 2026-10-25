//! [`ApiError`], the one failure every route answers with, and how it
//! becomes a problem document, a log line and a status: every `From` that
//! gives another module's error its problem type is here.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::problem::{Problem, ProblemType};

/// A failure, as one of the registered problem types (`docs/architecture/rules/http-api.md`).
///
/// A variant names what went wrong, never a status: the status, the `type`
/// URL and the title come from the type's declaration in [`crate::problem`],
/// so no call site picks a status by hand. That declaration also names the
/// variants that answer the type, and generates [`Self::problem_type`] from
/// them, so a new variant fails to compile until a type names it. The
/// `String` a variant carries is the `detail`, one sentence written for the
/// person reading it. A `500 Internal Server Error` carries the whole error
/// chain for the log and shows the client a fixed sentence.
#[derive(Debug)]
pub enum ApiError {
    /// `422 Unprocessable Entity`: fields that parsed and then broke a rule, every one of them.
    ValidationFailed(Vec<String>),
    /// `400 Bad Request`: the request cannot be read at all.
    MalformedBody(String),
    /// `400 Bad Request`: a line of an import batch is not the JSON Lines the server
    /// reads. A `malformed-body` that also carries the line as `line`.
    MalformedImportLine {
        /// The sentence, naming the line as a line of the batch.
        detail: String,
        /// The line of the batch, counted from 1 with blank lines included.
        line: usize,
    },
    /// `422 Unprocessable Entity`: lines of an import batch were read and broke a rule. A
    /// `validation-failed` that also carries the first such line as `line`.
    InvalidImportLines {
        /// One sentence per rule, naming the lines as lines of the batch.
        errors: Vec<String>,
        /// The first line that broke a rule, counted from 1 with blank
        /// lines included.
        line: usize,
    },
    /// `415 Unsupported Media Type`: `Content-Type` absent or not one the route accepts.
    UnsupportedMediaType(String),
    /// `413 Payload Too Large`: the body is over its cap. It carries no sentence of its own:
    /// every cap answers "the request body is too large", so a client sees
    /// one sentence for one condition whichever cap refused the body and
    /// however the body declared its size. The problem type's page names
    /// each cap.
    PayloadTooLarge,
    /// `401 Unauthorized`: a username, password or current-password check failed.
    InvalidCredentials(String),
    /// `401 Unauthorized`: no usable bearer token (none sent, or a header that
    /// is not `Bearer <token>`).
    AuthenticationRequired(String),
    /// `401 Unauthorized`: a bearer token was sent and refused because it is
    /// unknown, expired, or its account is gone. It is sent as an
    /// `authentication-required` whose challenge names
    /// `error="invalid_token"` (RFC 6750, section 3.1).
    TokenRefused(String),
    /// `409 Conflict`: the username already belongs to an account.
    UsernameTaken(String),
    /// `409 Conflict`: a Contact Group, Message Tag or Saved Search name collides.
    NameTaken(String),
    /// `403 Forbidden`: the demo account refuses a destructive operation.
    DemoAccountProtected(String),
    /// `403 Forbidden`: the route belongs to the owner.
    NotTheOwner(String),
    /// `403 Forbidden`: the server does not let strangers create their own account.
    RegistrationClosed(String),
    /// `403 Forbidden`: the credential is valid but lacks the scope the route needs.
    InsufficientScope(String),
    /// `403 Forbidden`: the account may not log in or act.
    AccountDisabled(String),
    /// `422 Unprocessable Entity`: the search language refused a word.
    SearchQueryInvalid {
        /// The sentence, naming the word and the list.
        detail: String,
        /// The `word:` the query used, when there is one.
        word: Option<&'static str>,
        /// A word the language does have, when one is close.
        did_you_mean: Option<&'static str>,
    },
    /// `409 Conflict`: the resource is not in a state that allows the operation.
    StateConflict(String),
    /// `409 Conflict`: a country picked for a phone number gives it the `+` form
    /// another identity already holds, and the request did not ask to merge.
    IdentityExists {
        /// The sentence, naming the number, its `+` form and who holds it.
        detail: String,
        /// Who holds the `+` form.
        holder: api_types::IdentityHolder,
    },
    /// `422 Unprocessable Entity`: a part, upload id or completion does not match the upload.
    AssetUploadInvalid(String),
    /// `404 Not Found`: the addressed resource does not exist for this account.
    NotFound(String),
    /// `405 Method Not Allowed`: the path exists but not for this method.
    MethodNotAllowed(String),
    /// `406 Not Acceptable`: `Accept` names nothing the route can produce.
    NotAcceptable(String),
    /// `401 Unauthorized`: the `media_link` in the URL opens nothing here.
    MediaLinkInvalid(String),
    /// `416 Range Not Satisfiable`: the `Range` selects no byte of a file `length` bytes long. The
    /// answer carries `Content-Range: bytes */<length>`.
    RangeNotSatisfiable {
        /// The sentence, naming the range.
        detail: String,
        /// The file's length in bytes.
        length: u64,
    },
    /// `408 Request Timeout`: a request body stopped arriving, or took too long to arrive.
    BodyStalled(String),
    /// `503 Service Unavailable`: the server is stopping and refused work it had not started.
    ServerStopping(String),
    /// `500 Internal Server Error`: unexpected failure. The whole context chain goes to the log;
    /// the client sees a fixed sentence and `about:blank`. One whose chain
    /// holds a busy database (`is_database_busy`) is no failure of the
    /// server: it answers `503 Service Unavailable` (`database-busy`) with
    /// `Retry-After`, and its chain goes on the refusal's `WARN` line.
    Internal(anyhow::Error),
}

impl ApiError {
    /// A validation failure with one sentence.
    pub fn validation(sentence: impl Into<String>) -> Self {
        Self::ValidationFailed(vec![sentence.into()])
    }

    /// `Ok` when a check that gathers every broken rule found none, else
    /// `validation-failed` listing them all (`docs/architecture/rules/http-api.md`,
    /// API-52).
    ///
    /// # Errors
    ///
    /// `validation-failed` carrying `errors` when it is not empty.
    pub(crate) fn unless_broken(errors: Vec<String>) -> Result<(), Self> {
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Self::ValidationFailed(errors))
        }
    }

    /// `404` for a `kind` of row this account does not hold, with the detail
    /// "{kind} not found", so the routes that use it word a miss the same way.
    pub(crate) fn not_found(kind: &str) -> Self {
        Self::NotFound(format!("{kind} not found"))
    }

    /// `401` for a credential whose account is gone: the token passed, but no
    /// account row has its id any more. It answers as a credential that names
    /// no account, because to the caller the two are the same.
    pub(crate) fn account_gone() -> Self {
        Self::TokenRefused("account no longer exists".into())
    }

    /// The sentence of a variant that carries nothing else, or `None` for a
    /// variant with fields of its own. The one list of those variants:
    /// [`Self::to_problem`] takes the `detail` from it and `Display` writes
    /// it, both through [`Self::sentence`]. A variant put in the `None` arm
    /// needs an arm of its own in both.
    fn message(&self) -> Option<&str> {
        match self {
            Self::MalformedBody(m)
            | Self::UnsupportedMediaType(m)
            | Self::InvalidCredentials(m)
            | Self::AuthenticationRequired(m)
            | Self::TokenRefused(m)
            | Self::UsernameTaken(m)
            | Self::NameTaken(m)
            | Self::DemoAccountProtected(m)
            | Self::NotTheOwner(m)
            | Self::RegistrationClosed(m)
            | Self::InsufficientScope(m)
            | Self::AccountDisabled(m)
            | Self::StateConflict(m)
            | Self::AssetUploadInvalid(m)
            | Self::NotFound(m)
            | Self::MethodNotAllowed(m)
            | Self::NotAcceptable(m)
            | Self::MediaLinkInvalid(m)
            | Self::BodyStalled(m)
            | Self::ServerStopping(m) => Some(m),
            Self::PayloadTooLarge => Some("the request body is too large"),
            Self::ValidationFailed(_)
            | Self::MalformedImportLine { .. }
            | Self::InvalidImportLines { .. }
            | Self::SearchQueryInvalid { .. }
            | Self::IdentityExists { .. }
            | Self::RangeNotSatisfiable { .. }
            | Self::Internal(_) => None,
        }
    }

    /// The sentence of a variant that the last arm of [`Self::to_problem`]
    /// or `Display` reached. A variant with fields of its own that has no arm
    /// above that one panics here, so the first test that answers or displays
    /// it fails rather than giving an empty sentence.
    fn sentence(&self) -> &str {
        let Some(sentence) = self.message() else {
            unreachable!("{self:?} has fields of its own and needs its own arm");
        };
        sentence
    }

    /// The status this failure answers.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.problem_type()
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, ProblemType::status)
    }

    /// The problem document this failure answers with. Every failure is
    /// logged here, once: an internal error at `ERROR` with its whole chain,
    /// which the document says nothing of, and a refused request, a busy
    /// database's included, with `log_refusal`'s line.
    #[must_use]
    pub fn to_problem(&self) -> Problem {
        let request_id = crate::request_id::current();
        let Some(kind) = self.problem_type() else {
            let Self::Internal(err) = self else {
                unreachable!("every variant but Internal has a problem type");
            };
            tracing::error!(error = %error_chain(err), "A request failed with 500 Internal Server Error");
            return Problem {
                kind: crate::problem::INTERNAL_TYPE.to_string(),
                title: "Internal server error".to_string(),
                status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                detail: Some("internal server error".to_string()),
                errors: None,
                request_id,
                word: None,
                did_you_mean: None,
                retry_after: None,
                line: None,
                holder: None,
            };
        };
        let cause = match self {
            Self::Internal(err) => Some(err),
            _ => None,
        };
        log_refusal(kind, cause);
        let mut problem = Problem {
            kind: kind.url(),
            title: kind.title().to_string(),
            status: kind.status().as_u16(),
            detail: None,
            errors: None,
            request_id,
            word: None,
            did_you_mean: None,
            retry_after: None,
            line: None,
            holder: None,
        };
        match self {
            Self::ValidationFailed(errors) => problem.errors = Some(errors.clone()),
            Self::SearchQueryInvalid {
                detail,
                word,
                did_you_mean,
            } => {
                problem.detail = Some(detail.clone());
                problem.word = word.map(str::to_string);
                problem.did_you_mean = did_you_mean.map(str::to_string);
            }
            Self::MalformedImportLine { detail, line } => {
                problem.detail = Some(detail.clone());
                problem.line = Some(*line as u64);
            }
            Self::RangeNotSatisfiable { detail, .. } => problem.detail = Some(detail.clone()),
            Self::IdentityExists { detail, holder } => {
                problem.detail = Some(detail.clone());
                problem.holder = Some(holder.clone());
            }
            Self::InvalidImportLines { errors, line } => {
                problem.errors = Some(errors.clone());
                problem.line = Some(*line as u64);
            }
            // An internal error's answer follows from its type, below.
            Self::Internal(_) => {}
            // The variants that carry only a sentence, which `message` lists.
            _ => problem.detail = Some(self.sentence().to_string()),
        }
        if kind == ProblemType::DatabaseBusy {
            problem.retry_after = Some(crate::problem::DATABASE_BUSY_RETRY_AFTER_SECS);
            problem.detail = Some(format!(
                "the database is busy, so try again in {} seconds",
                crate::problem::DATABASE_BUSY_RETRY_AFTER_SECS
            ));
        }
        problem
    }
}

/// The line a refused request leaves: its problem type's slug and its status,
/// under the request's span, which names the method, the path and the
/// request id: at `WARN` when [`ProblemType::refusal_is_a_warning`], at
/// `INFO` otherwise. The problem's
/// `detail` is never written: it can repeat what a person typed, and a log
/// line holds only ids, counts, routes and outcomes
/// (`docs/architecture/rules/server-log.md`). A refusal whose `cause` is an
/// internal error (a busy database) also carries that error's chain as
/// `error`, as the `ERROR` line of a `500` does, so a lock that never clears
/// names the step that met it.
fn log_refusal(kind: ProblemType, cause: Option<&anyhow::Error>) {
    let status = kind.status();
    let problem = kind.slug();
    let error = cause.map(error_chain);
    let error = error.as_deref();
    if kind.refusal_is_a_warning() {
        tracing::warn!(%problem, status = status.as_u16(), error, "The server refused a request");
    } else {
        tracing::info!(%problem, status = status.as_u16(), error, "The server refused a request");
    }
}

/// The message a person reads when the failure does not travel over HTTP:
/// the CLI, a log line, a test assertion. The client-facing body is built by
/// [`IntoResponse`], which hides an internal error's detail; here the whole
/// context chain is shown, because the reader is the operator.
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(e) => f.write_str(&error_chain(e)),
            Self::ValidationFailed(errors) | Self::InvalidImportLines { errors, .. } => {
                f.write_str(&errors.join("; "))
            }
            Self::SearchQueryInvalid { detail, .. }
            | Self::MalformedImportLine { detail, .. }
            | Self::IdentityExists { detail, .. }
            | Self::RangeNotSatisfiable { detail, .. } => f.write_str(detail),
            // The variants that carry only a sentence, which `message` lists.
            _ => f.write_str(self.sentence()),
        }
    }
}

/// The `WWW-Authenticate` challenge of a `401` whose request sent no Bearer
/// token.
pub(crate) const BEARER_CHALLENGE: &str = r#"Bearer realm="message-crate""#;

/// The `WWW-Authenticate` challenge of a `401` whose request sent a Bearer
/// token that the server refused (RFC 6750, section 3.1).
pub(crate) const REFUSED_TOKEN_CHALLENGE: &str =
    r#"Bearer realm="message-crate", error="invalid_token""#;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let problem = self.to_problem();
        // RFC 9110, section 15.5.17: a `416` says how long the file is.
        let content_range = match &self {
            Self::RangeNotSatisfiable { length, .. } => Some(format!("bytes */{length}")),
            _ => None,
        };
        let status = StatusCode::from_u16(problem.status).expect("a registered status");
        let mut response = (
            status,
            [(header::CONTENT_TYPE, Problem::CONTENT_TYPE)],
            Json(&problem),
        )
            .into_response();
        if let Some(secs) = problem.retry_after {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&secs.to_string()).expect("digits are a valid header value"),
            );
        }
        if let Some(content_range) = content_range {
            response.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&content_range).expect("digits are a valid header value"),
            );
        }
        // RFC 9110, section 15.5.2: every `401` names how to authenticate.
        // RFC 6750, section 3.1: a Bearer token that was sent and refused is
        // `invalid_token`; a request that sent none gets no error code.
        if status == StatusCode::UNAUTHORIZED {
            let challenge = if matches!(self, Self::TokenRefused(_)) {
                REFUSED_TOKEN_CHALLENGE
            } else {
                BEARER_CHALLENGE
            };
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static(challenge),
            );
        }
        response
    }
}

/// SQLite's primary result code `SQLITE_BUSY`. Its extended codes, such as
/// `SQLITE_BUSY_SNAPSHOT`, carry it in their low byte.
const SQLITE_BUSY: i32 = 5;

/// Whether `err`, anywhere in its chain, is a database too busy to take the
/// request: SQLite answered `SQLITE_BUSY` once the busy timeout ran out, or
/// no pool connection came free before sqlx's acquire timeout
/// (`PoolTimedOut`). Both pass once the import batch or the requests holding
/// the database finish, so the request answers `database-busy` rather than
/// `500 Internal Server Error` (#2084, #2124).
pub(crate) fn is_database_busy(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<sqlx::Error>()
            .is_some_and(|sqlx_error| match sqlx_error {
                sqlx::Error::PoolTimedOut => true,
                sqlx::Error::Database(db) => db
                    .code()
                    .and_then(|code| code.parse::<i32>().ok())
                    .is_some_and(|code| code & 0xff == SQLITE_BUSY),
                _ => false,
            })
    })
}

/// An error's whole context chain on one line, outermost first, so the log
/// shows the sqlx or io failure under the step that hit it. `Display` alone
/// would print only the outermost message.
///
/// A source the error before it already ends with, after a `": "`, is left
/// out, because it says nothing new: `sqlx::Error::Database` writes the
/// database's message after "error returned from database:" and gives that
/// same message again as its source (#2675). A cause whose words only happen
/// to end the step's phrase ("conversation" under "stage conversation") is
/// kept.
pub(crate) fn error_chain(err: &anyhow::Error) -> String {
    let mut line = String::new();
    let mut previous: Option<String> = None;
    for link in err.chain() {
        let text = link.to_string();
        let repeated = previous
            .as_deref()
            .is_some_and(|p| p == text || p.ends_with(&format!(": {text}")));
        if repeated {
            continue;
        }
        if !line.is_empty() {
            line.push_str(": ");
        }
        line.push_str(&text);
        previous = Some(text);
    }
    line
}

impl From<crate::db::imports::ImportLookupError> for ApiError {
    fn from(e: crate::db::imports::ImportLookupError) -> Self {
        match e {
            crate::db::imports::ImportLookupError::NotFound { import_id } => {
                Self::NotFound(format!("import {import_id} not found for this account"))
            }
            crate::db::imports::ImportLookupError::InvalidRun { message } => {
                Self::StateConflict(message)
            }
            err @ crate::db::imports::ImportLookupError::StageMove { .. } => {
                Self::StateConflict(err.to_string())
            }
            crate::db::imports::ImportLookupError::Db(err) => Self::Internal(err.into()),
        }
    }
}

impl From<crate::db::imports::StartImportError> for ApiError {
    fn from(e: crate::db::imports::StartImportError) -> Self {
        match e {
            err @ crate::db::imports::StartImportError::AlreadyActive => {
                // One wording for the 409, shared with the CLI paths that
                // surface the same error through anyhow.
                Self::StateConflict(err.to_string())
            }
            crate::db::imports::StartImportError::Db(err) => Self::Internal(err),
        }
    }
}

/// The one place an asset store failure gets its status
/// (`docs/architecture/rules/http-api.md`, API-26 and API-33).
impl From<crate::assets_api::AssetError> for ApiError {
    fn from(e: crate::assets_api::AssetError) -> Self {
        use crate::assets_api::AssetError;
        match e {
            err @ (AssetError::Mismatch { .. } | AssetError::Invalid(_)) => {
                Self::AssetUploadInvalid(err.to_string())
            }
            // The upload is busy, or the account's open uploads are at their
            // cap. Neither request is wrong: each meets a resource in the
            // wrong state.
            err @ (AssetError::Locked | AssetError::TooManyUploads) => {
                Self::StateConflict(err.to_string())
            }
            err @ AssetError::UploadNotFound => Self::NotFound(err.to_string()),
            AssetError::Internal(err) => Self::Internal(err),
        }
    }
}

/// The one place a sender's import failure gets its status: only a line that
/// is not JSON cannot be read (`400`); everything else was read and broke a
/// rule (`422`). A failure on one line carries it as `line`, a line of the
/// batch, so a client can map it back to a file of its own.
impl From<crate::imports_api::ImportFailure> for ApiError {
    fn from(e: crate::imports_api::ImportFailure) -> Self {
        use crate::imports_api::ImportFailure;
        let detail = e.batch_sentence();
        match (&e, e.line()) {
            (ImportFailure::NotJson { .. }, Some(line)) => {
                Self::MalformedImportLine { detail, line }
            }
            (_, Some(line)) => Self::InvalidImportLines {
                errors: vec![detail],
                line,
            },
            (_, None) => Self::validation(detail),
        }
    }
}

/// An import failure the sender can fix keeps its own status; anything else
/// is a `500` with its cause in the log.
impl From<crate::imports_api::ImportError> for ApiError {
    fn from(e: crate::imports_api::ImportError) -> Self {
        match e {
            crate::imports_api::ImportError::Rejected { failure, .. } => failure.into(),
            crate::imports_api::ImportError::Run(err) => err.into(),
            crate::imports_api::ImportError::AccountGone => Self::account_gone(),
            crate::imports_api::ImportError::Internal(err) => Self::Internal(err),
        }
    }
}

impl From<crate::db::saved_searches::SavedSearchError> for ApiError {
    fn from(e: crate::db::saved_searches::SavedSearchError) -> Self {
        use crate::db::saved_searches::SavedSearchError;
        match e {
            SavedSearchError::BadRequest(m) => Self::validation(m),
            SavedSearchError::Query(e) => e.into(),
            SavedSearchError::NotFound => Self::not_found("saved search"),
            SavedSearchError::NameTaken(m) | SavedSearchError::NoFreeName(m) => Self::NameTaken(m),
            SavedSearchError::Internal(e) => Self::Internal(e),
        }
    }
}

impl From<crate::db::named_membership::MembershipError> for ApiError {
    fn from(e: crate::db::named_membership::MembershipError) -> Self {
        use crate::db::named_membership::MembershipError;
        match e {
            MembershipError::BadRequest(m) => Self::validation(m),
            MembershipError::NotFound(m) => Self::NotFound(m),
            MembershipError::SetNotFound(label) => Self::not_found(label),
            MembershipError::NameTaken(m) => Self::NameTaken(m),
            MembershipError::Internal(e) => Self::Internal(e),
        }
    }
}

impl From<crate::db::SearchListError> for ApiError {
    fn from(e: crate::db::SearchListError) -> Self {
        match e {
            crate::db::SearchListError::Query(err) => err.into(),
            crate::db::SearchListError::Internal(err) => Self::Internal(err),
        }
    }
}

impl From<crate::db::conversation_messages::MessagePageError> for ApiError {
    fn from(e: crate::db::conversation_messages::MessagePageError) -> Self {
        use crate::db::conversation_messages::MessagePageError;
        match e {
            MessagePageError::Invalid(sentence) => Self::validation(sentence),
            MessagePageError::Internal(err) => Self::Internal(err),
        }
    }
}

impl From<crate::db::address_book::LoadError> for ApiError {
    fn from(e: crate::db::address_book::LoadError) -> Self {
        use crate::db::address_book::LoadError;
        match e {
            LoadError::Refused(reasons) => Self::ValidationFailed(reasons),
            LoadError::Failed(cause) => Self::Internal(cause.context("load address book")),
        }
    }
}

impl From<crate::db::address_book::VcardError> for ApiError {
    fn from(e: crate::db::address_book::VcardError) -> Self {
        use crate::db::address_book::VcardError;
        match e {
            VcardError::UnknownCharset(unknown) => Self::validation(unknown.to_string()),
            VcardError::Failed(cause) => Self::Internal(cause.context("convert vCard file")),
        }
    }
}

impl From<crate::accounts_api::ProfileUpdateError> for ApiError {
    fn from(e: crate::accounts_api::ProfileUpdateError) -> Self {
        use crate::accounts_api::ProfileUpdateError;
        match e {
            err @ ProfileUpdateError::UnknownTimeZone(_) => Self::validation(err.to_string()),
            ProfileUpdateError::Country(error) => error.into(),
            ProfileUpdateError::Db(err) => Self::Internal(err),
        }
    }
}

impl From<crate::contacts_api::ContactEditError> for ApiError {
    fn from(e: crate::contacts_api::ContactEditError) -> Self {
        use crate::contacts_api::ContactEditError;
        match e {
            ContactEditError::Refused(message) => Self::validation(message),
            ContactEditError::Country(error) => error.into(),
            ContactEditError::Failed(cause) => Self::Internal(cause),
        }
    }
}

impl From<crate::identity_country::CountryError> for ApiError {
    fn from(e: crate::identity_country::CountryError) -> Self {
        use crate::identity_country::CountryError;
        match e {
            CountryError::Refused(message) => Self::validation(message),
            CountryError::Exists { detail, holder } => Self::IdentityExists { detail, holder },
            CountryError::Conflict(message) => Self::StateConflict(message),
            CountryError::Failed(cause) => Self::Internal(cause),
        }
    }
}

impl From<crate::search::QueryError> for ApiError {
    fn from(e: crate::search::QueryError) -> Self {
        Self::SearchQueryInvalid {
            detail: e.message,
            word: e.field,
            did_you_mean: e.did_you_mean,
        }
    }
}

impl From<crate::text_caps::TooLong> for ApiError {
    fn from(e: crate::text_caps::TooLong) -> Self {
        Self::validation(e.to_string())
    }
}

impl From<crate::text_caps::RequiredTextError> for ApiError {
    fn from(e: crate::text_caps::RequiredTextError) -> Self {
        Self::validation(e.to_string())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

/// Run `f` on the blocking thread pool and answer its result. A task that
/// panics or is canceled answers an internal error naming `task`; an error
/// `f` returns becomes the [`ApiError`] it converts to.
pub(crate) async fn run_blocking<T, E>(
    task: &'static str,
    f: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, ApiError>
where
    T: Send + 'static,
    E: Into<ApiError> + Send + 'static,
{
    crate::request_id::spawn_blocking_in_current_span(f)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{task} task: {e}")))?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests;
