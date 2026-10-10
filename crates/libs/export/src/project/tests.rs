use super::*;
use message_crate_api_types::{MessageConversation, Participant, Tapback};

/// One page of `GET /v1/exports/{id}/messages` exactly as the server serializes
/// it: `service` on the message rather than on the conversation, an
/// attachment with no byte length, and a participant the source named
/// without recording an address, whose `identity` and `service` are `null`.
///
/// This is a string literal on purpose. Every other fixture in this module
/// builds the `message_crate_api_types` shapes in Rust, which is what let
/// three of them drift away from what the server sends without the
/// compiler or the suite noticing: `handle: String` rejected `"handle": null` and aborted every
/// Export Run of a conversation holding an address-less participant, and
/// `conversation.service` read a field the server has never sent, so every
/// exported message came out `IrService::Unknown`.
const EXPORT_PAGE_JSON: &str = r#"{
      "items": [
        {
          "id": 4021,
          "source": "imessage",
          "service": "iMessage",
          "guid": "3A9E-0001",
          "timestamp": "2015-03-12T18:05:22Z",
          "time_precision": "milliseconds",
          "sort_order": 0,
          "is_from_me": false,
          "sender": "+15555550100",
          "subject": null,
          "text": "dinner at seven?",
          "is_announcement": false,
          "reply_to": null,
          "reply_count": 0,
          "conversation": {
            "id": 9,
            "chat_identifier": "chat9000",
            "conversation_type": "group",
            "is_group": true,
            "group_title": "Book Club",
            "participants": [
              { "name": "Robert Smith", "identity": "+15555550100", "service": "imessage", "contact_id": 3 },
              { "name": "Sarah Vale", "identity": null, "service": null, "contact_id": 7 },
              { "name": "+15555550135", "identity": "+15555550135", "service": "imessage" }
            ]
          },
          "attachments": [
            {
              "path": "attachments/ab",
              "original_name": "menu.pdf",
              "mime_type": "application/pdf",
              "sha256": "ab",
              "is_sticker": false, "shown_as_is": false,
              "transcription": null
            }
          ],
          "tapbacks": [
            { "part_index": 0, "kind": "loved", "is_from_me": true }
          ],
          "earlier_versions": [],
          "matched_earlier_version": false
        }
      ],
      "total": 1,
      "limit": 500,
      "offset": 0
    }"#;

/// The whole page parses, an address-less participant survives it, and the
/// service the server sent reaches the IR message.
#[test]
fn a_real_export_page_parses_with_an_address_less_participant() {
    let page: message_crate_api_types::Page<message_crate_api_types::Message> =
        serde_json::from_str(EXPORT_PAGE_JSON).expect("the server's own page shape must parse");
    assert_eq!((page.items.len(), page.total), (1, 1));

    let participants = participants_from_seed(&page.items[0]);
    assert_eq!(participants.len(), 3);
    assert_eq!(participants[0].identity.as_deref(), Some("+15555550100"));
    assert_eq!(
        participants[0].display_name.as_deref(),
        Some("Robert Smith")
    );
    // No address at all: the name is all the server has for this person, so
    // it carries through as their display name.
    assert_eq!(participants[1].identity, None);
    assert_eq!(participants[1].display_name.as_deref(), Some("Sarah Vale"));
    // A name that is only the handle is still not a display name.
    assert_eq!(participants[2].identity.as_deref(), Some("+15555550135"));
    assert_eq!(participants[2].display_name, None);
}

/// The service rides on the message, so the IR message gets iMessage and
/// the kind that follows from it — not `Unknown`/`Mms`, which is what
/// reading `conversation.service` produced (issue #324).
#[test]
fn the_message_service_round_trips_from_a_real_export_page() {
    let page: message_crate_api_types::Page<message_crate_api_types::Message> =
        serde_json::from_str(EXPORT_PAGE_JSON).unwrap();
    assert_eq!(page.items[0].service.as_deref(), Some("iMessage"));

    let ir = to_ir_message(&page.items[0], false).unwrap();
    assert_eq!(ir.service, IrService::IMessage);
    assert_eq!(ir.message_kind, IrMessageKind::IMessage);
    // The attachment maps without a byte length: the server never sends one.
    assert_eq!(ir.attachments.len(), 1);
    assert_eq!(ir.attachments[0].size_bytes, None);
}

/// A conversation the holder used at two addresses keeps the split: each
/// message carries its own owner, and the header names none, because no
/// one address is the owner of every message (#1098).
#[test]
fn each_message_keeps_its_owner_and_a_split_conversation_names_none_in_the_header() {
    let mut by_phone = seed_message_with_participant(Participant {
        identity: Some("+15555550101".into()),
        name: "Sam".into(),
        service: None,
        contact_id: None,
    });
    by_phone.owner = Some("+15555550100".into());
    let mut by_email = by_phone.clone();
    by_email.owner = Some("me@example.com".into());
    let mut blank = by_phone.clone();
    blank.owner = Some("  ".into());

    let messages = vec![
        to_ir_message(&by_phone, false).unwrap(),
        to_ir_message(&by_email, false).unwrap(),
        to_ir_message(&blank, false).unwrap(),
    ];
    assert_eq!(
        messages
            .iter()
            .map(|m| m.owner_identity.as_deref())
            .collect::<Vec<_>>(),
        [Some("+15555550100"), Some("me@example.com"), None]
    );

    let doc = build_document("imessage", &by_phone, messages);
    assert_eq!(doc.export.owner_identity, None);
}

/// RFC 3339, whole seconds and milliseconds all land on the same instant,
/// and a number at the cut-off (10^10) is read as milliseconds.
#[test]
fn parses_each_timestamp_form_to_the_exact_millisecond() {
    const MS: i64 = 1_426_183_522_000;
    assert_eq!(parse_timestamp_unix_ms("2015-03-12T18:05:22Z").unwrap(), MS);
    assert_eq!(
        parse_timestamp_unix_ms("2015-03-12T19:05:22.250+01:00").unwrap(),
        MS + 250
    );
    assert_eq!(parse_timestamp_unix_ms("1426183522").unwrap(), MS);
    assert_eq!(parse_timestamp_unix_ms("1426183522250").unwrap(), MS + 250);
    assert_eq!(
        parse_timestamp_unix_ms("9999999999").unwrap(),
        9_999_999_999_000
    );
    assert_eq!(
        parse_timestamp_unix_ms("10000000000").unwrap(),
        10_000_000_000
    );
}

/// The message a reply quotes comes through the Export Run into the message's
/// `reply_to`, and the stored reactions into the message's `reactions`, each
/// under the person who reacted.
#[test]
fn a_reply_with_reactions_keeps_its_reply_to_and_reactions() {
    let mut msg = seed_message_with_participant(sam());
    msg.timestamp = "1426183522250".into();
    msg.reply_to = Some(message_crate_api_types::ReplyTo {
        guid: Some("origin-guid".into()),
        part_index: Some(2),
    });
    msg.tapbacks = vec![
        Tapback {
            part_index: 0,
            kind: "loved".into(),
            emoji: None,
            is_from_me: true,
            sender: None,
        },
        Tapback {
            part_index: 1,
            kind: "emoji".into(),
            emoji: Some("🎉".into()),
            is_from_me: false,
            sender: Some("+2".into()),
        },
    ];

    let ir = to_ir_message(&msg, false).unwrap();

    assert_eq!(ir.timestamp_unix_ms, 1_426_183_522_250);
    assert_eq!(
        ir.reply_to,
        Some(ReplyTo {
            guid: Some("origin-guid".into()),
            part_index: Some(2),
        })
    );
    assert!(
        ir.imessage.is_none(),
        "a reply alone makes no iMessage block"
    );
    assert_eq!(
        ir.reactions,
        [
            Reaction {
                part_index: 0,
                kind: "loved".into(),
                emoji: None,
                is_from_me: true,
                reactor_identity: None,
                reactor_display_name: None,
            },
            Reaction {
                part_index: 1,
                kind: "emoji".into(),
                emoji: Some("🎉".into()),
                is_from_me: false,
                reactor_identity: Some("+2".into()),
                reactor_display_name: None,
            },
        ]
    );
}

/// An SMS that carries a file is an MMS, and the document counts every
/// attachment across its messages.
#[test]
fn an_sms_with_a_file_is_an_mms_and_counts_toward_the_document() {
    let mut sms = seed_message_with_participant(sam());
    sms.service = Some("SMS".into());
    let plain = to_ir_message(&sms, false).unwrap();
    assert_eq!(plain.message_kind, IrMessageKind::Sms);

    let file: Attachment = serde_json::from_value(
        json!({ "sha256": "ab", "is_sticker": false, "shown_as_is": false }),
    )
    .unwrap();
    sms.attachments = vec![file.clone(), file.clone(), file];
    let with_files = to_ir_message(&sms, false).unwrap();
    assert_eq!(with_files.message_kind, IrMessageKind::Mms);

    let doc = build_document("sms", &sms, vec![with_files, plain]);
    assert_eq!(doc.conversation.stats.attachment_count, 3);
    assert_eq!(doc.conversation.stats.message_count, 2);
}

/// The file says the backup its messages came from when they all came
/// from one, and nothing when any two differ or none says.
#[test]
fn a_document_says_its_backup_only_when_every_message_came_from_it() {
    let backup_of = |seed: &Message| {
        build_document("imessage", seed, vec![])
            .export
            .backup_taken_at_unix_ms
    };
    let dated = |at: Option<&str>| {
        let mut msg = seed_message_with_participant(sam());
        msg.backup_taken_at = at.map(str::to_string);
        msg
    };
    let later = Some("2026-09-30T18:45:12.000Z");
    let earlier = Some("2026-09-01T10:00:00.000Z");

    assert_eq!(backup_of(&dated(None)), None);

    let mut seed = dated(later);
    common_backup_taken_at(&mut seed, &dated(later));
    assert_eq!(backup_of(&seed), Some(1_790_793_912_000));

    let mut seed = dated(earlier);
    common_backup_taken_at(&mut seed, &dated(later));
    common_backup_taken_at(&mut seed, &dated(None));
    assert_eq!(backup_of(&seed), None, "two backups: no one date is right");

    let mut seed = dated(later);
    common_backup_taken_at(&mut seed, &dated(None));
    common_backup_taken_at(&mut seed, &dated(later));
    assert_eq!(
        backup_of(&seed),
        None,
        "a message without a date stays without one"
    );
}

/// Whether the conversation is a group comes from the server's
/// `is_group`, never from reading `conversation_type` again.
#[test]
fn a_document_is_a_group_when_the_server_says_so() {
    let mut seed = seed_message_with_participant(sam());
    seed.conversation.conversation_type = " group ".into();
    seed.conversation.is_group = false;
    let doc = build_document("imessage", &seed, vec![]);
    assert_eq!(
        doc.conversation.conversation_type,
        IrConversationType::Individual
    );

    seed.conversation.conversation_type = "individual".into();
    seed.conversation.is_group = true;
    let doc = build_document("imessage", &seed, vec![]);
    assert_eq!(
        doc.conversation.conversation_type,
        IrConversationType::Group
    );
}

/// A conversation of orphaned messages is exported as one, so importing
/// the exported file again does not make its key a person (#1095).
#[test]
fn a_document_of_orphaned_messages_stays_orphaned() {
    let mut seed = seed_message_with_participant(Participant {
        identity: Some("+15555550154".into()),
        name: "Ada".into(),
        service: None,
        contact_id: None,
    });
    seed.conversation.chat_identifier = "orphaned:+15555550154".into();
    seed.conversation.conversation_type = "orphaned".into();
    let doc = build_document("imessage", &seed, vec![]);
    assert_eq!(
        doc.conversation.conversation_type,
        IrConversationType::Orphaned
    );
}

/// An announcement keeps its text; one with no text carries nothing.
#[test]
fn an_announcement_keeps_its_text() {
    let mut msg = seed_message_with_participant(sam());
    msg.is_announcement = true;
    msg.text = Some("Sam named the conversation \"Book Club\"".into());

    let ir = to_ir_message(&msg, false).unwrap();

    assert_eq!(ir.message_kind, IrMessageKind::Announcement);
    let imessage = ir
        .imessage
        .expect("an announcement makes an iMessage block");
    assert_eq!(
        imessage.announcement.as_deref(),
        Some("Sam named the conversation \"Book Club\"")
    );
    assert!(ir.reactions.is_empty());

    msg.text = None;
    let ir = to_ir_message(&msg, false).unwrap();
    assert!(ir.imessage.is_none());
}

/// The conversation file says the precision the server stored, so a
/// whole-second message is written back as whole seconds and a
/// millisecond time that ends in `.000` as milliseconds.
#[test]
fn a_message_keeps_the_precision_the_server_stored() {
    let mut msg = seed_message_with_participant(sam());
    msg.timestamp = "2015-03-12T18:05:22.000Z".into();
    for (stored, written) in [
        (
            message_crate_api_types::TimePrecision::Seconds,
            TimePrecision::Seconds,
        ),
        (
            message_crate_api_types::TimePrecision::Milliseconds,
            TimePrecision::Milliseconds,
        ),
    ] {
        msg.time_precision = stored;
        let ir = to_ir_message(&msg, false).unwrap();
        assert_eq!(ir.timestamp_unix_ms, 1_426_183_522_000);
        assert_eq!(ir.time_precision, written);
    }
}

#[test]
fn maps_basic_message() {
    let mut msg = seed_message_with_participant(sam());
    msg.guid = "basic-guid".into();
    msg.text = Some("hello".into());
    msg.service = Some("iMessage".into());
    let ir = to_ir_message(&msg, false).unwrap();
    assert_eq!(ir.guid, "basic-guid");
    assert_eq!(ir.text, "hello");
    assert_eq!(ir.service, IrService::IMessage);
}

/// The server's mark and the conversation file's are two types, one per
/// crate, so each mark of either must have a counterpart in the other
/// spelled the same, or a new one would be dropped on the way through.
#[test]
fn every_mark_has_a_counterpart_spelled_the_same() {
    for mark in Deletion::ALL {
        let api = message_crate_api_types::Deletion::parse(mark.as_str())
            .unwrap_or_else(|| panic!("the server has no mark {:?}", mark.as_str()));
        assert_eq!(deletion_from_api(api), mark);
    }
    for api in message_crate_api_types::Deletion::ALL {
        assert_eq!(deletion_from_api(api).as_str(), api.as_str());
    }
}

/// Each mark the server returns is written on the exported message as
/// the same mark, and a message with none carries none.
#[test]
fn a_deletion_mark_is_exported_as_the_same_mark() {
    let mut msg = seed_message_with_participant(sam());
    assert_eq!(to_ir_message(&msg, false).unwrap().deletion, None);
    for (api, ir) in [
        (
            message_crate_api_types::Deletion::DeletedInSourceApp,
            message_ir::Deletion::DeletedInSourceApp,
        ),
        (
            message_crate_api_types::Deletion::Unsent,
            message_ir::Deletion::Unsent,
        ),
    ] {
        msg.deletion = Some(api);
        assert_eq!(to_ir_message(&msg, false).unwrap().deletion, Some(ir));
    }
}

/// Each earlier version the server returns is written on the exported
/// message with its part, text and time, in the order the server gave.
#[test]
fn earlier_versions_are_exported_with_their_times() {
    let mut msg = seed_message_with_participant(sam());
    assert!(to_ir_message(&msg, false).unwrap().edits.is_empty());
    msg.earlier_versions = vec![
        message_crate_api_types::EarlierVersion {
            part_index: 0,
            text: Some("helo".into()),
            edited_at: Some("2015-03-12T18:05:22Z".into()),
            matched: true,
        },
        message_crate_api_types::EarlierVersion {
            part_index: 1,
            text: Some("second".into()),
            edited_at: None,
            matched: false,
        },
        // A version with no text, as iMazing records an edit, keeps its
        // part and time and no text.
        message_crate_api_types::EarlierVersion {
            part_index: 0,
            text: None,
            edited_at: Some("2015-03-12T18:06:22Z".into()),
            matched: false,
        },
    ];
    assert_eq!(
        to_ir_message(&msg, false).unwrap().edits,
        vec![
            EarlierVersion {
                part_index: 0,
                text: Some("helo".into()),
                edited_at_unix_ms: Some(1_426_183_522_000),
            },
            EarlierVersion {
                part_index: 1,
                text: Some("second".into()),
                edited_at_unix_ms: None,
            },
            EarlierVersion {
                part_index: 0,
                text: None,
                edited_at_unix_ms: Some(1_426_183_582_000),
            },
        ]
    );
}

/// A participant `name` distinct from the handle carries through as the
/// IR participant's display name.
#[test]
fn participants_from_seed_carries_a_real_name() {
    let seed = seed_message_with_participant(sam());
    let participants = participants_from_seed(&seed);
    assert_eq!(participants[0].identity.as_deref(), Some("+1"));
    assert_eq!(participants[0].display_name.as_deref(), Some("Sam"));
}

/// When the server has nothing to name the person, `name` falls back to
/// the handle (ADR-0006). That must not become a display name here — see
/// the comment on `participants_from_seed` for why.
#[test]
fn participants_from_seed_drops_a_name_that_is_just_the_handle() {
    let seed = seed_message_with_participant(Participant {
        identity: Some("+1".into()),
        name: "+1".into(),
        service: None,
        contact_id: None,
    });
    let participants = participants_from_seed(&seed);
    assert_eq!(participants[0].display_name, None);
}

/// Sam at `+1`, the participant most of these tests seed with.
fn sam() -> Participant {
    Participant {
        identity: Some("+1".into()),
        name: "Sam".into(),
        service: None,
        contact_id: None,
    }
}

/// A minimal `Message` carrying exactly one conversation participant.
fn seed_message_with_participant(participant: Participant) -> Message {
    Message {
        id: 1,
        source: "imessage".into(),
        service: Some("iMessage".into()),
        guid: "g1".into(),
        timestamp: "2015-03-12T18:05:22Z".into(),
        time_precision: message_crate_api_types::TimePrecision::Milliseconds,
        is_from_me: false,
        sender: Some("+1".into()),
        owner: None,
        subject: None,
        text: Some("hi".into()),
        is_announcement: false,
        reply_to: None,
        reply_count: 0,
        sort_order: 0,
        conversation: MessageConversation {
            id: 9,
            chat_identifier: "+1".into(),
            conversation_type: "individual".into(),
            is_group: false,
            group_title: None,
            shown_title: None,
            participants: vec![participant],
        },
        attachments: vec![],
        tapbacks: vec![],
        deletion: None,
        earlier_versions: Vec::new(),
        backup_taken_at: None,
        matched_earlier_version: false,
    }
}
