use super::*;
use crate::problem::ProblemType;
use crate::test_support::{expect_problem, expect_problem_for, test_fixture};

const TEST_ACCOUNT: i64 = 7;

#[tokio::test]
async fn api_token_cannot_exceed_its_owner() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    sqlx::query("UPDATE accounts SET can_import = 0 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();
    let created = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "tool",
        Permissions::all(),
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    let identity = resolve_auth_on_conn(
        &mut conn,
        &created.token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    assert!(
        !identity.permissions().import,
        "the account lost import, so its token must not have it"
    );
    assert!(identity.permissions().export);
}

/// The schema is made sure of when the pool opens, and never by a request:
/// a database another program stamped with another fingerprint while this
/// server serves it is not rebuilt by the next request, which would empty
/// it from inside a request (#2064).
#[tokio::test]
async fn a_request_never_rebuilds_the_database() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let token = session_tokens::insert_account_session_token(
        &mut conn,
        TEST_ACCOUNT,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    let other = crate::db::schema::SCHEMA_FINGERPRINT ^ 1;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA user_version = {other}"
    )))
    .execute(&mut *conn)
    .await
    .unwrap();

    resolve_auth_on_conn(
        &mut conn,
        &token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(accounts, 1, "the request left the database as it was");
    let stamped: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(stamped, other);
}

#[tokio::test]
async fn disabling_an_account_kills_its_live_session() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let token = session_tokens::insert_account_session_token(
        &mut conn,
        TEST_ACCOUNT,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    // The token works while the account is active.
    resolve_auth_on_conn(
        &mut conn,
        &token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();

    let err = resolve_auth_on_conn(
        &mut conn,
        &token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::AccountDisabled(_)),
        "a disabled account's existing token must stop working, got {err:?}"
    );
}

#[tokio::test]
async fn disabling_an_account_kills_its_live_api_token() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let created = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "tool",
        Permissions::all(),
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    let token = created.token;

    // The API token works while the account is active.
    resolve_auth_on_conn(
        &mut conn,
        &token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();

    let err = resolve_auth_on_conn(
        &mut conn,
        &token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ApiError::AccountDisabled(_)),
        "a disabled account's existing API token must stop working, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// The app a session connects with
// ---------------------------------------------------------------------------

fn app_headers(app: &str, version: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(APP_HEADER, app.parse().unwrap());
    headers.insert(APP_VERSION_HEADER, version.parse().unwrap());
    headers
}

#[test]
fn a_request_names_its_app_with_two_headers() {
    let app = connecting_app(&app_headers("desktop", "0.9.0+343fe0d8")).unwrap();
    assert_eq!(app.kind, session_tokens::AppKind::Desktop);
    assert_eq!(app.build, "0.9.0+343fe0d8");
}

#[test]
fn a_request_that_names_no_app_or_names_it_badly_records_nothing() {
    assert_eq!(connecting_app(&HeaderMap::new()), None);
    assert_eq!(connecting_app(&app_headers("toaster", "0.9.0")), None);
    assert_eq!(connecting_app(&app_headers("website", "")), None);
    assert_eq!(
        connecting_app(&app_headers("website", &"9".repeat(65))),
        None
    );
    let mut only_the_app = HeaderMap::new();
    only_the_app.insert(APP_HEADER, "website".parse().unwrap());
    assert_eq!(connecting_app(&only_the_app), None);
}

#[tokio::test]
async fn a_session_records_the_app_it_connects_with() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let token = session_tokens::insert_account_session_token(
        &mut conn,
        TEST_ACCOUNT,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    // A request that names no app is served and leaves the row as it was.
    resolve_auth_on_conn(
        &mut conn,
        &token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    assert_eq!(
        session_tokens::connecting_app_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap(),
        None
    );

    let desktop = session_tokens::ConnectingApp {
        kind: session_tokens::AppKind::Desktop,
        build: "0.9.0+343fe0d8".into(),
    };
    resolve_auth_on_conn(
        &mut conn,
        &token,
        Some(&desktop),
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    assert_eq!(
        session_tokens::connecting_app_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap(),
        Some(desktop)
    );

    // The same session from an updated app: the row follows, no new login.
    let website = session_tokens::ConnectingApp {
        kind: session_tokens::AppKind::Website,
        build: "0.10.0".into(),
    };
    resolve_auth_on_conn(
        &mut conn,
        &token,
        Some(&website),
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    assert_eq!(
        session_tokens::connecting_app_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap(),
        Some(website)
    );
}

/// An API token belongs to a program, not to either app, and has no session
/// row to write to.
#[tokio::test]
async fn an_api_token_records_no_app() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    session_tokens::insert_account_session_token(
        &mut conn,
        TEST_ACCOUNT,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    let created = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "tool",
        Permissions::all(),
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    let desktop = session_tokens::ConnectingApp {
        kind: session_tokens::AppKind::Desktop,
        build: "0.9.0".into(),
    };
    resolve_auth_on_conn(
        &mut conn,
        &created.token,
        Some(&desktop),
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    assert_eq!(
        session_tokens::connecting_app_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap(),
        None
    );
}

/// Make every UPDATE of `table` fail, as it does when another connection
/// holds SQLite's write lock past `busy_timeout`. Reads still succeed.
async fn fail_updates_of(conn: &mut SqliteConnection, table: &str) {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TEMP TRIGGER fail_update_{table} BEFORE UPDATE ON main.{table}
         BEGIN SELECT RAISE(ABORT, 'database is locked'); END"
    )))
    .execute(&mut *conn)
    .await
    .unwrap();
}

/// A failed `last_accessed_at` write does not fail the request (#1189).
#[tokio::test]
async fn an_api_token_works_when_its_last_used_time_cannot_be_written() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let created = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "tool",
        Permissions::all(),
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    fail_updates_of(&mut conn, "account_api_tokens").await;

    let identity = resolve_auth_on_conn(
        &mut conn,
        &created.token,
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .expect("the token is still accepted");

    assert_eq!(identity.account_id, TEST_ACCOUNT);
}

/// A failed write of the connecting app does not fail the request (#1189).
#[tokio::test]
async fn a_session_works_when_its_connecting_app_cannot_be_written() {
    let fixture = test_fixture().await;
    fixture.account_with_id(TEST_ACCOUNT, "alice").await;
    let mut conn = fixture.conn().await;
    let token = session_tokens::insert_account_session_token(
        &mut conn,
        TEST_ACCOUNT,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    fail_updates_of(&mut conn, "account_session_tokens").await;
    let desktop = session_tokens::ConnectingApp {
        kind: session_tokens::AppKind::Desktop,
        build: "0.10.0".into(),
    };

    let identity = resolve_auth_on_conn(
        &mut conn,
        &token,
        Some(&desktop),
        crate::test_support::fixed_instant(),
    )
    .await
    .expect("the session is still accepted");

    assert_eq!(identity.account_id, TEST_ACCOUNT);
}

/// The `Bearer` scheme matches in any case, because RFC 7235, section 2.1,
/// makes authentication schemes case-insensitive (#1211).
#[tokio::test]
async fn a_lower_case_bearer_scheme_is_accepted() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let response = server
        .get("/v1/session")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("bearer {}", user.token),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
}

/// A valid token under another scheme is still refused, so matching the
/// scheme without regard to case does not accept any word before the token.
#[tokio::test]
async fn a_valid_token_under_another_scheme_answers_401() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let response = server
        .get("/v1/session")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Basic {}", user.token),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 401);
}

/// No API token acts as the owner (ADR 0008). The route that issues tokens
/// refuses the owner, but a token row on the owner's account can still
/// exist: written by hand, or carried over in a restored database. A request
/// with it answers the same `401 Unauthorized` as a token the server never
/// issued, on a route the owner's session reaches, on one an import reaches,
/// and on logging out, and leaves no record of a use on the token.
#[tokio::test]
async fn an_api_token_on_the_owners_account_answers_401_like_an_unknown_token() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    crate::test_support::claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let created = {
        let mut conn = fixture.conn().await;
        api_tokens::create_api_token(
            &mut conn,
            account_profile::OWNER_ACCOUNT_ID,
            "by hand",
            Permissions::all(),
            None,
            // Live by the router's clock, so only the owner rule can refuse it.
            crate::test_support::routed_credential_instant(),
        )
        .await
        .unwrap()
    };

    let send = async |method: &str, path: &str, token: &str| match method {
        "GET" => crate::test_support::get_raw(&state, path, token).await,
        "DELETE" => crate::test_support::delete_raw(&state, path, token).await,
        other => panic!("no helper for {other}"),
    };
    for (method, path) in [
        ("GET", "/v1/session"),
        ("GET", "/v1/accounts"),
        ("GET", "/v1/imports"),
        ("DELETE", "/v1/session"),
    ] {
        let (status, text) = send(method, path, &created.token).await;
        let owners = expect_problem_for(
            &format!("{method} {path} with a token on the owner's account"),
            status,
            &text,
            ProblemType::AuthenticationRequired,
        );
        let (status, text) = send(method, path, "mc-api-never-issued").await;
        let unknown = expect_problem(status, &text, ProblemType::AuthenticationRequired);
        assert_eq!(owners.detail, unknown.detail, "{method} {path}");
    }

    let last_used: Option<String> =
        sqlx::query_scalar("SELECT last_accessed_at FROM account_api_tokens WHERE id = $1")
            .bind(created.id)
            .fetch_one(&mut *fixture.conn().await)
            .await
            .unwrap();
    assert_eq!(last_used, None, "a refused token records no use");
}

/// The Demo Account's refusal is one sentence in lowercase with no final
/// period, the form `docs/architecture/rules/http-api.md`, API-53, gives every
/// `detail`, and keeps "Demo Account" capitalized (#2817). Any other account
/// passes.
#[test]
fn the_demo_account_refusal_is_one_lowercase_sentence() {
    let demo = account_profile::DEMO_ACCOUNT_ID;
    let refusal = refuse_for_demo_account(demo, "password cannot be set")
        .expect_err("the Demo Account is refused");
    let ApiError::DemoAccountProtected(detail) = refusal else {
        panic!("not demo-account-protected: {refusal:?}");
    };
    assert_eq!(detail, "the Demo Account's password cannot be set");
    assert!(refuse_for_demo_account(demo + 1, "password cannot be set").is_ok());
}

/// One move of a Session's expiry runs at a time: a second is refused while
/// the first runs, and allowed once the first has ended however it ended.
#[test]
fn one_expiry_move_runs_at_a_time() {
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let first = MoveRunning::claim(&running).expect("nothing runs yet");
    assert!(MoveRunning::claim(&running).is_none(), "one is running");
    drop(first);
    assert!(MoveRunning::claim(&running).is_some(), "the first ended");
}
