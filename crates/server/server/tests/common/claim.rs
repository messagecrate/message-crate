//! Calls to a running server, and the claim, open registration, register,
//! and log in sequence that gives a test an owner and an account.

use serde_json::{Value, json};

/// The owner's password.
pub const OWNER_PASSWORD: &str = "Owner-Pw-7q2Lx9Vb";
/// The account's password.
pub const ALICE_PASSWORD: &str = "Alice-Pw-3kN8wZr4";

/// One call, answered with its status and JSON body (`Null` when it has
/// none).
pub async fn call(
    base: &str,
    method: reqwest::Method,
    path: &str,
    token: Option<&str>,
    body: Option<(&str, Vec<u8>)>,
) -> (reqwest::StatusCode, Value) {
    let mut request = super::client::http_client().request(method, format!("{base}{path}"));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some((content_type, body)) = body {
        request = request
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// `value` as a JSON body for `call`.
pub fn json_body(value: &Value) -> Option<(&'static str, Vec<u8>)> {
    Some(("application/json", serde_json::to_vec(value).unwrap()))
}

/// What `claimed_with_account` answers, each named so the two session
/// tokens cannot be swapped.
pub struct Claimed {
    /// The owner's session token.
    pub owner_token: String,
    /// Alice's account id.
    pub alice_id: i64,
    /// Alice's session token.
    pub alice_token: String,
}

/// Claims the Message Crate as `keeper`, opens public registration,
/// registers `alice`, and logs her in.
pub async fn claimed_with_account(base: &str) -> Claimed {
    use reqwest::Method;
    use reqwest::StatusCode as S;

    let (status, claimed) = call(
        base,
        Method::POST,
        "/v1/server/claim",
        None,
        json_body(&json!({ "username": "keeper", "password": OWNER_PASSWORD })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{claimed}");
    let owner = claimed["token"].as_str().unwrap().to_string();
    let (status, _) = call(
        base,
        Method::PATCH,
        "/v1/server/settings",
        Some(&owner),
        json_body(&json!({ "public_registration": true })),
    )
    .await;
    assert_eq!(status, S::OK);
    let (status, registered) = call(
        base,
        Method::POST,
        "/v1/accounts",
        None,
        json_body(&json!({ "username": "alice", "password": ALICE_PASSWORD })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{registered}");
    let alice_id = registered["account_id"].as_i64().unwrap();
    let (status, session) = call(
        base,
        Method::POST,
        "/v1/session",
        None,
        json_body(&json!({ "username": "alice", "password": ALICE_PASSWORD })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{session}");
    let alice = session["token"].as_str().unwrap().to_string();
    Claimed {
        owner_token: owner,
        alice_id,
        alice_token: alice,
    }
}
