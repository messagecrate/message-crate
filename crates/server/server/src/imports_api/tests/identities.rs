//! An identity is always on a contact, and every participant is a contact
//! through an identity (#1105).

use super::*;

/// A text-message group "Trip": Ada at a number, and Sarah Vale, whom the
/// source names with no address.
fn trip_with_a_name_only_member() -> String {
    format!(
        "{}\n{}",
        conversation_header("openextract", "chat2000000001")
            .group()
            .title("Trip")
            .participant("+15555550123", Some("Ada"))
            .participant_without_identity(Some("Sarah Vale")),
        message_line("g-trip-1105", "hi")
            .sms()
            .sender("+15555550123")
            .line(),
    )
}

/// A WhatsApp group that names Sarah Vale with no address too.
fn whatsapp_group_with_a_name_only_member() -> String {
    format!(
        "{}\n{}",
        conversation_header("whatsapp", "120363000000001105@g.us")
            .group()
            .title("Hikes")
            .participant("+15555550124", Some("Bo"))
            .participant_without_identity(Some("Sarah Vale")),
        message_line("g-hikes-1105", "hi")
            .service(IrService::Whatsapp)
            .kind(IrMessageKind::Unknown)
            .sender_display_name("Sarah Vale")
            .line(),
    )
}

async fn contact_named(conn: &mut SqliteConnection, name: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1 AND preferred_name = $2")
        .bind(TEST_ACCOUNT)
        .bind(name)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("no contact named {name:?}: {e}"))
}

/// C2-2: a person has one seat in a conversation. Renaming the contact a
/// name-only participant sits on must not give the next import of the same
/// backup a second seat and a second contact.
#[tokio::test]
async fn c2_2_renaming_a_name_only_contact_then_reimporting_adds_no_seat() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let (first_rows, first_contacts) = participant_and_contact_counts(&mut conn).await;
    // The person renames Sarah Vale's contact.
    sqlx::query(
        "UPDATE contacts SET preferred_name = 'Sarah V.' WHERE account_id = $1 AND preferred_name = 'Sarah Vale'",
    )
    .bind(TEST_ACCOUNT)
    .execute(&mut *conn)
    .await
    .unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let (second_rows, second_contacts) = participant_and_contact_counts(&mut conn).await;
    assert_eq!(
        (second_rows.clone(), second_contacts),
        (first_rows.clone(), first_contacts),
        "seats before {first_rows:?} after {second_rows:?}; contacts {first_contacts} -> {second_contacts}"
    );
    assert_every_person_is_on_a_contact(&mut conn, "a re-import after a rename").await;
}

/// C2-4: a contact the source knows only by name sits in its group, and its
/// totals count that group the way a contact with a number does.
#[tokio::test]
async fn c2_4_a_name_only_contact_counts_its_group() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let sarah = contact_named(&mut conn, "Sarah Vale").await;
    let ada = contact_named(&mut conn, "Ada").await;
    let ada_totals = crate::db::contacts::read::contact_totals(&mut conn, TEST_ACCOUNT, ada)
        .await
        .unwrap();
    let totals = crate::db::contacts::read::contact_totals(&mut conn, TEST_ACCOUNT, sarah)
        .await
        .unwrap();
    assert_eq!(
        totals.groups, 1,
        "Sarah Vale sits in the Trip group but counts {} groups (Ada counts {})",
        totals.groups, ada_totals.groups
    );
}

/// S6-1: search reaches a conversation through the contact its participant's
/// identity is on now, not through the contact the import first gave it.
#[tokio::test]
async fn s6_1_with_follows_an_identity_an_address_book_moved() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_ada_conversation(&mut conn, TEST_ACCOUNT, "+15555550123", "g-s6-1").await;
    let a = contact_named(&mut conn, "Ada").await;
    let conversation: i64 = sqlx::query_scalar("SELECT id FROM conversations")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    // The address book keeps Ada and moves her number to a new contact, Bea.
    crate::db::address_book::load(
        &mut conn,
        TEST_ACCOUNT,
        &format!(
            "contact_id,display_name,groups,service,identity_type,identity\n\
             {a},Ada,,,,\n\
             ,Bea,,phone,phone,+15555550123\n"
        ),
        crate::db::address_book::LoadMode::Append,
    )
    .await
    .unwrap();
    let b = contact_named(&mut conn, "Bea").await;

    use crate::search::ListKind;
    use crate::search::tests::run;
    assert_eq!(
        run(&mut conn, ListKind::Conversations, &format!("with:#{a}")).await,
        Vec::<i64>::new(),
        "Ada no longer holds the number"
    );
    assert_eq!(
        run(&mut conn, ListKind::Conversations, &format!("with:#{b}")).await,
        vec![conversation],
        "Bea holds the number now"
    );
    assert_every_person_is_on_a_contact(&mut conn, "an address book move").await;
}

/// A person the source names with no address is an identity of type `other`
/// holding the name, one on each service. One name on two services is one
/// person, and the run says how many such identities it met.
#[tokio::test]
async fn a_name_with_no_address_is_an_other_identity_on_each_service_and_one_contact() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let texts = import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let whatsapp = import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "whatsapp",
        &whatsapp_group_with_a_name_only_member(),
    )
    .await;
    assert_eq!(texts.other_identities, 1, "{texts:?}");
    assert_eq!(whatsapp.other_identities, 1, "{whatsapp:?}");

    let identities: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT h.service, h.raw, ch.contact_id FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         WHERE h.account_id = $1 AND h.handle_type = 'other' AND h.raw = 'Sarah Vale'
         ORDER BY h.service",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let sarah = contact_named(&mut conn, "Sarah Vale").await;
    assert_eq!(
        identities,
        [
            ("phone".to_string(), "Sarah Vale".to_string(), sarah),
            ("whatsapp".to_string(), "Sarah Vale".to_string(), sarah),
        ]
    );
    let totals = crate::db::contacts::read::contact_totals(&mut conn, TEST_ACCOUNT, sarah)
        .await
        .unwrap();
    assert_eq!(totals.groups, 2, "Trip and Hikes");
    assert_every_person_is_on_a_contact(&mut conn, "two imports naming one person").await;
}

/// A contact whose only identities are of type `other` is a name the import
/// could not tie to an address, so it is Unknown however it is named.
#[tokio::test]
async fn a_contact_with_only_other_identities_is_unknown() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let unknown: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT ct.preferred_name FROM contacts ct
         WHERE ct.account_id = $1 AND {}
         ORDER BY ct.preferred_name",
        crate::db::contacts::UNKNOWN_CONTACT_SQL
    ))
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(unknown, ["Sarah Vale"]);
}

/// ADR-0013 with the rule: when an import discards a trashed contact, the
/// fresh contact takes the identity the import met, and every other
/// identity the trashed contact had that is in a conversation goes to a new
/// contact with no name.
#[tokio::test]
async fn discarding_a_trashed_contact_leaves_none_of_its_identities_on_no_contact() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_ada_conversation(&mut conn, TEST_ACCOUNT, "+15555550123", "g-discard-1").await;
    import_ada_conversation(&mut conn, TEST_ACCOUNT, "ada@example.com", "g-discard-2").await;
    // Both of Ada's addresses are on one contact, which the person trashes.
    let ada: i64 = sqlx::query_scalar(
        "SELECT ch.contact_id FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id
         WHERE h.raw = '+15555550123'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE contact_handles SET contact_id = $1
         WHERE account_id = $2 AND handle_id IN (SELECT id FROM handles WHERE raw = 'ada@example.com')",
    )
    .bind(ada)
    .bind(TEST_ACCOUNT)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("DELETE FROM contacts WHERE account_id = $1 AND id <> $2")
        .bind(TEST_ACCOUNT)
        .bind(ada)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(ada)
        .execute(&mut *conn)
        .await
        .unwrap();

    // The next backup holds only her number.
    import_ada_conversation(&mut conn, TEST_ACCOUNT, "+15555550123", "g-discard-3").await;

    assert_every_person_is_on_a_contact(&mut conn, "an import discarded a trashed contact").await;
    let holders: Vec<(String, String)> = sqlx::query_as(
        "SELECT h.raw, ct.preferred_name FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         JOIN contacts ct ON ct.id = ch.contact_id
         WHERE h.account_id = $1
         ORDER BY h.raw",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        holders,
        [
            ("+15555550123".to_string(), "Ada".to_string()),
            ("ada@example.com".to_string(), String::new()),
        ],
        "the number is on the fresh contact; the address is Unknown again"
    );
}

/// A name-only participant whose contact an import discards keeps its
/// identity, and the identity goes to the fresh contact.
#[tokio::test]
async fn a_name_only_participant_keeps_its_identity_when_its_contact_is_discarded() {
    let fixture = test_fixture().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let sarah = contact_named(&mut conn, "Sarah Vale").await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(sarah)
        .execute(&mut *conn)
        .await
        .unwrap();
    let (first_rows, _) = participant_and_contact_counts(&mut conn).await;
    import_jsonl_text(
        &mut conn,
        TEST_ACCOUNT,
        "openextract",
        &trip_with_a_name_only_member(),
    )
    .await;
    let (second_rows, _) = participant_and_contact_counts(&mut conn).await;
    assert_eq!(second_rows, first_rows, "Sarah Vale keeps one seat");
    assert_every_person_is_on_a_contact(&mut conn, "an import discarded a name-only contact").await;
    let fresh = contact_named(&mut conn, "Sarah Vale").await;
    assert_ne!(fresh, sarah, "the trashed contact was replaced");
}
