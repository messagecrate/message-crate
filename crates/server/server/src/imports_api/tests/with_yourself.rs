//! Conversations the account holder has with themselves, and linking or
//! removing an identity of the account holder after an import.

use super::*;

/// A one-to-one chat whose identifier is one of the account's identities is a
/// conversation the holder has with themselves: Apple Messages' chat with the
/// owner's own number, WhatsApp's "Message yourself". It makes no contact and
/// no participant; both rows of a note are kept and the received one has no
/// sender. It is titled with the account's display name, or the address
/// without one, and `with:me` finds it and nothing else (#1094).
#[tokio::test]
async fn a_conversation_with_yourself_has_no_participants_and_goes_by_the_accounts_name() {
    let (state, fixture, token) = importer().await;
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let account = format!("/v1/accounts/{account_id}");
    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "identities": [{ "address": "+15555550199", "service": "phone" }] }),
    )
    .await;

    // Apple Messages lists nobody in the chat with the owner's own number;
    // WhatsApp lists the holder's number as the peer of "Message yourself".
    // Each source also carries one ordinary chat, which `with:me` must leave out.
    // The two sources' notes are an hour apart, so the dedupe every batch
    // runs keeps both.
    for (source, self_participant, service, sent_at) in [
        ("imessage", None, IrService::IMessage, 1_400_773_261_000),
        (
            "whatsapp",
            Some("+15555550199"),
            IrService::Whatsapp,
            1_400_776_861_000,
        ),
    ] {
        let path = batches_path(&state, &token, source).await;
        let message = |guid: &str, sender: Option<&str>, text: &str, reaction: Option<Reaction>| {
            let mut message = message_line(guid, text)
                .at(sent_at)
                .service(service)
                .kind(IrMessageKind::Unknown);
            message = match sender {
                Some(sender) => message.sender(sender),
                None => message.outgoing(),
            };
            if let Some(reaction) = reaction {
                message = message.reaction(reaction);
            }
            message.line()
        };
        // The holder's own reaction to the received copy of a note.
        let own_tapback = liked_by("+15555550199");
        let mut self_header = conversation_header(source, "+15555550199");
        // Apple Messages can carry a name the holder gave the chat; the
        // account's name still titles it.
        if source == "imessage" {
            self_header = self_header.title("Notes");
        }
        if let Some(identity) = self_participant {
            self_header = self_header.participant(identity, Some("Me"));
        }
        let ada_header =
            conversation_header(source, "+15555550101").participant("+15555550101", Some("Ada"));
        let body = [
            self_header.line(),
            message(&format!("{source}-note-sent"), None, "Note to self", None),
            message(
                &format!("{source}-note-received"),
                Some("+15555550199"),
                "Note to self",
                Some(own_tapback),
            ),
            ada_header.line(),
            message(&format!("{source}-ada"), Some("+15555550101"), "hi", None),
        ]
        .concat();
        let (status, text) =
            crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
        assert!(status.is_success(), "{source}: {status} {text}");
        let complete = path.replace("/batches", "/complete");
        let _: serde_json::Value = post_json(
            &state,
            &complete,
            &token,
            serde_json::json!({ "status": "completed" }),
        )
        .await;
    }

    let mut conn = fixture.conn().await;
    let holder_contacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND h.normalized = '+15555550199'",
    )
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(holder_contacts, 0, "no contact is made for the holder");
    let self_conversations: Vec<i64> = sqlx::query_scalar(
        "SELECT c.id FROM conversations c JOIN handles h ON h.id = c.chat_handle_id
         WHERE c.account_id = $1 AND h.normalized = '+15555550199' ORDER BY c.id",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(self_conversations.len(), 2, "{self_conversations:?}");
    for &id in &self_conversations {
        let participants: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM participants WHERE conversation_id = $1")
                .bind(id)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(participants, 0, "conversation {id} has no participants");
        let rows: Vec<(i64, Option<i64>)> = sqlx::query_as(
            "SELECT is_from_me, sender_handle_id FROM messages
             WHERE conversation_id = $1 ORDER BY is_from_me",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        assert_eq!(
            rows,
            [(0, None), (1, None)],
            "conversation {id} keeps both rows, and the received one has no sender"
        );
    }
    let tapback_senders: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT t.is_from_me, t.sender_handle_id FROM tapbacks t
         JOIN messages m ON m.id = t.message_id
         WHERE m.conversation_id = $1",
    )
    .bind(self_conversations[0])
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        tapback_senders,
        [(1, None)],
        "the holder's reaction in a conversation with yourself is theirs, with no sender"
    );
    drop(conn);

    let titles = |page: &serde_json::Value| -> Vec<(i64, String)> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["id"].as_i64().unwrap(),
                    c["shown_title"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    };
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=with%3Ame", &token).await;
    let mut found = titles(&page);
    found.sort_unstable();
    assert_eq!(
        found,
        self_conversations
            .iter()
            .map(|&id| (id, "+15555550199".to_string()))
            .collect::<Vec<_>>(),
        "with:me finds the two conversations with yourself, titled by the address: {page}"
    );
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["participants"] == serde_json::json!([])),
        "the holder is not read back as a participant from the chat's own identity: {page}"
    );

    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "preferred_name": "Sam Holder" }),
    )
    .await;
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=with%3Ame", &token).await;
    assert!(
        titles(&page).iter().all(|(_, title)| title == "Sam Holder") && page["total"] == 2,
        "the title follows the account's display name: {page}"
    );
    let page: serde_json::Value = get_json(
        &state,
        "/v1/conversations?q=title%3A%22Sam%20Holder%22",
        &token,
    )
    .await;
    assert_eq!(page["total"], 2, "title: reads the same title: {page}");
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=Holder", &token).await;
    assert_eq!(page["total"], 2, "plain text reads the same title: {page}");
    let page: serde_json::Value =
        get_json(&state, "/v1/messages?q=in%3A%22Sam%20Holder%22", &token).await;
    assert_eq!(page["total"], 4, "in: reads the same title: {page}");

    let page: serde_json::Value = get_json(&state, "/v1/messages?q=with%3Ame", &token).await;
    let messages = page["items"].as_array().unwrap();
    assert_eq!(messages.len(), 4, "{page}");
    for message in messages {
        assert_eq!(message["text"], "Note to self", "{message}");
        assert_eq!(
            message["conversation"]["shown_title"], "Sam Holder",
            "{message}"
        );
        assert!(
            message.get("sender").is_none_or(serde_json::Value::is_null),
            "{message}"
        );
    }
}

/// Import one conversation with the holder's number `+15555550199` (a note
/// sent and its received copy, the holder listed by number alone as the
/// chat's participant, as WhatsApp lists them) and one with Ada, and answer the holder's
/// conversation's id.
async fn import_notes_to_yourself(
    state: &crate::server::AppState,
    fixture: &crate::test_support::TestFixture,
    token: &str,
) -> i64 {
    let message = |guid: &str, sender: Option<&str>| {
        let message = message_line(guid, "Note to self")
            .at(1_400_773_261_000)
            .service(IrService::Whatsapp)
            .kind(IrMessageKind::Unknown);
        match sender {
            Some(sender) => message.sender(sender),
            None => message.outgoing(),
        }
        .line()
    };
    let body = [
        conversation_header("whatsapp", "+15555550199")
            .participant("+15555550199", None)
            .line(),
        message("note-sent", None),
        message("note-received", Some("+15555550199")),
        conversation_header("whatsapp", "+15555550101")
            .participant("+15555550101", Some("Ada"))
            .line(),
        message_line("ada", "hi")
            .at(1_400_773_262_000)
            .service(IrService::Whatsapp)
            .sender("+15555550101")
            .line(),
    ]
    .concat();
    import_whatsapp(state, token, body).await;
    sqlx::query_scalar(
        "SELECT c.id FROM conversations c JOIN handles h ON h.id = c.chat_handle_id
         WHERE h.normalized = '+15555550199'",
    )
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// Import `body`, a WhatsApp conversation file, in one completed Import Run.
async fn import_whatsapp(state: &crate::server::AppState, token: &str, body: String) {
    let path = batches_path(state, token, "whatsapp").await;
    let (status, text) =
        crate::test_support::post_raw(state, &path, token, "application/jsonl", body).await;
    assert!(status.is_success(), "{status} {text}");
    let _: serde_json::Value = post_json(
        state,
        &path.replace("/batches", "/complete"),
        token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
}

/// Link (`field` `identities`) or remove (`remove_identities`) the holder's
/// number `+15555550199` on WhatsApp as an identity of the `importer`
/// account.
async fn change_holder_identity(
    state: &crate::server::AppState,
    fixture: &crate::test_support::TestFixture,
    token: &str,
    field: &str,
) {
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let _: serde_json::Value = patch_json(
        state,
        &format!("/v1/accounts/{account_id}"),
        token,
        serde_json::json!({ field: [{ "address": "+15555550199", "service": "whatsapp" }] }),
    )
    .await;
}

/// The contact on the holder's number `+15555550199`, if any.
async fn holder_contact(fixture: &crate::test_support::TestFixture) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT ch.contact_id FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE h.normalized = '+15555550199'",
    )
    .fetch_optional(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// The participants of `conversation_id` as (number, name the backup or the
/// contact gave), by number.
async fn participants_of(
    fixture: &crate::test_support::TestFixture,
    conversation_id: i64,
) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT h.normalized, p.name_alias FROM participants p
         JOIN handles h ON h.id = p.handle_id
         WHERE p.conversation_id = $1
         ORDER BY h.normalized",
    )
    .bind(conversation_id)
    .fetch_all(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// A one-to-one conversation at the holder's number `+15555550199`, its
/// header listing the holder as `holder_name` and, when given, `other` too,
/// with one received note.
fn notes_to_yourself_file(holder_name: Option<&str>, other: Option<&str>) -> String {
    let mut header =
        conversation_header("whatsapp", "+15555550199").participant("+15555550199", holder_name);
    if let Some(other) = other {
        header = header.participant(other, None);
    }
    [
        header.line(),
        message_line("note-received", "Note to self")
            .at(1_400_773_261_000)
            .service(IrService::Whatsapp)
            .kind(IrMessageKind::Unknown)
            .sender("+15555550199")
            .line(),
    ]
    .concat()
}

/// What the account holds about the holder's number: the participants rows
/// of `conversation_id`, and the contacts on `+15555550199`.
async fn holder_rows(
    fixture: &crate::test_support::TestFixture,
    conversation_id: i64,
) -> (i64, i64) {
    let mut conn = fixture.conn().await;
    let participants: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM participants WHERE conversation_id = $1")
            .bind(conversation_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let contacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE h.normalized = '+15555550199'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    (participants, contacts)
}

/// Linking an identity after an import makes the conversations at that
/// address conversations with yourself, and the stored rows follow: the
/// participant goes, and so does the contact the import made for the holder,
/// so the holder is in neither Contacts nor Unknown. The reads agree: `with:me`
/// finds the conversation and it has no participants (#1662).
#[tokio::test]
async fn linking_an_identity_after_an_import_takes_the_holder_off_its_conversations() {
    let (state, fixture, token) = importer().await;
    let conversation_id = import_notes_to_yourself(&state, &fixture, &token).await;
    assert_eq!(
        holder_rows(&fixture, conversation_id).await,
        (1, 1),
        "before the identity is linked, the number is a participant with a contact"
    );
    let unknown: serde_json::Value =
        get_json(&state, "/v1/contacts?q=group%3Aunknown", &token).await;
    assert!(unknown.to_string().contains("+15555550199"), "{unknown}");

    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let _: serde_json::Value = patch_json(
        &state,
        &format!("/v1/accounts/{account_id}"),
        &token,
        serde_json::json!({ "identities": [{ "address": "+15555550199", "service": "whatsapp" }] }),
    )
    .await;

    assert_eq!(
        holder_rows(&fixture, conversation_id).await,
        (0, 0),
        "the conversation with yourself has no participant, and the holder no contact"
    );
    for path in ["/v1/contacts", "/v1/contacts?q=group%3Aunknown"] {
        let page: serde_json::Value = get_json(&state, path, &token).await;
        let text = page.to_string();
        assert!(!text.contains("+15555550199"), "{path}: {page}");
    }
    let contacts: serde_json::Value = get_json(&state, "/v1/contacts", &token).await;
    assert!(
        contacts.to_string().contains("+15555550101"),
        "Ada keeps her contact: {contacts}"
    );
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=with%3Ame", &token).await;
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{page}");
    assert_eq!(items[0]["id"], conversation_id, "{page}");
    assert_eq!(items[0]["participants"], serde_json::json!([]), "{page}");
}

/// Removing an identity after an import makes the conversations at that
/// address ordinary one-to-one conversations again, and the stored rows
/// follow: the chat's address is its participant, on a contact, as an import
/// writes it. The reads agree: `with:me` no longer finds it, and the
/// participant it shows opens that contact (#1662).
#[tokio::test]
async fn removing_an_identity_after_an_import_gives_its_conversations_their_participant_back() {
    let (state, fixture, token) = importer().await;
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let account = format!("/v1/accounts/{account_id}");
    let identity = serde_json::json!([{ "address": "+15555550199", "service": "whatsapp" }]);
    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "identities": identity }),
    )
    .await;
    let conversation_id = import_notes_to_yourself(&state, &fixture, &token).await;
    assert_eq!(
        holder_rows(&fixture, conversation_id).await,
        (0, 0),
        "imported with the identity linked, the conversation is with yourself"
    );

    let _: serde_json::Value = patch_json(
        &state,
        &account,
        &token,
        serde_json::json!({ "remove_identities": identity }),
    )
    .await;

    assert_eq!(
        holder_rows(&fixture, conversation_id).await,
        (1, 1),
        "the chat's address is its participant again, on a contact"
    );
    let page: serde_json::Value = get_json(&state, "/v1/conversations?q=with%3Ame", &token).await;
    assert_eq!(page["total"], 0, "{page}");
    let page: serde_json::Value = get_json(
        &state,
        &format!("/v1/conversations/{conversation_id}"),
        &token,
    )
    .await;
    let participants = page["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 1, "{page}");
    assert_eq!(participants[0]["identity"], "+15555550199", "{page}");
    assert!(participants[0]["contact_id"].is_i64(), "{page}");
    let unknown: serde_json::Value =
        get_json(&state, "/v1/contacts?q=group%3Aunknown", &token).await;
    assert!(unknown.to_string().contains("+15555550199"), "{unknown}");
}

/// The id of the one conversation at `chat`.
async fn conversation_at(fixture: &crate::test_support::TestFixture, chat: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT c.id FROM conversations c JOIN handles h ON h.id = c.chat_handle_id
         WHERE h.normalized = $1",
    )
    .bind(chat)
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// A contact an import named for the holder is kept when the holder's number
/// becomes an identity: an address book load renames a contact without
/// changing its origin, so the name may be the person's, and deleting the
/// contact would lose it. Its participant still goes (#1662).
#[tokio::test]
async fn linking_an_identity_keeps_a_named_holder_contact() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(
        &state,
        &token,
        notes_to_yourself_file(Some("Me (work)"), None),
    )
    .await;
    let conversation_id = conversation_at(&fixture, "+15555550199").await;
    let contact_id = holder_contact(&fixture)
        .await
        .expect("the import made a contact");

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(holder_contact(&fixture).await, Some(contact_id));
    let name: String = sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    assert_eq!(name, "Me (work)");
    assert_eq!(participants_of(&fixture, conversation_id).await, vec![]);
}

/// A holder contact the person put in the Trash is kept, with its Trash
/// entry, when the holder's number becomes an identity (#1662).
#[tokio::test]
async fn linking_an_identity_keeps_a_trashed_holder_contact() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, notes_to_yourself_file(None, None)).await;
    let contact_id = holder_contact(&fixture)
        .await
        .expect("the import made a contact");
    let status = crate::test_support::post_status(
        &state,
        &format!("/v1/contacts/{contact_id}/trash"),
        &token,
        serde_json::json!({}),
    )
    .await;
    assert!(status.is_success(), "{status}");

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(holder_contact(&fixture).await, Some(contact_id));
    let trashed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trashed_contacts WHERE contact_id = $1")
            .bind(contact_id)
            .fetch_one(&mut *fixture.conn().await)
            .await
            .unwrap();
    assert_eq!(trashed, 1);
}

/// Linking the holder's number drops only the participants at an identity,
/// as an import does: another participant the header listed in the same
/// conversation stays, with its contact (#1662).
#[tokio::test]
async fn linking_an_identity_keeps_the_other_participants() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(
        &state,
        &token,
        notes_to_yourself_file(None, Some("+15555550102")),
    )
    .await;
    let conversation_id = conversation_at(&fixture, "+15555550199").await;
    assert_eq!(participants_of(&fixture, conversation_id).await.len(), 2);

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(
        participants_of(&fixture, conversation_id).await,
        vec![("+15555550102".to_string(), None)]
    );
    assert_eq!(holder_contact(&fixture).await, None);
}

/// A group imported before the holder's number was linked lists the holder
/// as a participant. Once the number is an identity the holder is no
/// participant of it, as an import after the link would write it, and the
/// contact the import made for the holder goes (#1093, #1662).
#[tokio::test]
async fn linking_an_identity_takes_the_holder_off_its_groups() {
    let (state, fixture, token) = importer().await;
    let body = [
        conversation_header("whatsapp", "group-1662@g.us")
            .group()
            .title("Family")
            .participant("+15555550199", None)
            .participant("+15555550101", Some("Ada"))
            .line(),
        message_line("group-hi", "hi")
            .at(1_400_773_262_000)
            .service(IrService::Whatsapp)
            .sender("+15555550101")
            .line(),
    ]
    .concat();
    import_whatsapp(&state, &token, body).await;
    let conversation_id: i64 =
        sqlx::query_scalar("SELECT id FROM conversations WHERE group_title = 'Family'")
            .fetch_one(&mut *fixture.conn().await)
            .await
            .unwrap();
    assert_eq!(participants_of(&fixture, conversation_id).await.len(), 2);
    assert!(holder_contact(&fixture).await.is_some());

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(
        participants_of(&fixture, conversation_id).await,
        vec![("+15555550101".to_string(), Some("Ada".to_string()))]
    );
    assert_eq!(holder_contact(&fixture).await, None);
}

/// Linking the holder's number by mistake and removing it again gives the
/// conversation its participant back under the name the backup gave it: the
/// named contact was kept, and the participant takes its name (#1662).
#[tokio::test]
async fn removing_an_identity_linked_by_mistake_keeps_the_backups_name() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(
        &state,
        &token,
        notes_to_yourself_file(Some("Work phone"), None),
    )
    .await;
    let conversation_id = conversation_at(&fixture, "+15555550199").await;
    let contact_id = holder_contact(&fixture)
        .await
        .expect("the import made a contact");
    let before = participants_of(&fixture, conversation_id).await;
    assert_eq!(
        before,
        vec![("+15555550199".to_string(), Some("Work phone".to_string()))]
    );

    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert_eq!(participants_of(&fixture, conversation_id).await, vec![]);
    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert_eq!(participants_of(&fixture, conversation_id).await, before);
    assert_eq!(holder_contact(&fixture).await, Some(contact_id));
}

/// The holder named `holder_name` in the header of a group with Ada and of
/// a one-to-one conversation with Ada, each with one message from Ada whose
/// guid ends in `suffix`.
fn holder_listed_with_ada_file(holder_name: &str, suffix: &str) -> String {
    let message = |guid: &str| {
        message_line(&format!("{guid}-{suffix}"), "hi")
            .at(1_400_773_262_000)
            .service(IrService::Whatsapp)
            .sender("+15555550101")
            .line()
    };
    [
        conversation_header("whatsapp", "group-1662@g.us")
            .group()
            .title("Family")
            .participant("+15555550199", Some(holder_name))
            .participant("+15555550101", Some("Ada"))
            .line(),
        message("group-hi"),
        conversation_header("whatsapp", "+15555550101")
            .participant("+15555550101", Some("Ada"))
            .participant("+15555550199", Some(holder_name))
            .line(),
        message("ada-hi"),
    ]
    .concat()
}

/// The id of the group titled `Family`.
async fn family_group(fixture: &crate::test_support::TestFixture) -> i64 {
    sqlx::query_scalar("SELECT id FROM conversations WHERE group_title = 'Family'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap()
}

/// How many rows the three set-aside tables hold for the holder's number.
async fn set_aside_rows(fixture: &crate::test_support::TestFixture) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM participants_set_aside sa
                 JOIN handles h ON h.id = sa.handle_id WHERE h.normalized = '+15555550199')
              + (SELECT COUNT(*) FROM import_contacts_set_aside sa
                 JOIN handles h ON h.id = sa.handle_id WHERE h.normalized = '+15555550199')
              + (SELECT COUNT(*) FROM contact_group_members_set_aside sa
                 JOIN handles h ON h.id = sa.handle_id WHERE h.normalized = '+15555550199')",
    )
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap()
}

/// Link or remove the holder's number `+15555550199` on `service`.
async fn change_holder_identity_on(
    state: &crate::server::AppState,
    fixture: &crate::test_support::TestFixture,
    token: &str,
    field: &str,
    service: &str,
) {
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = 'importer'")
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    let _: serde_json::Value = patch_json(
        state,
        &format!("/v1/accounts/{account_id}"),
        token,
        serde_json::json!({ field: [{ "address": "+15555550199", "service": service }] }),
    )
    .await;
}

/// Linking the holder's number by mistake sets the holder's participant rows
/// aside, in a group and in a one-to-one conversation with someone else
/// alike. Removing it again puts them back with the name the backup gave
/// (#1662).
#[tokio::test]
async fn removing_an_identity_puts_back_the_holder_participants_it_set_aside() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_listed_with_ada_file("Matt", "1")).await;
    let group = family_group(&fixture).await;
    let with_ada = conversation_at(&fixture, "+15555550101").await;
    let expected = vec![
        ("+15555550101".to_string(), Some("Ada".to_string())),
        ("+15555550199".to_string(), Some("Matt".to_string())),
    ];
    assert_eq!(participants_of(&fixture, group).await, expected);
    assert_eq!(participants_of(&fixture, with_ada).await, expected);

    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert_eq!(participants_of(&fixture, group).await, expected[..1]);
    assert_eq!(participants_of(&fixture, with_ada).await, expected[..1]);
    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert_eq!(participants_of(&fixture, group).await, expected);
    assert_eq!(participants_of(&fixture, with_ada).await, expected);
}

/// A participant set aside keeps its identity alive. The holder's number is
/// linked on the phone service, so its WhatsApp row is set aside in the
/// group and stays on the named contact the import made. Taking that row off
/// the contact must not delete it, or its set-aside participant would go
/// with it, and removing the identity would give nothing back (#1662).
#[tokio::test]
async fn taking_a_set_aside_identity_off_its_contact_keeps_it_for_the_unlink() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_listed_with_ada_file("Matt", "1")).await;
    let group = family_group(&fixture).await;
    change_holder_identity_on(&state, &fixture, &token, "identities", "phone").await;
    let contact_id = holder_contact(&fixture)
        .await
        .expect("the named contact stays");
    let _: serde_json::Value = patch_json(
        &state,
        &format!("/v1/contacts/{contact_id}"),
        &token,
        serde_json::json!({
            "remove_identity": { "address": "+15555550199", "service": "whatsapp" }
        }),
    )
    .await;

    change_holder_identity_on(&state, &fixture, &token, "remove_identities", "phone").await;

    assert!(
        participants_of(&fixture, group)
            .await
            .contains(&("+15555550199".to_string(), Some("Matt".to_string())))
    );
}

/// An import while the holder's number is linked writes no participant for
/// the holder, whatever the backup now calls them. Removing the identity
/// then gives back the participant set aside, with the name it had (#1662).
#[tokio::test]
async fn removing_an_identity_keeps_the_set_aside_name_over_a_later_backups() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_listed_with_ada_file("Matt", "1")).await;
    let group = family_group(&fixture).await;
    change_holder_identity(&state, &fixture, &token, "identities").await;
    import_whatsapp(&state, &token, holder_listed_with_ada_file("Matthew", "2")).await;
    assert_eq!(family_group(&fixture).await, group);

    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert!(
        participants_of(&fixture, group)
            .await
            .contains(&("+15555550199".to_string(), Some("Matt".to_string())))
    );
}

/// A group with Ada, where the holder's number `+15555550199` sent a
/// received row but is not in the header.
fn holder_sent_in_a_group_file() -> String {
    let message = |guid: &str, sender: &str| {
        message_line(guid, "hi")
            .at(1_400_773_262_000)
            .service(IrService::Whatsapp)
            .sender(sender)
            .line()
    };
    [
        conversation_header("whatsapp", "group-1662@g.us")
            .group()
            .title("Family")
            .participant("+15555550101", Some("Ada"))
            .line(),
        message("group-ada", "+15555550101"),
        message("group-holder", "+15555550199"),
    ]
    .concat()
}

/// A received row the holder's second number sent in a group is no reason
/// to keep the contact the import made for that number once it is linked:
/// an import after the link gives such a sender no contact (#1093, #1662).
#[tokio::test]
async fn linking_an_identity_deletes_the_contact_of_a_sender_at_it() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_sent_in_a_group_file()).await;
    assert!(holder_contact(&fixture).await.is_some());

    change_holder_identity(&state, &fixture, &token, "identities").await;

    assert_eq!(holder_contact(&fixture).await, None);
    let unknown: serde_json::Value =
        get_json(&state, "/v1/contacts?q=group%3Aunknown", &token).await;
    assert!(!unknown.to_string().contains("+15555550199"), "{unknown}");
}

/// The import run records and the run's Contact Group keep the holder's
/// contact across a link and an unlink: the contact made at the unlink
/// takes the run's record and group membership the deleted one had (#1662).
#[tokio::test]
async fn removing_an_identity_gives_the_new_contact_the_runs_record_back() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_sent_in_a_group_file()).await;
    let record = |contact_id: i64| {
        let fixture = &fixture;
        async move {
            let mut conn = fixture.conn().await;
            let runs: Vec<(i64, String)> = sqlx::query_as(
                "SELECT import_id, reason FROM import_contacts WHERE contact_id = $1",
            )
            .bind(contact_id)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
            let groups: Vec<i64> = sqlx::query_scalar(
                "SELECT gm.group_id FROM contact_group_members gm
                 JOIN contact_groups g ON g.id = gm.group_id
                 WHERE gm.contact_id = $1 AND g.kind = 'import'",
            )
            .bind(contact_id)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
            (runs, groups)
        }
    };
    let before = record(holder_contact(&fixture).await.unwrap()).await;
    assert_eq!(before.0.len(), 1, "{before:?}");
    assert_eq!(before.1.len(), 1, "{before:?}");

    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert_eq!(holder_contact(&fixture).await, None);
    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    let contact_id = holder_contact(&fixture).await.expect("a contact again");
    assert_eq!(record(contact_id).await, before);
}

/// The run records set aside for the holder's number go to the contact the
/// number has when the identity is removed, here Ada's, to which the person
/// added the number while it was linked. None stay behind (#1662).
#[tokio::test]
async fn removing_an_identity_gives_set_aside_records_to_the_contact_it_has() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_sent_in_a_group_file()).await;
    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert!(set_aside_rows(&fixture).await > 0);
    let ada: i64 = sqlx::query_scalar(
        "SELECT ch.contact_id FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id WHERE h.normalized = '+15555550101'",
    )
    .fetch_one(&mut *fixture.conn().await)
    .await
    .unwrap();
    let _: serde_json::Value = patch_json(
        &state,
        &format!("/v1/contacts/{ada}"),
        &token,
        serde_json::json!({
            "add_identity": { "address": "+15555550199", "service": "whatsapp" }
        }),
    )
    .await;

    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert_eq!(holder_contact(&fixture).await, Some(ada));
    assert_eq!(set_aside_rows(&fixture).await, 0);
}

/// The run records set aside for the holder's number are forgotten when the
/// identity is removed and no conversation holds the number any more, so a
/// later link cannot give a new contact a record for an earlier run (#1662).
#[tokio::test]
async fn removing_an_identity_forgets_set_aside_records_nothing_takes() {
    let (state, fixture, token) = importer().await;
    import_whatsapp(&state, &token, holder_sent_in_a_group_file()).await;
    change_holder_identity(&state, &fixture, &token, "identities").await;
    assert!(set_aside_rows(&fixture).await > 0);
    // The group deleted for good while the number was linked.
    let group = family_group(&fixture).await;
    let path = format!("/v1/conversations/{group}");
    let trashed = crate::test_support::post_status(
        &state,
        &format!("{path}/trash"),
        &token,
        serde_json::json!({}),
    )
    .await;
    assert!(trashed.is_success(), "{trashed}");
    let deleted = crate::test_support::delete_status(&state, &path, &token).await;
    assert!(deleted.is_success(), "{deleted}");

    change_holder_identity(&state, &fixture, &token, "remove_identities").await;

    assert_eq!(holder_contact(&fixture).await, None);
    assert_eq!(set_aside_rows(&fixture).await, 0);
}
