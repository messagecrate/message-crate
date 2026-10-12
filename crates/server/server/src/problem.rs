//! The registry of problem types: what each kind of failure is, the status it
//! answers, the page that describes it (`docs/architecture/http-api.md`).
//!
//! This is the one place a type is declared. Each declaration names the
//! [`crate::server::ApiError`] variants that answer the type, and
//! [`crate::server::ApiError::problem_type`] is generated from those names.
//! The OpenAPI document describes the body through [`Problem`], and
//! `dump-error-docs` ([`crate::error_docs`]) writes one page per type under
//! `docs/src/content/docs/docs/developer/reference/errors/` from the same
//! declarations, so nothing has to be kept in step by hand.

use axum::http::StatusCode;
pub use message_crate_api_types::Problem;

use crate::server::{ApiError, is_database_busy};

/// Where the type pages are published; each `type` URL is this plus the slug.
pub const ERRORS_URL: &str = "https://messagecrate.app/docs/developer/reference/errors/";

/// The `type` of a `500 Internal Server Error`: no page could say anything a
/// reader could act on.
pub const INTERNAL_TYPE: &str = "about:blank";

/// Declares [`ProblemType`], its [`ProblemType::ALL`], each type's slug,
/// status and title, and [`ApiError::problem_type`], from one list, written
/// `Variant => ("slug", STATUS_CODE_CONSTANT, "Title") from <pattern>,`,
/// where `<pattern>` matches the [`ApiError`] variants that answer the type,
/// with an `if` guard where the variant alone does not decide it. A type
/// cannot be declared and left out of `ALL`: the error pages
/// ([`crate::error_docs`]) and the OpenAPI problem types
/// ([`crate::openapi::shared_parts`]) walk `ALL`, and a type missing from it
/// would ship with neither. Nor can a type be declared without its slug,
/// status, title or the variants that answer it, because the macro
/// requires all four in each entry. An [`ApiError`] variant no entry matches
/// fails to compile, because `problem_type` must match every variant.
/// [`ApiError::Internal`] is the one failure with no type: its arm comes
/// after the list's, so an entry with a guard can still take an internal
/// error, as `database-busy` does. The list's arms are matched in order, so
/// a variant an entry matches without a guard belongs to that entry: a later
/// entry that matches it again is never reached. rustc warns of that arm as
/// an unreachable pattern, and CI's Clippy run at `-D warnings` refuses it.
/// A guarded entry passes on what its guard refuses, so the arms after it
/// still see that variant. A new type also needs its [`ProblemType::page`]
/// arm, which the compiler demands.
macro_rules! problem_types {
    ($(
        $(#[$doc:meta])*
        $name:ident => ($slug:literal, $status:ident, $title:literal)
            from $answers:pat $(if $guard:expr)?,
    )*) => {
        /// One registered kind of failure.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ProblemType {
            $($(#[$doc])* $name,)*
        }

        impl ProblemType {
            /// Every registered type, in the order the docs index lists them:
            /// the order they are declared in.
            pub const ALL: [Self; [$(stringify!($name)),*].len()] = [$(Self::$name),*];

            /// The last segment of the `type` URL and the page's file name.
            #[must_use]
            pub const fn slug(self) -> &'static str {
                match self {
                    $(Self::$name => $slug,)*
                }
            }

            /// The status every problem of this type answers.
            #[must_use]
            pub const fn status(self) -> StatusCode {
                match self {
                    $(Self::$name => StatusCode::$status,)*
                }
            }

            /// The fixed, human-readable name every problem of this type carries.
            #[must_use]
            pub const fn title(self) -> &'static str {
                match self {
                    $(Self::$name => $title,)*
                }
            }
        }

        impl ApiError {
            /// The registered type, or `None` for an internal error.
            #[must_use]
            pub fn problem_type(&self) -> Option<ProblemType> {
                match self {
                    $($answers $(if $guard)? => Some(ProblemType::$name),)*
                    ApiError::Internal(_) => None,
                }
            }
        }
    };
}

problem_types! {
    /// A query parameter, path segment or body field parsed and then broke a rule.
    ValidationFailed => ("validation-failed", UNPROCESSABLE_ENTITY, "Validation failed")
        from ApiError::ValidationFailed(_) | ApiError::InvalidImportLines { .. },
    /// The request cannot be read: not valid JSON or JSONL, or a body that failed to arrive.
    MalformedBody => ("malformed-body", BAD_REQUEST, "Malformed body")
        from ApiError::MalformedBody(_) | ApiError::MalformedImportLine { .. },
    /// `Content-Type` is absent or not one the route accepts.
    UnsupportedMediaType => ("unsupported-media-type", UNSUPPORTED_MEDIA_TYPE, "Unsupported media type")
        from ApiError::UnsupportedMediaType(_),
    /// The body is over the configured cap.
    PayloadTooLarge => ("payload-too-large", PAYLOAD_TOO_LARGE, "Payload too large")
        from ApiError::PayloadTooLarge,
    /// A username, password, or current-password check failed.
    InvalidCredentials => ("invalid-credentials", UNAUTHORIZED, "Invalid credentials")
        from ApiError::InvalidCredentials(_),
    /// The bearer token is missing, malformed, unknown or expired.
    AuthenticationRequired => ("authentication-required", UNAUTHORIZED, "Authentication required")
        from ApiError::AuthenticationRequired(_) | ApiError::TokenRefused(_),
    /// A new account's username collides with an existing one.
    UsernameTaken => ("username-taken", CONFLICT, "Username taken")
        from ApiError::UsernameTaken(_),
    /// A Contact Group, Message Tag, or Saved Search name collides.
    NameTaken => ("name-taken", CONFLICT, "Name taken")
        from ApiError::NameTaken(_),
    /// The demo account refuses a destructive operation.
    DemoAccountProtected => ("demo-account-protected", FORBIDDEN, "Demo account protected")
        from ApiError::DemoAccountProtected(_),
    /// The account is not the owner.
    NotTheOwner => ("not-the-owner", FORBIDDEN, "Not the owner")
        from ApiError::NotTheOwner(_),
    /// The server does not let strangers create their own account.
    RegistrationClosed => ("registration-closed", FORBIDDEN, "Registration closed")
        from ApiError::RegistrationClosed(_),
    /// The token is valid but lacks the scope the route needs.
    InsufficientScope => ("insufficient-scope", FORBIDDEN, "Insufficient scope")
        from ApiError::InsufficientScope(_),
    /// The account exists but may not log in or act.
    AccountDisabled => ("account-disabled", FORBIDDEN, "Account disabled")
        from ApiError::AccountDisabled(_),
    /// The search language refused a word.
    SearchQueryInvalid => ("search-query-invalid", UNPROCESSABLE_ENTITY, "Search query invalid")
        from ApiError::SearchQueryInvalid { .. },
    /// The resource is not in a state that allows the operation.
    StateConflict => ("state-conflict", CONFLICT, "State conflict")
        from ApiError::StateConflict(_),
    /// A country picked for a phone number gives it a `+` form another
    /// identity already holds, and the request did not ask to merge them.
    IdentityExists => ("identity-exists", CONFLICT, "Identity exists")
        from ApiError::IdentityExists { .. },
    /// A part number, upload id, or completion does not match the upload.
    AssetUploadInvalid => ("asset-upload-invalid", UNPROCESSABLE_ENTITY, "Asset upload invalid")
        from ApiError::AssetUploadInvalid(_),
    /// The addressed resource does not exist for this account.
    NotFound => ("not-found", NOT_FOUND, "Not found")
        from ApiError::NotFound(_),
    /// The path exists and the method does not.
    MethodNotAllowed => ("method-not-allowed", METHOD_NOT_ALLOWED, "Method not allowed")
        from ApiError::MethodNotAllowed(_),
    /// `Accept` names nothing this route can produce.
    NotAcceptable => ("not-acceptable", NOT_ACCEPTABLE, "Not acceptable")
        from ApiError::NotAcceptable(_),
    /// A media link that is malformed, for another asset, expired, or minted
    /// by a Session that has ended.
    MediaLinkInvalid => ("media-link-invalid", UNAUTHORIZED, "Media link invalid")
        from ApiError::MediaLinkInvalid(_),
    /// A `Range` that selects no byte of the file.
    RangeNotSatisfiable => ("range-not-satisfiable", RANGE_NOT_SATISFIABLE, "Range not satisfiable")
        from ApiError::RangeNotSatisfiable { .. },
    /// An upload body stopped arriving, or took too long to arrive.
    UploadStalled => ("upload-stalled", REQUEST_TIMEOUT, "Upload stalled")
        from ApiError::UploadStalled(_),
    /// The server is stopping and refused work it had not started.
    ServerStopping => ("server-stopping", SERVICE_UNAVAILABLE, "Server stopping")
        from ApiError::ServerStopping(_),
    /// The database stayed busy for longer than the request could wait.
    DatabaseBusy => ("database-busy", SERVICE_UNAVAILABLE, "Database busy")
        from ApiError::Internal(err) if is_database_busy(err),
}

/// The seconds a `database-busy` answer's `Retry-After` asks a client to
/// wait before it sends the request again.
pub const DATABASE_BUSY_RETRY_AFTER_SECS: u64 = 5;

impl ProblemType {
    /// Whether the line a request refused with this type leaves in the
    /// server's log is a warning (`docs/architecture/server-log.md`): true
    /// for the types that answer `401 Unauthorized` and `403 Forbidden`,
    /// because a run of them is what a password guesser or an API token that
    /// lost its rights looks like, and for `database-busy`, because a run of
    /// it says the server has more work than its database keeps up with.
    /// The owner's Logs panel opens at warnings and up. Every other refusal
    /// is logged at `INFO`.
    #[must_use]
    pub fn refusal_is_a_warning(self) -> bool {
        self == Self::DatabaseBusy
            || matches!(
                self.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            )
    }

    /// The `type` URL: the page's address.
    #[must_use]
    pub fn url(self) -> String {
        format!("{ERRORS_URL}{}", self.slug())
    }

    /// The page's text: when the server answers this, and what to do about it.
    /// Markdown paragraphs, no heading; the generator adds the frontmatter.
    #[must_use]
    pub fn page(self) -> String {
        match self {
            Self::ValidationFailed => "A query parameter, path segment or body field was read and then broke a rule: a `limit` of zero, an id that is not a number, a name that is blank or too long, an unknown `sort` key or `status` value, a required parameter or body field that is missing or blank, a query parameter the route does not take.\n\n\
`errors` lists every rule the request broke, one sentence each, not only the first. Fix each one and send the request again.\n\n\
For an import batch, a line that is JSON and breaks a rule of message-ir (a wrong field, a message before any header, a message without a `guid`, an old `schema_version`) answers this type, and `line` carries the first such line of the request body, counted from 1 with blank lines included. It is a line of the batch, not of any file: a client that packed the batch from several files turns it into the file and line it came from. An unsafe attachment path and a stated SHA-256 that is not 64 hex digits answer this type too, and `line` carries the line of the message the attachment is on. A batch never carries an attachment's bytes, so its `path` names no file on the server: only a `digest_sha256` already uploaded through `/v1/assets` finds the Asset. An empty batch answers this type without `line`.".to_string(),
            Self::MalformedBody => "The request could not be read at all: the body is not valid JSON, an import line is not JSON or not UTF-8, or the body failed to arrive. Nothing was parsed, so nothing is reported field by field; `detail` says where reading stopped.\n\n\
For an import batch, `line` carries the line of the request body that could not be read, counted from 1 with blank lines included. It is a line of the batch, not of any file: a client that packed the batch from several files turns it into the file and line it came from.".to_string(),
            Self::UnsupportedMediaType => "The request's `Content-Type` is absent or not one this route accepts. An import body is `application/x-ndjson` or `application/jsonl`; a JSON route takes `application/json`. Send the right header with the same body.".to_string(),
            Self::PayloadTooLarge => format!(
                "The body is over the server's configured cap, whether announced by `Content-Length` or discovered while reading. `PUT /v1/assets/{{sha256}}` caps at the attachment size limit, which the owner sets in Server Settings and `GET /v1/server` reports as `asset_max_bytes`. Each part of a multipart upload caps at the part size the upload was given when it started. Every other cap is fixed in the server: `POST /v1/session`, `POST /v1/accounts` and `POST /v1/server/claim` at {}, any other JSON body at {}, an address book loaded with `POST /v1/contacts` at {}, an import batch at {}, the most the desktop app sends in one, and any other body at {}. Send less, or, for an attachment, have the owner raise the limit.",
                byte_size(crate::server::MAX_AUTH_BODY_BYTES),
                byte_size(crate::server::MAX_JSON_BODY_BYTES),
                byte_size(crate::contacts_api::address_book::MAX_ADDRESS_BOOK_BYTES),
                byte_size(message_crate_api_types::MAX_IMPORT_BATCH_BYTES),
                byte_size(crate::server::MAX_REQUEST_BODY_BYTES),
            ),
            Self::InvalidCredentials => "The username or password did not match an account, the account is disabled, or the current password given to confirm deleting an account or changing the owner's password was wrong. The server does not say which. A login to a disabled account answers this whether the password was right or not, so a guesser learns nothing. Check both and try again, or ask the owner whether the account is disabled. The server waits about a second before it answers each refused attempt, so a wrong password costs a moment before the next try. That wait is not protection for a Message Crate reachable from the internet: that is the job of the proxy in front of it.".to_string(),
            Self::AuthenticationRequired => "The request carried no usable credential: the `Authorization: Bearer <token>` header is missing, malformed, unknown or expired. Log in again, or issue a new API token, and send the new token.\n\n\
Every `401 Unauthorized` answer carries `WWW-Authenticate: Bearer realm=\"message-crate\"`. When the request sent a Bearer token and the server refused it, the challenge adds `error=\"invalid_token\"`.".to_string(),
            Self::UsernameTaken => "The username already belongs to an account on this server. Usernames are compared ignoring case. `demo` belongs to the Demo Account even while that account is absent. Pick another.".to_string(),
            Self::NameTaken => "A Contact Group, Message Tag or Saved Search with this name already exists for the account. Names are compared ignoring case. Pick another, or rename the existing one.".to_string(),
            Self::DemoAccountProtected => "The Demo Account refuses this operation, because it exists to be looked at and reset rather than changed. `detail` says what it refused. Log in as another account, or run `reset-demo` on the server to restore the Demo Data. The owner can also delete the Demo Account.".to_string(),
            Self::NotTheOwner => "This route belongs to the owner: creating accounts, changing server settings, or anything else only the owner may do. Ask the owner to do it, or to give you what you need.".to_string(),
            Self::RegistrationClosed => "This Message Crate does not let visitors create their own account: its owner has not opened registration, or nobody has claimed it yet. Ask the owner for an account.".to_string(),
            Self::InsufficientScope => "The credential was accepted but may not do this. An API token carries import and export permissions and never a logged-in session's full access; an account may be restricted from import, export or deletion by the owner. Use a session, a token with the right scope, or ask the owner.".to_string(),
            Self::AccountDisabled => "The account exists but the owner has disabled it, so an API token it already has may not act. Disabling the account also ended its Session, so a Session token it held answers `authentication-required`. A login to a disabled account answers `invalid-credentials` instead, so a refused login does not tell a guesser that the password was right. Ask the owner to enable it.".to_string(),
            Self::SearchQueryInvalid => "The search language refused the query. `detail` names the word and the list it was used on; `word` carries the word, and `did_you_mean` a word the language does have when one is close. The query language is documented in the search reference.".to_string(),
            Self::StateConflict => "The resource is not in a state that allows the operation: an import that is no longer running, already has a live run, or cannot move from its stage to the one asked for, a Message Crate that already has an owner, a delete on something not yet trashed, an asset upload that another request to it is still writing, an asset upload started by an account that already has as many in progress as it may have open at once, or a country picked on a contact for a number whose `+` form is one of the account's own identities, which is joined from the account's identities because the merge makes it the account's own. `detail` says which. Read the resource's current state and choose the operation it allows.".to_string(),
            Self::IdentityExists => "The country picked for a phone number written without its `+` code gives it a `+` form that another identity on the same service already holds, so the two are one number. `detail` names that identity and who holds it, and `holder` says the same for a program: `{\"kind\": \"contact\", \"contact_id\": …, \"name\": …}` for a contact (the name is empty for a contact with no name), `{\"kind\": \"account\"}` for one of the account's own identities, or `{\"kind\": \"nobody\"}`. Nothing changed. To join the two, send the same request again with `merge` set to `true`: every conversation, message and reaction of the number written without its `+` moves to the identity that holds the `+` form, and a one-to-one conversation with each becomes one. A number on a contact whose `+` form is one of the account's own identities is never joined from the contact: `PATCH /v1/contacts/{id}` answers `409 Conflict` (`state-conflict`) for it, and `PATCH /v1/accounts/{id}` joins it once the number is one of the account's identities too.".to_string(),
            Self::AssetUploadInvalid => "Something about the upload does not match what the server expected: the bytes do not hash to the claimed SHA-256, a part number is out of range or a part the wrong length, or a completion names parts that never arrived. `detail` says which. Start the upload again. An upload id that names no upload answers `not-found`, and a server that cannot store the file answers `500`.".to_string(),
            Self::NotFound => "No resource at that address exists for this account. An id that belongs to another account answers this too, so an unknown id and a forbidden one look the same, with one exception: the `{id}` of `/v1/accounts/{id}` itself. A caller who is neither the owner nor the account it names gets `403 Forbidden` there, whether or not the account exists. An id nested under it, such as another account's API token, still answers this.".to_string(),
            Self::MethodNotAllowed => "The path exists but does not take this method. The OpenAPI document lists each route's methods.".to_string(),
            Self::NotAcceptable => "The request's `Accept` header named nothing this route can produce. Every `/v1` route but the five that answer a file (the asset download, its preview, its thumbnail, the address book export and a file of the server's log) answers `application/json`, and a failure `application/problem+json`; send `Accept: application/json`, `*/*`, or no `Accept` at all.".to_string(),
            Self::MediaLinkInvalid => format!(
                "The `media_link` in the URL opens nothing here: it is not a media link, it was made for another asset or another account, it expired, or the Session that made it has ended. A media link lives {} minutes and no longer than the Session that made it. `detail` says which. Make a new one with `POST /v1/assets/{{sha256}}/media-links` and load the URL it answers; a request that sends `Authorization: Bearer <token>` needs no link.",
                crate::assets_api::media_links::MEDIA_LINK_TTL.as_secs() / 60
            ),
            Self::RangeNotSatisfiable => "The request's `Range` selects no byte of the file: it starts at or past the end, or asks for a suffix of no bytes. The `Content-Range` header (`bytes */<length>`) says how long the file is. Ask for a range inside it, or send no `Range` to get the whole file. A `Range` the server does not serve as one range, such as several ranges or another unit, is not refused: it answers the whole file.".to_string(),
            Self::UploadStalled => "The body of a request stopped arriving: no byte of it arrived for the server's stall limit, or it was still arriving at the server's limit for a whole body. The server stopped reading it and stored none of it. `detail` says which limit it met. Every body is held to the limits, so any route that reads one can answer this; an import batch, a `PUT /v1/assets/{sha256}` body and a part of a multipart upload are the bodies large enough to meet them.\n\n\
The connection failed, not the data: nothing in the body is wrong. The limits are 60 seconds with no byte and 300 seconds for a whole body unless the server's config file sets `[server] upload_stall_secs` and `upload_piece_secs`, and `GET /v1/server` states both, in seconds, as `upload_stall_secs` and `upload_piece_secs`. Send the same request again once the connection is better. The desktop app does not send it again by itself: running the import again continues it from where it stopped.".to_string(),
            Self::ServerStopping => "The server was told to stop (Ctrl-C, `docker stop`, or the desktop app closing) while this request waited to start, so it refused the request rather than start work it would make the stop wait for. An import batch waiting for its turn, or whose body was still arriving, answers this: nothing in it was stored. Send the same request once the server is running again.".to_string(),
            Self::DatabaseBusy => "The database was too busy to take this request in time: another request, such as an import batch, held the write lock for longer than this one could wait for it, or every database connection stayed taken while it waited for one. Wait the number of seconds in the `Retry-After` header (repeated as `retry_after` in the body) and send the same request again.\n\n\
Any route that reads or writes the database can answer this, so no route in the OpenAPI document lists it.".to_string(),
        }
    }
}

/// The problem types a handler names in its `#[utoipa::path(responses(...))]`
/// as failures of its own, beside the success it answers:
/// `responses((status = 200, body = ImportRun), crate::problem::openapi::StateConflict)`.
///
/// A handler never writes an error status out by hand. The failures every
/// route of a kind shares, such as `401` for a route that takes a credential
/// or `415` for one that takes a body, are added by
/// `crate::openapi::shared_parts`, which also files each type named here
/// under its status and builds the one problem response for it
/// (`docs/architecture/http-api.md`, "The reference").
pub mod openapi {
    use std::collections::BTreeMap;

    use utoipa::openapi::{RefOr, Response};

    use super::ProblemType;

    /// The response key a named type is held under until the shared parts
    /// file it under its status.
    pub(crate) const KEY_PREFIX: &str = "problem:";

    /// The key [`KEY_PREFIX`] makes for `kind`.
    pub(crate) fn key(kind: ProblemType) -> String {
        format!("{KEY_PREFIX}{}", kind.slug())
    }

    macro_rules! named {
        ($($name:ident),* $(,)?) => {$(
            #[doc = concat!("A handler's own `", stringify!($name), "` failure.")]
            pub struct $name;

            impl utoipa::IntoResponses for $name {
                fn responses() -> BTreeMap<String, RefOr<Response>> {
                    BTreeMap::from([(key(ProblemType::$name), RefOr::T(Response::new("")))])
                }
            }
        )*};
    }

    // The types a handler may name as its own failure, so this list is
    // shorter than `ProblemType::ALL` on purpose: the shared parts add the
    // failures every route of a kind shares, and `method-not-allowed`
    // belongs to no route. A handler that names a type left out here fails
    // to compile.
    named!(
        InvalidCredentials,
        UsernameTaken,
        NameTaken,
        DemoAccountProtected,
        NotTheOwner,
        RegistrationClosed,
        SearchQueryInvalid,
        StateConflict,
        IdentityExists,
        AssetUploadInvalid,
        RangeNotSatisfiable,
        UploadStalled,
        ServerStopping,
    );
}

/// A body cap as a page states it: whole mebibytes as `MiB`, anything
/// smaller as whole kibibytes.
fn byte_size(bytes: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{} KiB", bytes / KIB)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// The `payload-too-large` page states each body cap as the code
    /// applies it, so a cap that changes in the code changes on the page.
    #[test]
    fn the_payload_too_large_page_names_each_body_cap() {
        let page = ProblemType::PayloadTooLarge.page();
        for (cap, text) in [
            (crate::server::MAX_AUTH_BODY_BYTES, "32 KiB"),
            (crate::server::MAX_JSON_BODY_BYTES, "32 MiB"),
            (
                crate::contacts_api::address_book::MAX_ADDRESS_BOOK_BYTES,
                "8 MiB",
            ),
            (message_crate_api_types::MAX_IMPORT_BATCH_BYTES, "16 MiB"),
            (crate::server::MAX_REQUEST_BODY_BYTES, "512 MiB"),
        ] {
            assert_eq!(byte_size(cap), text);
            assert!(page.contains(text), "the page does not name {text}: {page}");
        }
    }

    /// A busy database is logged at `WARN`, so a run of them shows in the
    /// owner's Logs panel, while a server told to stop is routine (#2084).
    #[test]
    fn a_busy_database_is_a_warning_and_a_stopping_server_is_not() {
        assert!(ProblemType::DatabaseBusy.refusal_is_a_warning());
        assert!(!ProblemType::ServerStopping.refusal_is_a_warning());
    }

    #[test]
    fn every_type_has_a_distinct_slug_and_a_page() {
        let slugs: HashSet<&str> = ProblemType::ALL.iter().map(|t| t.slug()).collect();
        assert_eq!(slugs.len(), ProblemType::ALL.len());
        for t in ProblemType::ALL {
            assert!(!t.page().trim().is_empty(), "{} has no page text", t.slug());
            assert!(t.url().ends_with(t.slug()));
            // `server-stopping` and `database-busy` are the two types that
            // are not the client's doing: the remedy for each is to send the
            // same request again.
            if matches!(t, ProblemType::ServerStopping | ProblemType::DatabaseBusy) {
                assert_eq!(t.status(), StatusCode::SERVICE_UNAVAILABLE);
            } else {
                assert!(t.status().is_client_error(), "{} is not a 4xx", t.slug());
            }
        }
    }
}
