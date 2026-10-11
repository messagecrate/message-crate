//! A phone number carries its country only when that is certain (#1676): a
//! national number and its `+` form are one person in a run that states the
//! phone's country, two identities in a run that does not, and one again
//! once a person picks the country on the Contacts screen.

use super::*;
use crate::problem::ProblemType;
use crate::test_support::{expect_problem, patch_json_raw};
use message_crate_api_types::IdentityHolder;

/// One person's UK mobile, written in national form and with its `+` code.
const NATIONAL: &str = "07700 900123";
const FULL: &str = "+447700900123";

/// Create an Import Run stating `phone_country`, or none, and post `body`
/// as its one batch.
async fn import_with_country(
    state: &crate::server::AppState,
    token: &str,
    phone_country: Option<&str>,
    body: String,
) {
    import_run(state, token, "sms-backup-plus", phone_country, body).await;
}

/// [`import_with_country`] from `source`, completed once its one batch is in.
async fn import_run(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    phone_country: Option<&str>,
    body: String,
) {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({
            "source": source,
            "phone_country": phone_country,
        }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let (status, text) = crate::test_support::post_raw(
        state,
        &format!("/v1/imports/{id}/batches"),
        token,
        "application/jsonl",
        body,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let _: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
}

/// A one-to-one conversation with each spelling of `national` and `full`,
/// and a group whose members are the person under both spellings and
/// someone else.
fn both_spellings(national: &str, full: &str) -> String {
    let one_to_one = |chat: &str, guid: &str, ms: i64| {
        format!(
            "{}\n{}\n",
            conversation_header("sms-backup-plus", chat).participant(chat, None),
            message_line(guid, guid).at(ms).sender(chat),
        )
    };
    let group = conversation_header("sms-backup-plus", "group-1")
        .group()
        .participant(national, None)
        .participant(full, None)
        .participant("+447700900456", None);
    format!(
        "{}{}{group}\n{}\n",
        one_to_one(national, "m-national", 1_700_000_000_000),
        one_to_one(full, "m-full", 1_700_000_100_000),
        message_line("m-group", "m-group")
            .at(1_700_000_200_000)
            .sender(full),
    )
}

/// How many one-to-one conversations the account holds, and how many
/// members its group lists.
async fn conversations_and_members(state: &crate::server::AppState) -> (i64, i64) {
    let mut conn = state.db.acquire().await.unwrap();
    let individual: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversations WHERE conversation_type = 'individual'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let members: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM participants p
         JOIN conversations c ON c.id = p.conversation_id
         WHERE c.conversation_type = 'group'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    (individual, members)
}

/// The account's phone identities' keys, in order.
async fn phone_identities(state: &crate::server::AppState) -> Vec<String> {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_scalar(
        "SELECT normalized FROM handles
         WHERE handle_type = 'phone' ORDER BY normalized",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap()
}

/// With the country the run states, the national number is its `+` form:
/// one one-to-one conversation holding both messages, and one seat in the
/// group.
#[tokio::test]
async fn both_spellings_are_one_person_in_a_run_that_states_the_country() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "uk", "hunter2hunter2").await;
    import_with_country(
        &fixture.state,
        &account.token,
        Some("GB"),
        both_spellings(NATIONAL, FULL),
    )
    .await;

    assert_eq!(conversations_and_members(&fixture.state).await, (1, 2));
    assert_eq!(
        phone_identities(&fixture.state).await,
        [FULL, "+447700900456"]
    );
}

/// With no country stated, a number without `+` is not read as a US number:
/// `555 555 0119` and `+15555550119` stay two identities, and the bare one
/// has no `+` form.
#[tokio::test]
async fn both_spellings_stay_two_identities_when_no_country_is_stated() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "us", "hunter2hunter2").await;
    import_with_country(
        &fixture.state,
        &account.token,
        None,
        both_spellings("555 555 0119", "+15555550119"),
    )
    .await;

    assert_eq!(conversations_and_members(&fixture.state).await, (2, 3));
    assert_eq!(
        phone_identities(&fixture.state).await,
        ["+15555550119", "+447700900456", "5555550119"]
    );
}

/// A country the list does not hold is refused before the run is made.
#[tokio::test]
async fn an_unknown_phone_country_is_refused() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "zz", "hunter2hunter2").await;
    let (status, text) = crate::test_support::post_json_raw(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage", "phone_country": "ZZ" }),
    )
    .await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
}

/// The contact the identity `normalized` is on.
async fn contact_of(state: &crate::server::AppState, normalized: &str) -> i64 {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_scalar(
        "SELECT ch.contact_id FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id WHERE h.normalized = $1",
    )
    .bind(normalized)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Picking the country on the Contacts screen asks first, then merges: the
/// two one-to-one conversations become one, the group lists the person
/// once, and the national spelling is gone.
#[tokio::test]
async fn picking_the_country_on_the_contacts_screen_merges_the_two() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "pick", "hunter2hunter2").await;
    let state = &fixture.state;
    import_with_country(state, &account.token, None, both_spellings(NATIONAL, FULL)).await;
    assert_eq!(conversations_and_members(state).await, (2, 3));

    let contact = contact_of(state, "07700900123").await;
    let path = format!("/v1/contacts/{contact}");
    let pick = |merge: bool| {
        serde_json::json!({ "set_identity_country": {
            "address": "07700900123", "country": "GB", "merge": merge,
        }})
    };
    let (status, text) = patch_json_raw(state, &path, &account.token, pick(false)).await;
    let problem = expect_problem(status, &text, ProblemType::IdentityExists);
    assert!(
        problem.sentence().contains(FULL),
        "the question names the + form: {text}"
    );
    let holder = contact_of(state, FULL).await;
    assert_eq!(
        problem.holder,
        Some(IdentityHolder::Contact {
            contact_id: holder,
            name: String::new(),
        }),
        "the problem says who holds the + form: {text}"
    );
    assert_eq!(
        conversations_and_members(state).await,
        (2, 3),
        "nothing changes before the person says to merge"
    );

    let answer: serde_json::Value = patch_json(state, &path, &account.token, pick(true)).await;
    assert_eq!(conversations_and_members(state).await, (1, 2));
    assert_eq!(phone_identities(state).await, [FULL, "+447700900456"]);
    let identities: Vec<&str> = answer["identities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["address"].as_str().unwrap())
        .collect();
    assert_eq!(identities, [FULL], "the contact holds the + form");
    let mut conn = state.db.acquire().await.unwrap();
    let messages: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages m JOIN conversations c ON c.id = m.conversation_id
         WHERE c.conversation_type = 'individual'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(messages, 2, "the one conversation holds both messages");
    crate::test_support::assert_every_person_is_on_a_contact(&mut conn, "the merge").await;
}

/// A country picked for a number nobody else holds in its `+` form rewrites
/// the identity in place, and the identity no longer reads as one whose
/// country is unknown.
#[tokio::test]
async fn picking_the_country_of_a_number_no_one_else_holds_gives_it_its_plus_form() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "alone", "hunter2hunter2").await;
    let state = &fixture.state;
    let body = format!(
        "{}\n{}\n",
        conversation_header("sms-backup-plus", NATIONAL).participant(NATIONAL, None),
        message_line("m1", "m1").sender(NATIONAL),
    );
    import_with_country(state, &account.token, None, body).await;
    let contact = contact_of(state, "07700900123").await;
    let path = format!("/v1/contacts/{contact}");
    let before: serde_json::Value = get_json(state, &path, &account.token).await;
    assert_eq!(before["identities"][0]["country_unknown"], true);

    let after: serde_json::Value = patch_json(
        state,
        &path,
        &account.token,
        serde_json::json!({ "set_identity_country": { "address": "07700900123", "country": "GB" }}),
    )
    .await;
    assert_eq!(after["identities"][0]["address"], FULL);
    assert_eq!(after["identities"][0]["country_unknown"], false);
}

/// A short code has no `+` form in any country, so a country picked for it
/// is refused, naming the reason.
#[tokio::test]
async fn a_short_code_has_no_country_to_pick() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "short", "hunter2hunter2").await;
    let state = &fixture.state;
    let body = format!(
        "{}\n{}\n",
        conversation_header("sms-backup-plus", "73737").participant("73737", None),
        message_line("m1", "m1").sender("73737"),
    );
    import_with_country(state, &account.token, None, body).await;
    let contact = contact_of(state, "73737").await;
    let (status, text) = patch_json_raw(
        state,
        &format!("/v1/contacts/{contact}"),
        &account.token,
        serde_json::json!({ "set_identity_country": { "address": "73737", "country": "GB" }}),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
    assert!(problem.sentence().contains("too few digits"), "{text}");
}

/// My Identities picks a country the same way, for the account's own number.
#[tokio::test]
async fn picking_the_country_of_an_own_identity_gives_it_its_plus_form() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "own", "hunter2hunter2").await;
    let state = &fixture.state;
    let path = format!("/v1/accounts/{}", account.account_id);
    let _: serde_json::Value = patch_json(
        state,
        &path,
        &account.token,
        serde_json::json!({ "identities": [{ "address": NATIONAL, "service": "phone" }] }),
    )
    .await;
    let listed: serde_json::Value =
        get_json(state, &format!("{path}/identities"), &account.token).await;
    assert_eq!(listed["items"][0]["address"], "07700900123");
    assert_eq!(listed["items"][0]["country_unknown"], true);

    let _: serde_json::Value = patch_json(
        state,
        &path,
        &account.token,
        serde_json::json!({ "set_identity_country": { "address": "07700900123", "country": "GB" }}),
    )
    .await;
    let listed: serde_json::Value =
        get_json(state, &format!("{path}/identities"), &account.token).await;
    assert_eq!(listed["items"][0]["address"], FULL);
    assert_eq!(listed["items"][0]["country_unknown"], false);
}

/// When the `+` form is on another contact with a name, the merged identity
/// comes to the contact whose screen picked the country, so that contact is
/// not left with no identity; the other keeps its name.
#[tokio::test]
async fn a_merge_brings_the_plus_form_to_the_contact_that_picked_the_country() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "named", "hunter2hunter2").await;
    let state = &fixture.state;
    let body = format!(
        "{}\n{}\n{}\n{}\n",
        conversation_header("sms-backup-plus", NATIONAL).participant(NATIONAL, Some("Ada")),
        message_line("m1", "m1").sender(NATIONAL),
        conversation_header("sms-backup-plus", FULL).participant(FULL, Some("Ada L")),
        message_line("m2", "m2").at(1_700_000_100_000).sender(FULL),
    );
    import_with_country(state, &account.token, None, body).await;
    let picking = contact_of(state, "07700900123").await;
    let other = contact_of(state, FULL).await;
    assert_ne!(picking, other);

    let answer: serde_json::Value = patch_json(
        state,
        &format!("/v1/contacts/{picking}"),
        &account.token,
        serde_json::json!({ "set_identity_country": {
            "address": "07700900123", "country": "GB", "merge": true,
        }}),
    )
    .await;
    assert_eq!(answer["identities"][0]["address"], FULL);
    assert_eq!(contact_of(state, FULL).await, picking);
    let other: serde_json::Value =
        get_json(state, &format!("/v1/contacts/{other}"), &account.token).await;
    assert_eq!(other["name"], "Ada L", "the other contact keeps its name");
    assert_eq!(other["identities"], serde_json::json!([]));
}

/// Pick `country` for `address` on the contact `contact`, merging when
/// another identity holds the `+` form.
async fn pick_and_merge(
    state: &crate::server::AppState,
    token: &str,
    contact: i64,
    address: &str,
) -> serde_json::Value {
    patch_json(
        state,
        &format!("/v1/contacts/{contact}"),
        token,
        serde_json::json!({ "set_identity_country": {
            "address": address, "country": "GB", "merge": true,
        }}),
    )
    .await
}

/// How many messages the Messages list shows: those no dedupe hid.
async fn messages_shown(state: &crate::server::AppState, token: &str) -> usize {
    let page: serde_json::Value = get_json(state, "/v1/messages", token).await;
    page["items"].as_array().unwrap().len()
}

/// A one-to-one conversation with `chat` holding one message from it.
fn one_to_one(chat: &str, guid: &str) -> String {
    one_to_one_from("sms-backup-plus", chat, guid)
}

/// [`one_to_one`] as `source` writes it.
fn one_to_one_from(source: &str, chat: &str, guid: &str) -> String {
    format!(
        "{}\n{}\n",
        conversation_header(source, chat).participant(chat, None),
        message_line(guid, "On my way")
            .at(1_700_000_000_000)
            .sender(chat),
    )
}

/// When the `+` form a contact's number takes is one of the account's own
/// identities, the Contacts screen refuses the merge and names My
/// Identities, where it is done: joined there, the number's one-to-one
/// conversation becomes a conversation with yourself, and the account's
/// identity is on no contact and in no seat (#1662).
#[tokio::test]
async fn a_contact_s_number_joins_an_own_identity_on_my_identities_only() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "self", "hunter2hunter2").await;
    let state = &fixture.state;
    let path = format!("/v1/accounts/{}", account.account_id);
    let link = |address: &str| serde_json::json!({ "identities": [{ "address": address, "service": "phone" }] });
    let _: serde_json::Value = patch_json(state, &path, &account.token, link(FULL)).await;
    import_with_country(state, &account.token, None, one_to_one(NATIONAL, "m1")).await;
    let contact = contact_of(state, "07700900123").await;

    let (status, text) = patch_json_raw(
        state,
        &format!("/v1/contacts/{contact}"),
        &account.token,
        serde_json::json!({ "set_identity_country": {
            "address": "07700900123", "country": "GB", "merge": true,
        }}),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::StateConflict);
    assert!(problem.sentence().contains("My Identities"), "{text}");
    assert_eq!(phone_identities(state).await, [FULL, "07700900123"]);

    let _: serde_json::Value = patch_json(state, &path, &account.token, link("07700900123")).await;
    let _: serde_json::Value = patch_json(
        state,
        &path,
        &account.token,
        serde_json::json!({ "set_identity_country": {
            "address": "07700900123", "country": "GB", "merge": true,
        }}),
    )
    .await;

    let mut conn = state.db.acquire().await.unwrap();
    let (on_contacts, seats): (i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT COUNT(*) FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id
            WHERE h.normalized = $1),
           (SELECT COUNT(*) FROM participants p JOIN handles h ON h.id = p.handle_id
            WHERE h.normalized = $1)",
    )
    .bind(FULL)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!((on_contacts, seats), (0, 0));
    let with_yourself = crate::db::conversations::with_yourself_ids(&mut conn, account.account_id)
        .await
        .unwrap();
    assert_eq!(
        with_yourself.len(),
        1,
        "the conversation is one with yourself"
    );
}

/// Two sources' copies of one group, under each source's own chat
/// identifier, are one message by their members: once a member's national
/// spelling merges into its `+` form, every message of the group that listed
/// it is keyed again, not only those the merged number sent, and the copy
/// another member sent is hidden behind the other source's.
#[tokio::test]
async fn a_merge_keys_again_every_message_of_a_group_whose_member_changed() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "group", "hunter2hunter2").await;
    let state = &fixture.state;
    let other = "+447700900456";
    let group = |source: &str, chat: &str, member: &str| {
        format!(
            "{}\n{}\n",
            conversation_header(source, chat)
                .group()
                .participant(member, None)
                .participant(other, None),
            message_line(&format!("{source}-1"), "see you at six")
                .at(1_700_000_000_000)
                .sender(other),
        )
    };
    let token = &account.token;
    let sms = group("sms", "group-sms", NATIONAL);
    import_run(state, token, "sms", None, sms).await;
    let imessage = group("imessage", "group-imessage", FULL);
    import_run(state, token, "imessage", None, imessage).await;
    assert_eq!(messages_shown(state, token).await, 2);
    let contact = contact_of(state, "07700900123").await;

    pick_and_merge(state, token, contact, "07700900123").await;

    assert_eq!(messages_shown(state, token).await, 1);
}

/// When one of the two one-to-one conversations is in the Trash and the
/// other is not, the merged conversation is live: emptying the Trash must
/// not delete the live one's messages for good.
#[tokio::test]
async fn a_merge_with_a_trashed_conversation_keeps_the_live_one_live() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "trash", "hunter2hunter2").await;
    let state = &fixture.state;
    let body = format!("{}{}", one_to_one(NATIONAL, "m1"), one_to_one(FULL, "m2"));
    import_with_country(state, &account.token, None, body).await;
    {
        let mut conn = state.db.acquire().await.unwrap();
        sqlx::query(
            "INSERT INTO trashed_conversations (account_id, conversation_id)
             SELECT c.account_id, c.id FROM conversations c
             JOIN handles h ON h.id = c.chat_handle_id WHERE h.normalized = $1",
        )
        .bind(FULL)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    let contact = contact_of(state, "07700900123").await;
    pick_and_merge(state, &account.token, contact, "07700900123").await;

    let mut conn = state.db.acquire().await.unwrap();
    let trashed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM trashed_conversations")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(trashed, 0, "the merged conversation is out of the Trash");
}

/// A contact can hold a number whose country is unknown beside the `+` form
/// a run in a stated country gave the same digits. Picking the country for
/// the digits finds the row keyed by them, not the `+` row written the same
/// way, which would be refused as a number whose country is known.
#[tokio::test]
async fn picking_the_country_finds_the_number_keyed_by_the_digits_on_a_contact() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "both", "hunter2hunter2").await;
    let state = &fixture.state;
    import_with_country(
        state,
        &account.token,
        Some("GB"),
        one_to_one("07700900123", "m1"),
    )
    .await;
    import_with_country(state, &account.token, None, one_to_one("07700900123", "m2")).await;
    let contact = contact_of(state, FULL).await;
    {
        // The bare row goes on the same contact, as moving it there on the
        // Contacts screen would put it.
        let mut conn = state.db.acquire().await.unwrap();
        sqlx::query(
            "UPDATE contact_handles SET contact_id = $1
             WHERE handle_id = (SELECT id FROM handles WHERE normalized = '07700900123')",
        )
        .bind(contact)
        .execute(&mut *conn)
        .await
        .unwrap();
    }

    let answer = pick_and_merge(state, &account.token, contact, "07700900123").await;

    let identities: Vec<&str> = answer["identities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["address"].as_str().unwrap())
        .collect();
    assert_eq!(identities, [FULL]);
}

/// The same on My Identities: the account's number keyed by the digits is
/// found before its `+` form written the same way.
#[tokio::test]
async fn picking_the_country_finds_the_own_number_keyed_by_the_digits() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "ownboth", "hunter2hunter2").await;
    let state = &fixture.state;
    import_with_country(
        state,
        &account.token,
        Some("GB"),
        one_to_one("07700900123", "m1"),
    )
    .await;
    let path = format!("/v1/accounts/{}", account.account_id);
    let _: serde_json::Value = patch_json(
        state,
        &path,
        &account.token,
        serde_json::json!({ "identities": [
            { "address": FULL, "service": "phone" },
            { "address": "07700900123", "service": "phone" },
        ]}),
    )
    .await;

    let _: serde_json::Value = patch_json(
        state,
        &path,
        &account.token,
        serde_json::json!({ "set_identity_country": {
            "address": "07700900123", "country": "GB", "merge": true,
        }}),
    )
    .await;

    let listed: serde_json::Value =
        get_json(state, &format!("{path}/identities"), &account.token).await;
    let addresses: Vec<&str> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["address"].as_str().unwrap())
        .collect();
    assert_eq!(addresses, [FULL]);
}

/// Link the account's own number written `address`, on the phone service.
async fn link_own(
    state: &crate::server::AppState,
    account: &crate::test_support::RegisteredAccount,
    address: &str,
) {
    let _: serde_json::Value = patch_json(
        state,
        &format!("/v1/accounts/{}", account.account_id),
        &account.token,
        serde_json::json!({ "identities": [{ "address": address, "service": "phone" }] }),
    )
    .await;
}

/// Pick the United Kingdom for the account's own `07700900123` on My
/// Identities, merging into the identity holding its `+` form.
async fn pick_own_and_merge(
    state: &crate::server::AppState,
    account: &crate::test_support::RegisteredAccount,
) {
    let _: serde_json::Value = patch_json(
        state,
        &format!("/v1/accounts/{}", account.account_id),
        &account.token,
        serde_json::json!({ "set_identity_country": {
            "address": "07700900123", "country": "GB", "merge": true,
        }}),
    )
    .await;
}

/// A merge on My Identities when both numbers have a one-to-one
/// conversation: the national number's conversation, one with yourself,
/// joins the other and goes, and the with-yourself rule run after it does
/// not look for it.
#[tokio::test]
async fn an_own_number_merges_when_both_numbers_have_a_one_to_one_conversation() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "both11", "hunter2hunter2").await;
    let state = &fixture.state;
    let body = format!("{}{}", one_to_one(NATIONAL, "m1"), one_to_one(FULL, "m2"));
    import_with_country(state, &account.token, None, body).await;
    link_own(state, &account, "07700900123").await;

    pick_own_and_merge(state, &account).await;

    assert_eq!(conversations_and_members(state).await.0, 1);
    let mut conn = state.db.acquire().await.unwrap();
    let with_yourself = crate::db::conversations::with_yourself_ids(&mut conn, account.account_id)
        .await
        .unwrap();
    assert_eq!(
        with_yourself.len(),
        1,
        "the joined conversation is with yourself"
    );
}

/// A merge on My Identities makes the `+` form one of the account's own,
/// which changes the content keys of its conversations: a group that listed
/// it leaves the holder out of its key, so another source's copy of the
/// group, which never listed the holder, now matches it.
#[tokio::test]
async fn an_own_number_merge_keys_again_the_groups_of_the_plus_form() {
    let fixture = crate::test_support::test_fixture().await;
    let account =
        crate::test_support::register_via_api(&fixture.state, "ownkeys", "hunter2hunter2").await;
    let state = &fixture.state;
    let token = &account.token;
    let (ann, bob) = ("+447700900456", "+447700900789");
    let group = |source: &str, chat: &str, members: &[&str]| {
        let mut header = conversation_header(source, chat).group();
        for member in members {
            header = header.participant(member, None);
        }
        format!(
            "{header}\n{}\n",
            message_line(&format!("{source}-1"), "see you at six")
                .at(1_700_000_000_000)
                .sender(ann),
        )
    };
    let sms = group("sms", "group-sms", &[FULL, ann, bob]);
    import_run(state, token, "sms", None, sms).await;
    let imessage = group("imessage", "group-imessage", &[ann, bob]);
    import_run(state, token, "imessage", None, imessage).await;
    link_own(state, &account, "07700900123").await;
    assert_eq!(messages_shown(state, token).await, 2);

    pick_own_and_merge(state, &account).await;

    assert_eq!(messages_shown(state, token).await, 1);
}
