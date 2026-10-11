//! Import Run routes: stage changes and their summaries, completion and
//! discard records, creating a run, reading runs back, and another
//! account's run.

use super::*;

/// A fixture holding one running Import Run at `staging_review` whose
/// `summary_json` already carries `summary` — as if an earlier
/// `PATCH /v1/imports/{id}` recorded the Staging Review approval.
async fn run_with_summary(summary: serde_json::Value) -> (TestFixture, RegisteredAccount, i64) {
    let (fixture, account) = fixture_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage", "stage": "write" }),
    )
    .await;
    let import_id = created["id"].as_i64().expect("created run has an id");
    let mut conn = fixture.state.db.acquire().await.unwrap();
    crate::db::imports::set_import_stage(
        &mut conn,
        account.account_id,
        import_id,
        crate::db::imports::ImportStage::StagingReview,
        Some(&summary.to_string()),
    )
    .await
    .unwrap();
    (fixture, account, import_id)
}

/// The run's stored `summary_json`, decoded, or `None` when the
/// column is null.
async fn stored_summary(fixture: &TestFixture, import_id: i64) -> Option<serde_json::Value> {
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let raw: Option<String> = sqlx::query_scalar("SELECT summary_json FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    raw.map(|s| serde_json::from_str(&s).expect("stored summary_json is valid JSON"))
}

#[tokio::test]
async fn a_stage_change_with_a_summary_stores_it() {
    // The Review screen posts what the user approved so it survives a
    // reload — recomputing the summary from the directory is a different
    // question from what was actually approved.
    let (fixture, account) = fixture_with_account().await;
    let (location, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage", "stage": "write" }),
    )
    .await;
    let import_id = created["id"].as_i64().unwrap();
    assert_eq!(location, format!("/v1/imports/{import_id}"));

    patch_json::<serde_json::Value>(
        &fixture.state,
        &format!("/v1/imports/{import_id}"),
        &account.token,
        serde_json::json!({"stage": "staging_review", "summary": {"approved": true}}),
    )
    .await;

    assert_eq!(
        stored_summary(&fixture, import_id).await,
        Some(serde_json::json!({"approved": true}))
    );
}

#[tokio::test]
async fn running_import_run_reports_the_summary_a_stage_change_stored() {
    // The completion call is allowed to overwrite summary_json with the
    // outcome once the run finishes — that is the intended history
    // record. But mid-run, between an approval and completion, a
    // reload has nowhere else to read the approved plan back from:
    // the running run on GET /v1/imports?status=running must expose it too.
    let (fixture, account, import_id) =
        run_with_summary(serde_json::json!({"approved": true})).await;

    let page: serde_json::Value =
        get_json(&fixture.state, "/v1/imports?status=running", &account.token).await;
    let active = &page["items"][0];

    assert_eq!(active["id"], serde_json::json!(import_id));
    assert_eq!(active["summary"], serde_json::json!({"approved": true}));
}

#[tokio::test]
async fn a_stage_change_without_a_summary_does_not_erase_the_stored_one() {
    // Most stage changes carry nothing. Treating absent as null would
    // throw away the plan the outcome is judged against.
    let (fixture, account, import_id) =
        run_with_summary(serde_json::json!({"approved": true})).await;

    let run: serde_json::Value = patch_json(
        &fixture.state,
        &format!("/v1/imports/{import_id}"),
        &account.token,
        serde_json::json!({"stage": "upload"}),
    )
    .await;
    assert_eq!(
        run["id"],
        serde_json::json!(import_id),
        "a PATCH answers the run, the same record GET /v1/imports/{{id}} returns"
    );

    assert_eq!(
        stored_summary(&fixture, import_id).await,
        Some(serde_json::json!({"approved": true}))
    );
}

/// A run moved backwards is resumed at the wrong stage on the next visit
/// (#2167): the move is a state conflict naming both stages, and the run
/// keeps its stage and summary.
#[tokio::test]
async fn a_backward_stage_move_is_a_state_conflict() {
    let (fixture, account, import_id) =
        run_with_summary(serde_json::json!({"approved": true})).await;
    let path = format!("/v1/imports/{import_id}");

    let (status, text) = crate::test_support::patch_json_raw(
        &fixture.state,
        &path,
        &account.token,
        serde_json::json!({"stage": "parse", "summary": {"approved": false}}),
    )
    .await;
    let sentence = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::StateConflict,
    )
    .sentence();
    assert!(
        sentence.contains("staging_review") && sentence.contains("parse"),
        "the refusal names both stages: {sentence}"
    );

    let run: serde_json::Value = get_json(&fixture.state, &path, &account.token).await;
    assert_eq!(run["stage"], "staging_review");
    assert_eq!(
        stored_summary(&fixture, import_id).await,
        Some(serde_json::json!({"approved": true}))
    );
}

#[tokio::test]
async fn a_stage_answers_by_the_words_of_context_md_only() {
    // The stages and Reviews of an Import Run are named as CONTEXT.md names
    // them. The old spellings are refused, not read as aliases.
    let (fixture, account) = fixture_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage", "stage": "write" }),
    )
    .await;
    let path = format!("/v1/imports/{}", created["id"]);

    let (status, text) = crate::test_support::patch_json_raw(
        &fixture.state,
        &path,
        &account.token,
        serde_json::json!({"stage": "awaiting_gate_1"}),
    )
    .await;
    let sentence = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    )
    .sentence();
    assert!(
        sentence.contains(
            "expected one of `parse`, `write`, `staging_review`, `media`, `media_review`, `upload`"
        ),
        "the refusal lists the stages the server knows: {sentence}"
    );

    let run: serde_json::Value = patch_json(
        &fixture.state,
        &path,
        &account.token,
        serde_json::json!({"stage": "staging_review"}),
    )
    .await;
    assert_eq!(run["id"], created["id"]);
}

#[tokio::test]
async fn an_issue_names_the_stage_it_came_from() {
    let (fixture, account) = fixture_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let issue = |stage: &str| {
        serde_json::json!({
            "status": "completed_with_issues",
            "issues": [{ "kind": "skip", "stage": stage, "item": "a.jpg", "reason": "missing" }],
        })
    };

    // A finer part of the desktop app's work is not a Stage.
    let (status, text) = crate::test_support::post_json_raw(
        &fixture.state,
        &format!("/v1/imports/{id}/complete"),
        &account.token,
        issue("parse"),
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );

    let _: serde_json::Value = post_json(
        &fixture.state,
        &format!("/v1/imports/{id}/complete"),
        &account.token,
        issue("staging"),
    )
    .await;
    let run: serde_json::Value =
        get_json(&fixture.state, &format!("/v1/imports/{id}"), &account.token).await;
    assert_eq!(run["issues"][0]["stage"], "staging");
}

/// Completing a run the account does not have answers `404 Not Found`, and
/// completing one that already finished answers `409 Conflict`: each is a
/// run lookup failure, kept apart from a database fault, which answers
/// `500 Internal Server Error` (#1682).
#[tokio::test]
async fn completing_a_missing_or_finished_run_answers_its_own_status() {
    let (fixture, account) = fixture_with_account().await;
    let complete = |id: i64| {
        let state = fixture.state.clone();
        let token = account.token.clone();
        async move {
            crate::test_support::post_json_raw(
                &state,
                &format!("/v1/imports/{id}/complete"),
                &token,
                serde_json::json!({ "status": "completed" }),
            )
            .await
        }
    };

    let (status, text) = complete(999_999).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);

    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let (status, text) = complete(id).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let (status, text) = complete(id).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::StateConflict);
}

/// What a request can change on an Import Run: status, stage, approved plan
/// and finish time.
async fn run_state(
    state: &crate::server::AppState,
    import_id: i64,
) -> (String, Option<String>, Option<String>, Option<String>) {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_as("SELECT status, stage, summary_json, finished_at FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// The desktop app sends every call of an Import Run with the session logged
/// in at the time, so a run whose account logged out while it ran reaches
/// the server with the next account's session (#1085). Every route on a
/// run refuses another account's session as if the run did not exist, and
/// leaves the run as it was.
#[tokio::test]
async fn every_route_on_another_accounts_run_is_not_found_and_changes_nothing() {
    let (fixture, alice) = crate::test_support::fixture_with_account().await;
    let bob = crate::test_support::register_via_api(&fixture.state, "bob", "hunter2hunter2").await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &bob.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let bobs_run = created["id"].as_i64().unwrap();
    let run = format!("/v1/imports/{bobs_run}");
    let before = run_state(&fixture.state, bobs_run).await;

    let state = &fixture.state;
    let token = alice.token.as_str();
    let refusals = [
        (
            "PATCH stage",
            crate::test_support::patch_json_raw(
                state,
                &run,
                token,
                serde_json::json!({ "stage": "upload", "summary": { "approved": true } }),
            )
            .await,
        ),
        (
            "POST complete",
            crate::test_support::post_json_raw(
                state,
                &format!("{run}/complete"),
                token,
                serde_json::json!({"status":"completed"}),
            )
            .await,
        ),
        (
            "POST discard",
            crate::test_support::post_json_raw(
                state,
                &format!("{run}/discard"),
                token,
                serde_json::json!({"issues":[],"notes":[]}),
            )
            .await,
        ),
        (
            "POST batches",
            crate::test_support::post_raw(
                state,
                &format!("{run}/batches"),
                token,
                "application/jsonl",
                "{}\n",
            )
            .await,
        ),
        (
            "GET run",
            crate::test_support::get_raw(state, &run, token).await,
        ),
        (
            "GET contacts",
            crate::test_support::get_raw(state, &format!("{run}/contacts"), token).await,
        ),
    ];
    for (route, (status, text)) in refusals {
        crate::test_support::expect_problem_for(
            &format!("{route} on another account's run"),
            status,
            &text,
            crate::problem::ProblemType::NotFound,
        );
    }

    assert_eq!(run_state(&fixture.state, bobs_run).await, before);
    assert_eq!(before.0, "running");
}

/// A run's source names its messages and its attachment directory, and the
/// batch route takes it from the run without checking it again. So a blank
/// one has to be refused here, where the run is created.
#[tokio::test]
async fn creating_an_import_with_a_blank_source_is_a_validation_failure() {
    let (state, _fixture, token) = importer().await;
    for source in ["", "   "] {
        let (status, text) = crate::test_support::post_json_raw(
            &state,
            "/v1/imports",
            &token,
            serde_json::json!({ "source": source }),
        )
        .await;
        crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::ValidationFailed,
        );
        let errors: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            errors["errors"],
            serde_json::json!(["source is required"]),
            "{text}"
        );
    }
    let runs: serde_json::Value = get_json(&state, "/v1/imports", &token).await;
    assert_eq!(runs["total"], 0, "no run was created: {runs}");
}

/// The bug: `validate_source_id` checked the id trimmed and the run stored
/// it as given, so ` imessage ` made a run whose source held spaces.
#[tokio::test]
async fn creating_an_import_with_a_space_around_the_source_is_a_validation_failure() {
    let (state, _fixture, token) = importer().await;
    for source in [" imessage", "imessage ", " imessage "] {
        let (status, text) = crate::test_support::post_json_raw(
            &state,
            "/v1/imports",
            &token,
            serde_json::json!({ "source": source }),
        )
        .await;
        crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::ValidationFailed,
        );
    }
    let runs: serde_json::Value = get_json(&state, "/v1/imports", &token).await;
    assert_eq!(runs["total"], 0, "no run was created: {runs}");
}

/// Each JSON value a run stores as it is created is at most 64 KiB, and a
/// body over the cap creates no run. They were stored at any size a JSON body
/// could carry (#2183).
#[tokio::test]
async fn creating_an_import_with_an_oversized_json_value_is_a_validation_failure() {
    let (state, _fixture, token) = importer().await;
    let big = serde_json::json!({ "backupPath": "p".repeat(64 * 1024) });
    for field in ["form", "source_fingerprint", "source_identities"] {
        let (status, text) = crate::test_support::post_json_raw(
            &state,
            "/v1/imports",
            &token,
            serde_json::json!({ "source": "imessage", field: big }),
        )
        .await;
        let problem = crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::ValidationFailed,
        );
        assert!(
            problem.sentence().starts_with(&format!("{field}: ")),
            "{text}"
        );
    }
    let runs: serde_json::Value = get_json(&state, "/v1/imports", &token).await;
    assert_eq!(runs["total"], 0, "no run was created: {runs}");
}

/// An Import Error's `item` and `reason` are each at most 2,000 characters,
/// on a completion and a discard alike, and a body over the cap leaves the
/// run running. They were stored at any length a JSON body could carry
/// (#2183).
#[tokio::test]
async fn an_import_error_over_the_text_cap_is_refused_and_the_run_stays_running() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let long = "x".repeat(2_001);

    for (route, field, issue) in [
        (
            "complete",
            "item",
            serde_json::json!({ "kind": "skip", "stage": "media", "item": long, "reason": "r" }),
        ),
        (
            "discard",
            "reason",
            serde_json::json!({ "kind": "error", "stage": "media", "item": "a", "reason": long }),
        ),
    ] {
        let mut body = serde_json::json!({ "issues": [issue], "notes": [] });
        if route == "complete" {
            body["status"] = "completed_with_issues".into();
        }
        let (status, text) = crate::test_support::post_json_raw(
            state,
            &format!("/v1/imports/{id}/{route}"),
            token,
            body,
        )
        .await;
        let problem = crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::ValidationFailed,
        );
        assert!(
            problem
                .sentence()
                .starts_with(&format!("issues[0].{field}: ")),
            "{route}: {text}"
        );
    }
    // Every breach is named, not only the first.
    let (status, text) = crate::test_support::post_json_raw(
        state,
        &format!("/v1/imports/{id}/discard"),
        token,
        serde_json::json!({
            "issues": [
                { "kind": "skip", "stage": "media", "item": "a", "reason": "r" },
                { "kind": "note", "stage": "media", "item": long, "reason": long }
            ],
            "notes": []
        }),
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    let errors: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        errors["errors"],
        serde_json::json!([
            "issues[1].kind: invalid import issue kind 'note'; expected 'error' or 'skip'",
            "issues[1].item: must be at most 2000 characters",
            "issues[1].reason: must be at most 2000 characters"
        ]),
        "{text}"
    );
    let run: serde_json::Value = get_json(state, &format!("/v1/imports/{id}"), token).await;
    assert_eq!(run["status"], "running", "{run}");
}

/// The reference states the cap on an Import Error's text, so the desktop
/// app, which cuts its text to it, reads the same number the server holds.
#[test]
fn the_reference_states_the_import_error_text_cap() {
    let doc: serde_json::Value =
        serde_json::from_str(&crate::openapi::dump_openapi_json()).unwrap();
    let fields = &doc["components"]["schemas"]["ImportIssueRequest"]["properties"];
    for field in ["item", "reason"] {
        assert_eq!(
            fields[field]["maxLength"],
            crate::text_caps::MAX_IMPORT_ERROR_TEXT_CHARS,
            "{field}: {fields}"
        );
    }
}

/// A discarded run keeps the Import Errors the desktop app sends with the
/// discard: a run paused and then given up still has the record of what went
/// wrong before it stopped (#1479).
#[tokio::test]
async fn a_discard_records_the_issues_it_carries() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();

    let discarded: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/discard"),
        token,
        serde_json::json!({
            "issues": [{ "kind": "skip", "stage": "media", "item": "IMG_0001.heic", "reason": "convert failed" }],
            "notes": [],
        }),
    )
    .await;

    assert_eq!(discarded["status"], "cancelled", "{discarded}");
    let issues = discarded["issues"].as_array().expect("issues");
    assert_eq!(issues.len(), 1, "{discarded}");
    assert_eq!(issues[0]["kind"], "skip");
    assert_eq!(issues[0]["stage"], "media");
    assert_eq!(issues[0]["item"], "IMG_0001.heic");
    assert_eq!(issues[0]["reason"], "convert failed");
}

/// A completion records the notes the desktop app sends apart from its
/// Import Errors: a note is something the run did that is worth knowing, so
/// the run reads back with it in `notes` and its `issues` stay empty
/// (#1626).
#[tokio::test]
async fn a_completion_records_the_notes_it_carries_apart_from_its_issues() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imazing" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let note = serde_json::json!({
        "stage": "staging",
        "item": "Messages/IMG_0002.jpg",
        "text": "2 rows name this picture; its Live Photo video goes to the first of them in the CSV",
    });

    let completed: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({ "status": "completed", "notes": [note] }),
    )
    .await;

    assert_eq!(completed["status"], "completed", "{completed}");
    assert_eq!(completed["notes"], serde_json::json!([note]), "{completed}");
    assert_eq!(completed["note_count"], 1, "{completed}");
    assert_eq!(completed["issues"], serde_json::json!([]), "{completed}");
    let page: serde_json::Value = get_json(state, "/v1/imports", token).await;
    assert_eq!(page["items"][0]["note_count"], 1, "{page}");
    assert!(page["items"][0].get("notes").is_none(), "{page}");
    let run: serde_json::Value = get_json(state, &format!("/v1/imports/{id}"), token).await;
    assert_eq!(run["notes"], serde_json::json!([note]), "{run}");
}

/// A discarded run keeps the notes the desktop app sends with the discard,
/// as it keeps its Import Errors (#1626).
#[tokio::test]
async fn a_discard_records_the_notes_it_carries() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "sms-backup-plus" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let note = serde_json::json!({
        "stage": "staging",
        "item": "1.eml",
        "text": "This message records no phone number or email address for the other person.",
    });

    let discarded: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/discard"),
        token,
        serde_json::json!({ "issues": [], "notes": [note] }),
    )
    .await;

    assert_eq!(discarded["status"], "cancelled", "{discarded}");
    assert_eq!(discarded["notes"], serde_json::json!([note]), "{discarded}");
}

/// A discard's issues are checked the way a completion's are: a kind that is
/// neither `error` nor `skip` is refused, and the run stays running.
#[tokio::test]
async fn a_discard_with_an_unknown_issue_kind_is_refused() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();

    let body = serde_json::json!({
        "issues": [{ "kind": "warning", "stage": "staging", "item": "a.jsonl", "reason": "x" }],
        "notes": [],
    });
    let (status, text) = crate::test_support::post_json_raw(
        state,
        &format!("/v1/imports/{id}/discard"),
        token,
        body,
    )
    .await;

    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    let run: serde_json::Value = get_json(state, &format!("/v1/imports/{id}"), token).await;
    assert_eq!(run["status"], "running", "{run}");
}

/// An Import Run is one record wherever the interface hands one run out:
/// the answer to `complete` and to `discard` and `GET /v1/imports/{id}` are
/// the same JSON, issues included. The run's row in `GET /v1/imports` is the
/// same JSON without the issues, and both count them.
#[tokio::test]
async fn an_import_run_reads_the_same_from_every_route() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();

    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let completed_id = created["id"].as_i64().unwrap();
    let completed: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{completed_id}/complete"),
        token,
        serde_json::json!({
            "status": "completed_with_issues",
            "issues": [{ "kind": "skip", "stage": "staging", "item": "a.jsonl", "reason": "empty" }],
        }),
    )
    .await;
    assert_eq!(completed["issues"][0]["item"], "a.jsonl", "{completed}");

    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let discarded_id = created["id"].as_i64().unwrap();
    let discarded: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{discarded_id}/discard"),
        token,
        serde_json::json!({ "issues": [], "notes": [] }),
    )
    .await;
    assert_eq!(discarded["status"], "cancelled", "{discarded}");

    let page: serde_json::Value = get_json(state, "/v1/imports", token).await;
    for (id, answered) in [(completed_id, &completed), (discarded_id, &discarded)] {
        let got: serde_json::Value = get_json(state, &format!("/v1/imports/{id}"), token).await;
        assert_eq!(&got, answered, "GET /v1/imports/{id}");
        let listed = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|run| run["id"] == id)
            .unwrap_or_else(|| panic!("run {id} is listed: {page}"));
        let mut summary = answered.clone();
        let issues = summary
            .as_object_mut()
            .unwrap()
            .remove("issues")
            .expect("the run carries its issues");
        assert_eq!(summary["issue_count"], issues.as_array().unwrap().len());
        // The list leaves out the notes too, and counts them: one run
        // answers them.
        let notes = summary
            .as_object_mut()
            .unwrap()
            .remove("notes")
            .expect("the run carries its notes");
        assert_eq!(summary["note_count"], notes.as_array().unwrap().len());
        assert_eq!(listed, &summary, "GET /v1/imports, run {id}");
    }
}

/// Start an Import Run and complete it with `issues` skips, returning its id.
async fn run_with_issues(state: &crate::server::AppState, token: &str, issues: usize) -> i64 {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "whatsapp" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let issues: Vec<serde_json::Value> = (0..issues)
        .map(|n| {
            serde_json::json!({
                "kind": "skip", "stage": "staging", "item": format!("chat-{n}.txt"), "reason": "empty"
            })
        })
        .collect();
    let _: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({ "status": "completed_with_issues", "issues": issues }),
    )
    .await;
    id
}

/// #1559: a page of the Import Run list carries how many issues each run
/// recorded and none of the issues, however many a run recorded, so its size
/// does not grow with them. `GET /v1/imports/{id}` still answers every one.
#[tokio::test]
async fn the_import_run_list_counts_each_runs_issues_and_carries_none() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let many = run_with_issues(state, token, 600).await;
    let none = run_with_issues(state, token, 0).await;

    for path in [
        "/v1/imports".to_string(),
        format!("/v1/accounts/{}/imports", account.account_id),
    ] {
        let page: serde_json::Value = get_json(state, &path, token).await;
        for (id, count) in [(many, 600), (none, 0)] {
            let listed = page["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|run| run["id"] == id)
                .unwrap_or_else(|| panic!("{path}: run {id} is listed: {page}"));
            assert_eq!(listed["issue_count"], count, "{path}: run {id}");
            assert!(
                listed.get("issues").is_none(),
                "{path}: run {id} carries its issues"
            );
        }
    }

    let run: serde_json::Value = get_json(state, &format!("/v1/imports/{many}"), token).await;
    let issues = run["issues"].as_array().expect("the run's issues");
    assert_eq!(issues.len(), 600);
    assert_eq!(issues[599]["item"], "chat-599.txt");
}

/// #1559: a page of Import Runs is read in the list's own statements, the
/// count and the page, whatever rows the page holds: no statement runs once
/// per row for its issues or its contacts. Both lists shape the rows
/// without the database, the account's and the owner's alike.
#[tokio::test]
async fn a_page_of_import_runs_is_read_without_a_statement_per_row() {
    use sqlx::Connection as _;
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    for issues in [3, 0, 1] {
        run_with_issues(state, token, issues).await;
    }
    let mut conn = state.db.acquire().await.unwrap();
    conn.clear_cached_statements().await.unwrap();

    let query = ListImportsQuery {
        status: None,
        limit: None,
        offset: None,
        sort: None,
    };
    let rows = import_rows_page(&mut conn, account.account_id, query)
        .await
        .unwrap();

    // The connection's statement cache holds each distinct statement once,
    // however often it ran. A read per row is a statement of its own, so it
    // would show here as a third or fourth.
    assert_eq!(
        conn.cached_statements_size(),
        2,
        "the count and the page, and nothing per row"
    );
    let counts: Vec<u64> = rows.items.iter().map(|run| run.issue_count).collect();
    assert_eq!(counts, [1, 0, 3], "newest first");
    let owner: Page<OwnerImportRun> = rows.map(OwnerImportRun::from);
    assert_eq!(owner.items[2].issue_count, 3);
}
