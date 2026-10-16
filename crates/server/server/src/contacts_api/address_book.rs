//! The address book routes: `POST /v1/contacts` loads Message Crate's own
//! CSV into the account, `POST /v1/contacts/address-book` writes it, and
//! `POST /v1/contacts/vcard-address-book` writes it from a vCard file for a
//! person to edit. The file's rules and every direction are in
//! `db::address_book`; this module checks the request and hands it over.

use std::collections::HashSet;

use axum::extract::{Request, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::api_error::ApiError;
use crate::auth::{FullAccess, refuse_for_demo_account};
use crate::db::address_book::{self, LoadCounts, LoadError, LoadMode};
use crate::db::audit_trail::{self, AuditAction, AuditActor, Details};
use crate::db::contacts::read::contact_ids_matching;
use crate::extract::{Json, Query};
use crate::request_body::{content_type_base, read_body_limited};

/// What an address book load changed.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CreateContactsResponse {
    /// Contacts the load created.
    pub contacts_created: u64,
    /// Contacts the load renamed, or whose identities or Contact Group
    /// memberships it changed.
    pub contacts_updated: u64,
    /// Contacts the load deleted: the ones it left with neither a name nor
    /// an identity.
    pub contacts_deleted: u64,
    /// Identities linked to a contact that no contact held before.
    pub identities_added: u64,
    /// Identities taken from one contact and given to another.
    pub identities_moved: u64,
    /// Identities taken off a contact, which only Edit does.
    pub identities_removed: u64,
    /// Contact Groups the load created.
    pub groups_created: u64,
    /// One sentence for each phone number the file wrote without `+` that
    /// the load matched to the `+` key its contact holds, or that became a
    /// new identity. Each starts with its row number. A spreadsheet can drop
    /// the `+` from a number without showing it, so the load says how it
    /// read the number.
    pub notes: Vec<String>,
}

impl From<LoadCounts> for CreateContactsResponse {
    fn from(counts: LoadCounts) -> Self {
        Self {
            contacts_created: counts.contacts_created,
            contacts_updated: counts.contacts_updated,
            contacts_deleted: counts.contacts_deleted,
            identities_added: counts.identities_added,
            identities_moved: counts.identities_moved,
            identities_removed: counts.identities_removed,
            groups_created: counts.groups_created,
            notes: counts.notes,
        }
    }
}

/// Largest address book the load route accepts, in bytes.
///
/// A row is under a hundred bytes, so a few megabytes is already tens of
/// thousands of identities, and the whole file is read into memory before
/// parsing. The route reads the body itself against this cap, as the asset
/// routes do, and not through `crate::extract::Json`, whose cap is
/// [`crate::limits::MAX_JSON_BODY_BYTES`].
pub(crate) const MAX_ADDRESS_BOOK_BYTES: usize = 8 * 1024 * 1024;

/// Largest vCard file the conversion route accepts, in bytes.
///
/// A phone's contacts carry their photos inside the file, base64-encoded:
/// 544 cards from one iPhone, 184 of them with a photo, are about 20 MB. The
/// file is read into memory whole, as the address book is.
pub(crate) const MAX_VCARD_BYTES: usize = 64 * 1024 * 1024;

/// The file name Export gives the address book.
const EXPORT_FILE_NAME: &str = "address-book.csv";

/// The query of `POST /v1/contacts`.
#[derive(Debug, Deserialize)]
pub(crate) struct CreateContactsQuery {
    #[serde(default)]
    mode: LoadMode,
}

impl From<LoadError> for ApiError {
    fn from(error: LoadError) -> Self {
        match error {
            LoadError::Refused(reasons) => Self::ValidationFailed(reasons),
            LoadError::Failed(cause) => Self::Internal(cause.context("load address book")),
        }
    }
}

/// Load an address book into this account. The body is the file itself:
/// Message Crate's own CSV, one row per identity, with the columns
/// `contact_id`, `display_name`, `service`, `identity` and `groups`, as
/// `POST /v1/contacts/address-book` writes it.
///
/// `service` is `Text Message`, `Email` or `WhatsApp`, the words the screen
/// where an identity is added offers, ignoring case. The file states no
/// type: the service decides which types are valid and the address's shape
/// picks among them, as adding an identity does. `Text Message` carries a
/// phone number, an email address or any other name; `Email` is an email
/// address on Text Message; `WhatsApp` carries a phone number or one of
/// WhatsApp's own ids. An identity written exactly as the account keys it on
/// that service is that identity, whatever its type, so an exported file
/// loads back as it was. A row whose address the service cannot carry, such
/// as an email address on `WhatsApp`, is refused.
///
/// Rows that share a `contact_id` are one contact. An id the account holds
/// names that contact, a blank id makes a new contact for that row, and any
/// other text groups its rows into one new contact. `append` creates and
/// renames contacts and adds the identities and Contact Group memberships
/// the rows list. `edit` does the same and then takes off each contact in
/// the file every identity and membership its rows do not list. A contact
/// the file does not mention is left alone in both modes.
///
/// A phone number written without `+`, which a spreadsheet can save in place
/// of `+6555550100`, names the identity `+` and its digits when the row's
/// contact holds that key. Otherwise it is keyed as written: ten digits as a
/// US number, any other count as bare digits. `notes` names each row read
/// with its `+` back, and each such number that became a new identity.
///
/// One `'` before a cell that starts with `=`, `+`, `-`, `@`, a tab or a
/// carriage return is taken off, in any column, which undoes the `'` the
/// export writes there.
///
/// The load is one transaction. A file that breaks a rule is refused whole
/// with `422 Unprocessable Entity`, and `errors` holds one sentence for each
/// bad row, starting with its row number.
///
/// The Demo Account is refused with `demo-account-protected`, in both modes,
/// by its id: an Edit load deletes contacts for good and an Append load
/// stores real people's names and numbers in an account anyone can enter
/// (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`). The import,
/// export and delete permissions are not asked for; they govern messages and
/// imports, and any other account loads its address book with a session.
#[utoipa::path(
    post,
    path = "/v1/contacts",
    tag = "Contacts",
    security(("session" = [])),
    params(
        ("mode" = Option<LoadMode>, Query, description = "How the file is applied: `append` (the default) removes nothing; `edit` makes each contact in the file hold exactly the identities and Contact Group memberships its rows list.")
    ),
    request_body(
        content_type = "text/csv",
        content = String,
        description = "The address book: Message Crate's own CSV, one row per identity."
    ),
    responses(
        (status = 200, body = CreateContactsResponse),
        crate::problem::openapi::DemoAccountProtected,
    )
)]
pub(crate) async fn create_contacts(
    State(db): State<SqlitePool>,
    FullAccess(auth): FullAccess,
    Query(query): Query<CreateContactsQuery>,
    request: Request,
) -> Result<Json<CreateContactsResponse>, ApiError> {
    refuse_for_demo_account(auth.account_id, "address book cannot be loaded")?;
    if !content_type_base(request.headers())
        .is_some_and(|base| base.eq_ignore_ascii_case("text/csv"))
    {
        return Err(ApiError::UnsupportedMediaType(
            "Content-Type must be text/csv".into(),
        ));
    }
    let body = read_body_limited(request.into_body(), MAX_ADDRESS_BOOK_BYTES).await?;
    let content = std::str::from_utf8(&body)
        .map_err(|_| ApiError::MalformedBody("address book is not UTF-8 text".into()))?;
    if content.trim().is_empty() {
        return Err(ApiError::validation("address book is empty"));
    }
    let mut conn = db.acquire().await?;
    let now = chrono::Utc::now();
    let counts = address_book::load(&mut conn, auth.account_id, content, query.mode, now).await?;
    let count = |n: u64| Some(i64::try_from(n).unwrap_or(i64::MAX));
    let details = Details {
        mode: Some(query.mode),
        contacts_created: count(counts.contacts_created),
        contacts_updated: count(counts.contacts_updated),
        contacts_deleted: count(counts.contacts_deleted),
        ..Details::default()
    };
    audit_trail::record_about(
        &mut conn,
        AuditAction::AddressBookLoaded,
        AuditActor::Holder,
        auth.account_id,
        details,
        now,
    )
    .await?;
    Ok(Json(counts.into()))
}

/// Which contacts `POST /v1/contacts/address-book` writes: the Contacts
/// list's search and its checked rows.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub(crate) struct GetAddressBookRequest {
    /// A Contacts search, as `GET /v1/contacts` takes in `q`. Absent or
    /// empty matches every contact.
    #[serde(default)]
    q: Option<String>,
    /// Contact ids to keep from what `q` matches. Absent or empty keeps
    /// them all.
    #[serde(default)]
    ids: Vec<i64>,
}

/// Write the address book for the contacts a search matches, the checked
/// ones among them, or every contact when the body names neither.
///
/// The answer is `text/csv`, not JSON: one row per identity, with the
/// columns `contact_id`, `display_name`, `service`, `identity` and `groups`.
/// `service` is `Text Message`, `Email` for an email address on Text
/// Message, or `WhatsApp`. A contact's name and its Contact Group names,
/// separated by `;`, repeat on each of its rows. A contact with no name has
/// a blank `display_name`, and a contact with no identity is one row with
/// `service` and `identity` blank. Contacts in the trash are left out.
///
/// A cell that starts with `=`, `+`, `-`, `@`, a tab or a carriage return
/// is written with a `'` in front, in every column, because a spreadsheet
/// runs such a cell as a formula and drops the `+` of a phone number.
/// `POST /v1/contacts` loads the file back and takes that `'` off.
#[utoipa::path(
    post,
    path = "/v1/contacts/address-book",
    tag = "Contacts",
    security(("session" = [])),
    request_body = GetAddressBookRequest,
    responses(
        (
            status = 200,
            description = "The address book, as an attachment named `address-book.csv`",
            content_type = "text/csv",
            body = String,
            headers(("Content-Disposition" = String, description = "`attachment; filename=\"address-book.csv\"`"))
        ),
        crate::problem::openapi::SearchQueryInvalid
    )
)]
pub(crate) async fn get_address_book(
    State(db): State<SqlitePool>,
    FullAccess(auth): FullAccess,
    Json(body): Json<GetAddressBookRequest>,
) -> Result<Response, ApiError> {
    let mut conn = db.acquire().await?;
    let now = chrono::Utc::now();
    let q = body.q.as_deref().unwrap_or("").trim();
    let only: Option<HashSet<i64>> = if q.is_empty() {
        (!body.ids.is_empty()).then(|| body.ids.iter().copied().collect())
    } else {
        // The same compile the Contacts list runs, so the file holds the
        // rows the person was looking at.
        let clock =
            crate::db::account_profile::account_clock(&mut conn, auth.account_id, now).await?;
        let matched = contact_ids_matching(&mut conn, auth.account_id, q, clock).await?;
        Some(if body.ids.is_empty() {
            matched
        } else {
            body.ids
                .iter()
                .copied()
                .filter(|id| matched.contains(id))
                .collect()
        })
    };
    let written = address_book::export_csv(&mut conn, auth.account_id, only.as_ref()).await?;
    let count = |n: u64| Some(i64::try_from(n).unwrap_or(i64::MAX));
    let details = Details {
        contacts: count(written.contacts),
        identities: count(written.identities),
        ..Details::default()
    };
    audit_trail::record_about(
        &mut conn,
        AuditAction::AddressBookExported,
        AuditActor::Holder,
        auth.account_id,
        details,
        now,
    )
    .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{EXPORT_FILE_NAME}\""),
            ),
        ],
        written.csv,
    )
        .into_response())
}

/// The query of `POST /v1/contacts/vcard-address-book`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct GetVcardAddressBookQuery {
    #[serde(default)]
    country: Option<String>,
}

/// Write an address book from a vCard file, for the person to edit and then
/// load with `POST /v1/contacts`. The body is the file itself, one or more
/// cards. Nothing is read from or stored in the account.
///
/// The answer is `text/csv` in the address book's columns: one row for each
/// `TEL` and each `EMAIL` on each card. `contact_id` is `card1`, `card2`, …
/// by the card's position in the file, so a card's rows load as one new
/// contact. `display_name` is the card's `FN`, or its `N` assembled when it
/// has no `FN`, and blank when it has neither. `service` is `Text Message`
/// for a number and `Email` for an email address, never `WhatsApp`, because
/// a card cannot say a number is on WhatsApp. `groups` is blank:
/// `CATEGORIES` is not carried over. A card with no number and no email
/// address gives no rows.
///
/// A number written without `+` is written in its `+` form in `country` when
/// it is a valid number there. A number written with `+`, one that is not
/// valid in `country`, and every number when no `country` is given are
/// written exactly as the card has them. Nothing else is checked: the load
/// refuses what it cannot store and names the rows. Cells are written with
/// the `'` the export puts before a cell a spreadsheet would run.
///
/// A body that holds no `BEGIN:VCARD` line answers
/// `422 Unprocessable Entity`.
#[utoipa::path(
    post,
    path = "/v1/contacts/vcard-address-book",
    tag = "Contacts",
    security(("session" = [])),
    params(
        ("country" = Option<String>, Query, description = "The country a number written without its `+` code is read in, as an ISO 3166-1 code that `GET /v1/phone-countries` lists, such as `US`. Absent, every number is written as the card has it.")
    ),
    request_body(
        content_type = "text/vcard",
        content = String,
        description = "The vCard file: one or more cards."
    ),
    responses(
        (
            status = 200,
            description = "The address book, as an attachment named `address-book.csv`",
            content_type = "text/csv",
            body = String,
            headers(("Content-Disposition" = String, description = "`attachment; filename=\"address-book.csv\"`"))
        ),
    )
)]
pub(crate) async fn get_vcard_address_book(
    FullAccess(_auth): FullAccess,
    Query(query): Query<GetVcardAddressBookQuery>,
    request: Request,
) -> Result<Response, ApiError> {
    if !content_type_base(request.headers())
        .is_some_and(|base| base.eq_ignore_ascii_case("text/vcard"))
    {
        return Err(ApiError::UnsupportedMediaType(
            "Content-Type must be text/vcard".into(),
        ));
    }
    let country = match query.country.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(code) => Some(phone::country(code).ok_or_else(|| {
            ApiError::validation(format!(
                "country {code:?} is not a country GET /v1/phone-countries lists"
            ))
        })?),
    };
    let body = read_body_limited(request.into_body(), MAX_VCARD_BYTES).await?;
    let content = std::str::from_utf8(&body)
        .map_err(|_| ApiError::MalformedBody("vCard file is not UTF-8 text".into()))?;
    let Some(csv) = address_book::from_vcard(content, country)? else {
        return Err(ApiError::validation(
            "the file holds no vCard: it has no BEGIN:VCARD line",
        ));
    };
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{EXPORT_FILE_NAME}\""),
            ),
        ],
        csv,
    )
        .into_response())
}
