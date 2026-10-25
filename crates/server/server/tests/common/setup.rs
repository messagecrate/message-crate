//! Calls to a running server, and the setup, create account, and log in
//! sequence that gives a test the administrator and an account.

use serde_json::{Value, json};

/// The administrator's password.
pub const ADMIN_PASSWORD: &str = "Admin-Pw-7q2Lx9Vb";
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

/// What `set_up_with_account` answers, each named so the two session
/// tokens cannot be swapped.
pub struct SetUp {
    /// The administrator's session token.
    pub admin_token: String,
    /// Alice's account id.
    pub alice_id: i64,
    /// Alice's session token.
    pub alice_token: String,
}

/// Sets up the Message Crate with the administrator's password, creates
/// `alice` as the administrator, and logs her in.
pub async fn set_up_with_account(base: &str) -> SetUp {
    use reqwest::Method;
    use reqwest::StatusCode as S;

    let (status, set_up) = call(
        base,
        Method::POST,
        "/v1/server/setup",
        None,
        json_body(&json!({ "password": ADMIN_PASSWORD })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{set_up}");
    let admin = set_up["token"].as_str().unwrap().to_string();
    let (status, created) = call(
        base,
        Method::POST,
        "/v1/accounts",
        Some(&admin),
        json_body(&json!({ "username": "alice", "password": ALICE_PASSWORD })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{created}");
    let alice_id = created["account_id"].as_i64().unwrap();
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
    SetUp {
        admin_token: admin,
        alice_id,
        alice_token: alice,
    }
}
