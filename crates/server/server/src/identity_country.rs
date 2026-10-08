//! Picking the country of a phone number written without its `+` code, from
//! the Contacts screen (`PATCH /v1/contacts/{id}`) or the account's own
//! identities (`PATCH /v1/accounts/{id}`). Both routes take one
//! [`SetIdentityCountryRequest`] and run [`set_identity_country`]; the
//! statements are in `db::identity_country` (#1676).

use message_ir::HandleService;
use serde::Deserialize;
use sqlx::SqliteConnection;

use crate::db::contacts::{self, OnService};
use crate::db::handles::IdentityService;
use crate::db::identity_country::{self, CountryRefusal, Holder};

/// A country picked for one phone number written without its `+` code.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetIdentityCountryRequest {
    /// The identity as the list shows it: the number as typed, without `+`.
    pub address: String,
    /// The service the identity is on. When omitted, the identity is found
    /// on the phone service first, then WhatsApp.
    #[serde(default)]
    pub service: Option<IdentityService>,
    /// The country the number is in, as an ISO 3166-1 alpha-2 code such as
    /// `GB`, which `GET /v1/phone-countries` lists. The number takes the
    /// `+` form that country gives it.
    pub country: String,
    /// Join the identity to the one already holding that `+` form, when
    /// another does. Without it, such a request changes nothing and answers
    /// `409 Conflict` (`identity-exists`), naming that identity, so the
    /// person can be asked first.
    #[serde(default)]
    pub merge: bool,
}

/// Whose identity the country is picked for.
#[derive(Debug, Clone, Copy)]
pub enum Whose {
    /// One linked to this contact, from the Contacts screen.
    Contact(i64),
    /// One of the account's own identities.
    Account,
}

/// Why a country was not picked.
#[derive(Debug)]
pub enum CountryError {
    /// The request asks for something the server will not do; the sentence
    /// is written for the person.
    Refused(String),
    /// Another identity holds the `+` form and the request did not ask to
    /// merge; the sentence names it.
    Exists(String),
    /// Something failed that changing the request would not help.
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for CountryError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

impl From<CountryError> for crate::server::ApiError {
    fn from(error: CountryError) -> Self {
        match error {
            CountryError::Refused(message) => Self::validation(message),
            CountryError::Exists(message) => Self::IdentityExists(message),
            CountryError::Failed(cause) => Self::Internal(cause),
        }
    }
}

/// Give the identity `request` names the `+` form its country gives it, and
/// merge it into the identity already holding that form when the request
/// says to. Runs in the caller's write transaction.
///
/// # Errors
///
/// [`CountryError::Refused`] for an unknown country, an identity the caller
/// does not hold, one whose country is already known, or a number with no
/// `+` form there; [`CountryError::Exists`] when another identity holds the
/// form and `merge` is not set.
pub async fn set_identity_country(
    conn: &mut SqliteConnection,
    account_id: i64,
    whose: Whose,
    request: &SetIdentityCountryRequest,
) -> Result<(), CountryError> {
    let Some(country) = phone::country(&request.country) else {
        return Err(CountryError::Refused(format!(
            "country {:?} is not a country GET /v1/phone-countries lists",
            request.country
        )));
    };
    let address = request.address.trim();
    let service = request.service.map(HandleService::from);
    let handle_id = match whose {
        Whose::Contact(contact_id) => contacts::linked_handle_id(
            conn,
            account_id,
            contact_id,
            address,
            OnService::Preferring(service),
        )
        .await?
        .map(|(id, _)| id),
        Whose::Account => {
            identity_country::account_identity_id(conn, account_id, address, service).await?
        }
    };
    let Some(handle_id) = handle_id else {
        return Err(CountryError::Refused(format!(
            "{address} is not one of the identities shown here"
        )));
    };
    let form = match identity_country::plus_form(conn, account_id, handle_id, country).await? {
        Ok(form) => form,
        Err(CountryRefusal::CountryKnown) => {
            return Err(CountryError::Refused(format!(
                "{address} is not a phone number whose country is unknown"
            )));
        }
        Err(CountryRefusal::NoPlusForm(reason)) => {
            return Err(CountryError::Refused(format!(
                "{address} has no + form in {}: {reason}",
                country.code
            )));
        }
    };
    if let Some(existing) = form.existing.filter(|_| !request.merge) {
        let who = match identity_country::holder(conn, account_id, existing).await? {
            Holder::Contact(_, name) if name.is_empty() => "a contact with no name".to_string(),
            Holder::Contact(_, name) => name,
            Holder::Account => "this account, as one of its own identities".to_string(),
            Holder::Nobody => "no contact".to_string(),
        };
        return Err(CountryError::Exists(format!(
            "{address} in {} is {}, which is already an identity of {who}.",
            country.code, form.key
        )));
    }
    let edited_contact = match whose {
        Whose::Contact(contact_id) => Some(contact_id),
        Whose::Account => None,
    };
    let changed = identity_country::give_country(conn, account_id, &form, edited_contact).await?;
    // A merged conversation or sender changes the content keys of the
    // messages that moved, so their duplicate flags are worked out again.
    crate::dedupe::dedupe_changed_messages(
        conn,
        account_id,
        &changed,
        crate::dedupe::NEAR_WINDOW_SECS,
    )
    .await?;
    Ok(())
}
