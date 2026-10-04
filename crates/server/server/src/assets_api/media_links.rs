//! Media links: `POST /v1/assets/{sha256}/media-links`, and the check that
//! lets a request with no `Authorization` header read an asset by one.
//!
//! A media element (`<img>`, `<video>`, `<audio>`) loads its `src` itself and
//! cannot send the Session's `Authorization` header, so the web app asks for
//! a media link and puts the URL it answers in `src`. The link is
//! `<account_id>.<expires>.<signature>`, carried in the `media_link` query
//! parameter. The signature is an HMAC-SHA256, under a key the process makes
//! when it starts, over the account, the asset's fingerprint, the expiry and
//! the hash of the Session token that made it. Nothing is stored: the server
//! checks a link by signing its terms again with the Session the account
//! holds now, so a link opens one asset of one account, for an hour at most,
//! and ends with its Session or with a restart. Why this credential and not
//! another: `docs/architecture/http-api.md`, "Credentials and reach".

use std::fmt;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use hmac::{KeyInit, Mac};
use rand::TryRng;
use serde::Serialize;

use crate::assets_api::{Sha256, hex_encode, lookup_by_sha256_unverified};
use crate::db::{account_profile, session_tokens};
use crate::extract::Path as AxumPath;
use crate::server::{
    ApiError, AppState, Created, FullAccess, bearer_token, require_asset_read_access, resolve_auth,
};

/// How long a media link opens its asset after it is made.
pub(crate) const MEDIA_LINK_TTL: Duration = Duration::from_secs(60 * 60);

/// The query parameter a media link travels in.
pub(crate) const MEDIA_LINK_PARAM: &str = "media_link";

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// The process's key for signing media links. Its `Debug` hides it.
#[derive(Clone)]
pub(crate) struct MediaLinkKey([u8; 32]);

impl fmt::Debug for MediaLinkKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MediaLinkKey(..)")
    }
}

impl MediaLinkKey {
    /// A new key from the operating system's random source.
    ///
    /// # Panics
    ///
    /// When the operating system has no random source to give, the server
    /// cannot make any credential, a Session included, so it stops.
    pub(crate) fn random() -> Self {
        let mut key = [0u8; 32];
        rand::rngs::SysRng
            .try_fill_bytes(&mut key)
            .expect("the operating system's random source is available");
        Self(key)
    }

    /// The signature over one media link's terms.
    fn mac(
        &self,
        account_id: i64,
        sha256: &Sha256,
        expires: i64,
        session_hash: &str,
    ) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        mac.update(format!("{account_id}\n{sha256}\n{expires}\n{session_hash}").as_bytes());
        mac
    }

    /// The media link for `sha256` in `account_id`'s store, open until the
    /// Unix second `expires` while the Session whose token hashes to
    /// `session_hash` lasts: `<account_id>.<expires>.<signature>`.
    pub(crate) fn sign(
        &self,
        account_id: i64,
        sha256: &Sha256,
        expires: i64,
        session_hash: &str,
    ) -> String {
        let signature = self
            .mac(account_id, sha256, expires, session_hash)
            .finalize()
            .into_bytes();
        format!("{account_id}.{expires}.{}", hex_encode(&signature))
    }

    /// Whether `signature` is the one [`Self::sign`] gives these terms,
    /// compared in constant time.
    fn verifies(
        &self,
        account_id: i64,
        sha256: &Sha256,
        expires: i64,
        session_hash: &str,
        signature: &[u8],
    ) -> bool {
        self.mac(account_id, sha256, expires, session_hash)
            .verify_slice(signature)
            .is_ok()
    }
}

/// A media link's three parts, read but not yet checked.
struct LinkTerms {
    account_id: i64,
    expires: i64,
    signature: Vec<u8>,
}

impl LinkTerms {
    fn parse(link: &str) -> Option<Self> {
        let mut parts = link.split('.');
        let account_id = parts.next()?.parse().ok()?;
        let expires = parts.next()?.parse().ok()?;
        let signature = hex_decode(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            account_id,
            expires,
            signature,
        })
    }
}

/// Lower- or upper-case hex to bytes; `None` for anything else.
fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The current Unix second.
fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The account whose store an asset read reaches: the one the
/// `Authorization` header names, or, when the request sends none, the one
/// its `media_link` names. Taken by `GET /v1/assets/{sha256}` and
/// `GET /v1/assets/{sha256}/preview`, the two routes a media element loads.
///
/// A request that sends the header is judged by the header alone, as on
/// every other route: a session, or an API token with the export scope
/// (`require_asset_read_access`).
pub(crate) struct AssetReader {
    /// The account whose asset store the read looks in.
    pub(crate) account_id: i64,
}

impl axum::extract::FromRequestParts<AppState> for AssetReader {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if parts.headers.contains_key(header::AUTHORIZATION) {
            let auth = resolve_auth(&parts.headers, state).await?;
            require_asset_read_access(&auth)?;
            return Ok(Self {
                account_id: auth.account_id,
            });
        }
        let Some(link) = media_link_in(parts.uri.query()) else {
            return Err(ApiError::AuthenticationRequired(
                "missing Authorization: Bearer <token>, and no media_link".into(),
            ));
        };
        let AxumPath(sha256) =
            <AxumPath<Sha256> as axum::extract::FromRequestParts<AppState>>::from_request_parts(
                parts, state,
            )
            .await?;
        let account_id = open_media_link(state, &link, &sha256).await?;
        Ok(Self { account_id })
    }
}

/// The first `media_link` in a query string, decoded.
fn media_link_in(query: Option<&str>) -> Option<String> {
    let uri: axum::http::Uri = format!("/?{}", query?).parse().ok()?;
    axum::extract::Query::<Vec<(String, String)>>::try_from_uri(&uri)
        .ok()?
        .0
        .into_iter()
        .find_map(|(name, value)| (name == MEDIA_LINK_PARAM).then_some(value))
}

/// Check `link` against the asset at `sha256` and answer the account it
/// opens that asset for.
///
/// # Errors
///
/// `media-link-invalid` when the link cannot be read, has expired, was signed
/// for other terms, or its Session has ended; `account-disabled` when its
/// account is disabled.
async fn open_media_link(state: &AppState, link: &str, sha256: &Sha256) -> Result<i64, ApiError> {
    let invalid = |why: &str| ApiError::MediaLinkInvalid(format!("{why}; make a new media link"));
    let Some(terms) = LinkTerms::parse(link) else {
        return Err(invalid("the media_link is not a media link"));
    };
    if terms.expires <= now_unix() {
        return Err(invalid("the media link expired"));
    }
    let mut conn = state.db.acquire().await?;
    let session_hash = session_tokens::live_session_hash(&mut conn, terms.account_id).await?;
    let signed = session_hash.is_some_and(|session_hash| {
        state.media_link_key.verifies(
            terms.account_id,
            sha256,
            terms.expires,
            &session_hash,
            &terms.signature,
        )
    });
    if !signed {
        return Err(invalid(
            "the media link was not made for this asset, or the Session that made it has ended",
        ));
    }
    let auth = account_profile::load_account_auth(&mut conn, terms.account_id)
        .await?
        .ok_or_else(|| invalid("the account the media link names no longer exists"))?;
    if auth.disabled {
        return Err(ApiError::AccountDisabled("this account is disabled".into()));
    }
    Ok(terms.account_id)
}

/// A media link: the URLs a media element loads to read one asset with no
/// `Authorization` header, and when they stop working.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct MediaLink {
    /// `/v1/assets/{sha256}?media_link=…`: the asset's own bytes.
    url: String,
    /// `/v1/assets/{sha256}/preview?media_link=…`: the asset's Preview, which
    /// answers `404` when the asset has none (the attachment's
    /// `preview_mime_type` says whether it has one).
    preview_url: String,
    /// When the link stops working, RFC 3339 UTC. It stops sooner if the
    /// Session that made it ends.
    expires_at: String,
}

/// Make a media link: URLs that read one asset with no `Authorization`
/// header, for a media element's `src`.
///
/// A media element cannot send the Session's header, so the web app asks for
/// a link and loads the URLs it answers. The link opens this asset and its
/// Preview, in this account's store, for an hour, and stops sooner when the
/// Session that made it ends. Its URLs take `Range` like any read of the
/// asset. Only a Session makes one: a program sends its token in the header.
#[utoipa::path(
    post,
    path = "/v1/assets/{sha256}/media-links",
    tag = "Assets",
    security(("session" = [])),
    params(
        ("sha256" = String, Path, description = "Content SHA-256 hex")
    ),
    responses(
        (
            status = 201,
            body = MediaLink,
            description = "The media link was made",
            headers(("Location" = String, description = "The link's `url`, which reads the asset"))
        ),
    )
)]
pub(crate) async fn create_media_link(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    headers: HeaderMap,
    AxumPath(sha256): AxumPath<Sha256>,
) -> Result<Response, ApiError> {
    let assets_dir = state.cfg.paths.assets_dir_for_account(auth.account_id);
    let lookup = sha256.clone();
    let held = tokio::task::spawn_blocking(move || {
        lookup_by_sha256_unverified(&assets_dir, &lookup).is_some()
    })
    .await
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("asset lookup task: {e}")))?;
    if !held {
        return Err(ApiError::NotFound("asset not found".into()));
    }

    let session_hash = session_tokens::hash_api_token(&bearer_token(&headers)?);
    let ttl = i64::try_from(MEDIA_LINK_TTL.as_secs()).unwrap_or(i64::MAX);
    let expires = now_unix().saturating_add(ttl);
    let link = state
        .media_link_key
        .sign(auth.account_id, &sha256, expires, &session_hash);
    let url = format!("/v1/assets/{sha256}?{MEDIA_LINK_PARAM}={link}");
    let body = MediaLink {
        preview_url: format!("/v1/assets/{sha256}/preview?{MEDIA_LINK_PARAM}={link}"),
        expires_at: chrono::DateTime::from_timestamp(expires, 0)
            .unwrap_or_default()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        url: url.clone(),
    };
    Ok(Created {
        location: url,
        body,
    }
    .into_response())
}

#[cfg(test)]
mod tests;
