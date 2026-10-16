use super::*;
use crate::test_support::{fixed_instant, fixed_start_instant};

const ACCOUNT_ID: i64 = 7;

async fn setup_accounts_only() -> (sqlx::SqlitePool, tempfile::TempDir) {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_schema(&mut conn).await.unwrap();
    crate::test_support::insert_account_with_id(&mut conn, ACCOUNT_ID, "alice").await;
    (pool, dir)
}

/// Default start arguments for tests that only care that a running
/// Import Run exists, not about its stage or the fields the desktop app sets.
fn default_start_args(account_id: i64) -> StartImportArgs<'static> {
    StartImportArgs::new(account_id, "ios", "append", Some("message-crate"))
}

/// A completed run's arguments with every count and timing filled and no
/// issues or notes.
fn completed_args() -> CompleteImportArgs {
    CompleteImportArgs {
        status: "completed".into(),
        message_count: Some(10),
        attachment_count: Some(2),
        bytes_uploaded: Some(100),
        duration_ms: Some(48_000),
        parse_ms: Some(18_000),
        attachments_ms: Some(22_000),
        prepare_ms: Some(4_000),
        upload_ms: Some(8_000),
        summary_json: Some(r#"{"parse":{"messages":10}}"#.into()),
        issues: Vec::new(),
        notes: Vec::new(),
    }
}

/// Open a run at `parse` and move it through the stages in order to
/// `stage`, the only way a run reaches a later stage.
async fn open_run_at(conn: &mut SqliteConnection, stage: ImportStage) -> i64 {
    let id = start_import(conn, &default_start_args(ACCOUNT_ID), fixed_start_instant())
        .await
        .unwrap();
    let last = ImportStage::ALL.iter().position(|s| *s == stage).unwrap();
    // `ALL[0]` is `parse`, where the run opened.
    for next in &ImportStage::ALL[1..=last] {
        set_import_stage(conn, ACCOUNT_ID, id, *next, None)
            .await
            .unwrap();
    }
    id
}

#[tokio::test]
async fn complete_import_persists_timings_and_issues() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();

    let row = complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs {
            issues: vec![ImportIssueInput {
                kind: ImportErrorKind::Skip,
                stage: crate::db::imports::ImportIssueStage::Media,
                item: "photo.heic".into(),
                reason: "convert failed".into(),
            }],
            ..completed_args()
        },
        fixed_instant(),
    )
    .await
    .unwrap();

    assert_eq!(row.duration_ms, Some(48_000));
    assert_eq!(row.parse_ms, Some(18_000));
    assert_eq!(row.attachments_ms, Some(22_000));
    assert_eq!(row.prepare_ms, Some(4_000));
    assert_eq!(row.upload_ms, Some(8_000));
    assert_eq!(
        row.summary_json.as_deref(),
        Some(r#"{"parse":{"messages":10}}"#)
    );

    let issue_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM import_issues WHERE import_id = $1")
            .bind(import_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(issue_count, 1);
}

#[tokio::test]
async fn require_running_import_rejects_a_completed_import_and_returns_a_running_one() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs::succeeded(1, 0),
        fixed_instant(),
    )
    .await
    .unwrap();

    let err = require_running_import(&mut conn, ACCOUNT_ID, import_id)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("not running"), "{err}");

    let running = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    let row = require_running_import(&mut conn, ACCOUNT_ID, running)
        .await
        .unwrap();
    // The row is what a batch imports under; nothing in the request says.
    assert_eq!((row.source.as_str(), row.mode.as_str()), ("ios", "append"));
}

#[tokio::test]
async fn list_import_issues_returns_them_oldest_first() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs {
            issues: vec![
                ImportIssueInput {
                    kind: ImportErrorKind::Skip,
                    stage: crate::db::imports::ImportIssueStage::Media,
                    item: "photo.heic".into(),
                    reason: "convert failed".into(),
                },
                ImportIssueInput {
                    kind: ImportErrorKind::Error,
                    stage: crate::db::imports::ImportIssueStage::Upload,
                    item: "archive.zip".into(),
                    reason: "upload failed".into(),
                },
            ],
            ..completed_args()
        },
        fixed_instant(),
    )
    .await
    .unwrap();

    let row = get_owned_import(&mut conn, ACCOUNT_ID, import_id)
        .await
        .unwrap();
    assert_eq!(row.duration_ms, Some(48_000));
    assert_eq!(row.parse_ms, Some(18_000));
    let issues = list_import_issues(&mut conn, import_id).await.unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].kind, ImportErrorKind::Skip);
    assert_eq!(issues[0].stage, crate::db::imports::ImportIssueStage::Media);
    assert_eq!(issues[1].kind, ImportErrorKind::Error);
    assert_eq!(
        issues[1].stage,
        crate::db::imports::ImportIssueStage::Upload
    );
}

#[tokio::test]
async fn list_imports_includes_duration_ms() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs {
            parse_ms: None,
            attachments_ms: None,
            prepare_ms: None,
            upload_ms: None,
            summary_json: None,
            ..completed_args()
        },
        fixed_instant(),
    )
    .await
    .unwrap();

    let (imports, total) =
        list_imports_page(&mut conn, ACCOUNT_ID, None, &DEFAULT_IMPORT_SORT, 40, 0)
            .await
            .unwrap();
    assert_eq!((imports.len(), total), (1, 1));
    assert_eq!(imports[0].row.duration_ms, Some(48_000));
    let (running, total) = list_imports_page(
        &mut conn,
        ACCOUNT_ID,
        Some("running"),
        &DEFAULT_IMPORT_SORT,
        40,
        0,
    )
    .await
    .unwrap();
    assert_eq!((running.len(), total), (0, 0));
}

#[tokio::test]
async fn a_failed_import_is_recorded_as_failed() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();

    complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs::failed(),
        fixed_instant(),
    )
    .await
    .unwrap();

    let row = get_owned_import(&mut conn, ACCOUNT_ID, import_id)
        .await
        .unwrap();
    assert_eq!(row.status.as_str(), "failed");
}

#[tokio::test]
async fn the_list_sorted_by_start_ascending_puts_the_oldest_run_first() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    // Each run starts a minute before the one inserted ahead of it, so the
    // order by start is the reverse of the order by id, and only a sort that
    // reads `started_at` lists them oldest first.
    let mut started = Vec::new();
    for minutes_earlier in 0..2 {
        let import_id = start_import(
            &mut conn,
            &default_start_args(ACCOUNT_ID),
            fixed_start_instant() - chrono::TimeDelta::minutes(minutes_earlier),
        )
        .await
        .unwrap();
        complete_import(
            &mut conn,
            ACCOUNT_ID,
            import_id,
            &CompleteImportArgs::succeeded(1, 0),
            fixed_instant(),
        )
        .await
        .unwrap();
        started.push(import_id);
    }
    let oldest_first = [SortKey {
        key: ImportSort::StartedAt,
        direction: Direction::Asc,
    }];

    let (imports, _) = list_imports_page(&mut conn, ACCOUNT_ID, None, &oldest_first, 40, 0)
        .await
        .unwrap();

    let listed: Vec<i64> = imports.iter().map(|run| run.row.id).collect();
    let oldest_first_ids: Vec<i64> = started.iter().rev().copied().collect();
    assert_eq!(listed, oldest_first_ids);
}

/// The account's running Import Run through the list, as the desktop app
/// finds it: `status=running`, and at most one.
async fn listed_running_import(conn: &mut SqliteConnection, account: i64) -> Option<ImportRow> {
    let (items, _) = list_imports_page(conn, account, Some("running"), &DEFAULT_IMPORT_SORT, 1, 0)
        .await
        .unwrap();
    items.into_iter().next().map(|listed| listed.row)
}

#[tokio::test]
async fn running_import_run_round_trips_and_blocks_a_second() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let account = ACCOUNT_ID;

    assert!(
        listed_running_import(&mut conn, account).await.is_none(),
        "no run before one starts"
    );

    let args = StartImportArgs {
        run_dir: Some("/home/u/message-crate/staging-iphone-260830"),
        device_id: Some("device-a"),
        form_json: Some(r#"{"source":"imessage-ios"}"#),
        source_fingerprint: Some(r#"{"path":"/b","size_bytes":10}"#),
        ..StartImportArgs::new(account, "imessage", "append", Some("message-crate"))
    };
    let id = start_import(&mut conn, &args, fixed_start_instant())
        .await
        .unwrap();

    let active = listed_running_import(&mut conn, account)
        .await
        .expect("the run is running");
    assert_eq!(active.id, id);
    assert_eq!(active.stage, Some(ImportStage::Parse));
    assert_eq!(
        active.run_dir.as_deref(),
        Some("/home/u/message-crate/staging-iphone-260830")
    );
    assert_eq!(active.device_id.as_deref(), Some("device-a"));
    assert_eq!(json_column(active.form_json)["source"], "imessage-ios");

    assert!(
        matches!(
            start_import(&mut conn, &args, fixed_start_instant()).await,
            Err(StartImportError::AlreadyActive)
        ),
        "a second running Import Run is refused by the index, not by a race-prone check"
    );
}

#[tokio::test]
async fn stage_advances_and_discard_frees_the_slot() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let account = ACCOUNT_ID;
    let args = StartImportArgs::new(account, "imessage", "append", None);
    let id = start_import(&mut conn, &args, fixed_start_instant())
        .await
        .unwrap();

    for stage in [
        ImportStage::Write,
        ImportStage::StagingReview,
        ImportStage::Upload,
    ] {
        set_import_stage(&mut conn, account, id, stage, None)
            .await
            .unwrap();
    }
    let active = listed_running_import(&mut conn, account).await.unwrap();
    assert_eq!(active.stage, Some(ImportStage::Upload));

    discard_import(&mut conn, account, id, &[], &[], fixed_instant())
        .await
        .unwrap();
    assert!(
        listed_running_import(&mut conn, account).await.is_none(),
        "a discarded run is no longer running"
    );
    let row = get_owned_import(&mut conn, account, id).await.unwrap();
    assert_eq!(row.status.as_str(), "cancelled");
    assert!(row.finished_at.is_some(), "a discard closes the run");

    // The slot is genuinely free.
    start_import(&mut conn, &args, fixed_start_instant())
        .await
        .expect("a new Import Run can start");
}

/// One issue and one note, for the tests that check when they are recorded.
fn one_issue_and_one_note() -> (Vec<ImportIssueInput>, Vec<ImportNoteRow>) {
    let issue = ImportIssueInput {
        kind: ImportErrorKind::Error,
        stage: ImportIssueStage::Media,
        item: "photo.heic".into(),
        reason: "convert failed".into(),
    };
    let note = ImportNoteRow {
        stage: ImportIssueStage::Media,
        item: "clip.mov".into(),
        text: "kept the original".into(),
    };
    (vec![issue], vec![note])
}

/// The `created_at` of every issue and every note recorded for `import_id`.
async fn issue_and_note_stamps(conn: &mut SqliteConnection, import_id: i64) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT created_at FROM import_issues WHERE import_id = $1
         UNION ALL
         SELECT created_at FROM import_notes WHERE import_id = $1",
    )
    .bind(import_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap()
}

/// A discard closes the run, and records its issues and notes, at the
/// instant its caller passes, not at the clock's.
#[tokio::test]
async fn discard_import_stamps_the_run_and_its_issues_and_notes_with_the_instant_passed() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    let (issues, notes) = one_issue_and_one_note();

    discard_import(&mut conn, ACCOUNT_ID, id, &issues, &notes, fixed_instant())
        .await
        .unwrap();

    let at = fixed_instant().to_rfc3339();
    let row = get_owned_import(&mut conn, ACCOUNT_ID, id).await.unwrap();
    assert_eq!(row.finished_at.as_deref(), Some(at.as_str()));
    assert_eq!(
        issue_and_note_stamps(&mut conn, id).await,
        [at.as_str(), at.as_str()]
    );
}

/// A completion records its issues and notes at the instant its caller
/// passes, the instant the run finishes.
#[tokio::test]
async fn complete_import_stamps_its_issues_and_notes_with_the_instant_passed() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    let (issues, notes) = one_issue_and_one_note();
    let args = CompleteImportArgs {
        status: "completed_with_issues".into(),
        issues,
        notes,
        ..Default::default()
    };

    complete_import(&mut conn, ACCOUNT_ID, id, &args, fixed_instant())
        .await
        .unwrap();

    let at = fixed_instant().to_rfc3339();
    assert_eq!(
        issue_and_note_stamps(&mut conn, id).await,
        [at.as_str(), at.as_str()]
    );
}

#[tokio::test]
async fn running_import_finds_the_running_import_run_by_account_and_skips_finished_ones() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let account = ACCOUNT_ID;

    assert!(
        running_import(&mut conn, account).await.unwrap().is_none(),
        "no running Import Run before one starts"
    );

    let args = StartImportArgs::new(account, "imessage", "append", None);
    let finished = start_import(&mut conn, &args, fixed_start_instant())
        .await
        .unwrap();
    complete_import(
        &mut conn,
        account,
        finished,
        &CompleteImportArgs::succeeded(1, 0),
        fixed_instant(),
    )
    .await
    .unwrap();
    let stranded = start_import(&mut conn, &args, fixed_start_instant())
        .await
        .unwrap();

    let running = running_import(&mut conn, account)
        .await
        .unwrap()
        .expect("the stranded Import Run is running");
    assert_eq!(running.id, stranded, "a finished Import Run is left out");

    discard_import(&mut conn, account, stranded, &[], &[], fixed_instant())
        .await
        .unwrap();
    assert!(
        running_import(&mut conn, account).await.unwrap().is_none(),
        "a discarded Import Run is no longer running"
    );
}

#[tokio::test]
async fn completing_an_import_run_frees_the_slot_too() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let account = ACCOUNT_ID;
    let args = StartImportArgs::new(account, "imessage", "append", None);
    let id = start_import(&mut conn, &args, fixed_start_instant())
        .await
        .unwrap();
    complete_import(
        &mut conn,
        account,
        id,
        &CompleteImportArgs::succeeded(10, 2),
        fixed_instant(),
    )
    .await
    .unwrap();
    assert!(listed_running_import(&mut conn, account).await.is_none());
}

#[test]
fn every_stage_round_trips_through_its_string() {
    // The column's spelling is the wire's: a stage the API accepts is stored
    // as a word every later read of the row can parse.
    for stage in ImportStage::ALL {
        assert_eq!(ImportStage::parse(stage.as_str()), Some(stage));
        assert_eq!(serde_json::to_value(stage).unwrap(), stage.as_str());
    }
    for stage in ImportIssueStage::ALL {
        assert_eq!(ImportIssueStage::parse(stage.as_str()), Some(stage));
        assert_eq!(serde_json::to_value(stage).unwrap(), stage.as_str());
    }
    assert_eq!(ImportStage::parse("gate_1"), None);
}

/// The moves the desktop app makes: a review re-recorded after its write
/// failed is a stage to itself, a resumed Staging writes `write` again, and
/// approving moves to the next stage, or from the Staging Review straight to
/// Upload when the import has no Media Stage.
const DESKTOP_APP_MOVES: [(ImportStage, ImportStage); 6] = [
    (ImportStage::Parse, ImportStage::Write),
    (ImportStage::Write, ImportStage::StagingReview),
    (ImportStage::StagingReview, ImportStage::Media),
    (ImportStage::StagingReview, ImportStage::Upload),
    (ImportStage::Media, ImportStage::MediaReview),
    (ImportStage::MediaReview, ImportStage::Upload),
];

#[test]
fn a_stage_moves_only_to_itself_the_next_stage_or_from_staging_review_to_upload() {
    for from in ImportStage::ALL {
        for to in ImportStage::ALL {
            let expected = from == to || DESKTOP_APP_MOVES.contains(&(from, to));
            assert_eq!(
                from.may_move_to(to),
                expected,
                "{} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }
}

/// A running run with no stored stage has no stage to check a move from, so
/// the move is an error rather than passing unchecked (#2638).
#[tokio::test]
async fn a_running_run_without_a_stage_refuses_a_move() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE imports SET stage = NULL WHERE id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let err = set_import_stage(&mut conn, ACCOUNT_ID, id, ImportStage::Upload, None)
        .await
        .unwrap_err();

    assert!(matches!(err, ImportLookupError::Db(_)), "{err:?}");
    let row = get_owned_import(&mut conn, ACCOUNT_ID, id).await.unwrap();
    assert_eq!(row.stage, None, "the refused move wrote no stage");
}

/// A run moved backwards is resumed at the wrong stage on the next visit
/// (#1227, #2167), so the server refuses the move and keeps the stage.
#[tokio::test]
async fn a_backward_stage_move_is_refused_and_the_stage_kept() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let id = open_run_at(&mut conn, ImportStage::Upload).await;

    let err = set_import_stage(&mut conn, ACCOUNT_ID, id, ImportStage::Parse, Some("{}"))
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            ImportLookupError::StageMove {
                from: ImportStage::Upload,
                to: ImportStage::Parse,
                ..
            }
        ),
        "{err:?}"
    );
    let message = err.to_string();
    assert!(
        message.contains("upload") && message.contains("parse"),
        "the refusal names both stages: {message}"
    );
    let row = get_owned_import(&mut conn, ACCOUNT_ID, id).await.unwrap();
    assert_eq!(row.stage, Some(ImportStage::Upload));
    assert_eq!(row.summary_json, None, "a refused move records no summary");
}

/// Every move the desktop app makes is accepted, each review written twice as a
/// resume after a failed write does.
#[tokio::test]
async fn every_move_the_desktop_app_makes_is_accepted() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    for (from, to) in DESKTOP_APP_MOVES {
        let id = open_run_at(&mut conn, from).await;
        for stage in [to, to] {
            set_import_stage(&mut conn, ACCOUNT_ID, id, stage, None)
                .await
                .unwrap_or_else(|e| panic!("{} -> {}: {e}", from.as_str(), to.as_str()));
        }
        let row = get_owned_import(&mut conn, ACCOUNT_ID, id).await.unwrap();
        assert_eq!(row.stage, Some(to));
        discard_import(&mut conn, ACCOUNT_ID, id, &[], &[], fixed_instant())
            .await
            .unwrap();
    }
}

/// A finished run is its permanent record. Completing a discarded run once
/// marked it completed and filed its issues against it; completing a
/// completed run filed them a second time.
#[tokio::test]
async fn complete_import_refuses_a_run_that_has_finished() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let with_issue = || CompleteImportArgs {
        status: "completed_with_issues".into(),
        issues: vec![ImportIssueInput {
            kind: ImportErrorKind::Skip,
            stage: crate::db::imports::ImportIssueStage::Media,
            item: "photo.heic".into(),
            reason: "convert failed".into(),
        }],
        ..Default::default()
    };
    let issue_count = async |conn: &mut sqlx::SqliteConnection, import_id: i64| -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM import_issues WHERE import_id = $1")
            .bind(import_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap()
    };

    let discarded = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    discard_import(&mut conn, ACCOUNT_ID, discarded, &[], &[], fixed_instant())
        .await
        .unwrap();
    let err = complete_import(
        &mut conn,
        ACCOUNT_ID,
        discarded,
        &with_issue(),
        fixed_instant(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ImportLookupError::InvalidRun { .. }));
    let row = get_owned_import(&mut conn, ACCOUNT_ID, discarded)
        .await
        .unwrap();
    assert_eq!(row.status.as_str(), "cancelled");
    assert_eq!(issue_count(&mut conn, discarded).await, 0);

    let completed = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    complete_import(
        &mut conn,
        ACCOUNT_ID,
        completed,
        &with_issue(),
        fixed_instant(),
    )
    .await
    .unwrap();
    let err = complete_import(
        &mut conn,
        ACCOUNT_ID,
        completed,
        &CompleteImportArgs::failed(),
        fixed_instant(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ImportLookupError::InvalidRun { .. }));
    let row = get_owned_import(&mut conn, ACCOUNT_ID, completed)
        .await
        .unwrap();
    assert_eq!(row.status.as_str(), "completed_with_issues");
    assert_eq!(issue_count(&mut conn, completed).await, 1);
}

/// The desktop app posts `complete` while the person clicks Discard. The
/// discard read the run as running, the completion committed, and the discard
/// then rewrote the completed run as `cancelled` with a new `finished_at`.
#[tokio::test]
async fn a_discard_that_lands_after_the_run_completed_is_refused() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();

    let mut other_conn = pool.acquire().await.unwrap();
    let mut other = crate::db::begin_write(&mut other_conn).await.unwrap();
    complete_elsewhere(&mut other, id).await;
    let err = crate::db::write_tx::commit_during(
        other,
        discard_import(&mut conn, ACCOUNT_ID, id, &[], &[], fixed_instant()),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, ImportLookupError::InvalidRun { .. }),
        "{err:?}"
    );
    let row = get_owned_import(&mut conn, ACCOUNT_ID, id).await.unwrap();
    assert_eq!(row.status.as_str(), "completed");
    assert_eq!(row.finished_at.as_deref(), Some(COMPLETED_AT));
}

/// A stage change that lands after the run completed left a stage on a
/// finished run.
#[tokio::test]
async fn a_stage_change_that_lands_after_the_run_completed_is_refused() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();

    let mut other_conn = pool.acquire().await.unwrap();
    let mut other = crate::db::begin_write(&mut other_conn).await.unwrap();
    complete_elsewhere(&mut other, id).await;
    let err = crate::db::write_tx::commit_during(
        other,
        set_import_stage(&mut conn, ACCOUNT_ID, id, ImportStage::Upload, None),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, ImportLookupError::InvalidRun { .. }),
        "{err:?}"
    );
    let stage: Option<String> = sqlx::query_scalar("SELECT stage FROM imports WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(stage, None);
}

/// When [`complete_elsewhere`] says the run finished.
const COMPLETED_AT: &str = "2026-10-02T12:00:00+00:00";

/// Complete run `id` on `other`, as `complete_import` leaves a run.
async fn complete_elsewhere(other: &mut crate::db::WriteTx<'_>, id: i64) {
    sqlx::query(
        "UPDATE imports SET status = 'completed', stage = NULL, finished_at = $1 WHERE id = $2",
    )
    .bind(COMPLETED_AT)
    .bind(id)
    .execute(&mut **other)
    .await
    .unwrap();
}

/// The database column and the wire carry one spelling of each status: the
/// row is written with `as_str` and the response with serde.
#[test]
fn every_import_status_serializes_as_the_word_the_database_holds() {
    for status in ImportStatus::ALL {
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::Value::String(status.as_str().to_string())
        );
        assert_eq!(ImportStatus::parse(status.as_str()), Some(status));
    }
    assert_eq!(ImportStatus::Cancelled.as_str(), "cancelled");
    assert_eq!(ImportStatus::parse("canceled"), None);
}

#[tokio::test]
async fn an_import_row_maps_the_same_whatever_order_the_select_lists_its_columns() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(
        &mut conn,
        &default_start_args(ACCOUNT_ID),
        fixed_start_instant(),
    )
    .await
    .unwrap();
    complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &completed_args(),
        fixed_instant(),
    )
    .await
    .unwrap();

    let reversed = IMPORT_COLUMNS
        .split(',')
        .map(str::trim)
        .rev()
        .collect::<Vec<_>>()
        .join(", ");
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {reversed} FROM imports WHERE id = $1"
    )))
    .bind(import_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let in_order = get_owned_import(&mut conn, ACCOUNT_ID, import_id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(import_from_row(&row).unwrap()).unwrap(),
        serde_json::to_value(in_order).unwrap(),
    );
}
