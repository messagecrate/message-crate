use super::*;
use crate::test_support::{test_fixture, with_trigger_lifted};

/// A refused login as a username nobody holds keeps the text typed only when
/// it could be a username. Anything else, such as a password typed into the
/// username field, is recorded with no username, so it never enters the trail.
/// The text kept is the text checked: surrounding whitespace is not kept.
#[tokio::test]
async fn a_refused_login_keeps_the_typed_text_only_when_it_could_be_a_username() {
    let fixture = test_fixture().await;
    let mut conn = fixture.conn().await;
    for typed in [
        "hunter2!Secret",
        "two words",
        &"x".repeat(129),
        "nobody.here_2",
        " padded.name ",
    ] {
        record_refused_login(&mut conn, typed, None, AuditReason::UnknownUsername, None)
            .await
            .unwrap();
    }
    let (items, total) = page(&mut conn, Scope::All, 10, 0).await.unwrap();
    assert_eq!(total, 5);
    let kept: Vec<Option<&str>> = items.iter().rev().map(|i| i.username.as_deref()).collect();
    assert_eq!(
        kept,
        [None, None, None, Some("nobody.here_2"), Some("padded.name")]
    );
    assert!(
        items
            .iter()
            .all(|i| i.reason == Some(AuditReason::UnknownUsername))
    );
}

/// Deleting an account keeps its Import Runs and their counts, and drops what
/// describes the person's messages: the run's issues and notes, its form, its staging
/// directory and the addresses the backup sent from. A run still open is closed.
#[tokio::test]
async fn deleting_an_account_keeps_an_import_runs_counts_and_drops_its_details() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let mut conn = fixture.conn().await;
    let import_id: i64 = sqlx::query_scalar(
        "INSERT INTO imports (account_id, source, mode, status, started_at, message_count,
                              run_dir, form_json, source_identities)
         VALUES ($1, 'imessage', 'append', 'running', '2026-10-01T00:00:00+00:00', 12,
                 '/home/alice/staging', '{}', '[\"+15555550123\"]')
         RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO import_issues (import_id, kind, stage, item, reason, created_at)
         VALUES ($1, 'skip', 'staging', 'chat-with-bob.txt', 'unreadable', 'now')",
    )
    .bind(import_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO import_notes (import_id, stage, item, text, created_at)
         VALUES ($1, 'staging', 'bob@example.com', 'kept by this email address', 'now')",
    )
    .bind(import_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    account_profile::delete_account(&mut conn, account, AuditActor::Owner)
        .await
        .unwrap();

    let row: (
        Option<i64>,
        Option<String>,
        String,
        i64,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT account_id, username, status, message_count, run_dir, source_identities
             FROM imports WHERE id = $1",
    )
    .bind(import_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            None,
            Some("alice".into()),
            "cancelled".into(),
            12,
            None,
            None
        )
    );
    let issues: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM import_issues")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(issues, 0);
    let notes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM import_notes")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(notes, 0);
}

/// A password change renews the Session, so a live Session never reads as
/// expired, and the renewal is a new entry: the `logged_in` entry keeps the
/// expiry it was written with.
#[tokio::test]
async fn a_renewed_session_does_not_read_as_expired() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let mut conn = fixture.conn().await;
    crate::db::session_tokens::open_session(&mut conn, account, "alice", None)
        .await
        .unwrap();
    let stale = "2000-01-01T00:00:00+00:00";
    with_trigger_lifted(&mut conn, "audit_entries_never_edited", async |conn| {
        sqlx::query("UPDATE audit_entries SET session_expires_at = $1")
            .bind(stale)
            .execute(conn)
            .await
            .unwrap();
    })
    .await;
    crate::credentials::change_password_on_conn(&mut conn, account, None)
        .await
        .unwrap();
    let (items, _) = page(&mut conn, Scope::Account(account), 10, 0)
        .await
        .unwrap();
    let actions: Vec<AuditAction> = items.iter().map(|item| item.action).collect();
    assert_eq!(actions, [AuditAction::PasswordSet, AuditAction::LoggedIn]);
    let login_expiry: String = sqlx::query_scalar(
        "SELECT session_expires_at FROM audit_entries WHERE action = 'logged_in'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(login_expiry, stale, "the logged_in entry is never edited");
}

/// Let the account's Session run out: its row stays, as it does until its
/// token is presented again, and its login reads as expired.
async fn expire_session(conn: &mut SqliteConnection) {
    sqlx::query("UPDATE account_session_tokens SET expires_at = '1'")
        .execute(&mut *conn)
        .await
        .unwrap();
    with_trigger_lifted(conn, "audit_entries_never_edited", async |conn| {
        sqlx::query(
            "UPDATE audit_entries SET session_expires_at = '2000-01-01T00:00:00+00:00'
             WHERE action = 'logged_in'",
        )
        .execute(conn)
        .await
        .unwrap();
    })
    .await;
}

/// How each Session ended, oldest login first: `None` for one still live.
async fn session_ends(conn: &mut SqliteConnection, account: i64) -> Vec<(AuditReason, String)> {
    let (items, _) = page(conn, Scope::Account(account), 50, 0).await.unwrap();
    let mut ends: Vec<(AuditReason, String)> = items
        .into_iter()
        .filter(|item| item.action == AuditAction::SessionEnded)
        .map(|item| (item.reason.unwrap(), item.at))
        .collect();
    ends.reverse();
    ends
}

/// A Session that ran out before the next login stays ended at its expiry:
/// the login does not record it `replaced` at the login's time.
#[tokio::test]
async fn a_login_after_a_session_expired_does_not_replace_it() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let mut conn = fixture.conn().await;
    crate::db::session_tokens::open_session(&mut conn, account, "alice", None)
        .await
        .unwrap();
    expire_session(&mut conn).await;
    crate::db::session_tokens::open_session(&mut conn, account, "alice", None)
        .await
        .unwrap();
    assert_eq!(
        session_ends(&mut conn, account).await,
        [(
            AuditReason::Expired,
            "2000-01-01T00:00:00+00:00".to_string()
        )]
    );
}

/// Revoking an account's Sessions leaves one that already ran out ended at
/// its expiry: `reset-owner-password` long after the last login does not
/// record it `revoked`.
#[tokio::test]
async fn revoking_leaves_an_expired_session_ended_at_its_expiry() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let mut conn = fixture.conn().await;
    crate::db::session_tokens::open_session(&mut conn, account, "alice", None)
        .await
        .unwrap();
    expire_session(&mut conn).await;
    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    crate::db::session_tokens::revoke_account_sessions(&mut tx, account, AuditActor::CommandLine)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        session_ends(&mut conn, account).await,
        [(
            AuditReason::Expired,
            "2000-01-01T00:00:00+00:00".to_string()
        )]
    );
}

/// Deleting an account ends its live Session as `revoked` by whoever
/// deleted it, so the login does not read as live, then as expired, after
/// the account is gone.
#[tokio::test]
async fn deleting_an_account_ends_its_live_session() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let mut conn = fixture.conn().await;
    crate::db::session_tokens::open_session(&mut conn, account, "alice", None)
        .await
        .unwrap();
    account_profile::delete_account(&mut conn, account, AuditActor::Owner)
        .await
        .unwrap();
    let (items, _) = page(&mut conn, Scope::All, 50, 0).await.unwrap();
    let ended: Vec<(Option<AuditReason>, AuditActor)> = items
        .iter()
        .filter(|item| item.action == AuditAction::SessionEnded)
        .map(|item| (item.reason, item.actor))
        .collect();
    assert_eq!(ended, [(Some(AuditReason::Revoked), AuditActor::Owner)]);
}

/// What started a run, as one run table's row records it.
type RecordedCredential = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

async fn recorded_credential(
    conn: &mut SqliteConnection,
    table: RunTable,
    run_id: i64,
) -> RecordedCredential {
    sqlx::query_as(&format!(
        "SELECT credential, app_kind, app_build, api_token_label, api_token_hint
         FROM {} WHERE id = $1",
        table.name()
    ))
    .bind(run_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Each run table records what started its own run: an Import Run and an
/// Export Run with the same id each keep the credential recorded for it.
#[tokio::test]
async fn each_run_table_records_its_own_runs_credential() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let mut conn = fixture.conn().await;
    let import_id: i64 = sqlx::query_scalar(
        "INSERT INTO imports (account_id, source, mode, status, started_at)
         VALUES ($1, 'imessage', 'append', 'running', '2026-10-01T00:00:00+00:00')
         RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let export_id = crate::db::exports::start_export(
        &mut conn,
        &crate::db::exports::StartExportArgs {
            account_id: account,
            scope: &message_crate_api_types::ExportScope::Everything,
            tool: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(import_id, export_id, "the test needs one id in both tables");

    let session = CredentialUsed::Session(Some(ConnectingApp {
        kind: AppKind::Desktop,
        build: "0.10.0+aaaa1111".to_string(),
    }));
    let token = CredentialUsed::ApiToken {
        label: "nightly backup".to_string(),
        hint: "mc-api-Sd..mE".to_string(),
    };
    record_run_credential(&mut conn, RunTable::Imports, import_id, &session)
        .await
        .unwrap();
    record_run_credential(&mut conn, RunTable::Exports, export_id, &token)
        .await
        .unwrap();

    assert_eq!(
        recorded_credential(&mut conn, RunTable::Imports, import_id).await,
        (
            "session".to_string(),
            Some("desktop".to_string()),
            Some("0.10.0+aaaa1111".to_string()),
            None,
            None
        )
    );
    assert_eq!(
        recorded_credential(&mut conn, RunTable::Exports, export_id).await,
        (
            "api_token".to_string(),
            None,
            None,
            Some("nightly backup".to_string()),
            Some("mc-api-Sd..mE".to_string())
        )
    );
}

/// The schema refuses a statement that edits or deletes an Audit Trail
/// entry, whoever runs it, and still lets through the two writes the server
/// makes: deleting an account, which unlinks its entries, and the 90-day trim
/// of refused logins as a username nobody held (ADR 0020, #2277).
#[tokio::test]
async fn the_schema_refuses_an_edit_or_delete_of_an_audit_trail_entry() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let other = fixture.account("bob").await;
    let mut conn = fixture.conn().await;
    crate::db::session_tokens::open_session(&mut conn, account, "alice", None)
        .await
        .unwrap();
    record_refused_login(
        &mut conn,
        "alice",
        Some((account, "alice")),
        AuditReason::WrongPassword,
        None,
    )
    .await
    .unwrap();

    let move_to_other = format!("UPDATE audit_entries SET account_id = {other}");
    assert_refused(
        &mut conn,
        &[
            "UPDATE audit_entries SET action = 'account_created' WHERE action = 'logged_in'",
            "UPDATE audit_entries SET at = '2000-01-01T00:00:00+00:00'",
            "UPDATE audit_entries SET details = '{}'",
            &move_to_other,
            "DELETE FROM audit_entries WHERE action = 'logged_in'",
            "DELETE FROM audit_entries WHERE action = 'login_refused'",
        ],
    )
    .await;
    assert_eq!(count_entries(&mut conn).await, 2);

    assert!(
        account_profile::delete_account(&mut conn, account, AuditActor::Owner)
            .await
            .unwrap()
    );
    let unlinked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_entries
         WHERE account_id IS NULL AND deletion_entry_id IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let total = count_entries(&mut conn).await;
    assert_eq!(unlinked, total, "every entry about alice is kept, unlinked");
    // Once set, an entry's deletion_entry_id and NULL account_id stay.
    assert_refused(
        &mut conn,
        &[
            "UPDATE audit_entries SET deletion_entry_id = NULL",
            "UPDATE audit_entries SET deletion_entry_id = deletion_entry_id + 1",
            &move_to_other,
        ],
    )
    .await;

    sqlx::query(
        "INSERT INTO audit_entries (at, action, actor, username, reason)
         VALUES ('2000-01-01T00:00:00+00:00', 'login_refused', 'anonymous',
                 'nobody', 'unknown_username')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    assert_eq!(trim_refused_logins(&mut conn).await.unwrap(), 1);
    assert_eq!(count_entries(&mut conn).await, total);
}

/// Run each of `statements`, and check the schema refuses every one.
async fn assert_refused(conn: &mut SqliteConnection, statements: &[&str]) {
    for statement in statements {
        let error = sqlx::query(statement)
            .execute(&mut *conn)
            .await
            .expect_err(statement)
            .to_string();
        assert!(error.contains("an Audit Trail entry is never"), "{error}");
    }
}

/// How many Audit Trail entries there are.
async fn count_entries(conn: &mut SqliteConnection) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit_entries")
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}
