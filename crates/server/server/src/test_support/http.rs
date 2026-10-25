//! Requests against the real app over HTTP: [`serve`] starts it, and the
//! helpers here add accounts and log in, issue one request with each verb, and
//! check the problem body of a failed one.

use axum::http::StatusCode;
use serde::de::DeserializeOwned;

use super::http_client;
use crate::server::{AppState, http_app};

/// An account a test added, with a live session token for it.
pub struct RegisteredAccount {
    /// The new account's id.
    pub account_id: i64,
    /// The username it was created with.
    pub username: String,
    /// A live session token for it.
    pub token: String,
}

/// A running instance of the real axum app on an ephemeral port.
///
/// The task is aborted when this value drops, so it must stay alive until the
/// response body has been read. A helper must not hand back a response whose
/// server has already been told to stop, so the body is always read before
/// this value drops, regardless of how the runtime handles shutdown.
///
/// It builds each request a test sends it, [`Self::request`] and the verbs
/// beside it, with its own client and its own address, so a test names only
/// the method and the path.
pub struct TestServer {
    base: String,
    client: reqwest::Client,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    /// A `method` request to `path` on this server. `path` starts with `/`
    /// and may carry a query.
    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client.request(method, format!("{}{path}", self.base))
    }

    /// A `GET` request to `path` on this server.
    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, path)
    }

    /// A `HEAD` request to `path` on this server.
    pub fn head(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::HEAD, path)
    }

    /// A `POST` request to `path` on this server.
    pub fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::POST, path)
    }

    /// A `PUT` request to `path` on this server.
    pub fn put(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::PUT, path)
    }

    /// A `PATCH` request to `path` on this server.
    pub fn patch(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::PATCH, path)
    }

    /// A `DELETE` request to `path` on this server.
    pub fn delete(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::DELETE, path)
    }

    /// `http://127.0.0.1:<port>`, for a test that reaches the server other
    /// than through [`Self::request`]: over a raw socket, or with a URL it
    /// builds itself.
    pub fn base(&self) -> &str {
        &self.base
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Start the real axum app for `state` on an ephemeral port.
///
/// Every HTTP helper below goes through this; a test that serves a router
/// other than `http_app` (the public auth router on its own, say) takes
/// [`serve_router`] directly.
pub async fn serve(state: &AppState) -> TestServer {
    serve_router(http_app(state.clone())).await
}

/// Start `app` on an ephemeral port.
///
/// This is the one place in the test suite that binds a listener. The server
/// task is aborted when the returned [`TestServer`] drops.
pub async fn serve_router(app: axum::Router) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = crate::request_id::spawn_in_current_span(async move {
        axum::serve(listener, app).await.unwrap();
    });
    TestServer {
        base: format!("http://{address}"),
        client: http_client(),
        handle,
    }
}

/// Issue one request against a freshly started app and read the whole
/// response. `body` is a content type and the bytes to send with it.
async fn request(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    token: Option<&str>,
    body: Option<(&str, reqwest::Body)>,
) -> (StatusCode, String) {
    let server = serve(state).await;
    let mut req = server.request(method, path);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    if let Some((content_type, body)) = body {
        req = req
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
    }
    let response = req.send().await.unwrap();
    let status = response.status();
    // Read the body before `server` drops and aborts the task.
    let text = response.text().await.unwrap();
    (status, text)
}

/// The JSON body every typed helper sends.
fn json_body(value: serde_json::Value) -> (&'static str, reqwest::Body) {
    (
        "application/json",
        reqwest::Body::from(serde_json::to_vec(&value).expect("test JSON always serializes")),
    )
}

/// Decode a response the caller expects to be `200 OK` with a JSON body.
fn expect_ok<T: DeserializeOwned>(what: &str, status: StatusCode, text: &str) -> T {
    assert_eq!(status, StatusCode::OK, "{what} must succeed, got: {text}");
    decode(what, text)
}

/// Decode a response body into the caller's type, or panic naming the
/// request and the body. The body may be JSON of the wrong shape, so the
/// message does not call it non-JSON.
fn decode<T: DeserializeOwned>(what: &str, text: &str) -> T {
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("{what} answered a body that does not decode ({e}): {text}"))
}

/// Create an account the way the administrator's `POST /v1/accounts` does,
/// with no display name and no phone, then log in as it through
/// `POST /v1/session` and return it with the live session token.
///
/// The account is written to the database rather than through the route,
/// because the route takes the administrator's Session and opening one would
/// end the Session a test may already hold for `admin`.
pub async fn add_account_and_log_in(
    state: &AppState,
    username: &str,
    password: &str,
) -> RegisteredAccount {
    let hash = crate::credentials::hash_password(password).expect("hash the account password");
    let mut conn = state.db.acquire().await.expect("acquire to add an account");
    let mut tx = crate::db::begin_write(&mut conn)
        .await
        .expect("begin the account write");
    let account_id =
        crate::db::account_profile::insert_account(&mut tx, username, Some(&hash), None)
            .await
            .unwrap_or_else(|e| panic!("add account {username}: {e:#}"));
    crate::db::account_profile::set_must_set_up_profile(&mut tx, account_id, true)
        .await
        .expect("mark the account as owing profile setup");
    crate::db::audit_trail::record(
        &mut tx,
        &crate::db::audit_trail::NewEntry::about(
            crate::db::audit_trail::AuditAction::AccountCreated,
            crate::db::audit_trail::AuditActor::Admin,
            (account_id, username),
        ),
        chrono::Utc::now(),
    )
    .await
    .expect("record the account's creation");
    tx.commit().await.expect("commit the account write");
    drop(conn);

    let body = log_in(state, username, password).await;
    RegisteredAccount {
        account_id,
        username: username.to_string(),
        token: body["token"].as_str().unwrap().to_string(),
    }
}

/// Give the administrator `password` and log in as `admin`. The password is
/// written straight to the database, not through `POST /v1/server/setup`,
/// so nothing is recorded in the Audit Trail for it.
pub async fn log_in_as_admin(state: &AppState, password: &str) -> RegisteredAccount {
    let hash = crate::credentials::hash_password(password).expect("hash the admin password");
    let mut conn = state.db.acquire().await.expect("acquire for setup");
    crate::db::account_profile::update_password_hash(
        &mut conn,
        crate::db::account_profile::ADMIN_ACCOUNT_ID,
        Some(&hash),
    )
    .await
    .expect("set the admin password");
    drop(conn);

    let body = log_in(state, crate::db::account_profile::ADMIN_USERNAME, password).await;
    RegisteredAccount {
        account_id: crate::db::account_profile::ADMIN_ACCOUNT_ID,
        username: crate::db::account_profile::ADMIN_USERNAME.to_string(),
        token: body["token"].as_str().unwrap().to_string(),
    }
}

/// The status of a login attempt, `POST /v1/session`.
pub async fn login_status(state: &AppState, username: &str, password: &str) -> StatusCode {
    request(
        state,
        reqwest::Method::POST,
        "/v1/session",
        None,
        Some(json_body(
            serde_json::json!({ "username": username, "password": password }),
        )),
    )
    .await
    .0
}

/// A login attempt, `POST /v1/session`: the status and the response text, so
/// a refused login can be checked with [`expect_problem`].
pub async fn log_in_raw(state: &AppState, username: &str, password: &str) -> (StatusCode, String) {
    request(
        state,
        reqwest::Method::POST,
        "/v1/session",
        None,
        Some(json_body(
            serde_json::json!({ "username": username, "password": password }),
        )),
    )
    .await
}

/// Log in through `POST /v1/session`, asserting the `201 Created` and the
/// `Location: /v1/session` the singleton answers with, and return the body
/// (`token`, `account_id`, `username`).
pub async fn log_in(state: &AppState, username: &str, password: &str) -> serde_json::Value {
    let server = serve(state).await;
    let response = server
        .post("/v1/session")
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .unwrap();
    let (location, body) = created(&format!("logging in as {username}"), response).await;
    assert_eq!(
        location.as_deref(),
        Some("/v1/session"),
        "logging in must answer Location: /v1/session"
    );
    body
}

/// GET a path with a Bearer token, returning only the status.
pub async fn get_status(state: &AppState, path: &str, token: &str) -> StatusCode {
    request(state, reqwest::Method::GET, path, Some(token), None)
        .await
        .0
}

/// GET a path with a Bearer token and decode the JSON body.
pub async fn get_json<T: DeserializeOwned>(state: &AppState, path: &str, token: &str) -> T {
    let (status, text) = request(state, reqwest::Method::GET, path, Some(token), None).await;
    expect_ok(&format!("GET {path}"), status, &text)
}

/// POST a JSON body with a Bearer token and decode the JSON response.
pub async fn post_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> T {
    let (status, text) = request(
        state,
        reqwest::Method::POST,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_ok(&format!("POST {path}"), status, &text)
}

/// POST a JSON body to a route that creates one resource, asserting
/// `201 Created` and a `Location` under the request path, and returning the
/// body. The `Location` is handed back with it so a test can check it names
/// the id the body carries.
pub async fn post_created_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (String, T) {
    let server = serve(state).await;
    let response = server
        .post(path)
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&body).expect("test JSON always serializes"))
        .send()
        .await
        .unwrap();
    let (location, parsed) = created(&format!("POST {path}"), response).await;
    let location =
        location.unwrap_or_else(|| panic!("POST {path} answered 201 without a Location"));
    let collection = path.split('?').next().unwrap_or(path);
    assert!(
        location.starts_with(&format!("{collection}/")),
        "POST {path} Location must name a member under it, got {location}"
    );
    (location, parsed)
}

/// The `Location` header of a response, if it has one that is text.
pub fn location(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Read a response the caller expects to be `201 Created` with a JSON body:
/// assert the status, with the body in the message, and return the
/// `Location` header and the decoded body. `what` names the request in the
/// assertion messages, as it does for [`expect_ok`].
async fn created<T: DeserializeOwned>(
    what: &str,
    response: reqwest::Response,
) -> (Option<String>, T) {
    let status = response.status();
    let location = location(&response);
    let text = response.text().await.unwrap();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{what} must answer 201 Created, got: {text}"
    );
    (location, decode(what, &text))
}

/// The problem document a failure answered, or a panic naming the body
/// that was not one.
pub fn problem(text: &str) -> api_types::Problem {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not a problem document ({e}): {text}"))
}

/// Assert a failure is a problem of `kind` with the status the registry
/// gives it, and hand the document back for any further check.
pub fn expect_problem(
    status: StatusCode,
    text: &str,
    kind: crate::problem::ProblemType,
) -> api_types::Problem {
    check_problem(status, text, kind, text)
}

/// [`expect_problem`] with `what` at the head of every failure: it names
/// the check, as in `attempt 3 as aLice` for one of a loop, or says why a
/// single refusal is expected.
pub fn expect_problem_for(
    what: &str,
    status: StatusCode,
    text: &str,
    kind: crate::problem::ProblemType,
) -> api_types::Problem {
    check_problem(status, text, kind, &format!("{what}: {text}"))
}

/// The checks of [`expect_problem`], with `context` in every failure.
fn check_problem(
    status: StatusCode,
    text: &str,
    kind: crate::problem::ProblemType,
    context: &str,
) -> api_types::Problem {
    let problem = problem(text);
    assert_eq!(status, kind.status(), "{context}");
    assert_eq!(problem.status, kind.status().as_u16(), "{context}");
    assert_eq!(problem.kind, kind.url(), "{context}");
    assert_eq!(problem.title, kind.title(), "{context}");
    assert!(problem.request_id.is_some(), "no request_id: {context}");
    problem
}

/// Read a response a test sent itself and check it with [`expect_problem`].
pub async fn expect_problem_response(
    response: reqwest::Response,
    kind: crate::problem::ProblemType,
) -> api_types::Problem {
    let status = response.status();
    let text = response.text().await.unwrap();
    expect_problem(status, &text, kind)
}

/// Assert a failure is the `500 Internal Server Error` problem, which has no
/// [`ProblemType`](crate::problem::ProblemType) for [`expect_problem`] to take.
pub fn expect_internal_problem(status: StatusCode, text: &str) -> api_types::Problem {
    let problem = problem(text);
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{text}");
    assert_eq!(problem.status, 500, "{text}");
    assert_eq!(problem.kind, crate::problem::INTERNAL_TYPE, "{text}");
    assert_eq!(problem.title, "Internal server error", "{text}");
    assert!(problem.request_id.is_some(), "no request_id: {text}");
    problem
}

/// POST a JSON body with a Bearer token, returning only the status.
pub async fn post_status(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::POST,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await
    .0
}

/// Send a JSON body with a Bearer token, returning the status and the
/// response text, for a refusal checked with [`expect_problem`]. The four
/// `*_json_raw` helpers below name the method.
async fn json_raw(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    request(state, method, path, Some(token), Some(json_body(body))).await
}

/// POST a JSON body with a Bearer token, returning the status and the
/// response text, for a refusal checked with [`expect_problem`].
pub async fn post_json_raw(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    json_raw(state, reqwest::Method::POST, path, token, body).await
}

/// PUT a JSON body with a Bearer token, returning the status and the
/// response text, for a refusal checked with [`expect_problem`].
pub async fn put_json_raw(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    json_raw(state, reqwest::Method::PUT, path, token, body).await
}

/// POST a JSON body with no credential at all, returning only the status.
/// For the routes a stranger calls: logging in, setting up the server.
pub async fn post_status_logged_out(
    state: &AppState,
    path: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::POST,
        path,
        None,
        Some(json_body(body)),
    )
    .await
    .0
}

/// POST a JSON body with no credential at all, returning the status and the
/// response text, so a refusal can be checked with [`expect_problem`].
pub async fn post_logged_out(
    state: &AppState,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    request(
        state,
        reqwest::Method::POST,
        path,
        None,
        Some(json_body(body)),
    )
    .await
}

/// PUT a JSON body with a Bearer token, asserting 200 and parsing the body.
pub async fn put_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> T {
    let (status, text) = request(
        state,
        reqwest::Method::PUT,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_ok(&format!("PUT {path}"), status, &text)
}

/// DELETE with a JSON body and a Bearer token, returning only the status.
/// An account deleting itself or its messages carries its confirmation in
/// the body.
pub async fn delete_status_with_body(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::DELETE,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await
    .0
}

/// DELETE with a JSON body and a Bearer token, returning the status and the
/// response text, for a refusal checked with [`expect_problem`].
pub async fn delete_json_raw(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    json_raw(state, reqwest::Method::DELETE, path, token, body).await
}

/// DELETE with a JSON body and a Bearer token, asserting 200 and parsing
/// the body.
pub async fn delete_json_with_body<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> T {
    let (status, text) = request(
        state,
        reqwest::Method::DELETE,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_ok(&format!("DELETE {path}"), status, &text)
}

/// PUT a JSON body with a Bearer token, returning only the status.
pub async fn put_status(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::PUT,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await
    .0
}

/// PATCH a JSON body with a Bearer token, returning only the status.
pub async fn patch_status(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::PATCH,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await
    .0
}

/// PATCH a JSON body expecting a problem of `kind`, checked with
/// [`expect_problem`], and return the problem's sentence.
///
/// A route test asserting only a status cannot tell a refusal the person can
/// act on from a different refusal with the same status, so a route that
/// starts answering the wrong sentence stays green.
pub async fn patch_failure(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
    kind: crate::problem::ProblemType,
) -> String {
    let (status, text) = request(
        state,
        reqwest::Method::PATCH,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_problem(status, &text, kind).sentence()
}

/// PATCH a JSON body with a Bearer token, returning the status and the
/// response text, for a refusal checked with [`expect_problem`].
pub async fn patch_json_raw(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    json_raw(state, reqwest::Method::PATCH, path, token, body).await
}

/// PATCH a JSON body with a Bearer token and decode the JSON response.
pub async fn patch_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> T {
    let (status, text) = request(
        state,
        reqwest::Method::PATCH,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_ok(&format!("PATCH {path}"), status, &text)
}

/// DELETE a path with a Bearer token, returning only the status.
pub async fn delete_status(state: &AppState, path: &str, token: &str) -> StatusCode {
    request(state, reqwest::Method::DELETE, path, Some(token), None)
        .await
        .0
}

/// DELETE a path with a Bearer token and decode the JSON response.
pub async fn delete_json<T: DeserializeOwned>(state: &AppState, path: &str, token: &str) -> T {
    let (status, text) = request(state, reqwest::Method::DELETE, path, Some(token), None).await;
    expect_ok(&format!("DELETE {path}"), status, &text)
}

/// POST a body that is not JSON (JSONL, plain text, an empty body) with a
/// Bearer token and an explicit Content-Type, returning the status and the
/// response text. For routes whose contract is the raw body, such as
/// `POST /v1/imports/{id}/batches`.
pub async fn post_raw(
    state: &AppState,
    path: &str,
    token: &str,
    content_type: &str,
    body: impl Into<reqwest::Body>,
) -> (StatusCode, String) {
    request(
        state,
        reqwest::Method::POST,
        path,
        Some(token),
        Some((content_type, body.into())),
    )
    .await
}

/// PUT a body that is not JSON with a Bearer token and an explicit
/// Content-Type, returning the status and the response text. For routes whose
/// contract is the raw body, such as `PUT /v1/assets/{sha256}`.
pub async fn put_raw(
    state: &AppState,
    path: &str,
    token: &str,
    content_type: &str,
    body: impl Into<reqwest::Body>,
) -> (StatusCode, String) {
    request(
        state,
        reqwest::Method::PUT,
        path,
        Some(token),
        Some((content_type, body.into())),
    )
    .await
}

/// GET a path with a Bearer token, returning the status and the raw response
/// text. For asserting on a non-JSON or malformed body, such as the error
/// fallbacks' JSON that a plain `get_json` would panic decoding on failure.
pub async fn get_raw(state: &AppState, path: &str, token: &str) -> (StatusCode, String) {
    request(state, reqwest::Method::GET, path, Some(token), None).await
}

/// DELETE a path with a Bearer token, returning the status and the raw
/// response text. For asserting on the body of a fallback response, such as
/// the problem document a wrong method produces.
pub async fn delete_raw(state: &AppState, path: &str, token: &str) -> (StatusCode, String) {
    request(state, reqwest::Method::DELETE, path, Some(token), None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        SeedConversation, SeedMessage, fixture_with_account, seed_conversation, stored_time,
    };

    /// A body far larger than one TCP segment must come back whole. This
    /// pins the ordering in `request`: the response is read before the
    /// `TestServer` drops and aborts the task serving it.
    #[tokio::test]
    async fn a_large_response_body_is_read_before_the_server_stops() {
        let (fixture, user) = fixture_with_account().await;
        for i in 0..300 {
            seed_conversation(
                &fixture.state,
                &SeedConversation {
                    account_id: user.account_id,
                    handle: &format!("+1015555{i:04}"),
                    conversation_type: "one-to-one",
                    group_title: None,
                    source_file: "seed.jsonl",
                    messages: &[SeedMessage {
                        source: "imessage",
                        timestamp: stored_time("2020-01-01T00:00:00.000Z"),
                        is_from_me: true,
                        body: "hello, this is a message long enough to add up",
                    }],
                },
            )
            .await;
        }

        let (status, text) =
            get_raw(&fixture.state, "/v1/conversations?limit=300", &user.token).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert!(
            text.len() > 64 * 1024,
            "the fixture must produce a body bigger than one segment, got {} bytes",
            text.len()
        );
        let page: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("truncated body ({e}): {} bytes", text.len()));
        assert_eq!(page["items"].as_array().unwrap().len(), 300);
    }
}
