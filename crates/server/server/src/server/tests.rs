use super::*;
use crate::auth::{ImportAccess, require_import_access, resolve_auth_on_conn};
use crate::extract::{Json, Path as AxumPath};
use crate::imports_api::dto::{
    CompleteImportRequest, CreateImportRequest, DiscardImportRequest, ImportIssueRequest,
    UpdateImportRequest,
};
use crate::imports_api::run_control::Importer;
use crate::imports_api::{
    complete_import, create_import, discard_import, get_import, list_imports, update_import,
};
use crate::problem::ProblemType;
use crate::test_support::{ImportingAccount, expect_problem, test_fixture};
use axum::extract::{FromRef, State};
use message_crate_api_types::ImportMode;

/// The account's running Import Run through `GET /v1/imports?status=running`,
/// as the desktop app finds it.
async fn running_import(
    state: &AppState,
    token: &str,
) -> Option<crate::imports_api::dto::ImportRunSummary> {
    list_imports(
        State(state.db.clone()),
        import_access(state, token).await,
        crate::extract::Query(crate::imports_api::dto::ListImportsQuery {
            sort: None,
            status: Some("running".into()),
            limit: None,
            offset: None,
        }),
    )
    .await
    .unwrap()
    .0
    .items
    .into_iter()
    .next()
}

/// Resolve the token the way the `ImportAccess` extractor would, at the
/// test's fixed instant, for tests that call import handlers directly
/// instead of over HTTP.
async fn import_access(state: &AppState, token: &str) -> ImportAccess {
    let mut conn = state.db.acquire().await.unwrap();
    let auth = resolve_auth_on_conn(&mut conn, token, None, crate::test_support::fixed_instant())
        .await
        .unwrap();
    require_import_access(&auth).unwrap();
    ImportAccess(auth)
}

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

#[tokio::test]
async fn imports_complete_and_detail_surface_timings_and_issues() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let body = CompleteImportRequest {
        status: "completed".into(),
        bytes_uploaded: Some(100),
        duration_ms: Some(48_000),
        parse_ms: Some(18_000),
        attachments_ms: Some(22_000),
        prepare_ms: Some(4_000),
        upload_ms: Some(8_000),
        summary: Some(serde_json::json!({
            "parse": { "messages": 10 },
            "convert": { "files": 2 }
        })),
        issues: vec![
            ImportIssueRequest {
                kind: message_crate_api_types::ImportErrorKind::Skip,
                stage: crate::db::imports::ImportIssueStage::Media,
                item: "photo.heic".into(),
                reason: "convert failed".into(),
            },
            ImportIssueRequest {
                kind: message_crate_api_types::ImportErrorKind::Error,
                stage: crate::db::imports::ImportIssueStage::Upload,
                item: "archive.zip".into(),
                reason: "upload failed".into(),
            },
        ],
        notes: Vec::new(),
    };

    let response = complete_import(
        State(Importer::from_ref(&state)),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap();
    assert_eq!(response.0.run.status.as_str(), "completed");
    // Counted from what the run holds, which is nothing here.
    assert_eq!(response.0.run.message_count, 0);
    assert_eq!(response.0.run.attachment_count, 0);
    assert_eq!(response.0.run.bytes_uploaded, 100);

    let detail = get_import(
        State(state.db.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
    )
    .await
    .unwrap();
    let value = detail.0;
    assert_eq!(value.run.id, import_id);
    assert_eq!(value.run.duration_ms, Some(48_000));
    assert_eq!(value.run.parse_ms, Some(18_000));
    assert_eq!(value.run.attachments_ms, Some(22_000));
    assert_eq!(value.run.prepare_ms, Some(4_000));
    assert_eq!(value.run.upload_ms, Some(8_000));
    assert_eq!(value.run.summary["parse"]["messages"], 10);
    assert_eq!(value.issues.len(), 2);
    assert_eq!(
        value.issues[0].kind,
        message_crate_api_types::ImportErrorKind::Skip
    );
    assert_eq!(
        value.issues[0].stage,
        crate::db::imports::ImportIssueStage::Media
    );
    assert_eq!(
        value.issues[1].kind,
        message_crate_api_types::ImportErrorKind::Error
    );
    assert_eq!(
        value.issues[1].stage,
        crate::db::imports::ImportIssueStage::Upload
    );
}

#[tokio::test]
async fn imports_complete_stores_completed_with_issues_status() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let body = CompleteImportRequest {
        status: "completed_with_issues".into(),
        bytes_uploaded: Some(100),
        duration_ms: None,
        parse_ms: None,
        attachments_ms: None,
        prepare_ms: None,
        upload_ms: None,
        summary: None,
        issues: Vec::new(),
        notes: Vec::new(),
    };
    let response = complete_import(
        State(Importer::from_ref(&state)),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap();
    assert_eq!(response.0.run.status.as_str(), "completed_with_issues");
}

#[tokio::test]
async fn imports_complete_rejects_unknown_status() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let body = CompleteImportRequest {
        status: "victorious".into(),
        bytes_uploaded: None,
        duration_ms: None,
        parse_ms: None,
        attachments_ms: None,
        prepare_ms: None,
        upload_ms: None,
        summary: None,
        issues: Vec::new(),
        notes: Vec::new(),
    };
    let err = complete_import(
        State(Importer::from_ref(&state)),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ApiError::ValidationFailed(_)));

    // The session is untouched.
    let mut conn = state.db.acquire().await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(status, "running");
}

#[tokio::test]
async fn imports_get_handler_returns_not_found_for_missing_import() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let err = get_import(
        State(state.db.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id + 1),
    )
    .await
    .unwrap_err();

    match err {
        ApiError::NotFound(msg) => {
            assert!(msg.contains("import"));
            assert!(msg.contains("not found"));
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[tokio::test]
async fn active_import_run_is_empty_then_reports_the_live_one() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;

    let body = CreateImportRequest {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: Some("message-crate".into()),
        run_dir: Some("/home/u/message-crate/staging-260830".into()),
        device_id: Some("device-a".into()),
        form: Some(serde_json::json!({ "source": "imessage-ios" })),
        source_fingerprint: Some(serde_json::json!({ "size_bytes": 42 })),
        source_identities: None,
        phone_country: None,
    };
    // The fixture already started an Import Run; discard it so this one can start.
    let _ = discard_import(
        State(Importer::from_ref(&state)),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(DiscardImportRequest {
            issues: Vec::new(),
            notes: Vec::new(),
        }),
    )
    .await
    .unwrap();

    let created = create_import(
        State(state.db.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(session.id, created.body.id);
    assert_eq!(session.stage, Some(crate::db::imports::ImportStage::Parse));
    assert_eq!(
        session.run_dir.as_deref(),
        Some("/home/u/message-crate/staging-260830")
    );
    assert_eq!(session.device_id.as_deref(), Some("device-a"));
    assert_eq!(session.form["source"], "imessage-ios");
}

/// A stored form snapshot never carries credentials, whatever the
/// client posts: the row outlives the run, and the secret must not.
#[tokio::test]
async fn a_stored_form_snapshot_drops_credentials() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let _ = discard_import(
        State(Importer::from_ref(&state)),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(DiscardImportRequest {
            issues: Vec::new(),
            notes: Vec::new(),
        }),
    )
    .await
    .unwrap();

    let body = CreateImportRequest {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        run_dir: None,
        device_id: None,
        // A client that has not learned the rule.
        form: Some(serde_json::json!({
            "source": "imessage-ios",
            "backupPassword": "hunter2",
            "whatsappKey": "0123456789abcdef",
        })),
        source_fingerprint: None,
        source_identities: None,
        phone_country: None,
    };
    let _ = create_import(
        State(state.db.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(
        session.form["source"], "imessage-ios",
        "the rest of the snapshot is kept"
    );
    assert!(
        session.form.get("backupPassword").is_none(),
        "backupPassword was stored: {}",
        session.form
    );
    assert!(
        session.form.get("whatsappKey").is_none(),
        "whatsappKey was stored: {}",
        session.form
    );
}

/// The identity list a client read from the backup rides on the Import Run
/// so a resumed Staging Review can show it without re-reading the backup.
#[tokio::test]
async fn imports_create_stores_source_identities() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let _ = discard_import(
        State(Importer::from_ref(&state)),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(DiscardImportRequest {
            issues: Vec::new(),
            notes: Vec::new(),
        }),
    )
    .await
    .unwrap();

    let body = CreateImportRequest {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        run_dir: None,
        device_id: None,
        form: None,
        source_fingerprint: None,
        source_identities: Some(serde_json::json!(["+15555550110", "owner@example.com"])),
        phone_country: None,
    };
    let _ = create_import(
        State(state.db.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(
        session.source_identities,
        serde_json::json!(["+15555550110", "owner@example.com"])
    );
}

#[tokio::test]
async fn a_second_import_run_is_refused_with_conflict() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, .. } = fixture.account_with_running_import().await;
    let body = CreateImportRequest {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        run_dir: None,
        device_id: None,
        form: None,
        source_fingerprint: None,
        source_identities: None,
        phone_country: None,
    };
    let err = create_import(
        State(state.db.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap_err();
    let ApiError::StateConflict(message) = &err else {
        panic!("expected StateConflict, got {err:?}");
    };
    // The 409 has to name the way out: a stranded session is resumed or
    // discarded from the desktop app's Import screen, or discarded with
    // `message-crate-server imports discard`.
    assert!(
        message.contains("Import in the desktop app"),
        "the conflict names how to clear the session: {message}"
    );
}

#[tokio::test]
async fn stage_endpoint_advances() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;

    let _ = update_import(
        State(state.db.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(UpdateImportRequest {
            stage: crate::db::imports::ImportStage::Write,
            summary: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        running_import(&state, &token).await.unwrap().stage,
        Some(crate::db::imports::ImportStage::Write)
    );
}

#[tokio::test]
async fn discard_frees_the_slot() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let ImportingAccount { token, import_id } = fixture.account_with_running_import().await;
    let _ = discard_import(
        State(Importer::from_ref(&state)),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(DiscardImportRequest {
            issues: Vec::new(),
            notes: Vec::new(),
        }),
    )
    .await
    .unwrap();
    assert!(running_import(&state, &token).await.is_none());
}

/// `/v1/contacts/{id}` takes an `i64`, and three literal routes sit beside
/// it: `summaries`, `unmatched-identities`, and `address-book`. This test
/// covers the first two. Each is a `POST`, and editing a contact is a `PATCH`, so if the `{id}` route ever swallowed one of them
/// the request would come back 405 (no `POST` on `/v1/contacts/{id}`)
/// instead of reaching its own handler. Each assertion below distinguishes
/// "matched my route and rejected my body" from "matched the wrong route".
#[tokio::test]
async fn literal_contact_routes_are_not_captured_by_the_id_route() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let user =
        crate::test_support::register_via_api(&state, "contact-routes", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::get_status(&state, "/v1/contacts", &user.token).await,
        StatusCode::OK
    );

    // A real id still reaches the detail handler: an unknown contact is its
    // 404, not a 400 from a failed `i64` path parse.
    let (status, text) =
        crate::test_support::get_raw(&state, "/v1/contacts/999999", &user.token).await;
    expect_problem(status, &text, ProblemType::NotFound);

    for path in [
        "/v1/contacts/summaries",
        "/v1/contacts/unmatched-identities",
    ] {
        let status =
            crate::test_support::post_status(&state, path, &user.token, serde_json::json!({}))
                .await;
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{path} was captured by /v1/contacts/{{id}}"
        );
    }
}

/// The `ImportAccess` extractor guards `GET /v1/imports`: with
/// `can_import` off, the endpoint refuses; turned back on, it succeeds.
#[tokio::test]
async fn import_endpoint_honors_can_import_flag() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let owner =
        crate::test_support::claim_as_owner(&state, "import-guard-keeper", "hunter2hunter2").await;
    let user =
        crate::test_support::register_via_api(&state, "import-guard-user", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_import": false }),
        )
        .await,
        StatusCode::OK
    );
    // can_import=false refuses GET /v1/imports.
    let (status, text) = crate::test_support::get_raw(&state, "/v1/imports", &user.token).await;
    expect_problem(status, &text, ProblemType::InsufficientScope);

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_import": true }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/imports", &user.token).await,
        StatusCode::OK,
        "can_import=true must allow GET /v1/imports"
    );
}

/// The `ExportAccess` extractor guards `GET /v1/exports`: with `can_export`
/// off, the endpoint refuses; turned back on, it succeeds.
#[tokio::test]
async fn export_endpoint_honors_can_export_flag() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let owner =
        crate::test_support::claim_as_owner(&state, "export-guard-keeper", "hunter2hunter2").await;
    let user =
        crate::test_support::register_via_api(&state, "export-guard-user", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": false }),
        )
        .await,
        StatusCode::OK
    );
    // can_export=false refuses GET /v1/exports.
    let (status, text) = crate::test_support::get_raw(&state, "/v1/exports", &user.token).await;
    expect_problem(status, &text, ProblemType::InsufficientScope);

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": true }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/exports", &user.token).await,
        StatusCode::OK,
        "can_export=true must allow GET /v1/exports"
    );
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

/// A wrong password is `invalid-credentials` at `401 Unauthorized`, with no
/// `Retry-After`: the server waited before answering, so there is nothing to
/// wait for.
#[tokio::test]
async fn a_wrong_password_is_401_invalid_credentials() {
    let (fixture, _) = crate::test_support::fixture_with_account().await;
    let state = fixture.state.clone();
    let server = crate::test_support::serve(&state).await;
    let wrong = server
        .post("/v1/session")
        .json(&serde_json::json!({ "username": "alice", "password": "not-it-at-all" }))
        .send()
        .await
        .unwrap();
    assert!(!wrong.headers().contains_key(header::RETRY_AFTER));
    let status = wrong.status();
    let text = wrong.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::InvalidCredentials,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some("invalid username or password")
    );
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
