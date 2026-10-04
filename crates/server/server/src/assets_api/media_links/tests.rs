use axum::http::StatusCode;
use serde_json::Value;

use super::*;
use crate::assets_api::tests::{
    ORIGINAL_BYTES, PREVIEW_BYTES, UNCONVERTED_BYTES, fetch, seed_attachment_with_preview,
};
use crate::problem::ProblemType;
use crate::test_support::{RegisteredAccount, expect_problem};

/// `POST /v1/assets/{sha256}/media-links` as `token`: the status, the
/// `Location`, and the body.
async fn mint(state: &AppState, sha256: &str, token: &str) -> (StatusCode, Option<String>, String) {
    let server = crate::test_support::serve(state).await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/assets/{sha256}/media-links", server.base()))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (status, location, response.text().await.unwrap())
}

/// A media link for `sha256` that `account` minted, read back as JSON.
async fn minted(state: &AppState, sha256: &str, account: &RegisteredAccount) -> Value {
    let (status, location, text) = mint(state, sha256, &account.token).await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        location.as_deref(),
        body["url"].as_str(),
        "the Location names the link"
    );
    body
}

/// The `media_link` value out of a URL the server minted.
fn link_of(url: &str) -> String {
    url.split_once("media_link=")
        .map(|(_, link)| link.to_string())
        .unwrap_or_else(|| panic!("no media_link in {url}"))
}

/// A media element cannot send the session's `Authorization` header, so the
/// web app puts a media link in `src`: with no header at all, it opens the
/// asset and its preview, and streams a range of either.
#[tokio::test]
async fn a_media_link_opens_its_asset_and_preview_with_no_header() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = &fixture.state;
    let seeded = seed_attachment_with_preview(state, user.account_id).await;
    let sha = &seeded.with_preview;
    let link = minted(state, sha, &user).await;

    let url = link["url"].as_str().unwrap();
    assert!(
        url.starts_with(&format!("/v1/assets/{sha}?media_link=")),
        "{url}"
    );
    let preview_url = link["preview_url"].as_str().unwrap();
    assert!(
        preview_url.starts_with(&format!("/v1/assets/{sha}/preview?media_link=")),
        "{preview_url}"
    );
    let expires_at = chrono::DateTime::parse_from_rfc3339(link["expires_at"].as_str().unwrap())
        .expect("expires_at is RFC 3339");
    let lifetime = expires_at.with_timezone(&chrono::Utc) - chrono::Utc::now();
    assert!(
        lifetime > chrono::Duration::minutes(55) && lifetime <= chrono::Duration::hours(1),
        "a media link lives an hour: {lifetime}"
    );

    let original = fetch(state, url, None, &[]).await;
    assert_eq!(original.status, StatusCode::OK, "{}", original.text());
    assert_eq!(original.body, ORIGINAL_BYTES);
    let preview = fetch(state, preview_url, None, &[]).await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.text());
    assert_eq!(preview.body, PREVIEW_BYTES);

    let part = fetch(state, url, None, &[("range", "bytes=-3")]).await;
    assert_eq!(part.status, StatusCode::PARTIAL_CONTENT, "{}", part.text());
    assert_eq!(part.body, &ORIGINAL_BYTES[ORIGINAL_BYTES.len() - 3..]);
}

/// A media link names one asset. Put on another asset's address, even one
/// the same account holds, it opens nothing.
#[tokio::test]
async fn a_media_link_opens_only_its_own_asset() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = &fixture.state;
    let seeded = seed_attachment_with_preview(state, user.account_id).await;
    let link = link_of(
        minted(state, &seeded.with_preview, &user).await["url"]
            .as_str()
            .unwrap(),
    );

    for path in [
        format!("/v1/assets/{}", seeded.without_preview),
        format!("/v1/assets/{}/preview", seeded.without_preview),
    ] {
        let answer = fetch(state, &format!("{path}?media_link={link}"), None, &[]).await;
        expect_problem(answer.status, &answer.text(), ProblemType::MediaLinkInvalid);
        assert_ne!(answer.body, UNCONVERTED_BYTES);
    }
}

/// A media link reads for the account that minted it and no other: changing
/// the account it names breaks it, even where the other account holds the
/// same file, and a stranger's session cannot mint one for an asset it does
/// not hold.
#[tokio::test]
async fn a_media_link_reads_only_for_its_own_account() {
    let (fixture, alice) = crate::test_support::fixture_with_account().await;
    let state = &fixture.state;
    let bob =
        crate::test_support::register_via_api(state, "bob", crate::test_support::PASSWORD).await;
    let seeded = seed_attachment_with_preview(state, alice.account_id).await;
    let sha = &seeded.with_preview;
    // Bob holds the very same file in his own store.
    let (status, text) = crate::test_support::put_raw(
        state,
        &format!("/v1/assets/{sha}"),
        &bob.token,
        "application/octet-stream",
        ORIGINAL_BYTES.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");

    let link = link_of(minted(state, sha, &alice).await["url"].as_str().unwrap());
    let (account, rest) = link.split_once('.').unwrap();
    assert_eq!(account, alice.account_id.to_string());
    let as_bob = format!("{}.{rest}", bob.account_id);
    let answer = fetch(
        state,
        &format!("/v1/assets/{sha}?media_link={as_bob}"),
        None,
        &[],
    )
    .await;
    expect_problem(answer.status, &answer.text(), ProblemType::MediaLinkInvalid);

    // Bob holds nothing under the fingerprint of Alice's unconverted photo.
    let (status, _location, text) = mint(state, &seeded.without_preview, &bob.token).await;
    expect_problem(status, &text, ProblemType::NotFound);
}

/// A media link expires an hour after it is minted, and a link whose expiry
/// has been moved is refused for its signature.
#[tokio::test]
async fn a_media_link_expires() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = &fixture.state;
    let seeded = seed_attachment_with_preview(state, user.account_id).await;
    let sha = Sha256::parse(&seeded.with_preview).unwrap();
    let session = crate::db::session_tokens::hash_api_token(&user.token);
    let now = chrono::Utc::now().timestamp();

    let fresh = state.media_link_key.sign(&MediaLinkTerms {
        account_id: user.account_id,
        sha256: &sha,
        expires: now + 60,
        session_hash: &session,
    });
    let answer = fetch(
        state,
        &format!("/v1/assets/{sha}?media_link={fresh}"),
        None,
        &[],
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.text());

    let expired = state.media_link_key.sign(&MediaLinkTerms {
        account_id: user.account_id,
        sha256: &sha,
        expires: now - 1,
        session_hash: &session,
    });
    let answer = fetch(
        state,
        &format!("/v1/assets/{sha}?media_link={expired}"),
        None,
        &[],
    )
    .await;
    let problem = expect_problem(answer.status, &answer.text(), ProblemType::MediaLinkInvalid);
    assert!(
        problem.detail.unwrap_or_default().contains("expired"),
        "the detail says the link expired"
    );

    // The expiry is part of what is signed: moving it breaks the link.
    let minted_link = link_of(
        minted(state, sha.as_str(), &user).await["url"]
            .as_str()
            .unwrap(),
    );
    let parts: Vec<&str> = minted_link.split('.').collect();
    let later = format!("{}.{}.{}", parts[0], now + 86_400, parts[2]);
    let answer = fetch(
        state,
        &format!("/v1/assets/{sha}?media_link={later}"),
        None,
        &[],
    )
    .await;
    expect_problem(answer.status, &answer.text(), ProblemType::MediaLinkInvalid);
}

/// A media link lives no longer than the Session that minted it: logging out
/// ends both.
#[tokio::test]
async fn a_media_link_ends_with_its_session() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = &fixture.state;
    let seeded = seed_attachment_with_preview(state, user.account_id).await;
    let url = minted(state, &seeded.with_preview, &user).await["url"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(fetch(state, &url, None, &[]).await.status, StatusCode::OK);

    assert_eq!(
        crate::test_support::delete_status(state, "/v1/session", &user.token).await,
        StatusCode::NO_CONTENT
    );
    let answer = fetch(state, &url, None, &[]).await;
    expect_problem(answer.status, &answer.text(), ProblemType::MediaLinkInvalid);
}

/// A value that is not a media link at all is refused as one, and a request
/// with neither a header nor a link still asks for a credential.
#[tokio::test]
async fn a_malformed_media_link_is_refused() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = &fixture.state;
    let seeded = seed_attachment_with_preview(state, user.account_id).await;
    let sha = &seeded.with_preview;
    for link in ["", "nonsense", "1.2", "1.2.zz", "x.y.00"] {
        let answer = fetch(
            state,
            &format!("/v1/assets/{sha}?media_link={link}"),
            None,
            &[],
        )
        .await;
        expect_problem(answer.status, &answer.text(), ProblemType::MediaLinkInvalid);
    }
    let answer = fetch(state, &format!("/v1/assets/{sha}"), None, &[]).await;
    expect_problem(
        answer.status,
        &answer.text(),
        ProblemType::AuthenticationRequired,
    );
}

/// Only a person's Session mints a media link, because only a screen has a
/// media element to put one in: an API token and the owner are refused, and
/// an asset the account does not hold answers `404`.
#[tokio::test]
async fn only_a_session_mints_a_media_link_for_an_asset_it_holds() {
    let fixture = crate::test_support::test_fixture().await;
    let state = &fixture.state;
    let owner = crate::test_support::claim_as_owner(state, "keeper", "hunter2hunter2").await;
    let user = crate::test_support::register_via_api(state, "user", "hunter2hunter2").await;
    let seeded = seed_attachment_with_preview(state, user.account_id).await;
    let sha = &seeded.with_preview;

    let (_location, created): (String, Value) = crate::test_support::post_created_json(
        state,
        &format!("/v1/accounts/{}/api-tokens", user.account_id),
        &user.token,
        serde_json::json!({ "label": "t", "can_import": true, "can_export": true }),
    )
    .await;
    let token = created["token"].as_str().unwrap();
    let (status, _location, text) = mint(state, sha, token).await;
    expect_problem(status, &text, ProblemType::InsufficientScope);
    let (status, _location, text) = mint(state, sha, &owner.token).await;
    expect_problem(status, &text, ProblemType::InsufficientScope);

    let unknown = Sha256::of_bytes(b"a file nobody stored");
    let (status, _location, text) = mint(state, unknown.as_str(), &user.token).await;
    expect_problem(status, &text, ProblemType::NotFound);
}
