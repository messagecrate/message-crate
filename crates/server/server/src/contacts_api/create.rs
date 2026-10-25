//! `POST /v1/contacts`: add one contact typed by hand (New Contact), or load
//! an address book. The body's `Content-Type` says which: a JSON body is one
//! contact, a `text/csv` body is the address book, which
//! `super::address_book` loads. Adding a contact checks each identity it is
//! given, then creates the contact, links its identities and writes its Audit
//! Trail entry in one write transaction.

use axum::extract::{FromRequest, Request, State};
use axum::response::{IntoResponse, Response};
use ir::IdentityService;
use serde::Deserialize;
use sqlx::{SqliteConnection, SqlitePool};

use super::address_book::load_address_book;
use super::{AddContactIdentityRequest, Contact, get_contact_detail};
use crate::api_error::ApiError;
use crate::auth::{FullAccess, refuse_for_demo_account};
use crate::db::account_profile::is_account_identity;
use crate::db::address_book::LoadMode;
use crate::db::audit_trail::{self, AuditAction, ContactChange};
use crate::db::contacts::{self, IdentityGoes, Origin};
use crate::db::{handles, trash};
use crate::extract::{Json, Query};
use crate::request_body::content_type_base;
use crate::server::Created;
use crate::text_caps::{MAX_IDENTITY_ADDRESS_CHARS, MAX_PERSON_NAME_CHARS, capped_text};

/// Most identities one New Contact may carry. A person is reached at a few
/// addresses, so a longer list is a mistake, and the cap bounds the checks
/// each identity costs.
pub(crate) const MAX_NEW_CONTACT_IDENTITIES: usize = 100;

/// The query of `POST /v1/contacts`.
#[derive(Debug, Deserialize)]
pub(crate) struct CreateContactsQuery {
    /// How an address book is applied; a contact typed by hand takes none.
    #[serde(default)]
    mode: Option<LoadMode>,
}

/// A contact typed by hand, the JSON body of `POST /v1/contacts`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateContactRequest {
    /// The contact's name, trimmed. Blank or over 200 characters is refused
    /// with `422 Unprocessable Entity`.
    pub(crate) name: String,
    /// The identities the contact is reached at, each with its service; none
    /// when left out. An identity another contact holds, in the Trash or
    /// not, or one of the account's own identities, is refused with
    /// `422 Unprocessable Entity`, and then nothing is created. At most 100.
    #[serde(default)]
    pub(crate) identities: Vec<AddContactIdentityRequest>,
}

/// Add contacts: one typed by hand, as JSON, or an address book, as CSV.
///
/// # A contact typed by hand
///
/// A JSON body, `application/json`, is one new contact: a name and zero or
/// more identities. The answer is `201 Created`, with the contact in the
/// body and its path in `Location`. A contact with a name and no identity is
/// in the Unknown Contact Group, because nothing reaches it yet. Each
/// identity is typed as adding one to a contact types it: the service
/// decides which types are valid and the address's shape picks among them.
///
/// An identity that is on another contact, named or not, in the Trash or
/// not, is refused with `422 Unprocessable Entity`, and so is one of the
/// account's own identities. Each refusal is one entry of `errors`, starting
/// with the identity's place in the list, as `identities[0].address: ` and
/// then the address and the contact that holds it. Nothing is created. The
/// person removes the identity from the new contact, or goes to the contact
/// that holds it. No merge is offered, because the new contact does not
/// exist yet.
///
/// The Audit Trail records the new contact as a `contact_added` entry with
/// its id. `mode` is refused for a contact typed by hand: it says how an
/// address book is applied. The Demo Account is refused with
/// `demo-account-protected`, by its id, because anyone can enter it
/// (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
///
/// # An address book
///
/// A `text/csv` body is an address book, loaded into this account, and the
/// answer is `200 OK` with what the load changed. The body is the file:
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
/// WhatsApp's own ids (`…@lid`, `…@g.us`, `…@broadcast`, `…@newsletter`),
/// which are `other`. An identity written exactly as the account keys it on
/// that service is that identity, whatever its type, so an exported file
/// loads back as it was. A row whose address the service cannot carry, such
/// as an email address on `WhatsApp` (an `@` address at any other domain),
/// is refused, where adding an identity stores it as `other`.
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
///
/// Any other `Content-Type` is refused with `415 Unsupported Media Type`.
#[utoipa::path(
    post,
    path = "/v1/contacts",
    tag = "Contacts",
    security(("session" = [])),
    params(
        ("mode" = Option<LoadMode>, Query, description = "How an address book is applied: `append` (the default) removes nothing; `edit` makes each contact in the file hold exactly the identities and Contact Group memberships its rows list. Refused with a JSON body.")
    ),
    request_body(
        description = "A contact typed by hand, as JSON, or the address book: Message Crate's own CSV, one row per identity.",
        content(
            (CreateContactRequest = "application/json"),
            (String = "text/csv")
        )
    ),
    responses(
        (
            status = 201,
            body = Contact,
            description = "The contact typed by hand was added",
            headers(("Location" = String, description = "Path of the new contact"))
        ),
        (status = 200, body = super::address_book::CreateContactsResponse, description = "The address book was loaded"),
        crate::problem::openapi::DemoAccountProtected,
    )
)]
pub(crate) async fn create_contacts(
    State(db): State<SqlitePool>,
    FullAccess(auth): FullAccess,
    Query(query): Query<CreateContactsQuery>,
    request: Request,
) -> Result<Response, ApiError> {
    let base = content_type_base(request.headers()).map(str::to_ascii_lowercase);
    match base.as_deref() {
        Some("text/csv") => {
            let mode = query.mode.unwrap_or_default();
            let loaded = load_address_book(&db, auth.account_id, mode, request).await?;
            Ok(Json(loaded).into_response())
        }
        Some("application/json") => {
            refuse_for_demo_account(auth.account_id, "contacts cannot be added")?;
            if query.mode.is_some() {
                return Err(ApiError::validation(
                    "mode applies to an address book, not to a contact typed by hand",
                ));
            }
            let Json(body) = Json::<CreateContactRequest>::from_request(request, &()).await?;
            let contact = create_contact(&db, auth.account_id, &body).await?;
            Ok(Created {
                location: format!("/v1/contacts/{}", contact.id),
                body: contact,
            }
            .into_response())
        }
        _ => Err(ApiError::UnsupportedMediaType(
            "Content-Type must be application/json or text/csv".into(),
        )),
    }
}

/// One identity of a New Contact, checked: its place in the request, the
/// address as typed, and the `handles` row it is.
struct CheckedIdentity<'a> {
    index: usize,
    address: &'a str,
    handle_id: i64,
}

/// Create the contact `body` describes under `account_id`, with its
/// identities and its `contact_added` entry, and answer it as
/// `GET /v1/contacts/{id}` would.
async fn create_contact(
    db: &SqlitePool,
    account_id: i64,
    body: &CreateContactRequest,
) -> Result<Contact, ApiError> {
    let name = capped_text("name", &body.name, MAX_PERSON_NAME_CHARS)
        .map_err(|e| ApiError::validation(e.to_string()))?;
    let mut errors = Vec::new();
    if name.is_empty() {
        errors.push("name must not be empty".to_string());
    }
    if body.identities.len() > MAX_NEW_CONTACT_IDENTITIES {
        errors.push(format!(
            "identities may list at most {MAX_NEW_CONTACT_IDENTITIES}"
        ));
    }
    let mut addresses = Vec::with_capacity(body.identities.len());
    for (index, identity) in body.identities.iter().enumerate() {
        let field = format!("identities[{index}].address");
        match capped_text(&field, &identity.address, MAX_IDENTITY_ADDRESS_CHARS) {
            Ok("") => errors.push(format!("{field}: must not be empty")),
            Ok(address) => addresses.push((index, address, identity.service)),
            Err(too_long) => errors.push(too_long.to_string()),
        }
    }
    if !errors.is_empty() {
        return Err(ApiError::ValidationFailed(errors));
    }

    let mut conn = db.acquire().await?;
    let now = chrono::Utc::now();
    // One write transaction: a refusal rolls back the identity rows the
    // checks made, and no other write can link an identity between its check
    // and the link.
    let mut tx = crate::db::begin_write(&mut conn).await?;
    let mut checked = Vec::with_capacity(addresses.len());
    for (index, address, service) in addresses {
        let service = service.map_or(IdentityService::TextMessage, IdentityService::from);
        let handle_id = handles::identity_handle_id(&mut tx, account_id, address, service).await?;
        if let Some(refusal) = held_elsewhere(&mut tx, account_id, address, handle_id).await? {
            errors.push(format!("identities[{index}].address: {refusal}"));
        }
        checked.push(CheckedIdentity {
            index,
            address,
            handle_id,
        });
    }
    if !errors.is_empty() {
        return Err(ApiError::ValidationFailed(errors));
    }

    let contact_id = contacts::create_contact(&mut tx, account_id, name, Origin::User).await?;
    for identity in &checked {
        let goes = IdentityGoes::To(contact_id, Origin::User);
        contacts::move_identity(&mut tx, account_id, identity.handle_id, goes)
            .await
            .map_err(|cause| {
                ApiError::Internal(cause.context(format!(
                    "link identities[{}] ({}) to the new contact",
                    identity.index, identity.address
                )))
            })?;
    }
    contacts::touch_contact(&mut tx, account_id, contact_id, now).await?;
    let added = ContactChange {
        contact_id: Some(contact_id),
        contact_ids: &[],
    };
    audit_trail::record_contact_change(&mut tx, AuditAction::ContactAdded, account_id, added, now)
        .await?;
    let contact = get_contact_detail(&mut tx, account_id, contact_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("new contact missing after create")))?;
    tx.commit().await?;
    Ok(contact)
}

/// Why the identity `handle_id`, typed as `address`, cannot go on a new
/// contact, or `None` when it can: it is one of the account's own
/// identities (CONTACT-46), or another contact holds it. The sentence names
/// that contact by its name and its id, so the person can find it even when
/// it has no name or shares its name with another.
async fn held_elsewhere(
    conn: &mut SqliteConnection,
    account_id: i64,
    address: &str,
    handle_id: i64,
) -> Result<Option<String>, ApiError> {
    if is_account_identity(conn, account_id, handle_id).await? {
        return Ok(Some(format!(
            "{address} is one of the account's own identities"
        )));
    }
    let Some(holder) = contacts::contact_id_for_handle(conn, account_id, handle_id).await? else {
        return Ok(None);
    };
    let name = contacts::preferred_name(conn, account_id, holder).await?;
    let who = if name.is_empty() {
        format!("contact {holder}, which has no name")
    } else {
        format!("{name} (contact {holder})")
    };
    let trashed = trash::is_contact_trashed(conn, account_id, holder).await?;
    let place = if trashed { ", in the Trash" } else { "" };
    Ok(Some(format!("{address} is already on {who}{place}")))
}

#[cfg(test)]
mod tests;
