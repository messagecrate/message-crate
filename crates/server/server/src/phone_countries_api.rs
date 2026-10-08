//! `GET /v1/phone-countries`: the countries a phone number written without
//! its `+` code can be read in, for the import form's country and the
//! country picked for one identity (#1676). A fixed reference list, so it
//! answers a page like every other list (`docs/architecture/http-api.md`,
//! "Lists"). The table is `phone::COUNTRIES`, the one the server keys
//! numbers by, so the list never offers a country the server would refuse.

use crate::extract::{Json, Query};
use serde::{Deserialize, Serialize};

use crate::paging::{DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, page_of, page_params};
use crate::server::{ApiError, FullAccess};

/// One country a phone number can be read in.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct PhoneCountry {
    /// ISO 3166-1 alpha-2 code, upper case, such as `GB`: the value the
    /// import form's `phone_country` and an identity's `country` take. `XK`
    /// is Kosovo.
    code: String,
    /// Country calling code without the `+`, such as `44`. Countries that
    /// share a numbering plan share one: `1` is the United States, Canada and
    /// much of the Caribbean.
    calling_code: String,
}

/// The paging the list takes.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct ListPhoneCountriesQuery {
    /// Page size, default 40, max 500.
    #[serde(default)]
    limit: Option<usize>,
    /// Page offset, max 50000.
    #[serde(default)]
    offset: Option<usize>,
}

/// The countries a phone number written without its `+` code can be read
/// in, by ISO code.
#[utoipa::path(
    get,
    path = "/v1/phone-countries",
    tag = "Contacts",
    security(("session" = [])),
    params(ListPhoneCountriesQuery),
    responses(
        (status = 200, body = crate::paging::Page<PhoneCountry>),
    )
)]
pub(crate) async fn list_phone_countries(
    FullAccess(_auth): FullAccess,
    Query(query): Query<ListPhoneCountriesQuery>,
) -> Result<Json<Page<PhoneCountry>>, ApiError> {
    let params = page_params(
        query.limit,
        query.offset,
        DEFAULT_LIST_LIMIT,
        Some(MAX_LIST_OFFSET),
    )?;
    let countries = phone::COUNTRIES
        .iter()
        .map(|country| PhoneCountry {
            code: country.code.to_string(),
            calling_code: country.calling_code.to_string(),
        })
        .collect();
    Ok(Json(page_of(countries, params)))
}
