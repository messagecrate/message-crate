//! Import Run handlers called directly rather than over HTTP: creating,
//! advancing, completing, discarding and reading a run.

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
use crate::server::AppState;
use crate::test_support::ImportingAccount;
use axum::extract::{FromRef, State};

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
                kind: api_types::ImportErrorKind::Skip,
                stage: crate::db::imports::ImportIssueStage::Media,
                item: "photo.heic".into(),
                reason: "convert failed".into(),
            },
            ImportIssueRequest {
                kind: api_types::ImportErrorKind::Error,
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
    assert_eq!(value.issues[0].kind, api_types::ImportErrorKind::Skip);
    assert_eq!(
        value.issues[0].stage,
        crate::db::imports::ImportIssueStage::Media
    );
    assert_eq!(value.issues[1].kind, api_types::ImportErrorKind::Error);
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
