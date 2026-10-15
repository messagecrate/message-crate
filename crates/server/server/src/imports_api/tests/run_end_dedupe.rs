//! The full dedupe runs once per Import Run, when the run ends, and not
//! after each batch (#2040): a batch leaves its duplicates shown, and every
//! way the run ends hides them, the complete route, the discard route and
//! the server's `imports discard` command alike.

use super::*;

/// 2015-03-12T18:04:22Z, the second every message here falls in.
const SECOND: i64 = 1_426_183_462_000;

/// The chat every message here is in, and its one other participant, who
/// sends every message.
const CHAT: &str = "+15555550123";

/// One `source` conversation with [`CHAT`] holding one "see you at six"
/// message, `guid`.
fn batch(source: &str, guid: &str) -> String {
    let header = conversation_header(source, CHAT).participant(CHAT, None);
    let message = message_line(guid, "see you at six").sender(CHAT).at(SECOND);
    format!("{header}\n{message}\n")
}

/// The `sms` copy of the message, imported and completed first, and an
/// `imessage` run, still running, whose one batch brought a second copy,
/// `m-six`. Returns the running run's id.
async fn a_run_holding_a_duplicate(state: &crate::server::AppState, token: &str) -> i64 {
    import_one_batch(state, token, "sms", "append", batch("sms", "n-six")).await;
    let (run, _) = crate::test_support::start_import_run(
        state,
        token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    crate::test_support::post_import_batch(state, token, run, batch("imessage", "m-six")).await;
    run
}

/// A batch hides no duplicate: the copy it brought stays shown while the
/// run is open, and the run's completion hides it.
#[tokio::test]
async fn a_batchs_duplicate_is_shown_until_its_run_completes() {
    let (state, _fixture, token) = importer().await;
    let run = a_run_holding_a_duplicate(&state, &token).await;
    assert_eq!(
        hidden_behind(&state, "m-six").await,
        None,
        "the batch left its duplicate shown"
    );

    crate::test_support::complete_import_run(&state, &token, run).await;

    assert_eq!(
        hidden_behind(&state, "m-six").await.as_deref(),
        Some("n-six")
    );
}

/// A run discarded through the route hides the duplicates its batches
/// brought, which stay in the account.
#[tokio::test]
async fn a_discarded_run_hides_the_duplicates_its_batches_brought() {
    let (state, _fixture, token) = importer().await;
    let run = a_run_holding_a_duplicate(&state, &token).await;

    let discarded: serde_json::Value = post_json(
        &state,
        &format!("/v1/imports/{run}/discard"),
        &token,
        serde_json::json!({ "issues": [], "notes": [] }),
    )
    .await;

    assert_eq!(discarded["status"], "cancelled", "{discarded}");
    assert_eq!(
        hidden_behind(&state, "m-six").await.as_deref(),
        Some("n-six")
    );
}

/// The server's `imports discard` command, the way out of a run a killed
/// client left open, hides the duplicates of the run it discards.
#[tokio::test]
async fn the_imports_discard_command_hides_the_duplicates_of_the_run_it_discards() {
    let (state, _fixture, token) = importer().await;
    let run = a_run_holding_a_duplicate(&state, &token).await;
    let mut conn = state.db.acquire().await.unwrap();
    let account: i64 = sqlx::query_scalar("SELECT account_id FROM imports WHERE id = $1")
        .bind(run)
        .fetch_one(&mut *conn)
        .await
        .unwrap();

    let discarded = super::super::pipeline::discard_running_import_and_dedupe(
        &mut conn,
        account,
        crate::test_support::fixed_instant(),
        Progress::Log,
    )
    .await
    .unwrap();
    drop(conn);

    assert_eq!(discarded.map(|row| row.id), Some(run));
    assert_eq!(
        hidden_behind(&state, "m-six").await.as_deref(),
        Some("n-six")
    );
}
