//! Merge Contacts and its undo through the router (CONTACT-57).

use axum::http::StatusCode;
use ir::IdentityType;
use sqlx::SqliteConnection;

use crate::db::contact_merges::MAX_MERGED_CONTACTS;
use crate::db::handles::upsert_handle_row;
use crate::problem::ProblemType;
use crate::test_support::{
    LoggedInAccount, TestFixture, conversation_header, expect_problem, fixture_with_account,
    get_status, import_jsonl_text, message_line, post_json, post_json_raw,
};

/// A contact named `name` holding `address` as an identity of `kind` on Text
/// Message. The identity is not one of the account's.
async fn contact(
    conn: &mut SqliteConnection,
    account: i64,
    name: &str,
    address: &str,
    kind: IdentityType,
) -> i64 {
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let (handle_id, _) = upsert_handle_row(conn, account, address, kind, None)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id, origin)
         VALUES ($1, $2, $3, 'import')",
    )
    .bind(account)
    .bind(handle_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    contact_id
}

/// Put `contact_id` in the Contact Group `name`, making the group when the
/// account has none by that name.
async fn join_group(conn: &mut SqliteConnection, account: i64, contact_id: i64, name: &str) {
    sqlx::query(
        "INSERT INTO contact_groups (account_id, name) VALUES ($1, $2)
         ON CONFLICT (account_id, name) DO NOTHING",
    )
    .bind(account)
    .bind(name)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_group_members (contact_id, group_id)
         SELECT $3, id FROM contact_groups WHERE account_id = $1 AND name = $2",
    )
    .bind(account)
    .bind(name)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();
}

/// The names of the Contact Groups `contact_id` is in, A to Z.
async fn groups_of(conn: &mut SqliteConnection, contact_id: i64) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT g.name FROM contact_group_members gm
         JOIN contact_groups g ON g.id = gm.group_id
         WHERE gm.contact_id = $1 ORDER BY g.name",
    )
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap()
}

/// The contact `address` is on.
async fn holder_of(conn: &mut SqliteConnection, account: i64, address: &str) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT ch.contact_id FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE h.account_id = $1 AND h.normalized = $2",
    )
    .bind(account)
    .bind(address)
    .fetch_optional(&mut *conn)
    .await
    .unwrap()
}

/// The contact's name, or `None` when the account has no such contact.
async fn name_of(conn: &mut SqliteConnection, contact_id: i64) -> Option<String> {
    sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_optional(&mut *conn)
        .await
        .unwrap()
}

/// Each Import Run's record of `contact_id`, as `(run, reason)`.
async fn import_records(conn: &mut SqliteConnection, contact_id: i64) -> Vec<(i64, String)> {
    sqlx::query_as(
        "SELECT import_id, reason FROM import_contacts WHERE contact_id = $1 ORDER BY import_id",
    )
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap()
}

/// Merge `from` into `target` and answer the merged contact.
async fn merge(
    fixture: &TestFixture,
    account: &LoggedInAccount,
    target: i64,
    from: &[i64],
) -> serde_json::Value {
    post_json(
        &fixture.state,
        &format!("/v1/contacts/{target}/merge"),
        &account.token,
        serde_json::json!({ "from": from }),
    )
    .await
}

/// Undo the merge into `target` and answer the target.
async fn unmerge(
    fixture: &TestFixture,
    account: &LoggedInAccount,
    target: i64,
) -> serde_json::Value {
    post_json(
        &fixture.state,
        &format!("/v1/contacts/{target}/unmerge"),
        &account.token,
        serde_json::json!({}),
    )
    .await
}

/// The ids the Contacts list shows, in its order.
async fn listed(fixture: &TestFixture, account: &LoggedInAccount) -> Vec<i64> {
    let page: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn a_merge_moves_identities_and_groups_and_the_merged_contact_leaves_every_list() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    let nicholas = contact(
        &mut conn,
        a,
        "Nicholas Koch",
        "+12155550112",
        IdentityType::Phone,
    )
    .await;
    let nick = contact(
        &mut conn,
        a,
        "Nick Koch",
        "nick.koch@example.com",
        IdentityType::Email,
    )
    .await;
    let unknown = contact(&mut conn, a, "", "+12155550113", IdentityType::Phone).await;
    join_group(&mut conn, a, nicholas, "Family").await;
    join_group(&mut conn, a, nick, "Family").await;
    join_group(&mut conn, a, nick, "Book Club").await;
    drop(conn);

    let merged = merge(&fixture, &account, nicholas, &[nick, unknown]).await;
    assert_eq!(merged["id"], nicholas);
    assert_eq!(merged["name"], "Nicholas Koch");
    let mut addresses: Vec<&str> = merged["identities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["address"].as_str().unwrap())
        .collect();
    addresses.sort_unstable();
    assert_eq!(
        addresses,
        ["+12155550112", "+12155550113", "nick.koch@example.com"]
    );
    assert_eq!(merged["groups"], serde_json::json!(["Book Club", "Family"]));

    assert_eq!(listed(&fixture, &account).await, [nicholas]);
    for gone in [nick, unknown] {
        assert_eq!(
            get_status(
                &fixture.state,
                &format!("/v1/contacts/{gone}"),
                &account.token
            )
            .await,
            StatusCode::NOT_FOUND,
            "a merged contact's page is not found"
        );
    }
    let mut conn = fixture.conn().await;
    // One row for the group both were in.
    let family_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_group_members gm
         JOIN contact_groups g ON g.id = gm.group_id WHERE g.name = 'Family'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(family_rows, 1);
}

#[tokio::test]
async fn undo_brings_back_each_merged_contact_as_it_was_and_the_target_as_it_was() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    let nicholas = contact(
        &mut conn,
        a,
        "Nicholas Koch",
        "+12155550112",
        IdentityType::Phone,
    )
    .await;
    let nick = contact(
        &mut conn,
        a,
        "Nick Koch",
        "nick.koch@example.com",
        IdentityType::Email,
    )
    .await;
    join_group(&mut conn, a, nicholas, "Family").await;
    join_group(&mut conn, a, nick, "Family").await;
    join_group(&mut conn, a, nick, "Book Club").await;
    drop(conn);

    merge(&fixture, &account, nicholas, &[nick]).await;
    let target = unmerge(&fixture, &account, nicholas).await["contact"].clone();
    assert_eq!(target["groups"], serde_json::json!(["Family"]));
    assert_eq!(target["identities"].as_array().unwrap().len(), 1);

    let mut conn = fixture.conn().await;
    assert_eq!(name_of(&mut conn, nick).await.as_deref(), Some("Nick Koch"));
    assert_eq!(
        holder_of(&mut conn, a, "nick.koch@example.com").await,
        Some(nick)
    );
    assert_eq!(groups_of(&mut conn, nick).await, ["Book Club", "Family"]);
    assert_eq!(groups_of(&mut conn, nicholas).await, ["Family"]);
    drop(conn);
    let mut ids = listed(&fixture, &account).await;
    ids.sort_unstable();
    assert_eq!(ids, [nicholas, nick]);
}

#[tokio::test]
async fn import_run_records_move_with_the_merge_and_come_back_with_the_undo() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    // The first run makes Ada at her number; the second makes her again at
    // her email address, and names the number's contact nothing new.
    for (address, guid) in [("+15555550100", "g1"), ("ada@example.com", "g2")] {
        let header = conversation_header("imessage", address).participant(address, Some("Ada"));
        let message = message_line(guid, "hi").sender(address);
        import_jsonl_text(&mut conn, a, "imessage", &format!("{header}\n{message}\n")).await;
    }
    let phone = holder_of(&mut conn, a, "+15555550100").await.unwrap();
    let email = holder_of(&mut conn, a, "ada@example.com").await.unwrap();
    assert_ne!(phone, email);
    let phone_records = import_records(&mut conn, phone).await;
    let email_records = import_records(&mut conn, email).await;
    drop(conn);

    merge(&fixture, &account, phone, &[email]).await;
    let mut conn = fixture.conn().await;
    let mut both: Vec<(i64, String)> = phone_records
        .iter()
        .chain(email_records.iter())
        .cloned()
        .collect();
    both.sort_unstable();
    assert_eq!(import_records(&mut conn, phone).await, both);
    drop(conn);

    unmerge(&fixture, &account, phone).await;
    let mut conn = fixture.conn().await;
    assert_eq!(import_records(&mut conn, phone).await, phone_records);
    assert_eq!(import_records(&mut conn, email).await, email_records);
}

#[tokio::test]
async fn undo_leaves_an_identity_the_person_moved_since_where_it_is() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    let ada = contact(&mut conn, a, "Ada", "+15555550100", IdentityType::Phone).await;
    let lovelace = contact(
        &mut conn,
        a,
        "Lovelace",
        "+15555550101",
        IdentityType::Phone,
    )
    .await;
    let grace = contact(&mut conn, a, "Grace", "+15555550102", IdentityType::Phone).await;
    drop(conn);

    merge(&fixture, &account, ada, &[lovelace]).await;
    // The person moves Lovelace's old number to Grace.
    let mut conn = fixture.conn().await;
    sqlx::query(
        "UPDATE contact_handles SET contact_id = $1
         WHERE handle_id = (SELECT id FROM handles WHERE normalized = '+15555550101')",
    )
    .bind(grace)
    .execute(&mut *conn)
    .await
    .unwrap();
    drop(conn);

    let answer = unmerge(&fixture, &account, ada).await;
    assert_eq!(answer["restored"], serde_json::json!([lovelace]));
    let mut conn = fixture.conn().await;
    assert_eq!(holder_of(&mut conn, a, "+15555550101").await, Some(grace));
    // Lovelace has a name, so she comes back though she has no identity.
    assert_eq!(
        name_of(&mut conn, lovelace).await.as_deref(),
        Some("Lovelace")
    );
}

#[tokio::test]
async fn a_new_merge_makes_the_one_before_permanent() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    let ada = contact(&mut conn, a, "Ada", "+15555550100", IdentityType::Phone).await;
    let lovelace = contact(
        &mut conn,
        a,
        "Lovelace",
        "+15555550101",
        IdentityType::Phone,
    )
    .await;
    let grace = contact(&mut conn, a, "Grace", "+15555550102", IdentityType::Phone).await;
    let hopper = contact(&mut conn, a, "Hopper", "+15555550103", IdentityType::Phone).await;
    drop(conn);

    merge(&fixture, &account, ada, &[lovelace]).await;
    merge(&fixture, &account, grace, &[hopper]).await;
    let (status, text) = post_json_raw(
        &fixture.state,
        &format!("/v1/contacts/{ada}/unmerge"),
        &account.token,
        serde_json::json!({}),
    )
    .await;
    expect_problem(status, &text, ProblemType::StateConflict);

    unmerge(&fixture, &account, grace).await;
    let mut conn = fixture.conn().await;
    assert_eq!(name_of(&mut conn, hopper).await.as_deref(), Some("Hopper"));
    assert_eq!(name_of(&mut conn, lovelace).await, None, "gone for good");
    drop(conn);
    // Nothing is left to undo.
    let (status, text) = post_json_raw(
        &fixture.state,
        &format!("/v1/contacts/{grace}/unmerge"),
        &account.token,
        serde_json::json!({}),
    )
    .await;
    expect_problem(status, &text, ProblemType::StateConflict);
}

/// A contact with no name whose identities all left the target since the
/// merge has nothing to come back with, so it stays gone, and the undo's
/// answer leaves it out.
#[tokio::test]
async fn undo_leaves_out_a_nameless_contact_with_nothing_left() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    let ada = contact(&mut conn, a, "Ada", "+15555550100", IdentityType::Phone).await;
    let nameless = contact(&mut conn, a, "", "+15555550101", IdentityType::Phone).await;
    let grace = contact(&mut conn, a, "Grace", "+15555550102", IdentityType::Phone).await;
    drop(conn);

    merge(&fixture, &account, ada, &[nameless]).await;
    let mut conn = fixture.conn().await;
    sqlx::query(
        "UPDATE contact_handles SET contact_id = $1
         WHERE handle_id = (SELECT id FROM handles WHERE normalized = '+15555550101')",
    )
    .bind(grace)
    .execute(&mut *conn)
    .await
    .unwrap();
    drop(conn);

    let answer = unmerge(&fixture, &account, ada).await;
    assert_eq!(answer["restored"], serde_json::json!([]));
    assert_eq!(answer["contact"]["id"], ada);
    let mut conn = fixture.conn().await;
    assert_eq!(name_of(&mut conn, nameless).await, None);
}

/// A merge deletes the previous merge's contacts for good, so an account
/// the administrator kept from deleting may not merge.
#[tokio::test]
async fn an_account_without_the_delete_permission_merges_nothing() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    let ada = contact(&mut conn, a, "Ada", "+15555550100", IdentityType::Phone).await;
    let grace = contact(&mut conn, a, "Grace", "+15555550101", IdentityType::Phone).await;
    drop(conn);
    fixture.turn_off_delete(a).await;
    let (status, text) = post_json_raw(
        &fixture.state,
        &format!("/v1/contacts/{ada}/merge"),
        &account.token,
        serde_json::json!({ "from": [grace] }),
    )
    .await;
    expect_problem(status, &text, ProblemType::InsufficientScope);
}

/// `contacts.id` is never handed out twice, so a contact made while a merge
/// stands cannot take the id the undo gives back.
#[tokio::test]
async fn a_contact_made_after_a_merge_never_takes_a_merged_contacts_id() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    let ada = contact(&mut conn, a, "Ada", "+15555550100", IdentityType::Phone).await;
    let newest = contact(
        &mut conn,
        a,
        "Lovelace",
        "+15555550101",
        IdentityType::Phone,
    )
    .await;
    drop(conn);

    merge(&fixture, &account, ada, &[newest]).await;
    let mut conn = fixture.conn().await;
    let made = contact(&mut conn, a, "Grace", "+15555550102", IdentityType::Phone).await;
    assert!(made > newest, "{made} reuses an id at or below {newest}");
    drop(conn);
    unmerge(&fixture, &account, ada).await;
    let mut conn = fixture.conn().await;
    assert_eq!(
        name_of(&mut conn, newest).await.as_deref(),
        Some("Lovelace")
    );
}

#[tokio::test]
async fn a_merge_is_refused_whole_with_the_status_its_rule_gives() {
    let (fixture, account) = fixture_with_account().await;
    let a = account.account_id;
    let mut conn = fixture.conn().await;
    let ada = contact(&mut conn, a, "Ada", "+15555550100", IdentityType::Phone).await;
    let grace = contact(&mut conn, a, "Grace", "+15555550101", IdentityType::Phone).await;
    let trashed = contact(&mut conn, a, "Trashed", "+15555550102", IdentityType::Phone).await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(a)
        .bind(trashed)
        .execute(&mut *conn)
        .await
        .unwrap();
    let mine = contact(&mut conn, a, "Me", "+15555550103", IdentityType::Phone).await;
    crate::db::account_profile::link_account_handle(
        &mut conn,
        a,
        "+15555550103",
        IdentityType::Phone,
    )
    .await
    .unwrap();
    drop(conn);

    let too_many: Vec<i64> = (1..MAX_MERGED_CONTACTS as i64)
        .map(|i| grace + 100 + i)
        .chain([grace])
        .collect();
    let cases: [(i64, serde_json::Value, ProblemType); 8] = [
        (
            ada,
            serde_json::json!({ "from": [] }),
            ProblemType::ValidationFailed,
        ),
        (
            ada,
            serde_json::json!({ "from": [ada, grace] }),
            ProblemType::ValidationFailed,
        ),
        (
            ada,
            serde_json::json!({ "from": [grace, grace] }),
            ProblemType::ValidationFailed,
        ),
        (
            ada,
            serde_json::json!({ "from": too_many }),
            ProblemType::ValidationFailed,
        ),
        (
            ada,
            serde_json::json!({ "from": [grace, 999_999] }),
            ProblemType::ValidationFailed,
        ),
        (
            999_999,
            serde_json::json!({ "from": [grace] }),
            ProblemType::NotFound,
        ),
        (
            ada,
            serde_json::json!({ "from": [grace, trashed] }),
            ProblemType::StateConflict,
        ),
        (
            ada,
            serde_json::json!({ "from": [mine] }),
            ProblemType::StateConflict,
        ),
    ];
    for (target, body, kind) in cases {
        let (status, text) = post_json_raw(
            &fixture.state,
            &format!("/v1/contacts/{target}/merge"),
            &account.token,
            body.clone(),
        )
        .await;
        expect_problem(status, &text, kind);
        let mut conn = fixture.conn().await;
        assert_eq!(
            holder_of(&mut conn, a, "+15555550101").await,
            Some(grace),
            "{body} changed nothing"
        );
    }
}

#[tokio::test]
async fn the_demo_account_merges_nothing() {
    let (fixture, _account) = fixture_with_account().await;
    let (demo, token) = fixture.demo_account_session().await;
    let mut conn = fixture.conn().await;
    let ada = contact(&mut conn, demo, "Ada", "+15555550100", IdentityType::Phone).await;
    let grace = contact(
        &mut conn,
        demo,
        "Grace",
        "+15555550101",
        IdentityType::Phone,
    )
    .await;
    drop(conn);
    let (status, text) = post_json_raw(
        &fixture.state,
        &format!("/v1/contacts/{ada}/merge"),
        &token,
        serde_json::json!({ "from": [grace] }),
    )
    .await;
    expect_problem(status, &text, ProblemType::DemoAccountProtected);
}

#[tokio::test]
async fn unmerge_of_an_unknown_contact_is_not_found() {
    let (fixture, account) = fixture_with_account().await;
    let (status, text) = post_json_raw(
        &fixture.state,
        "/v1/contacts/999999/unmerge",
        &account.token,
        serde_json::json!({}),
    )
    .await;
    expect_problem(status, &text, ProblemType::NotFound);
}

/// The reference's `max_items` is a literal, so it is held to the cap here.
#[test]
fn the_documented_cap_is_the_servers() {
    let schema: serde_json::Value =
        serde_json::from_str(&crate::openapi::dump_openapi_json()).unwrap();
    assert_eq!(
        schema["components"]["schemas"]["MergeContactRequest"]["properties"]["from"]["maxItems"],
        MAX_MERGED_CONTACTS - 1
    );
}
