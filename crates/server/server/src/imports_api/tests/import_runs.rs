//! Import Run routes: stage changes and their summaries, completion and
//! discard records, creating a run, reading runs back, another account's
//! run, and the `can_import` permission that guards them.

use super::*;
use crate::imports_api::dto::ListImportsQuery;
use crate::problem::ProblemType;
use crate::server::AppState;
use crate::test_support::{
    add_account_and_log_in, expect_problem, get_raw, get_status, log_in_as_admin, patch_status,
    post_json_raw,
};
use axum::http::StatusCode;

/// Start an Import Run over HTTP with `body` and return its id.
async fn start_run(state: &AppState, token: &str, body: serde_json::Value) -> i64 {
    let (_, created): (String, serde_json::Value) =
        post_created_json(state, "/v1/imports", token, body).await;
    created["id"].as_i64().expect("created run has an id")
}

/// A fixture holding one running Import Run at `staging_review` whose
/// `summary_json` already carries `summary` — as if an earlier
/// `PATCH /v1/imports/{id}` recorded the Staging Review approval.
async fn run_with_summary(summary: serde_json::Value) -> (TestFixture, LoggedInAccount, i64) {
    let (fixture, account) = fixture_with_account().await;
    let import_id = start_run(
        &fixture.state,
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    crate::db::imports::set_import_stage(
        &mut conn,
        account.account_id,
        import_id,
        crate::db::imports::ImportStage::Write,
        None,
    )
    .await
    .unwrap();
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
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let import_id = created["id"].as_i64().unwrap();
    assert_eq!(location, format!("/v1/imports/{import_id}"));

    patch_json::<serde_json::Value>(
        &fixture.state,
        &format!("/v1/imports/{import_id}"),
        &account.token,
        serde_json::json!({"stage": "write"}),
    )
    .await;
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
    let import_id = start_run(
        &fixture.state,
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let path = format!("/v1/imports/{import_id}");

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
        serde_json::json!({"stage": "write"}),
    )
    .await;
    assert_eq!(run["id"], import_id);
}

/// `POST /v1/imports` takes no stage: every run opens at `parse`, and a
/// `stage` in the body is ignored like any other field the request does not
/// name (#2858).
#[tokio::test]
async fn a_run_opens_at_parse_whatever_stage_the_body_names() {
    let (fixture, account) = fixture_with_account().await;
    let import_id = start_run(
        &fixture.state,
        &account.token,
        serde_json::json!({ "source": "imessage", "stage": "upload" }),
    )
    .await;

    let run: serde_json::Value = get_json(
        &fixture.state,
        &format!("/v1/imports/{import_id}"),
        &account.token,
    )
    .await;
    assert_eq!(run["stage"], "parse", "{run}");
}

#[tokio::test]
async fn an_issue_names_the_stage_it_came_from() {
    let (fixture, account) = fixture_with_account().await;
    let id = start_run(
        &fixture.state,
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
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

    let id = start_run(
        &fixture.state,
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let (status, text) = complete(id).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let (status, text) = complete(id).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::StateConflict);
}

/// What a request can change on an Import Run: status, stage, approved plan
/// and finish time.
async fn run_state(
    state: &AppState,
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
    let bob =
        crate::test_support::add_account_and_log_in(&fixture.state, "bob", "hunter2hunter2").await;
    let bobs_run = start_run(
        &fixture.state,
        &bob.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
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

/// `tool`, `run_dir` and `device_id` each have a cap, and a value one
/// character over it creates no run. They were stored at any length a JSON
/// body could carry (#2668).
#[tokio::test]
async fn creating_an_import_with_an_oversized_text_field_is_a_validation_failure() {
    let (state, _fixture, token) = importer().await;
    for (field, cap) in [
        ("tool", crate::text_caps::MAX_IMPORT_TOOL_CHARS),
        ("run_dir", crate::text_caps::MAX_PATH_CHARS),
        ("device_id", crate::text_caps::MAX_DEVICE_ID_CHARS),
    ] {
        let (status, text) = crate::test_support::post_json_raw(
            &state,
            "/v1/imports",
            &token,
            serde_json::json!({ "source": "imessage", field: "é".repeat(cap + 1) }),
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
            serde_json::json!([format!("{field}: must be at most {cap} characters")]),
            "{text}"
        );
    }
    let runs: serde_json::Value = get_json(&state, "/v1/imports", &token).await;
    assert_eq!(runs["total"], 0, "no run was created: {runs}");
}

/// A value at its cap is stored. `tool` and `device_id` are stored trimmed,
/// and `run_dir` as given, because a space at either end of a path is part
/// of its name and the desktop app finds the directory by it.
#[tokio::test]
async fn an_import_stores_its_text_fields_at_the_cap() {
    let (state, _fixture, token) = importer().await;
    let tool = "t".repeat(crate::text_caps::MAX_IMPORT_TOOL_CHARS);
    let device_id = "d".repeat(crate::text_caps::MAX_DEVICE_ID_CHARS);
    let run_dir = format!(" {} ", "p".repeat(crate::text_caps::MAX_PATH_CHARS - 2));
    let id = start_run(
        &state,
        &token,
        serde_json::json!({
            "source": "imessage",
            "tool": format!(" {tool} "),
            "run_dir": run_dir,
            "device_id": format!(" {device_id} "),
        }),
    )
    .await;
    let run: serde_json::Value = get_json(&state, &format!("/v1/imports/{id}"), &token).await;
    assert_eq!(run["tool"], tool.as_str());
    assert_eq!(run["run_dir"], run_dir.as_str());
    assert_eq!(run["device_id"], device_id.as_str());
}

/// A blank `tool` or `device_id`, and an empty `run_dir`, are stored as
/// none, as a value left out is. Stored as `""`, a blank `device_id` read
/// as none only because the desktop app tests it for truth.
#[tokio::test]
async fn an_import_stores_a_blank_tool_device_id_or_run_dir_as_none() {
    let (state, _fixture, token) = importer().await;
    let id = start_run(
        &state,
        &token,
        serde_json::json!({
            "source": "imessage",
            "tool": "  ",
            "device_id": " \t ",
            "run_dir": "",
        }),
    )
    .await;
    let run: serde_json::Value = get_json(&state, &format!("/v1/imports/{id}"), &token).await;
    for field in ["tool", "device_id", "run_dir"] {
        assert_eq!(run[field], serde_json::Value::Null, "{field}: {run}");
    }
}

/// A completion with an Import Error and a note both over the cap names
/// both, not only the first list it checked.
#[tokio::test]
async fn a_completion_names_every_issue_and_note_over_the_cap() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;
    let long = "x".repeat(2_001);
    let (status, text) = crate::test_support::post_json_raw(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({
            "status": "completed_with_issues",
            "issues": [{ "kind": "skip", "stage": "media", "item": long, "reason": "r" }],
            "notes": [{ "stage": "staging", "item": "a", "text": long }]
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
            "issues[0].item: must be at most 2000 characters",
            "notes[0].text: must be at most 2000 characters"
        ]),
        "{text}"
    );
}

/// A completion with a status the route does not take and a note over the
/// cap names both in one refusal, so the client fixes them in one go rather
/// than meeting the status only once the note is fixed (#2868).
#[tokio::test]
async fn a_completion_names_a_bad_status_with_a_note_over_the_cap() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (id, _) = crate::test_support::start_import_run(
        state,
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let (status, text) = crate::test_support::post_json_raw(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({
            "status": "finished",
            "notes": [{ "stage": "staging", "item": "a", "text": "x".repeat(2_001) }]
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
            "invalid import status 'finished'; expected 'completed', 'completed_with_issues', or 'failed'",
            "notes[0].text: must be at most 2000 characters"
        ]),
        "{text}"
    );
    crate::test_support::complete_import_run(state, token, id).await;
}

/// A new Import Run's body that breaks a rule of each kind the route checks
/// is refused once, naming every one, and starts no run (#2868).
#[tokio::test]
async fn starting_a_run_names_every_rule_the_body_breaks() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let (status, text) = crate::test_support::post_json_raw(
        state,
        "/v1/imports",
        token,
        serde_json::json!({
            "source": "iMessage",
            "phone_country": "ZZ",
            "device_id": "d".repeat(65),
            "form": { "pad": "f".repeat(70_000) }
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
            "source id 'iMessage' must use only lowercase letters, digits, hyphens, and underscores",
            "phone_country \"ZZ\" is not a country GET /v1/phone-countries lists",
            "device_id: must be at most 64 characters",
            "form: must be at most 65536 bytes of JSON"
        ]),
        "{text}"
    );
    // Nothing was started, so the same account starts a run at once.
    crate::test_support::start_import_run(
        state,
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
}

/// A note's `item` and `text` are each at most 2,000 characters, on a
/// completion and a discard alike, and a body over the cap leaves the run
/// running. They were stored at any length a JSON body could carry (#2668).
#[tokio::test]
async fn a_note_over_the_text_cap_is_refused_and_the_run_stays_running() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;
    let long = "x".repeat(crate::text_caps::MAX_IMPORT_NOTE_TEXT_CHARS + 1);

    for route in ["complete", "discard"] {
        let mut body = serde_json::json!({
            "issues": [],
            "notes": [
                { "stage": "staging", "item": "a", "text": "kept" },
                { "stage": "staging", "item": long, "text": long }
            ]
        });
        if route == "complete" {
            body["status"] = "completed".into();
        }
        let (status, text) = crate::test_support::post_json_raw(
            state,
            &format!("/v1/imports/{id}/{route}"),
            token,
            body,
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
                "notes[1].item: must be at most 2000 characters",
                "notes[1].text: must be at most 2000 characters"
            ]),
            "{route}: {text}"
        );
    }
    let run: serde_json::Value = get_json(state, &format!("/v1/imports/{id}"), token).await;
    assert_eq!(run["status"], "running", "{run}");
    assert_eq!(run["notes"], serde_json::json!([]), "{run}");
}

/// The reference states each cap on the text an Import Run stores, so the
/// desktop app, which keeps within them, reads the numbers the server holds.
#[test]
fn the_reference_states_the_import_run_text_caps() {
    let doc: serde_json::Value =
        serde_json::from_str(&crate::openapi::dump_openapi_json()).unwrap();
    let schemas = &doc["components"]["schemas"];
    // Each description states its cap as the reference writes a number.
    for (schema, field, cap, stated) in [
        (
            "CreateImportRequest",
            "tool",
            crate::text_caps::MAX_IMPORT_TOOL_CHARS,
            "over 64 characters",
        ),
        (
            "CreateImportRequest",
            "run_dir",
            crate::text_caps::MAX_PATH_CHARS,
            "over 4,096 characters",
        ),
        (
            "CreateImportRequest",
            "device_id",
            crate::text_caps::MAX_DEVICE_ID_CHARS,
            "over 64 characters",
        ),
        (
            "ImportNoteRequest",
            "item",
            crate::text_caps::MAX_IMPORT_NOTE_TEXT_CHARS,
            "over 2,000 characters",
        ),
        (
            "ImportNoteRequest",
            "text",
            crate::text_caps::MAX_IMPORT_NOTE_TEXT_CHARS,
            "over 2,000 characters",
        ),
    ] {
        let property = &schemas[schema]["properties"][field];
        assert_eq!(property["maxLength"], cap, "{schema}.{field}: {property}");
        // The description keeps the doc comment's line breaks.
        let described = property["description"]
            .as_str()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        assert!(
            described.contains(stated),
            "{schema}.{field} does not say {stated:?}: {described}"
        );
    }
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
    let id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;
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
                { "kind": "error", "stage": "media", "item": long, "reason": long }
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
    let id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

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

    assert_eq!(discarded["status"], "canceled", "{discarded}");
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
    let id = start_run(state, token, serde_json::json!({ "source": "imazing" })).await;
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
    let id = start_run(
        state,
        token,
        serde_json::json!({ "source": "sms-backup-plus" }),
    )
    .await;
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

    assert_eq!(discarded["status"], "canceled", "{discarded}");
    assert_eq!(discarded["notes"], serde_json::json!([note]), "{discarded}");
}

/// An Import Error's kind is `error` or `skip` and nothing else, on a
/// completion and on a discard alike: `note`, which a run reports but which
/// goes in `notes`, and a word no run reports are both refused in the
/// problem envelope, and the run stays running.
#[tokio::test]
async fn an_issue_kind_other_than_error_or_skip_is_refused() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

    for kind in ["note", "warning"] {
        let issues = serde_json::json!([{ "kind": kind, "stage": "staging", "item": "a.jsonl", "reason": "x" }]);
        for (route, body) in [
            (
                "complete",
                serde_json::json!({ "status": "completed", "issues": issues }),
            ),
            (
                "discard",
                serde_json::json!({ "issues": issues, "notes": [] }),
            ),
        ] {
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
            assert!(problem.sentence().contains(kind), "{route} {kind}: {text}");
        }
    }
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

    let completed_id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;
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

    let discarded_id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;
    let discarded: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{discarded_id}/discard"),
        token,
        serde_json::json!({ "issues": [], "notes": [] }),
    )
    .await;
    assert_eq!(discarded["status"], "canceled", "{discarded}");

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
async fn run_with_issues(state: &AppState, token: &str, issues: usize) -> i64 {
    let id = start_run(state, token, serde_json::json!({ "source": "whatsapp" })).await;
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
/// without the database, the account's and the administrator's alike.
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
    let admin: Page<AdminImportRunSummary> = rows.map(AdminImportRunSummary::from);
    assert_eq!(admin.items[2].issue_count, 3);
}

/// The `ImportAccess` extractor guards `GET /v1/imports`: with
/// `can_import` off, the endpoint refuses; turned back on, it succeeds.
#[tokio::test]
async fn import_endpoint_honors_can_import_flag() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let admin = log_in_as_admin(&state, "hunter2hunter2").await;
    let user = add_account_and_log_in(&state, "import-guard-user", "hunter2hunter2").await;

    assert_eq!(
        patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &admin.token,
            serde_json::json!({ "can_import": false }),
        )
        .await,
        StatusCode::OK
    );
    // can_import=false refuses GET /v1/imports.
    let (status, text) = get_raw(&state, "/v1/imports", &user.token).await;
    expect_problem(status, &text, ProblemType::InsufficientScope);

    assert_eq!(
        patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &admin.token,
            serde_json::json!({ "can_import": true }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        get_status(&state, "/v1/imports", &user.token).await,
        StatusCode::OK,
        "can_import=true must allow GET /v1/imports"
    );
}

/// The account's running Import Run through `GET /v1/imports?status=running`,
/// as the desktop app finds it, or `None` when the page is empty.
async fn running_run(state: &AppState, token: &str) -> Option<serde_json::Value> {
    let page: serde_json::Value = get_json(state, "/v1/imports?status=running", token).await;
    let items = page["items"].as_array().expect("a page of runs");
    assert!(items.len() <= 1, "an account runs one Import Run: {page}");
    items.first().cloned()
}

#[tokio::test]
async fn imports_complete_and_detail_surface_timings_and_issues() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let import_id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

    let completed: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{import_id}/complete"),
        token,
        serde_json::json!({
            "status": "completed",
            "bytes_uploaded": 100,
            "duration_ms": 48_000,
            "parse_ms": 18_000,
            "attachments_ms": 22_000,
            "prepare_ms": 4_000,
            "upload_ms": 8_000,
            "summary": { "parse": { "messages": 10 }, "convert": { "files": 2 } },
            "issues": [
                { "kind": "skip", "stage": "media", "item": "photo.heic", "reason": "convert failed" },
                { "kind": "error", "stage": "upload", "item": "archive.zip", "reason": "upload failed" },
            ],
        }),
    )
    .await;
    assert_eq!(completed["status"], "completed", "{completed}");
    // Counted from what the run holds, which is nothing here.
    assert_eq!(completed["message_count"], 0, "{completed}");
    assert_eq!(completed["attachment_count"], 0, "{completed}");
    assert_eq!(completed["bytes_uploaded"], 100, "{completed}");

    let detail: serde_json::Value =
        get_json(state, &format!("/v1/imports/{import_id}"), token).await;
    assert_eq!(detail["id"], import_id);
    assert_eq!(detail["duration_ms"], 48_000);
    assert_eq!(detail["parse_ms"], 18_000);
    assert_eq!(detail["attachments_ms"], 22_000);
    assert_eq!(detail["prepare_ms"], 4_000);
    assert_eq!(detail["upload_ms"], 8_000);
    assert_eq!(detail["summary"]["parse"]["messages"], 10);
    let issues = detail["issues"].as_array().expect("the run's issues");
    assert_eq!(issues.len(), 2, "{detail}");
    assert_eq!(issues[0]["kind"], "skip");
    assert_eq!(issues[0]["stage"], "media");
    assert_eq!(issues[1]["kind"], "error");
    assert_eq!(issues[1]["stage"], "upload");
}

#[tokio::test]
async fn imports_complete_stores_completed_with_issues_status() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let import_id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

    let completed: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{import_id}/complete"),
        token,
        serde_json::json!({ "status": "completed_with_issues", "bytes_uploaded": 100 }),
    )
    .await;
    assert_eq!(completed["status"], "completed_with_issues", "{completed}");
    let detail: serde_json::Value =
        get_json(state, &format!("/v1/imports/{import_id}"), token).await;
    assert_eq!(detail["status"], "completed_with_issues", "{detail}");
}

#[tokio::test]
async fn imports_complete_rejects_unknown_status() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let import_id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

    let (status, text) = post_json_raw(
        state,
        &format!("/v1/imports/{import_id}/complete"),
        token,
        serde_json::json!({ "status": "victorious" }),
    )
    .await;
    expect_problem(status, &text, ProblemType::ValidationFailed);

    // The run is untouched.
    let run = running_run(state, token).await.expect("the run still runs");
    assert_eq!(run["id"], import_id);
}

#[tokio::test]
async fn reading_a_missing_import_run_is_not_found() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let import_id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

    let (status, text) = get_raw(state, &format!("/v1/imports/{}", import_id + 1), token).await;
    let problem = expect_problem(status, &text, ProblemType::NotFound);
    let detail = problem.detail.unwrap_or_default();
    assert!(
        detail.contains("import") && detail.contains("not found"),
        "{text}"
    );
}

#[tokio::test]
async fn active_import_run_is_empty_then_reports_the_live_one() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    assert!(running_run(state, token).await.is_none());

    let import_id = start_run(
        state,
        token,
        serde_json::json!({
            "source": "imessage",
            "mode": "replace",
            "tool": "message-crate",
            "run_dir": "/home/u/message-crate/staging-260830",
            "device_id": "device-a",
            "form": { "source": "imessage-ios" },
            "source_fingerprint": { "size_bytes": 42 },
        }),
    )
    .await;

    let run = running_run(state, token)
        .await
        .expect("a running run is listed");
    assert_eq!(run["id"], import_id);
    assert_eq!(run["stage"], "parse");
    // The Upload reads its run's mode from this row (#3165).
    assert_eq!(run["mode"], "replace");
    assert_eq!(run["run_dir"], "/home/u/message-crate/staging-260830");
    assert_eq!(run["device_id"], "device-a");
    assert_eq!(run["form"]["source"], "imessage-ios");
}

/// A stored form snapshot never carries credentials, whatever the
/// client posts: the row outlives the run, and the secret must not.
#[tokio::test]
async fn a_stored_form_snapshot_drops_credentials() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    start_run(
        state,
        token,
        serde_json::json!({
            "source": "imessage",
            // A client that has not learned the rule.
            "form": {
                "source": "imessage-ios",
                "backupPassword": "hunter2",
                "whatsappKey": "0123456789abcdef",
            },
        }),
    )
    .await;

    let run = running_run(state, token)
        .await
        .expect("a running run is listed");
    let form = &run["form"];
    assert_eq!(
        form["source"], "imessage-ios",
        "the rest of the snapshot is kept"
    );
    assert!(
        form.get("backupPassword").is_none(),
        "backupPassword was stored: {form}"
    );
    assert!(
        form.get("whatsappKey").is_none(),
        "whatsappKey was stored: {form}"
    );
}

/// The identity list a client read from the backup rides on the Import Run
/// so a resumed Staging Review can show it without re-reading the backup.
#[tokio::test]
async fn imports_create_stores_source_identities() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    start_run(
        state,
        token,
        serde_json::json!({
            "source": "imessage",
            "source_identities": ["+15555550110", "owner@example.com"],
        }),
    )
    .await;

    let run = running_run(state, token)
        .await
        .expect("a running run is listed");
    assert_eq!(
        run["source_identities"],
        serde_json::json!(["+15555550110", "owner@example.com"])
    );
}

#[tokio::test]
async fn a_second_import_run_is_refused_with_conflict() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

    let (status, text) = post_json_raw(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::StateConflict);
    // The 409 has to name the way out: a stranded run is resumed or
    // discarded from the desktop app's Import screen, or discarded with
    // `message-crate-server imports discard`.
    let detail = problem.detail.unwrap_or_default();
    assert!(
        detail.contains("Import in the desktop app"),
        "the conflict names how to clear the run: {detail}"
    );
}

#[tokio::test]
async fn stage_endpoint_advances() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let import_id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

    let _: serde_json::Value = patch_json(
        state,
        &format!("/v1/imports/{import_id}"),
        token,
        serde_json::json!({ "stage": "write" }),
    )
    .await;
    let run = running_run(state, token).await.expect("the run still runs");
    assert_eq!(run["stage"], "write");
}

#[tokio::test]
async fn discard_frees_the_slot() {
    let (fixture, account) = fixture_with_account().await;
    let state = &fixture.state;
    let token = account.token.as_str();
    let import_id = start_run(state, token, serde_json::json!({ "source": "imessage" })).await;

    let _: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{import_id}/discard"),
        token,
        serde_json::json!({ "issues": [], "notes": [] }),
    )
    .await;
    assert!(running_run(state, token).await.is_none());
    start_run(state, token, serde_json::json!({ "source": "imessage" })).await;
}
