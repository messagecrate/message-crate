use std::fs;
use std::path::{Path, PathBuf};

use std::sync::Arc;

use tokio::sync::OwnedSemaphorePermit;

use super::pipeline::{
    FixedImportArgs, IMPORT_CONTACT_GROUP_NAME_SQL, ImportOptions, complete_run_at,
    import_contact_group_name, import_jsonl_files, import_jsonl_files_on_conn,
};
use super::run_control::MAX_CONCURRENT_IMPORTS;
use super::*;
use crate::assets_api;
use crate::db::engine;
use crate::db::imports::CompleteImportArgs;
use crate::progress::Progress;
use crate::test_support::{
    ConversationHeaderLine, MessageLine, RegisteredAccount, TestFixture,
    assert_every_person_is_on_a_contact, attachment, conversation_header, fixture_with_account,
    get_json, import_ada_conversation, import_jsonl_text, message_line, patch_json,
    post_created_json, post_json, test_fixture,
};
use message_ir::{
    Deletion, EarlierVersion, IrAttachment, IrImessage, IrMessageKind, IrService, Reaction,
};
use tempfile::TempDir;

const TEST_ACCOUNT: i64 = 7;

/// An incoming WhatsApp message line sent at 1700000000000, of the `sms`
/// kind, as the WhatsApp batches in these tests write it.
fn whatsapp_line(guid: &str, text: &str) -> MessageLine {
    message_line(guid, text)
        .at(1_700_000_000_000)
        .sms()
        .service(IrService::Whatsapp)
}

fn write_jsonl(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

/// Open a verify connection to an on-disk test database.
async fn open_verify(db: &Path) -> (sqlx::SqlitePool, sqlx::pool::PoolConnection<sqlx::Sqlite>) {
    let pool = engine::open_pool_for_path(db).await.unwrap();
    let conn = pool.acquire().await.unwrap();
    (pool, conn)
}

fn replace_opts<'a>(assets: &'a Path, root: &'a Path, source: &'a str) -> ImportOptions<'a> {
    ImportOptions::fixed(FixedImportArgs {
        assets_dir: assets,
        asset_root: Some(root),
        mode: ImportMode::Replace,
        source,
        account_id: TEST_ACCOUNT,
        import_id: None,
        phone_country: None,
        now: crate::test_support::fixed_instant(),
    })
}

fn missing_attachment(name: &str) -> IrAttachment {
    IrAttachment {
        size_bytes: Some(12),
        missing_reason: Some("not_found".into()),
        ..attachment(
            &format!("attachments/{name}"),
            name,
            "application/octet-stream",
        )
    }
}

/// A reaction of `kind` to the first part of a message, from `reactor`.
fn reaction_from(reactor: &str, kind: &str) -> Reaction {
    Reaction {
        part_index: 0,
        kind: kind.into(),
        emoji: None,
        is_from_me: false,
        reactor_identity: Some(reactor.into()),
        reactor_display_name: None,
    }
}

/// The reaction of `reactor`, who liked a message.
fn liked_by(reactor: &str) -> Reaction {
    reaction_from(reactor, "liked")
}

/// One earlier version of `g-edit`, as an `edits` entry.
fn edit_version(part: u32, text: &str, ms: i64) -> EarlierVersion {
    EarlierVersion {
        part_index: part,
        text: Some(text.into()),
        edited_at_unix_ms: Some(ms),
    }
}

/// Append-mode options for the `g-edit` tests.
fn edit_options<'a>(assets: &'a Path, root: &'a Path) -> ImportOptions<'a> {
    ImportOptions::fixed(FixedImportArgs {
        assets_dir: assets,
        asset_root: Some(root),
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        import_id: None,
        phone_country: None,
        now: crate::test_support::fixed_instant(),
    })
}

/// A registered account may import with its session token: `can_import`
/// is on by default, which `server.rs`'s `can_import = 0` test relies on
/// to prove the opposite case.
async fn importer() -> (
    crate::server::AppState,
    crate::test_support::TestFixture,
    String,
) {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "importer", "hunter2hunter2").await;
    let state = fixture.state.clone();
    (state, fixture, account.token)
}

/// Create an Import Run for `source` and hand back the path its batches
/// are posted to.
pub(super) async fn batches_path(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
) -> String {
    let (run, _) = crate::test_support::start_import_run(
        state,
        token,
        serde_json::json!({ "source": source }),
    )
    .await;
    format!("/v1/imports/{run}/batches")
}

/// One `source` conversation with `+15555550107`, one message per guid. The
/// message `g-gone` carries a missing attachment and a tapback, so a wipe
/// that leaves children behind shows.
fn wipe_test_batch(source: &str, guids: &[&str]) -> String {
    let mut lines = vec![
        conversation_header(source, "+15555550107")
            .owner("+15555550106", Some("Me"))
            .participant("+15555550107", None)
            .to_string(),
    ];
    for guid in guids {
        let mut message = message_line(guid, guid)
            .at(1_700_000_000_000)
            .sms()
            .sender("+15555550107");
        if *guid == "g-gone" {
            message = message
                .attachment(missing_attachment("gone.bin"))
                .reaction(liked_by("+15555550107"));
        }
        lines.push(message.to_string());
    }
    lines.join("\n") + "\n"
}

/// Create an Import Run for `source` in `mode`, post `body` as its one
/// batch, and complete the run so the account may start another.
async fn import_one_batch(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    mode: &str,
    body: String,
) {
    import_one_batch_requesting(
        state,
        token,
        serde_json::json!({ "source": source, "mode": mode }),
        body,
    )
    .await;
}

/// Create an Import Run with `request` as the `POST /v1/imports` body,
/// post `body` as its one batch, and complete the run so the account may
/// start another. A test that checks something between the steps calls
/// them itself: [`crate::test_support::start_import_run`],
/// [`crate::test_support::post_import_batch`] and
/// [`crate::test_support::complete_import_run`].
async fn import_one_batch_requesting(
    state: &crate::server::AppState,
    token: &str,
    request: serde_json::Value,
    body: String,
) {
    let (run, _) = crate::test_support::start_import_run(state, token, request).await;
    crate::test_support::post_import_batch(state, token, run, body).await;
    crate::test_support::complete_import_run(state, token, run).await;
}

/// Complete `import_id` and hand back the date its row says it
/// finished on, the `YYYY-MM-DD` the shortcuts are named after.
async fn complete_run(state: &crate::server::AppState, token: &str, import_id: i64) -> String {
    crate::test_support::complete_import_run(state, token, import_id).await;
    let mut conn = state.db.acquire().await.unwrap();
    let finished_at: String = sqlx::query_scalar("SELECT finished_at FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    finished_at[..10].to_string()
}

/// Complete `import_id` at [`crate::test_support::fixed_instant`], so its
/// shortcuts are named after that day at any hour the test runs. It calls
/// `complete_run_at` directly, as the complete route does through
/// `complete_run`, so it skips the route's token and ownership checks. It
/// first moves the run's start to the day before, so the run starts before
/// it finishes and a name taken from the start rather than the finish shows
/// as the wrong day.
pub(super) async fn complete_run_at_fixed_instant(state: &crate::server::AppState, import_id: i64) {
    let finished = crate::test_support::fixed_instant();
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query("UPDATE imports SET started_at = $1 WHERE id = $2")
        .bind((finished - chrono::Duration::days(1)).to_rfc3339())
        .bind(import_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let account: i64 = sqlx::query_scalar("SELECT account_id FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    complete_run_at(
        &mut conn,
        account,
        import_id,
        &CompleteImportArgs {
            status: "completed".into(),
            ..Default::default()
        },
        finished,
    )
    .await
    .unwrap();
}

/// Create an Import Run for `source`, post `body` as its one batch, and hand
/// back the run's id, still running.
async fn posted_run(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    body: String,
) -> i64 {
    let path = batches_path(state, token, source).await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let (status, text) =
        crate::test_support::post_raw(state, &path, token, "application/jsonl", body).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    import_id
}

/// Create an Import Run for `source`, post `body` as its one batch, complete
/// it through the route, and hand back the run's id and the day it finished.
pub(super) async fn completed_run(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    body: String,
) -> (i64, String) {
    let import_id = posted_run(state, token, source, body).await;
    let date = complete_run(state, token, import_id).await;
    (import_id, date)
}

/// Create an Import Run for `source`, post `body` as its one batch, complete
/// it with [`complete_run_at_fixed_instant`], and hand back the run's id.
pub(super) async fn completed_run_at_fixed_instant(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    body: String,
) -> i64 {
    let import_id = posted_run(state, token, source, body).await;
    complete_run_at_fixed_instant(state, import_id).await;
    import_id
}

/// The guid of the message `guid` is hidden behind, or `None` when it is
/// shown.
pub(super) async fn hidden_behind(state: &crate::server::AppState, guid: &str) -> Option<String> {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_scalar(
        r"
        SELECT w.guid
        FROM messages m
        LEFT JOIN messages w ON w.id = m.duplicate_of
        WHERE m.guid = $1
        ",
    )
    .bind(guid)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Each conversation of `TEST_ACCOUNT` with its number of participant rows,
/// and the account's number of contacts.
async fn participant_and_contact_counts(conn: &mut SqliteConnection) -> (Vec<(i64, i64)>, i64) {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT c.id, COUNT(p.id) FROM conversations c
         LEFT JOIN participants p ON p.conversation_id = c.id
         WHERE c.account_id = $1
         GROUP BY c.id ORDER BY c.id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    (rows, contacts)
}

fn s1_header() -> ConversationHeaderLine {
    conversation_header("imessage", "+15555550119").participant("+15555550119", Some("Bob"))
}

fn s1_message(guid: &str, sender: &str, ms: i64, text: &str) -> MessageLine {
    message_line(guid, text).at(ms).sender(sender)
}

mod attachments;
mod backup_dates;
mod batches;
mod changed_content_dedupe;
mod completion;
mod conversations;
mod edits;
mod identities;
mod import_runs;
mod participants;
mod phone_country;
mod promote;
mod reactions;
mod run_end_dedupe;
mod search_index;
mod staging;
mod time_precision;
mod with_yourself;
