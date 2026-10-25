use serde_json::{Value, json};

use crate::db::account_profile;
use crate::db::audit_trail::{self, AuditAction, AuditEntry, Scope};
use crate::db::contacts::{self, Origin};
use crate::db::handles;
use crate::problem::ProblemType;
use crate::test_support::{
    LoggedInAccount, TestFixture, expect_problem, fixture_with_account, get_json,
    post_created_json, post_json_raw,
};
use ir::IdentityType;

/// A contact of `account` named `name` (empty for none), holding `address`
/// on Text Message, made as an import makes one: the address is no
/// identity of the account's own.
async fn contact_holding(
    fixture: &TestFixture,
    account: &LoggedInAccount,
    name: &str,
    address: &str,
) -> i64 {
    let mut conn = fixture.conn().await;
    let contact = contacts::create_contact(&mut conn, account.account_id, name, Origin::Import)
        .await
        .unwrap();
    let (handle, _) = handles::upsert_handle_row(
        &mut conn,
        account.account_id,
        address,
        IdentityType::Phone,
        Some("text_message"),
    )
    .await
    .unwrap();
    contacts::link_handle_to_contact(
        &mut conn,
        account.account_id,
        handle,
        contact,
        Origin::Import,
    )
    .await
    .unwrap();
    contact
}

/// How many contacts `account` holds.
async fn contact_count(fixture: &TestFixture, account: &LoggedInAccount) -> i64 {
    let mut conn = fixture.conn().await;
    sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(account.account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// The Audit Trail entries about `account`, newest first.
async fn entries(fixture: &TestFixture, account: &LoggedInAccount) -> Vec<AuditEntry> {
    let mut conn = fixture.conn().await;
    audit_trail::page(
        &mut conn,
        Scope::Account(account.account_id),
        100,
        0,
        chrono::Utc::now(),
    )
    .await
    .unwrap()
    .0
}

/// `POST /v1/contacts` with a JSON body, expecting a refusal of `kind`, and
/// the refusal's `errors`.
async fn refused(
    fixture: &TestFixture,
    account: &LoggedInAccount,
    body: Value,
    kind: ProblemType,
) -> Vec<String> {
    let (status, text) = post_json_raw(&fixture.state, "/v1/contacts", &account.token, body).await;
    expect_problem(status, &text, kind)
        .errors
        .unwrap_or_default()
}

/// A JSON body is one contact typed by hand: `201 Created`, its path in
/// `Location`, the contact in the body with each identity typed by its
/// service, and a `contact_added` entry naming it by id (CONTACT-56,
/// CONTACT-57).
#[tokio::test]
async fn a_contact_typed_by_hand_is_created_with_its_identities_and_recorded() {
    let (fixture, account) = fixture_with_account().await;
    let (location, contact): (String, Value) = post_created_json(
        &fixture.state,
        "/v1/contacts",
        &account.token,
        json!({
            "name": "  Mom ",
            "identities": [
                { "address": "+15555550100" },
                { "address": "+15555550100", "service": "whatsapp" },
                { "address": "mom@example.com", "service": "text_message" },
            ],
        }),
    )
    .await;
    let id = contact["id"].as_i64().unwrap();
    assert_eq!(location, format!("/v1/contacts/{id}"));
    assert_eq!(contact["name"], "Mom");
    assert_eq!(contact["unknown"], false);
    let mut identities: Vec<(&str, &str)> = contact["identities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["address"].as_str().unwrap(),
                i["service"].as_str().unwrap(),
            )
        })
        .collect();
    identities.sort_unstable();
    assert_eq!(
        identities,
        vec![
            ("+15555550100", "text_message"),
            ("+15555550100", "whatsapp"),
            ("mom@example.com", "email"),
        ]
    );

    let read: Value = get_json(&fixture.state, &location, &account.token).await;
    assert_eq!(read["id"], id);

    let all = entries(&fixture, &account).await;
    let added: Vec<&AuditEntry> = all
        .iter()
        .filter(|e| e.action == AuditAction::ContactAdded)
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].contact_id, Some(id));
    assert_eq!(added[0].contact_ids, None);
}

/// A contact may be created with a name and no identity. Nothing reaches
/// it, so it is Unknown, and the Contacts list shows it by its name.
#[tokio::test]
async fn a_contact_with_a_name_and_no_identity_is_created_and_listed() {
    let (fixture, account) = fixture_with_account().await;
    let (_, contact): (String, Value) = post_created_json(
        &fixture.state,
        "/v1/contacts",
        &account.token,
        json!({ "name": "Aunt May" }),
    )
    .await;
    assert_eq!(contact["identities"], json!([]));
    assert_eq!(contact["unknown"], true);

    let list: Value = get_json(&fixture.state, "/v1/contacts", &account.token).await;
    let row = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == contact["id"])
        .expect("the new contact is listed");
    assert_eq!(row["name"], "Aunt May");
    assert_eq!(row["addresses"], json!([]));
}

/// An identity another contact holds is refused with
/// `422 Unprocessable Entity`, naming that contact, and nothing is created:
/// no merge is offered (CONTACT-56). A nameless holder and one in the Trash
/// are refused alike, each named as what it is.
#[tokio::test]
async fn an_identity_another_contact_holds_is_refused_naming_the_holder() {
    let (fixture, account) = fixture_with_account().await;
    contact_holding(&fixture, &account, "Sam", "+15555550101").await;
    contact_holding(&fixture, &account, "", "+15555550102").await;
    let trashed = contact_holding(&fixture, &account, "Old Friend", "+15555550103").await;
    {
        let mut conn = fixture.conn().await;
        sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
            .bind(account.account_id)
            .bind(trashed)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    let before = contact_count(&fixture, &account).await;

    let errors = refused(
        &fixture,
        &account,
        json!({
            "name": "Mom",
            "identities": [
                { "address": "+15555550199" },
                { "address": "+15555550101" },
                { "address": "+15555550102" },
                { "address": "+15555550103" },
            ],
        }),
        ProblemType::ValidationFailed,
    )
    .await;
    assert_eq!(
        errors,
        vec![
            "identities[1].address: +15555550101 is already on Sam".to_string(),
            "identities[2].address: +15555550102 is already on a contact with no name".to_string(),
            "identities[3].address: +15555550103 is already on Old Friend in the Trash".to_string(),
        ]
    );
    assert_eq!(contact_count(&fixture, &account).await, before);
    // The free identity the refused request named is not left behind either.
    let mut conn = fixture.conn().await;
    let stray: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM handles WHERE account_id = $1 AND normalized = '+15555550199'",
    )
    .bind(account.account_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(stray, 0);
}

/// One of the account's own identities is never on a contact (CONTACT-46),
/// so a New Contact that names one is refused.
#[tokio::test]
async fn one_of_the_accounts_own_identities_is_refused() {
    let (fixture, account) = fixture_with_account().await;
    {
        let mut conn = fixture.conn().await;
        account_profile::link_account_handle(
            &mut conn,
            account.account_id,
            "+15555550111",
            IdentityType::Phone,
        )
        .await
        .unwrap();
    }
    let errors = refused(
        &fixture,
        &account,
        json!({ "name": "Me", "identities": [{ "address": "+15555550111" }] }),
        ProblemType::ValidationFailed,
    )
    .await;
    assert_eq!(
        errors,
        vec!["identities[0].address: +15555550111 is one of the account's own identities"]
    );
}

/// A blank name, a blank address and `mode` are each refused, every one in
/// one answer, before anything is looked up.
#[tokio::test]
async fn a_blank_name_or_address_and_a_mode_are_refused() {
    let (fixture, account) = fixture_with_account().await;
    let errors = refused(
        &fixture,
        &account,
        json!({ "name": "  ", "identities": [{ "address": " " }] }),
        ProblemType::ValidationFailed,
    )
    .await;
    assert_eq!(
        errors,
        vec![
            "name must not be empty".to_string(),
            "identities[0].address: must not be empty".to_string(),
        ]
    );

    let (status, text) = post_json_raw(
        &fixture.state,
        "/v1/contacts?mode=edit",
        &account.token,
        json!({ "name": "Mom" }),
    )
    .await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
    assert_eq!(contact_count(&fixture, &account).await, 0);
}

/// The Demo Account is refused by its id, because anyone can enter it.
#[tokio::test]
async fn the_demo_account_is_refused_a_new_contact() {
    let (fixture, _) = fixture_with_account().await;
    let (demo, token) = fixture.demo_account_session().await;
    let (status, text) = post_json_raw(
        &fixture.state,
        "/v1/contacts",
        &token,
        json!({ "name": "Mom" }),
    )
    .await;
    expect_problem(status, &text, ProblemType::DemoAccountProtected);
    let mut conn = fixture.conn().await;
    let added: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contacts WHERE account_id = $1 AND preferred_name = 'Mom'",
    )
    .bind(demo)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(added, 0);
}
