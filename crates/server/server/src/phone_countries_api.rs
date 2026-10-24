//! `GET /v1/phone-countries`: the countries a phone number written without
//! its `+` code can be read in, for the import form's country and the
//! country picked for one identity (#1676). A fixed reference list, so it
//! answers a page like every other list (`docs/architecture/rules/http-api.md`,
//! "Lists"). The table is `phone::COUNTRIES`, the one the server keys
//! numbers by, so the list never offers a country the server would refuse.

use crate::extract::{Json, Query};
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;
use crate::auth::LoggedIn;
use crate::paging::{DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, page_of, page_params};

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
/// in, by ISO code. Any logged-in person reads it, the owner included: the
/// owner picks the country of a number on an account it opens from User
/// Accounts (`set_identity_country` on `PATCH /v1/accounts/{id}`).
#[utoipa::path(
    get,
    path = "/v1/phone-countries",
    tag = "Contacts",
    security(
        ("session" = []),
        ("session" = ["owner"])
    ),
    params(ListPhoneCountriesQuery),
    responses(
        (status = 200, body = crate::paging::Page<PhoneCountry>),
    )
)]
pub(crate) async fn list_phone_countries(
    LoggedIn(_auth): LoggedIn,
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

#[cfg(test)]
mod tests {
    use crate::paging::Page;
    use crate::test_support::{claim_as_owner, get_json, test_fixture};

    /// The owner picks the country of a managed account's number from that
    /// account's Profile tab (`set_identity_country` on
    /// `PATCH /v1/accounts/{id}`), so the owner's session reads the list the
    /// picker offers (#2987).
    #[tokio::test]
    async fn the_owner_reads_the_country_list() {
        let fixture = test_fixture().await;
        let owner = claim_as_owner(&fixture.state, "keeper", "hunter2hunter2").await;

        let page: Page<serde_json::Value> = get_json(
            &fixture.state,
            "/v1/phone-countries?limit=500&offset=0",
            &owner.token,
        )
        .await;

        assert_eq!(page.total, phone::COUNTRIES.len() as u64);
        assert!(
            page.items.iter().any(|country| country["code"] == "GB"),
            "the owner's list names GB: {:?}",
            page.items
        );
    }
}
