//! Picking the country of a phone number written without its `+` code, from
//! the Contacts screen (`PATCH /v1/contacts/{id}`) or the account's own
//! identities (`PATCH /v1/accounts/{id}`). Both routes take one
//! [`SetIdentityCountryRequest`] and run [`set_identity_country`]; the
//! statements are in `db::identity_country` (#1676).

use message_crate_api_types::IdentityHolder;
use message_ir::IdentityService;
use serde::Deserialize;
use sqlx::SqliteConnection;

use crate::db::account_profile;
use crate::db::contacts::{self, OnService};
use crate::db::handles::ApiIdentityService;
use crate::db::identity_country::{self, CountryRefusal};
use crate::imports_api::with_yourself;

/// A country picked for one phone number written without its `+` code.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetIdentityCountryRequest {
    /// The identity as the list shows it: the number as typed, without `+`.
    pub address: String,
    /// The service the identity is on. When omitted, the identity is found
    /// on the phone service first, then WhatsApp.
    #[serde(default)]
    pub service: Option<ApiIdentityService>,
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
#[derive(Debug, thiserror::Error)]
pub enum CountryError {
    /// The request asks for something the server will not do; the sentence
    /// is written for the person.
    #[error("{0}")]
    Refused(String),
    /// Another identity holds the `+` form and the request did not ask to
    /// merge: the sentence names it, and `holder` says who holds it.
    #[error("{detail}")]
    Exists {
        /// The sentence, naming the number, its `+` form and who holds it.
        detail: String,
        /// Who holds the `+` form.
        holder: IdentityHolder,
    },
    /// The request cannot be done from where it was sent: a number on a
    /// contact whose `+` form is one of the account's own identities.
    #[error("{0}")]
    Conflict(String),
    /// Something failed that changing the request would not help.
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

impl From<CountryError> for crate::server::ApiError {
    fn from(error: CountryError) -> Self {
        match error {
            CountryError::Refused(message) => Self::validation(message),
            CountryError::Exists { detail, holder } => Self::IdentityExists { detail, holder },
            CountryError::Conflict(message) => Self::StateConflict(message),
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
    let service = request.service.map(IdentityService::from);
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
            account_profile::account_identity_id(conn, account_id, address, service).await?
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
    let holder = match form.existing {
        Some(existing) => Some(identity_country::holder(conn, account_id, existing).await?),
        None => None,
    };
    if matches!(whose, Whose::Contact(_)) && holder == Some(IdentityHolder::Account) {
        // Joining a contact's number to the account holder's own makes its
        // conversations ones with yourself and takes the number off the
        // contact, so it is done where the account's identities are.
        return Err(CountryError::Conflict(format!(
            "{address} in {} is {}, which is one of this account's own identities. \
             Add {address} to My Identities and pick its country there to join the two.",
            country.code, form.key
        )));
    }
    if let Some(holder) = holder.filter(|_| !request.merge) {
        let held = match &holder {
            IdentityHolder::Contact { name, .. } if name.is_empty() => {
                "another identity of a contact with no name".to_string()
            }
            IdentityHolder::Contact { name, .. } => format!("another identity of {name}"),
            IdentityHolder::Account => "one of this account's own identities".to_string(),
            IdentityHolder::Nobody => "another identity, on no contact".to_string(),
        };
        return Err(CountryError::Exists {
            detail: format!(
                "{address} in {} is {}, which is already {held}.",
                country.code, form.key
            ),
            holder,
        });
    }
    let edited_contact = match whose {
        Whose::Contact(contact_id) => Some(contact_id),
        Whose::Account => None,
    };
    // A merge can make one of the account's identities the chat handle of a
    // one-to-one conversation, or a member of a group, so the with-yourself
    // rule runs again around it, as it does for any identity change (#1662).
    let before = with_yourself::before_identity_change(conn, account_id).await?;
    let changed = identity_country::give_country(conn, account_id, &form, edited_contact).await?;
    with_yourself::follow_identities(conn, before).await?;
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
