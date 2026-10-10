use super::*;
use crate::test_support::FixtureDb;
use chat_db_fixture::{
    DELETED_GUID, DELETED_TEXT, FRIEND_EMAIL, FRIEND_PHONE, FRIEND_PHONE_EMAIL,
    GROUP_CHAT_IDENTIFIER, GROUP_TITLE, OWNER, OWNER_EMAIL, PARTLY_UNSENT_GUID, PARTLY_UNSENT_TEXT,
    SHRUNK_GROUP_IDENTIFIER, UNSENT_GUID,
};
use imessage_reader_protocol::AttachmentSource;
use std::collections::HashMap;

/// The handles of a roster, in roster order.
fn handles_of(participants: &[Participant]) -> Vec<&str> {
    participants.iter().map(|p| p.identity.as_str()).collect()
}

#[test]
fn send_effect_is_appended_once() {
    assert_eq!(with_send_effect("hi".into(), None), "hi");
    assert_eq!(with_send_effect(String::new(), Some("Slam")), "Slam");
    assert_eq!(with_send_effect("hi".into(), Some("Slam")), "hi\n\nSlam");
    assert_eq!(with_send_effect("hi Slam".into(), Some("Slam")), "hi Slam");
}

#[test]
fn tapback_lines_name_the_reaction() {
    assert_eq!(tapback_human_line("loved", None, "add"), "Loved a message");
    assert_eq!(tapback_human_line("loved", None, "remove"), "Removed Heart");
    assert_eq!(tapback_human_line("emoji", Some("🔥"), "add"), "🔥 reacted");
}

#[test]
fn empty_fields_are_dropped() {
    assert!(is_empty(&ImessageRecord::default()));
    let fields = ImessageRecord {
        send_effect: Some("Slam".to_string()),
        ..ImessageRecord::default()
    };
    assert!(!is_empty(&fields));
}

#[test]
fn blank_strings_become_none() {
    assert_eq!(trimmed(Some("  a  ".to_string())).as_deref(), Some("a"));
    assert_eq!(trimmed(Some("   ".to_string())), None);
    assert_eq!(trimmed(None), None);
    assert_eq!(json_if_any::<u8>(&[]), None);
    assert_eq!(json_if_any(&[1u8]), Some(serde_json::json!([1])));
}

#[test]
fn a_tapback_kind_is_its_name_and_an_emoji_keeps_the_emoji() {
    assert_eq!(tapback_kind(Tapback::Loved), ("loved", None));
    assert_eq!(tapback_kind(Tapback::Sticker), ("sticker", None));
    assert_eq!(
        tapback_kind(Tapback::Emoji(Some("🔥"))),
        ("emoji", Some("🔥".to_string()))
    );
}

#[test]
fn a_reply_points_at_its_originator_and_a_tapback_at_its_target() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let mut message = FixtureDb::messages(&session).remove(1);

    assert_eq!(reply_to(&message, None), None);

    message.thread_originator_guid = Some("guid-1".to_string());
    message.thread_originator_part = Some("0/1".to_string());
    assert_eq!(
        reply_to(&message, None),
        Some(ReplyTo {
            guid: Some("guid-1".to_string()),
            part_index: Some(0),
        })
    );

    let tapback = TapbackFields {
        kind: "loved",
        emoji: None,
        action: "add",
        associated_guid: Some("guid-1".to_string()),
        associated_part: Some(0),
    };
    assert_eq!(tapback.message_kind(), "tapback");
    assert_eq!(tapback.text(), "Loved a message");
    assert_eq!(
        reply_to(&message, Some(&tapback)),
        None,
        "a tapback is never a reply"
    );
    assert_eq!(tapback.associated_guid.as_deref(), Some("guid-1"));
}

/// A poll is exported; a vote on it and a row that adds an option are
/// noise Apple's own export skips. Messages writes a vote as reaction
/// type 4000, and an update as a poll balloon that points at another
/// row's poll.
#[test]
fn a_poll_is_kept_and_its_votes_and_updates_are_noise() {
    const POLLS: &str =
        "com.apple.messages.MSMessageExtensionBalloonPlugin:0000000000:com.apple.messages.Polls";
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let base = || FixtureDb::messages(&session).remove(1);

    assert!(!is_poll_noise(&base()), "a plain message");

    let mut poll = base();
    poll.balloon_bundle_id = Some(POLLS.to_string());
    assert!(poll.is_poll());
    assert!(!is_poll_noise(&poll), "the poll itself");

    let mut vote = base();
    vote.associated_message_type = Some(4000);
    vote.associated_message_guid = Some(poll.guid.clone());
    assert!(is_poll_noise(&vote), "a vote");

    let mut update = base();
    update.guid = "guid-update".to_string();
    update.balloon_bundle_id = Some(POLLS.to_string());
    update.associated_message_guid = Some(poll.guid.clone());
    assert!(is_poll_noise(&update), "an added option");
}

/// A tapback row read off the message's own fields: reaction 2000 is a
/// heart added, 3000 a heart removed, and the target guid is the part
/// prefix stripped off `associated_message_guid`.
#[test]
fn a_tapback_row_is_read_off_the_message() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let mut message = FixtureDb::messages(&session).remove(1);
    // Messages stores a target as `p:<part>/<36-character guid>`.
    let target = "0F3A3C1E-9B7D-4E2A-8C6F-1D2E3F4A5B6C";
    message.associated_message_type = Some(2000);
    message.associated_message_guid = Some(format!("p:1/{target}"));
    let Variant::Tapback(_, action, kind) = message.variant() else {
        panic!("a heart tapback");
    };
    let tapback = TapbackFields::from_variant(&message, action, kind);
    assert_eq!((tapback.kind, tapback.action), ("loved", "add"));
    assert_eq!(tapback.associated_guid.as_deref(), Some(target));
    assert_eq!(tapback.associated_part, Some(1));

    message.associated_message_type = Some(1000);
    let Variant::Tapback(_, action, kind) = message.variant() else {
        panic!("a sticker tapback");
    };
    let sticker = TapbackFields::from_variant(&message, action, kind);
    assert_eq!(sticker.message_kind(), "sticker_tapback");
}

/// Every fixture row becomes a record: the photo message is an iMessage
/// carrying one attachment on disk, the reply is outgoing and named by
/// the caller id, and the group message carries the group's roster and
/// title.
#[test]
fn every_fixture_row_builds_a_record() {
    let fixture = FixtureDb::write();
    let session = fixture.session_with_contacts();
    let messages = FixtureDb::messages(&session);

    let (conversation, photo) = build_record(&session, &messages[0]).unwrap();
    assert_eq!(conversation.chat_identifier, FRIEND_PHONE);
    assert_eq!(conversation.conversation_type, "individual");
    assert_eq!(conversation.group_title, None);
    assert_eq!(conversation.participants.len(), 1);
    assert_eq!(
        conversation.participants[0].display_name.as_deref(),
        Some("Sam Example")
    );
    assert_eq!(photo.guid, "guid-1");
    assert!(!photo.outgoing);
    assert_eq!(photo.message_kind, "imessage");
    assert_eq!(photo.service, "iMessage");
    assert_eq!(photo.sender_identity.as_deref(), Some(FRIEND_PHONE));
    assert_eq!(photo.sender_display_name.as_deref(), Some("Sam Example"));
    assert_eq!(photo.owner_identity, OWNER);
    assert_eq!(photo.owner_display_name.as_deref(), Some(OWNER));
    assert_eq!(photo.attachments.len(), 1);
    assert_eq!(
        photo.attachments[0].original_name.as_deref(),
        Some("photo.jpg")
    );
    assert!(matches!(
        &photo.attachments[0].source,
        AttachmentSource::Path { path, size_hint: Some(17) } if path.ends_with("photo.jpg")
    ));
    assert_eq!(
        photo.timestamp_unix_ms, 1_578_307_200_000,
        "2001 + 600,000,000 s"
    );

    let (_, reply) = build_record(&session, &messages[1]).unwrap();
    assert!(reply.outgoing);
    assert_eq!(reply.text, "Nice");
    assert_eq!(reply.sender_identity, None);
    let fields = reply.imessage.expect("the parsed body is one part");
    assert_eq!(
        fields.parts,
        Some(serde_json::json!([{ "index": 0, "kind": "run", "text": "Nice" }]))
    );
    assert_eq!(reply.reply_to, None);
    assert_eq!(fields.tapback_kind, None);
    assert_eq!(fields.app, None);
    assert!(reply.attachments.is_empty());

    let (group, message) = build_record(&session, &messages[2]).unwrap();
    assert_eq!(group.chat_identifier, GROUP_CHAT_IDENTIFIER);
    assert_eq!(group.conversation_type, "group");
    assert_eq!(group.group_title.as_deref(), Some(GROUP_TITLE));
    let mut handles: Vec<_> = group
        .participants
        .iter()
        .map(|p| p.identity.as_str())
        .collect();
    handles.sort_unstable();
    assert_eq!(handles, vec![FRIEND_PHONE, FRIEND_EMAIL]);
    assert_eq!(message.text, "Saturday works");
    assert_eq!(message.sender_display_name.as_deref(), Some("Robin"));
}

/// Apple's `chat.style` says what a chat is: 43 a group, 45 one-to-one.
/// The number of members and the title decide nothing: a named chat
/// with one member is one-to-one, and the unnamed group with one member
/// left is a group.
#[test]
fn a_chat_is_a_group_by_its_style() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let chat = |rowid: i32, identifier: &str, display_name: Option<&str>| Chat {
        rowid,
        chat_identifier: identifier.to_string(),
        service_name: None,
        display_name: display_name.map(str::to_string),
        is_filtered: None,
        is_blackholed: None,
        is_pending_review: None,
    };
    assert_eq!(
        participants_for(&session, &chat(1, FRIEND_PHONE, None)).1,
        "individual"
    );
    assert_eq!(
        participants_for(&session, &chat(1, FRIEND_PHONE, Some("Just us"))).1,
        "individual",
        "a name does not make a group"
    );
    assert_eq!(
        participants_for(&session, &chat(2, GROUP_CHAT_IDENTIFIER, None)).1,
        "group"
    );
    let (members, kind) = participants_for(&session, &chat(4, SHRUNK_GROUP_IDENTIFIER, None));
    assert_eq!(kind, "group", "one member left does not make it one-to-one");
    assert_eq!(handles_of(&members), vec![FRIEND_EMAIL]);
}

/// An older `chat.db` has no `style` column. The identifier decides
/// then: a group id (`chat…`) is a group, an address is one-to-one.
#[test]
fn without_a_style_column_a_group_id_makes_a_group() {
    let fixture = FixtureDb::write();
    rusqlite::Connection::open(&fixture.db_path)
        .unwrap()
        .execute("ALTER TABLE chat DROP COLUMN style", [])
        .unwrap();
    let session = fixture.session();
    let messages = FixtureDb::messages(&session);

    let (shrunk, _) = build_record(&session, &messages[7]).unwrap();
    assert_eq!(shrunk.chat_identifier, SHRUNK_GROUP_IDENTIFIER);
    assert_eq!(shrunk.conversation_type, "group");
    let (direct, _) = build_record(&session, &messages[0]).unwrap();
    assert_eq!(direct.conversation_type, "individual");
}

/// Two handles that share a `person_centric_id` stay two addresses.
/// `imessage-database` joins them into one string for display; none of
/// that string reaches a sender, a participant or a reactor.
#[test]
fn a_phone_and_email_of_one_person_stay_two_addresses() {
    let fixture = FixtureDb::write();
    let mut session = fixture.session_with_contacts();
    let messages = FixtureDb::messages(&session);
    assert_eq!(raw_handle(&session, 1).as_deref(), Some(FRIEND_PHONE));
    assert_eq!(raw_handle(&session, 3).as_deref(), Some(FRIEND_PHONE_EMAIL));

    let (direct, photo) = build_record(&session, &messages[0]).unwrap();
    assert_eq!(handles_of(&direct.participants), vec![FRIEND_PHONE]);
    assert_eq!(photo.sender_identity.as_deref(), Some(FRIEND_PHONE));
    let (_, new_address) = build_record(&session, &messages[6]).unwrap();
    assert_eq!(
        new_address.sender_identity.as_deref(),
        Some(FRIEND_PHONE_EMAIL)
    );

    let mut heart = FixtureDb::messages(&session).remove(6);
    heart.associated_message_type = Some(2000);
    heart.associated_message_guid = Some(format!("p:0/{}", messages[1].guid));
    session.tapbacks.insert(
        messages[1].guid.clone(),
        HashMap::from([(0usize, vec![heart])]),
    );
    let reactions = build_reactions(&session, &messages[1]);
    assert_eq!(
        reactions[0].reactor_identity.as_deref(),
        Some(FRIEND_PHONE_EMAIL)
    );
    assert_eq!(
        reactions[0].reactor_display_name.as_deref(),
        Some("Sam Example")
    );
}

/// A received row with handle 0 has no sender, in a group as in a
/// one-to-one chat, and nothing is named "Me".
#[test]
fn handle_zero_on_a_received_row_is_no_sender() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    assert_eq!(raw_handle(&session, 0), None);
    let messages = FixtureDb::messages(&session);

    let (_, in_group) = build_record(&session, &messages[8]).unwrap();
    assert_eq!(in_group.text, "No sender");
    assert!(!in_group.outgoing);
    assert_eq!(in_group.sender_identity, None);
    assert_eq!(in_group.sender_display_name, None);

    let mut in_direct = FixtureDb::messages(&session).remove(0);
    in_direct.handle_id = Some(0);
    let (_, record) = build_record(&session, &in_direct).unwrap();
    assert_eq!(record.sender_identity, None, "not inferred from the chat");
    assert_eq!(record.sender_display_name, None);
}

/// Apple lists the owner's own handle among a group's members. The
/// owner is never a participant.
#[test]
fn the_owner_is_not_a_participant_of_a_group() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let messages = FixtureDb::messages(&session);
    let (group, _) = build_record(&session, &messages[2]).unwrap();
    assert_eq!(group.chat_identifier, GROUP_CHAT_IDENTIFIER);
    assert_eq!(
        handles_of(&group.participants),
        vec![FRIEND_PHONE, FRIEND_EMAIL]
    );
}

/// A chat with no `chat_handle_join` rows keeps its own identifier, and
/// a one-to-one chat takes the identifier's address as its participant.
/// A message in no chat is orphaned and keeps its sender.
#[test]
fn a_chat_with_no_handle_rows_keeps_its_identifier() {
    let fixture = FixtureDb::write();
    let session = fixture.session_with_contacts();
    let messages = FixtureDb::messages(&session);

    let (chat, record) = build_record(&session, &messages[6]).unwrap();
    assert_eq!(chat.chat_identifier, FRIEND_PHONE_EMAIL);
    assert_eq!(chat.conversation_type, "individual");
    assert_eq!(handles_of(&chat.participants), vec![FRIEND_PHONE_EMAIL]);
    assert_eq!(
        chat.participants[0].display_name.as_deref(),
        Some("Sam Example")
    );
    assert_eq!(record.chat_identifier, FRIEND_PHONE_EMAIL);

    let (lost, record) = build_record(&session, &messages[11]).unwrap();
    assert_eq!(lost.chat_identifier, format!("orphaned:{FRIEND_EMAIL}"));
    assert_eq!(record.sender_identity.as_deref(), Some(FRIEND_EMAIL));

    // A group with no handle rows lists nobody; its sender is still named.
    rusqlite::Connection::open(&fixture.db_path)
        .unwrap()
        .execute("DELETE FROM chat_handle_join WHERE chat_id = 4", [])
        .unwrap();
    let session = fixture.session();
    let (shrunk, record) = build_record(&session, &messages[7]).unwrap();
    assert_eq!(shrunk.chat_identifier, SHRUNK_GROUP_IDENTIFIER);
    assert_eq!(shrunk.conversation_type, "group");
    assert!(shrunk.participants.is_empty());
    assert_eq!(record.sender_identity.as_deref(), Some(FRIEND_EMAIL));
}

/// The owner's chat with the owner's own number keeps that identifier
/// and lists nobody. Its sent and its received row are both exported.
#[test]
fn a_chat_with_the_owners_own_address_has_no_participants() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let messages = FixtureDb::messages(&session);

    let (chat, sent) = build_record(&session, &messages[9]).unwrap();
    assert_eq!(chat.chat_identifier, OWNER);
    assert_eq!(chat.conversation_type, "individual");
    assert!(chat.participants.is_empty(), "{:?}", chat.participants);
    assert!(sent.outgoing);
    let (chat, received) = build_record(&session, &messages[10]).unwrap();
    assert_eq!(chat.chat_identifier, OWNER);
    assert!(chat.participants.is_empty());
    assert!(!received.outgoing);
    assert_eq!(received.text, "Note to self");
}

/// Fields a row carries through to its record: the subject when it has
/// one, the message a reply in a thread answers, and MMS for a
/// non-iMessage row with an attachment.
#[test]
fn a_record_keeps_the_subject_the_reply_link_and_the_mms_kind() {
    let fixture = FixtureDb::write();
    let session = fixture.session();

    let mut messages = FixtureDb::messages(&session);
    let mut reply = messages.remove(1);
    reply.subject = Some("Plans".to_string());
    reply.thread_originator_guid = Some("guid-1".to_string());
    reply.thread_originator_part = Some("1/2".to_string());
    let (_, record) = build_record(&session, &reply).unwrap();
    assert_eq!(record.subject.as_deref(), Some("Plans"));
    assert_eq!(
        record.reply_to,
        Some(ReplyTo {
            guid: Some("guid-1".to_string()),
            part_index: Some(1),
        })
    );

    reply.subject = Some(String::new());
    reply.thread_originator_guid = None;
    let (_, record) = build_record(&session, &reply).unwrap();
    assert_eq!(record.subject, None);
    assert_eq!(record.reply_to, None);

    reply.service = Some("SMS".to_string());
    let (_, record) = build_record(&session, &reply).unwrap();
    assert_eq!(record.message_kind, "sms");

    let mut photo = messages.remove(0);
    photo.service = Some("SMS".to_string());
    let (_, record) = build_record(&session, &photo).unwrap();
    assert_eq!(record.attachments.len(), 1);
    assert_eq!(record.message_kind, "mms");
}

/// The owner's address rides on each row as `destination_caller_id`
/// carries it: the email for a row sent from the email account, bare
/// (the `E:` prefix lives only in `chat.account_login`), and empty for a
/// row Apple wrote with NULL, which is still outgoing and still named
/// `Me` when the caller id option is on.
#[test]
fn the_owner_address_on_a_row_is_the_bare_caller_id_or_nothing() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let messages = FixtureDb::messages(&session);

    let (conversation, from_mac) = build_record(&session, &messages[3]).unwrap();
    assert_eq!(conversation.chat_identifier, FRIEND_EMAIL);
    assert_eq!(conversation.conversation_type, "individual");
    let handles: Vec<_> = conversation
        .participants
        .iter()
        .map(|p| p.identity.as_str())
        .collect();
    assert_eq!(
        handles,
        vec![FRIEND_EMAIL],
        "the owner is not a participant"
    );
    assert!(from_mac.outgoing);
    assert_eq!(from_mac.text, "From my Mac");
    assert_eq!(from_mac.sender_identity, None);
    assert_eq!(from_mac.owner_identity, OWNER_EMAIL);
    assert_eq!(from_mac.owner_display_name.as_deref(), Some(OWNER_EMAIL));

    let (conversation, still_me) = build_record(&session, &messages[4]).unwrap();
    assert_eq!(conversation.chat_identifier, FRIEND_PHONE);
    assert!(still_me.outgoing);
    assert_eq!(still_me.text, "Still me");
    assert_eq!(still_me.sender_identity, None);
    assert_eq!(still_me.owner_identity, "", "NULL comes through as empty");
    assert_eq!(still_me.owner_display_name.as_deref(), Some(ME));
}

/// Some iPhone rows store the caller id as `tel:+1…`. The prefix is
/// Apple's spelling, not part of the address, so it is gone by the time
/// the row crosses the pipe: the owner handle on this row equals the
/// identity the identities request reports for the same phone (#686).
#[test]
fn a_tel_prefixed_caller_id_crosses_the_pipe_bare() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let messages = FixtureDb::messages(&session);

    let (conversation, from_the_car) = build_record(&session, &messages[5]).unwrap();
    assert_eq!(conversation.chat_identifier, FRIEND_PHONE);
    assert!(from_the_car.outgoing);
    assert_eq!(from_the_car.text, "From the car");
    assert_eq!(from_the_car.owner_identity, OWNER);
    assert_eq!(from_the_car.owner_display_name.as_deref(), Some(OWNER));
}

/// The fixture's photo message was read a minute after it arrived, so
/// it carries that stamp; the outgoing "Nice" row has `date_read` NULL
/// and must carry no receipt rather than Apple's epoch (issue #630).
#[test]
fn an_unread_message_has_no_read_receipt_and_a_read_one_has_the_stamp() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let messages = FixtureDb::messages(&session);

    let (_, photo) = build_record(&session, &messages[0]).unwrap();
    let fields = photo.imessage.expect("the photo row has Apple fields");
    let receipt = fields
        .read_receipt_rfc3339
        .expect("a read message carries its receipt");
    let read_at = chrono::DateTime::parse_from_rfc3339(&receipt).unwrap();
    assert_eq!(
        read_at.timestamp_millis(),
        1_578_307_260_000,
        "2001 + 600,000,060 s"
    );

    let (_, reply) = build_record(&session, &messages[1]).unwrap();
    assert_eq!(reply.imessage.unwrap().read_receipt_rfc3339, None);
}

/// Orphaned messages sit in one conversation for each person, with that
/// person as its only participant, and sent rows that name nobody in one
/// with nobody. Each is keyed `orphaned:` and the person's address, so
/// the person's one-to-one conversation stays as it is (#1095).
#[test]
fn orphaned_rows_make_one_conversation_for_each_person() {
    let fixture = FixtureDb::write();
    let session = fixture.session_with_contacts();
    let orphaned = |index: usize| {
        let mut message = FixtureDb::messages(&session).remove(index);
        message.chat_id = Some(42);
        message.deleted_from = None;
        resolve_context(&session, &message).conversation
    };

    let sam = orphaned(0);
    assert_eq!(sam.chat_identifier, format!("orphaned:{FRIEND_PHONE}"));
    assert_eq!(sam.conversation_type, "orphaned");
    assert_eq!(handles_of(&sam.participants), vec![FRIEND_PHONE]);
    assert_eq!(
        sam.participants[0].display_name.as_deref(),
        Some("Sam Example")
    );

    let robin = orphaned(2);
    assert_eq!(robin.chat_identifier, format!("orphaned:{FRIEND_EMAIL}"));
    assert_eq!(robin.conversation_type, "orphaned");
    assert_eq!(handles_of(&robin.participants), vec![FRIEND_EMAIL]);

    for mine in [1, 3] {
        let holder = orphaned(mine);
        assert_eq!(holder.chat_identifier, "orphaned:");
        assert_eq!(holder.conversation_type, "orphaned");
        assert!(holder.participants.is_empty());
    }

    let (one_to_one, _) = build_record(&session, &FixtureDb::messages(&session)[0]).unwrap();
    assert_eq!(one_to_one.chat_identifier, FRIEND_PHONE);
    assert_eq!(one_to_one.conversation_type, "individual");
}

/// An orphaned row that names the other person sits in that person's
/// orphaned conversation whichever way it went: a sent row names its
/// recipient in `handle_id`, as Apple writes a one-to-one chat's sent
/// rows. A row that names nobody, sent with `handle_id` 0, received with
/// no sender, or received from one of the holder's own addresses, sits
/// in the one keyed `orphaned:` alone, with no participants (#1778).
#[test]
fn orphaned_rows_sit_with_the_person_they_name_in_either_direction() {
    let fixture = FixtureDb::write();
    let session = fixture.session_with_contacts();
    let orphaned = |index: usize, handle_id: Option<i32>| {
        let mut message = FixtureDb::messages(&session).remove(index);
        message.chat_id = Some(42);
        message.deleted_from = None;
        if let Some(handle_id) = handle_id {
            message.handle_id = Some(handle_id);
        }
        resolve_context(&session, &message)
    };

    // Message 2 is sent; handle 2 is Robin at FRIEND_EMAIL. Reading the
    // handle places the row with Robin without making Robin its sender.
    let sent = orphaned(1, Some(2));
    assert!(sent.is_from_me);
    assert_eq!(sent.sender_identity, None);
    assert_eq!(sent.sender_display_name, None);
    let sent_to_robin = sent.conversation;
    // Message 3 is received from Robin.
    let from_robin = orphaned(2, None).conversation;
    for robin in [&sent_to_robin, &from_robin] {
        assert_eq!(robin.chat_identifier, format!("orphaned:{FRIEND_EMAIL}"));
        assert_eq!(robin.conversation_type, "orphaned");
        assert_eq!(handles_of(&robin.participants), vec![FRIEND_EMAIL]);
        assert_eq!(robin.participants[0].display_name.as_deref(), Some("Robin"));
    }

    // Message 2 sent with handle 0, message 9 received with handle 0,
    // and message 11 received from the owner's own handle.
    for nobody in [orphaned(1, None), orphaned(8, None), orphaned(10, None)]
        .map(|context| context.conversation)
    {
        assert_eq!(nobody.chat_identifier, "orphaned:");
        assert_eq!(nobody.conversation_type, "orphaned");
        assert!(nobody.participants.is_empty());
    }
}

/// A row whose chat is gone lands in the orphaned conversation of the
/// person it names, and one whose service is unknown is SMS or MMS by
/// its attachments.
#[test]
fn an_orphaned_row_and_a_non_imessage_row_are_classified() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let mut message = FixtureDb::messages(&session).remove(1);
    message.chat_id = Some(42);
    message.service = Some("SMS".to_string());

    let context = resolve_context(&session, &message);
    assert_eq!(context.conversation.chat_identifier, "orphaned:");
    assert!(context.conversation.participants.is_empty());
    assert_eq!(context.service, "SMS");

    assert_eq!(
        classify_row(&session, &message, "SMS", false, None).kind,
        "sms"
    );
    assert_eq!(
        classify_row(&session, &message, "SMS", true, None).kind,
        "mms"
    );
    assert_eq!(
        classify_row(&session, &message, "iMessage", true, None).kind,
        "imessage"
    );
}

/// The classifier's other branches over a fixture row with its fields
/// changed: a rename announcement, a location share, a balloon and a
/// send effect.
#[test]
fn announcements_locations_balloons_and_effects_are_classified() {
    let fixture = FixtureDb::write();
    let session = fixture.session_with_contacts();
    let base = FixtureDb::messages(&session).remove(2);

    let mut rename = FixtureDb::messages(&session).remove(2);
    rename.item_type = 2;
    rename.group_title = Some("New name".to_string());
    assert!(rename.is_announcement());
    let row = classify_row(&session, &rename, "iMessage", false, None);
    assert_eq!(row.kind, "announcement");
    assert_eq!(row.text, "Robin named the conversation New name");
    assert_eq!(
        announcement_text(&session, &rename).as_deref(),
        Some("Robin named the conversation New name")
    );

    let mut added = FixtureDb::messages(&session).remove(2);
    added.item_type = 1;
    added.group_action_type = 0;
    added.other_handle = Some(1);
    added.is_from_me = true;
    // The owner is named by the caller id when that option is on; only
    // a bare `Me` becomes `You`.
    assert_eq!(
        announcement_text(&session, &added).as_deref(),
        Some("+15555550106 added Sam Example to the conversation.")
    );
    added.destination_caller_id = None;
    assert_eq!(
        announcement_text(&session, &added).as_deref(),
        Some("You added Sam Example to the conversation.")
    );

    let mut location = FixtureDb::messages(&session).remove(2);
    location.item_type = 4;
    location.share_status = false;
    location.share_direction = Some(true);
    location.text = None;
    let row = classify_row(&session, &location, "iMessage", false, None);
    assert_eq!(row.kind, "location_share");
    assert!(row.text.starts_with("Shared location "), "{}", row.text);
    assert!(row.shared_location.is_some());

    let mut balloon = FixtureDb::messages(&session).remove(2);
    balloon.balloon_bundle_id =
        Some("com.apple.PassbookUIService.PeerPaymentMessagesExtension".to_string());
    let row = classify_row(&session, &balloon, "iMessage", false, None);
    assert_eq!(row.kind, "balloon");
    assert_eq!(row.text, "Saturday works");
    assert_eq!(
        row.app.as_ref().and_then(balloon_kind_label).as_deref(),
        Some("apple_pay")
    );

    let mut slam = base;
    slam.expressive_send_style_id = Some("com.apple.MobileSMS.expressivesend.impact".to_string());
    let row = classify_row(&session, &slam, "iMessage", false, None);
    assert_eq!(row.text, "Saturday works\n\nSent with Slam");
    assert_eq!(row.send_effect.as_deref(), Some("Sent with Slam"));
    let fields = imessage_fields(&session, &slam, row, &[]);
    assert_eq!(fields.send_effect.as_deref(), Some("Sent with Slam"));
    assert!(!is_empty(&fields));
}

/// Reactions on a parent are listed in part, date and rowid order, with
/// the reactor named and whether the owner reacted; a removed tapback is
/// left out.
#[test]
fn a_parents_reactions_are_listed_in_order_and_named() {
    let fixture = FixtureDb::write();
    let mut session = fixture.session_with_contacts();
    let messages = FixtureDb::messages(&session);
    let parent = &messages[1];

    let mut heart = FixtureDb::messages(&session).remove(0);
    heart.associated_message_type = Some(2000);
    heart.associated_message_guid = Some(format!("p:0/{}", parent.guid));
    heart.date = 20;
    let mut fire = FixtureDb::messages(&session).remove(2);
    fire.associated_message_type = Some(2006);
    fire.associated_message_emoji = Some("🔥".to_string());
    fire.associated_message_guid = Some(format!("p:0/{}", parent.guid));
    fire.date = 10;
    fire.is_from_me = true;
    let mut removed = FixtureDb::messages(&session).remove(0);
    removed.associated_message_type = Some(3000);
    removed.associated_message_guid = Some(format!("p:0/{}", parent.guid));
    session.tapbacks.insert(
        parent.guid.clone(),
        HashMap::from([(0usize, vec![heart, fire, removed])]),
    );

    assert_eq!(
        build_reactions(&session, parent),
        [
            Reaction {
                part_index: 0,
                kind: "emoji".into(),
                emoji: Some("🔥".into()),
                is_from_me: true,
                reactor_identity: None,
                reactor_display_name: Some(OWNER.into()),
            },
            Reaction {
                part_index: 0,
                kind: "loved".into(),
                emoji: None,
                is_from_me: false,
                reactor_identity: Some(FRIEND_PHONE.into()),
                reactor_display_name: Some("Sam Example".into()),
            },
        ]
    );

    assert!(build_reactions(&session, &messages[0]).is_empty());
}

/// A message the owner deleted in Messages is Deleted in the source app,
/// in its chat and with the text the database still holds. A message
/// unsent whole is Unsent, with no text, and is a message rather than an
/// announcement. A message only partly unsent keeps the text left and
/// carries no mark.
#[test]
fn a_deleted_and_an_unsent_message_carry_their_mark_and_a_partly_unsent_one_none() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let messages = FixtureDb::messages(&session);
    let record = |guid: &str| {
        let message = messages.iter().find(|m| m.guid == guid).unwrap();
        build_record(&session, message).unwrap().1
    };

    let deleted = record(DELETED_GUID);
    assert_eq!(deleted.deletion, Some(Deletion::DeletedInSourceApp));
    assert_eq!(deleted.text, DELETED_TEXT);
    assert_eq!(deleted.chat_identifier, FRIEND_PHONE);

    let unsent = record(UNSENT_GUID);
    assert_eq!(unsent.deletion, Some(Deletion::Unsent));
    assert_eq!(unsent.text, "");
    assert_eq!(unsent.message_kind, "imessage");
    assert!(
        unsent
            .imessage
            .as_ref()
            .is_none_or(|im| im.announcement.is_none()),
        "an unsent message is not an announcement"
    );

    let partly = record(PARTLY_UNSENT_GUID);
    assert_eq!(partly.deletion, None);
    assert_eq!(partly.text, PARTLY_UNSENT_TEXT);
}

/// A row whose every part was unsent is Unsent even when attachment rows
/// are still joined to it, and is not the "unsent a message"
/// announcement either, so the fact it was unsent is never lost. A row
/// only partly unsent with an attachment left, and no text, is not
/// marked: the attachment is what is left.
#[test]
fn a_fully_unsent_row_is_unsent_whatever_attachment_rows_remain() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let by_guid = |guid: &str| {
        FixtureDb::messages(&session)
            .into_iter()
            .find(|m| m.guid == guid)
            .unwrap()
    };

    let unsent = by_guid(UNSENT_GUID);
    let mark = deletion(&unsent, true);
    assert_eq!(mark, Some(Deletion::Unsent));
    let row = classify_row(&session, &unsent, "iMessage", true, mark);
    assert_eq!(row.kind, "imessage");
    assert_eq!(row.announcement, None);

    let mut partly = by_guid(PARTLY_UNSENT_GUID);
    partly.text = None;
    assert_eq!(deletion(&partly, true), None);
    assert_eq!(deletion(&partly, false), Some(Deletion::Unsent));
}

/// The whole stream over the fixture: eighteen rows seen, none skipped.
/// Each conversation is announced once, before its first message, and
/// the stream ends with the full parse count and the done event. The
/// chat with no handle rows is its own conversation, apart from the
/// message in no chat.
#[test]
fn the_fixture_streams_without_a_failure() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let events = crate::log::capture::events(|| stream_export(&session).unwrap());

    let lines: Vec<String> = events
        .iter()
        .map(|event| match event["event"].as_str().unwrap() {
            "conversation" => format!("conversation {}", event["chat_identifier"]),
            "message" => format!("message {} in {}", event["guid"], event["chat_identifier"]),
            other => other.to_string(),
        })
        .collect();
    assert_eq!(
        lines,
        [
            r#"conversation "+15555550107""#,
            r#"message "guid-1" in "+15555550107""#,
            r#"message "guid-2" in "+15555550107""#,
            r#"conversation "chat100""#,
            r#"message "guid-3" in "chat100""#,
            r#"conversation "friend@example.com""#,
            r#"message "guid-4" in "friend@example.com""#,
            r#"message "guid-5" in "+15555550107""#,
            r#"message "guid-6" in "+15555550107""#,
            r#"conversation "sam@example.com""#,
            r#"message "guid-7" in "sam@example.com""#,
            r#"conversation "chat200""#,
            r#"message "guid-8" in "chat200""#,
            r#"message "guid-9" in "chat100""#,
            r#"conversation "+15555550106""#,
            r#"message "guid-10" in "+15555550106""#,
            r#"message "guid-11" in "+15555550106""#,
            r#"conversation "orphaned:friend@example.com""#,
            r#"message "guid-12" in "orphaned:friend@example.com""#,
            r#"message "00000000-0000-4000-8000-000000000013" in "chat100""#,
            r#"message "guid-14" in "chat100""#,
            r#"message "guid-15" in "chat100""#,
            r#"message "guid-16" in "+15555550107""#,
            r#"message "guid-17" in "+15555550107""#,
            r#"message "guid-18" in "+15555550107""#,
            "progress",
            "export_done",
        ]
    );
    assert_eq!(
        events[25],
        serde_json::json!({"event": "progress", "stage": "parse", "done": 18, "total": 18})
    );
    assert_eq!(
        events[26],
        serde_json::json!({"event": "export_done", "messages_seen": 18, "failures": 0})
    );
}

/// A seconds stamp from an older database reads the same time as a
/// nanoseconds one, and says it is whole seconds (#1970).
#[test]
fn a_seconds_stamp_from_an_older_database_reads_the_same_as_a_nanoseconds_one() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let mut message = FixtureDb::messages(&session).remove(1);
    assert_eq!(
        timestamp_unix_ms(&message, session.offset),
        (1_578_307_260_000, TimePrecision::Milliseconds)
    );
    message.date = 600_000_060;
    assert_eq!(
        timestamp_unix_ms(&message, session.offset),
        (1_578_307_260_000, TimePrecision::Seconds),
        "a seconds stamp from an older database reads the same, in whole seconds"
    );
}

/// A nanosecond stamp keeps its milliseconds. `Message::date` cuts the
/// time to the second, so read through it every Apple Messages time
/// ended in `.000` while it said `milliseconds`.
#[test]
fn a_nanoseconds_stamp_keeps_its_milliseconds() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let mut message = FixtureDb::messages(&session).remove(1);
    message.date = chat_db_fixture::apple_nanos(600_000_060) + 123_456_789;
    assert_eq!(
        timestamp_unix_ms(&message, session.offset),
        (1_578_307_260_123, TimePrecision::Milliseconds)
    );
}

#[test]
fn a_timestamp_falls_back_to_the_raw_stamp_when_the_date_is_invalid() {
    let fixture = FixtureDb::write();
    let session = fixture.session();
    let mut message = FixtureDb::messages(&session).remove(1);
    // Ten trillion seconds before 2001 is outside the range chrono can
    // hold, so `Message::date` fails and the raw stamp is read instead.
    message.date = -10_000_000_000_000;
    assert!(message.date(session.offset).is_err());
    assert_eq!(
        timestamp_unix_ms(&message, session.offset),
        (
            (-10_000_000_000_000 + session.offset) * 1000,
            TimePrecision::Seconds
        ),
        "a time read from the raw stamp is whole seconds"
    );
}
