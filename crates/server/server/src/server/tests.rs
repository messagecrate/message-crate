use super::*;
use crate::test_support::test_fixture;

async fn get_path(state: AppState, path: &str) -> reqwest::Response {
    let server = crate::test_support::serve(&state).await;
    server.get(path).send().await.unwrap()
}

#[tokio::test]
async fn openapi_ui_off_does_not_serve_spec() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    assert!(!state.cfg.require_server().unwrap().openapi_ui);
    let response = get_path(state, "/openapi.json").await;
    assert_ne!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        "application/json"
    );
}

#[tokio::test]
async fn openapi_ui_on_serves_spec_without_token() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    {
        let cfg = Arc::make_mut(&mut state.cfg);
        cfg.server.as_mut().unwrap().openapi_ui = true;
    }
    let response = get_path(state, "/openapi.json").await;
    assert_eq!(response.status(), StatusCode::OK);
    let v: serde_json::Value = response.json().await.unwrap();
    assert!(v["openapi"].as_str().unwrap().starts_with("3."));
}

/// Every response carries an id the server made, a failure repeats it in the
/// body, and an id the client sends is dropped rather than kept.
#[tokio::test]
async fn every_response_carries_a_server_made_request_id_and_a_problem_repeats_it() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = fixture.state.clone();
    let server = crate::test_support::serve(&state).await;

    let ok = server
        .get("/v1/conversations")
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let id = ok.headers()[crate::request_id::HEADER]
        .to_str()
        .unwrap()
        .to_string();
    assert!(uuid::Uuid::parse_str(&id).is_ok(), "not a UUID: {id}");

    let failed = server
        .get("/v1/conversations/abc/sources")
        .bearer_auth(&user.token)
        .header(crate::request_id::HEADER, "chosen-by-the-client")
        .send()
        .await
        .unwrap();
    let header = failed.headers()[crate::request_id::HEADER]
        .to_str()
        .unwrap()
        .to_string();
    assert_ne!(header, "chosen-by-the-client");
    assert_eq!(
        failed.headers()[header::CONTENT_TYPE],
        crate::problem::Problem::CONTENT_TYPE
    );
    let status = failed.status();
    let text = failed.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(problem.request_id.as_deref(), Some(header.as_str()));
    assert!(problem.errors.is_some_and(|e| !e.is_empty()), "{text}");
}

/// Every `/v1` operation in the document refuses a query parameter it does
/// not declare, and accepts the ones it does: walked over the whole document
/// once, as `docs/architecture/http-api.md` asks of a rule every route
/// follows, so a new route is covered without anyone remembering it.
#[tokio::test]
async fn every_operation_refuses_a_query_parameter_it_does_not_declare() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let server = crate::test_support::serve(&state).await;
    let spec: serde_json::Value =
        serde_json::from_str(&crate::openapi::dump_openapi_json()).unwrap();

    let mut checked = 0;
    for (template, item) in spec["paths"].as_object().unwrap() {
        if !template.starts_with("/v1/") {
            continue;
        }
        // Any segment matches a route; the refusal comes before the handler
        // reads it, so the value never matters.
        let path = template
            .split('/')
            .map(|seg| if seg.starts_with('{') { "1" } else { seg })
            .collect::<Vec<_>>()
            .join("/");
        for (method, op) in item.as_object().unwrap() {
            let method = reqwest::Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let declared: Vec<&str> = op["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["in"] == "query")
                .filter_map(|p| p["name"].as_str())
                .collect();
            let response = server
                .request(method.clone(), &format!("{path}?no_such_parameter=1"))
                .send()
                .await
                .unwrap();
            let status = response.status();
            let text = response.text().await.unwrap();
            if method == reqwest::Method::HEAD {
                // A HEAD answer has no body to read a problem from.
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "HEAD {template}");
            } else {
                let problem = crate::test_support::expect_problem(
                    status,
                    &text,
                    crate::problem::ProblemType::ValidationFailed,
                );
                let errors = problem.errors.unwrap().join(" ");
                assert!(
                    errors.contains("no_such_parameter")
                        && declared.iter().all(|name| errors.contains(name)),
                    "{method} {template} must name the parameter and the ones it takes: {errors}"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 60, "walked only {checked} operations");
}

/// Two cases the walk above cannot reach. A path segment's name is not a
/// query parameter the route takes, and a `HEAD` the `GET` handler answers is
/// held to the `GET` operation's parameters.
#[tokio::test]
async fn a_path_parameters_name_and_a_head_request_are_held_to_the_declared_query() {
    let fixture = crate::test_support::test_fixture().await;
    let server = crate::test_support::serve(&fixture.state).await;

    let response = server.get("/v1/conversations/1?id=1").send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(
        problem.errors.unwrap(),
        ["unknown query parameter 'id'; this route takes no query parameters"]
    );

    let response = server
        .head("/v1/conversations?no_such_parameter=1")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// `/health` answers a probe that accepts only text, as a container health
/// check or a load balancer may send (#1221): it is outside `/v1`, so its
/// `Accept` is not checked.
#[tokio::test]
async fn health_answers_a_probe_that_accepts_only_text() {
    let fixture = crate::test_support::test_fixture().await;
    let server = crate::test_support::serve(&fixture.state).await;

    let response = server
        .get("/health")
        .header(header::ACCEPT, "text/plain")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "ok\n");
}

/// `/health` fails as a `/v1` call does when the database cannot be
/// reached, so a probe does not read a server as up while every `/v1` call
/// fails (#2202).
#[tokio::test]
async fn health_fails_when_the_database_cannot_be_reached() {
    let fixture = crate::test_support::test_fixture().await;
    let server = crate::test_support::serve(&fixture.state).await;
    fixture.state.db.close().await;

    let response = server.get("/health").send().await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
}

/// `/health` fails within its time limit when every pool connection is held,
/// rather than waiting for one as long as a `/v1` call would (#2202). No
/// connection is given back while it waits, so the answer does not depend on
/// how fast the machine is.
#[tokio::test]
async fn health_fails_when_every_pool_connection_is_held() {
    let fixture = crate::test_support::test_fixture().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let mut held = Vec::new();
    for _ in 0..fixture.state.db.options().get_max_connections() {
        held.push(fixture.state.db.acquire().await.unwrap());
    }

    let response = server.get("/health").send().await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    drop(held);
}

/// The website is served from the directory the config names, so a server
/// started somewhere other than beside a `static` directory, as the desktop
/// app's is (#970), still has its website.
#[tokio::test]
async fn the_website_is_served_from_the_configured_directory() {
    let fixture = crate::test_support::test_fixture().await;
    let site = fixture.dir().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), "<title>the site</title>").unwrap();
    let mut state = fixture.state.clone();
    let mut cfg = (*state.cfg).clone();
    cfg.server.as_mut().unwrap().static_dir = site;
    state.cfg = std::sync::Arc::new(cfg);

    let (status, body) = crate::test_support::get_raw(&state, "/", "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "<title>the site</title>");
}
