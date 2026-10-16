use super::*;
use message_crate_core::{CancelFlag, LogSink, ProgressSink};
use serde_json::{Map, json};
use staging::AttachmentSpool;
use std::fs;
use std::sync::LazyLock;

const OWNER: &str = "+15555550100";

/// Write `documents` with a session, then read the backup back with the SBR reader.
fn round_trip(documents: &[ConversationDocument]) -> Vec<ConversationDocument> {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = SbrBackupSession::create(tmp.path()).unwrap();
    for document in documents {
        session.append_document(document).unwrap();
    }
    let path = session.finish().unwrap();
    let owners = [OWNER.to_string()];
    let (read, report) = read_with_bytes(&path, &owners);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    read
}

/// Read `path` with the SBR reader, then put each attachment's bytes back
/// on it from the spool, so a test can compare payloads.
fn read_with_bytes(
    path: &Path,
    owners: &[String],
) -> (Vec<ConversationDocument>, crate::ReadReport) {
    let spool_dir = tempfile::tempdir().unwrap();
    let spool = AttachmentSpool::new(spool_dir.path());
    let (mut documents, report) = crate::read_backup(path, read_options(owners, &spool)).unwrap();
    for attachment in documents
        .iter_mut()
        .flat_map(|document| document.messages.iter_mut())
        .flat_map(|message| message.attachments.iter_mut())
    {
        attachment.bytes = attachment
            .digest_sha256
            .as_deref()
            .and_then(|digest| spool.path(digest))
            .map(|spooled| fs::read(spooled).unwrap());
    }
    (documents, report)
}

/// Sinks that drop what they get, and a cancel flag nobody sets, for
/// reader options a helper returns.
static NO_LOG: LazyLock<LogSink> = LazyLock::new(LogSink::none);
static NO_PROGRESS: LazyLock<ProgressSink> = LazyLock::new(ProgressSink::none);
static NO_CANCEL: LazyLock<CancelFlag> = LazyLock::new(CancelFlag::default);

/// Reader options that spool attachment payloads and stage nothing.
fn read_options<'a>(owners: &'a [String], spool: &'a AttachmentSpool) -> crate::ReadOptions<'a> {
    crate::ReadOptions {
        owner_phones: owners,
        attachments_dir: None,
        spool: Some(spool),
        files: message_crate_core::AttachmentFiles::Skip,
        sinks: message_crate_core::Sinks {
            log: &NO_LOG,
            progress: &NO_PROGRESS,
            cancel: &NO_CANCEL,
        },
    }
}

/// Each `<addr>` of an MMS as (address, type).
fn addrs(message: &Message) -> Vec<(String, String)> {
    message.source.as_ref().unwrap().fields["addrs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["address"].as_str().unwrap().to_string(),
                a["type"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// Each MMS part's name with the bytes its `data` decoded to, if any.
fn part_payloads(message: &Message) -> Vec<(String, Option<Vec<u8>>)> {
    message.source.as_ref().unwrap().fields["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let name = p["name"].as_str().or(p["ct"].as_str()).unwrap();
            let bytes = p["data_sha256"].as_str().map(|digest| {
                message
                    .attachments
                    .iter()
                    .find(|a| a.digest_sha256.as_deref() == Some(digest))
                    .and_then(|a| a.bytes.clone())
                    .unwrap()
            });
            (name.to_string(), bytes)
        })
        .collect()
}

/// The element attributes the message was written with.
fn attrs(message: &Message) -> &Value {
    &message.source.as_ref().unwrap().fields["attrs"]
}

/// The `contact_name` attribute the message was written with, if any.
fn contact_name(message: &Message) -> Option<&str> {
    attrs(message).get("contact_name").and_then(|v| v.as_str())
}

fn ir_attachment(name: &str, bytes: &[u8]) -> Attachment {
    Attachment {
        original_name: Some(name.into()),
        bytes: Some(bytes.to_vec()),
        ..ir::testutil::sample_attachment()
    }
}

fn pair(address: &str, addr_type: &str) -> (String, String) {
    (address.to_string(), addr_type.to_string())
}

#[test]
fn a_group_mms_keeps_its_addresses_and_attachment() {
    let mut document = ir::testutil::sample_document("look");
    document.conversation.chat_identifier = "chat-group".into();
    document.conversation.conversation_type = ConversationType::Group;
    document.conversation.participants = vec![
        ir::Participant {
            identity: Some("+15555550101".into()),
            display_name: Some("Ana".into()),
            country: None,
        },
        ir::Participant {
            identity: Some("+15555550102".into()),
            display_name: Some("Lee".into()),
            country: None,
        },
    ];
    let incoming = &mut document.messages[0];
    incoming.message_kind = MessageKind::Mms;
    incoming.source = None;
    incoming.sender_identity = Some("+15555550102".into());
    incoming.sender_display_name = Some("Lee".into());
    incoming.attachments = vec![ir_attachment("pic.jpg", b"first image")];
    let mut outgoing = incoming.clone();
    outgoing.guid = "outgoing".into();
    outgoing.timestamp_unix_ms += 1000;
    outgoing.direction = Direction::Outgoing;
    outgoing.sender_identity = Some(OWNER.into());
    outgoing.sender_display_name = None;
    outgoing.text = "nice".into();
    outgoing.attachments.clear();
    document.messages.push(outgoing);

    let read = round_trip(&[document]);
    assert_eq!(read.len(), 1, "both messages land in one group");
    let [incoming, outgoing] = read[0].messages.as_slice() else {
        panic!("two messages: {:?}", read[0].messages)
    };

    assert_eq!(incoming.direction, Direction::Incoming);
    assert_eq!(incoming.text, "look");
    assert_eq!(incoming.sender_identity.as_deref(), Some("+15555550102"));
    // A group's `contact_name` holds the participants' names, as SMS Backup
    // & Restore writes it, so it names the group and not the sender.
    assert_eq!(contact_name(incoming), Some("Ana, Lee"));
    assert_eq!(incoming.sender_display_name, None);
    assert_eq!(
        addrs(incoming),
        [
            pair("+15555550102", MMS_ADDR_FROM),
            pair(OWNER, MMS_ADDR_TO),
            pair("+15555550101", MMS_ADDR_TO),
        ]
    );
    assert_eq!(
        part_payloads(incoming),
        [
            ("text_0.txt".to_string(), None),
            ("pic.jpg".to_string(), Some(b"first image".to_vec())),
        ]
    );

    assert_eq!(outgoing.direction, Direction::Outgoing);
    assert_eq!(outgoing.text, "nice");
    assert_eq!(contact_name(outgoing), Some("Ana, Lee"));
    assert_eq!(
        addrs(outgoing),
        [
            pair(OWNER, MMS_ADDR_FROM),
            pair("+15555550101", MMS_ADDR_TO),
            pair("+15555550102", MMS_ADDR_TO),
        ]
    );
}

#[test]
fn an_incoming_sms_keeps_its_sender_and_contact_name() {
    let mut document = ir::testutil::sample_document("hello ir");
    document.messages[0].source = None;
    let read = round_trip(&[document]);
    let conversation = &read[0].conversation;
    assert_eq!(conversation.participants.len(), 1);
    assert_eq!(
        conversation.participants[0].identity.as_deref(),
        Some("+15555550101")
    );
    assert_eq!(
        conversation.participants[0].display_name.as_deref(),
        Some("Sam")
    );
    let message = &read[0].messages[0];
    assert_eq!(
        message.message_kind,
        MessageKind::Sms,
        "a text stays an <sms>"
    );
    assert_eq!(message.direction, Direction::Incoming);
    assert_eq!(message.sender_identity.as_deref(), Some("+15555550101"));
    assert_eq!(contact_name(message), Some("Sam"));
    assert_eq!(message.text, "hello ir");
}

/// An SMS from another app, such as iMessage's SMS fallback, has no SBR
/// fields to restore, so the element is built from the message alone. A
/// photo in a one-to-one conversation makes it an `<mms>`, whatever kind the
/// source app called the message.
#[test]
fn a_one_to_one_photo_from_another_app_keeps_its_conversation_sender_subject_and_bytes() {
    let mut document = ir::testutil::sample_document("look at this");
    let message = &mut document.messages[0];
    message.source = None;
    message.message_kind = MessageKind::Sms;
    message.subject = Some("Holiday".into());
    message.attachments = vec![Attachment {
        mime_type: Some("image/png".into()),
        ..ir_attachment("beach.png", b"png bytes")
    }];

    let read = round_trip(&[document]);
    assert_eq!(read.len(), 1);
    let conversation = &read[0].conversation;
    assert_eq!(conversation.chat_identifier, "+15555550101");
    assert_eq!(conversation.conversation_type, ConversationType::OneToOne);
    assert_eq!(
        conversation.participants[0].display_name.as_deref(),
        Some("Sam")
    );
    let read_back = &read[0].messages[0];
    assert_eq!(read_back.direction, Direction::Incoming);
    assert_eq!(read_back.sender_identity.as_deref(), Some("+15555550101"));
    assert_eq!(read_back.subject.as_deref(), Some("Holiday"));
    assert_eq!(read_back.text, "look at this");
    assert_eq!(read_back.attachments.len(), 1);
    let photo = &read_back.attachments[0];
    assert_eq!(photo.original_name.as_deref(), Some("beach.png"));
    assert_eq!(photo.mime_type.as_deref(), Some("image/png"));
    assert_eq!(photo.bytes.as_deref(), Some(b"png bytes".as_slice()));
}

/// A group message with no attachment is still an `<mms>`: an `<sms>` has
/// one address and would land in a one-to-one conversation with whoever sent
/// it.
#[test]
fn a_text_only_group_message_from_another_app_stays_in_its_group() {
    let mut document = ir::testutil::sample_document("who is in?");
    document.conversation.chat_identifier = "chat-group".into();
    document.conversation.conversation_type = ConversationType::Group;
    document.conversation.participants.push(ir::Participant {
        identity: Some("+15555550102".into()),
        display_name: Some("Lee".into()),
        country: None,
    });
    let incoming = &mut document.messages[0];
    incoming.source = None;
    incoming.message_kind = MessageKind::Sms;
    incoming.sender_identity = Some("+15555550102".into());
    incoming.sender_display_name = Some("Lee".into());
    let mut outgoing = incoming.clone();
    outgoing.timestamp_unix_ms += 1000;
    outgoing.direction = Direction::Outgoing;
    outgoing.sender_identity = Some(OWNER.into());
    outgoing.sender_display_name = None;
    outgoing.text = "me".into();
    document.messages.push(outgoing);

    let read = round_trip(&[document]);
    assert_eq!(read.len(), 1, "both messages land in one group");
    let conversation = &read[0].conversation;
    assert_eq!(conversation.conversation_type, ConversationType::Group);
    let mut handles: Vec<_> = conversation
        .participants
        .iter()
        .filter_map(|p| p.identity.as_deref())
        .collect();
    handles.sort_unstable();
    assert_eq!(handles, ["+15555550101", "+15555550102"]);
    let [incoming, outgoing] = read[0].messages.as_slice() else {
        panic!("two messages: {:?}", read[0].messages)
    };
    assert_eq!(incoming.direction, Direction::Incoming);
    assert_eq!(incoming.sender_identity.as_deref(), Some("+15555550102"));
    assert_eq!(contact_name(incoming), Some("Sam, Lee"));
    assert_eq!(incoming.sender_display_name, None);
    assert_eq!(incoming.text, "who is in?");
    assert!(incoming.attachments.is_empty());
    assert_eq!(outgoing.direction, Direction::Outgoing);
    assert_eq!(outgoing.text, "me");
    assert_eq!(contact_name(outgoing), Some("Sam, Lee"));
    assert_eq!(attrs(outgoing)["address"], "+15555550101~+15555550102");
}

/// A group conversation in which one participant has an identity is written
/// with that one identity and reads back as one-to-one, so its `contact_name`
/// is the one-to-one value and not every participant's name.
#[test]
fn a_group_with_one_identified_participant_is_named_as_one_to_one() {
    let mut document = ir::testutil::sample_document("hi");
    document.conversation.chat_identifier = "chat-group".into();
    document.conversation.conversation_type = ConversationType::Group;
    document.conversation.participants.push(ir::Participant {
        identity: None,
        display_name: Some("Lee".into()),
        country: None,
    });
    document.messages[0].source = None;

    let read = round_trip(&[document]);
    assert_eq!(read.len(), 1);
    assert_eq!(
        read[0].conversation.conversation_type,
        ConversationType::OneToOne
    );
    let message = &read[0].messages[0];
    assert_eq!(contact_name(message), Some("Sam"));
    assert_eq!(message.sender_display_name.as_deref(), Some("Sam"));
}

/// Two participants with one identity written two ways, or the owner listed
/// as a participant, are one peer to the reader. A group conversation of Sam
/// and either of those reads back as one-to-one with Sam, so its
/// `contact_name` is Sam's name and not a list.
#[test]
fn a_group_the_reader_finds_one_peer_in_is_named_as_one_to_one() {
    for (handle, name) in [("+1 555-555-0101", "Sam (work)"), (OWNER, "Me")] {
        let mut document = ir::testutil::sample_document("hi");
        document.conversation.chat_identifier = "chat-group".into();
        document.conversation.conversation_type = ConversationType::Group;
        document.conversation.participants.push(ir::Participant {
            identity: Some(handle.into()),
            display_name: Some(name.into()),
            country: None,
        });
        document.messages[0].source = None;

        let read = round_trip(&[document]);
        assert_eq!(read.len(), 1, "{handle}");
        assert_eq!(
            read[0].conversation.conversation_type,
            ConversationType::OneToOne,
            "{handle}"
        );
        let message = &read[0].messages[0];
        assert_eq!(contact_name(message), Some("Sam"), "{handle}");
        assert_eq!(
            message.sender_display_name.as_deref(),
            Some("Sam"),
            "{handle}"
        );
    }
}

/// Apps other than SMS Backup & Restore put the owner's own address on a
/// sent message. The `<sms>` is still addressed to the other person.
#[test]
fn a_sent_sms_carrying_the_owners_address_stays_in_the_peers_chat() {
    let mut document = ir::testutil::sample_document("on my way");
    let message = &mut document.messages[0];
    message.source = None;
    message.direction = Direction::Outgoing;
    message.sender_identity = Some(OWNER.into());
    message.sender_display_name = Some("Me".into());
    message.subject = Some("Tonight".into());

    let read = round_trip(&[document]);
    assert_eq!(read.len(), 1);
    let conversation = &read[0].conversation;
    assert_eq!(conversation.chat_identifier, "+15555550101");
    assert_eq!(conversation.participants.len(), 1);
    assert_eq!(
        conversation.participants[0].identity.as_deref(),
        Some("+15555550101")
    );
    assert_eq!(
        conversation.participants[0].display_name.as_deref(),
        Some("Sam")
    );
    let read_back = &read[0].messages[0];
    assert_eq!(read_back.message_kind, MessageKind::Sms);
    assert_eq!(read_back.direction, Direction::Outgoing);
    assert_eq!(read_back.subject.as_deref(), Some("Tonight"));
    assert_eq!(read_back.text, "on my way");
}

/// A received message whose sender address is an empty string is from the
/// chat's other person, as an `<sms>` and as an `<mms>`.
#[test]
fn a_received_message_with_an_empty_sender_address_is_from_the_peer() {
    let mut document = ir::testutil::sample_document("text");
    let text = &mut document.messages[0];
    text.source = None;
    text.sender_identity = Some(String::new());
    let mut photo = text.clone();
    photo.timestamp_unix_ms += 1000;
    photo.text = "photo".into();
    photo.attachments = vec![ir_attachment("pic.jpg", b"image")];
    document.messages.push(photo);

    let read = round_trip(&[document]);
    assert_eq!(read.len(), 1, "both messages land in the peer's chat");
    assert_eq!(read[0].conversation.chat_identifier, "+15555550101");
    let [text, photo] = read[0].messages.as_slice() else {
        panic!("two messages: {:?}", read[0].messages)
    };
    assert_eq!(text.sender_identity.as_deref(), Some("+15555550101"));
    assert_eq!(photo.sender_identity.as_deref(), Some("+15555550101"));
    assert_eq!(
        addrs(photo),
        [
            pair("+15555550101", MMS_ADDR_FROM),
            pair(OWNER, MMS_ADDR_TO)
        ]
    );
}

/// Attachment bytes go back on the part they came from, whether the
/// attachment still carries the part's digest or a media transform
/// cleared it. A text part and a part whose data was empty get none.
#[test]
fn restored_mms_parts_get_back_their_own_attachment_bytes() {
    let b64 = encode_part_data;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(
        &input,
        format!(
            r#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part seq="0" ct="application/smil" name="smil.xml" data="{}"/><part seq="1" ct="text/plain" name="text_0.txt" text="hi"/><part seq="2" ct="image/jpeg" name="a.jpg" data="{}"/><part seq="3" ct="image/png" name="empty.png" data=""/><part seq="4" ct="image/jpeg" name="bad.jpg" data="@@@not-base64@@@"/><part seq="5" ct="image/jpeg" name="c.jpg" data="{}"/><part seq="6" ct="image/jpeg" name="a-copy.jpg" data="{}"/></parts><addrs><addr address="+15555550101" type="137"/><addr address="{OWNER}" type="151"/></addrs></mms></smses>"#,
            b64(b"<smil><body/></smil>"),
            b64(b"first"),
            b64(b"second"),
            b64(b"first"),
        ),
    )
    .unwrap();
    let owners = [OWNER.to_string()];
    let (documents, _) = read_with_bytes(&input, &owners);
    assert_eq!(
        documents[0].messages[0].attachments.len(),
        3,
        "a.jpg, c.jpg, a-copy.jpg"
    );
    let payloads = |a_copy: Option<&[u8]>| {
        vec![
            ("smil.xml".to_string(), None),
            ("text_0.txt".to_string(), None),
            ("a.jpg".to_string(), Some(b"first".to_vec())),
            ("empty.png".to_string(), None),
            ("bad.jpg".to_string(), None),
            ("c.jpg".to_string(), Some(b"second".to_vec())),
            ("a-copy.jpg".to_string(), a_copy.map(<[u8]>::to_vec)),
        ]
    };

    let read = round_trip(&documents);
    assert_eq!(
        part_payloads(&read[0].messages[0]),
        payloads(Some(b"first"))
    );

    // The second attachment lost its digest: it falls back to the first
    // attachment not already taken, not to the first in the list.
    let mut transformed = documents.clone();
    transformed[0].messages[0].attachments[1].digest_sha256 = None;
    let read = round_trip(&transformed);
    assert_eq!(
        part_payloads(&read[0].messages[0]),
        payloads(Some(b"first"))
    );

    // No attachment has a digest: they go to the payload parts in order,
    // and with the third attachment gone the last part, with nothing left
    // to take, gets no data.
    transformed[0].messages[0].attachments.truncate(2);
    for attachment in &mut transformed[0].messages[0].attachments {
        attachment.digest_sha256 = None;
    }
    let read = round_trip(&transformed);
    assert_eq!(part_payloads(&read[0].messages[0]), payloads(None));
}

/// SMS Backup & Restore can describe only SMS and MMS. An iMessage or a
/// WhatsApp message written as `<sms>` would come back from a re-import as
/// an SMS under a new id, so the archive leaves it out and counts it (ADR 0021).
#[test]
fn the_archive_writes_only_sms_and_mms_and_counts_the_rest() {
    let mut imessage = ir::testutil::sample_imessage_document();
    // iMessage's SMS fallback is an SMS, whatever app it came from.
    let mut fallback = imessage.messages[0].clone();
    fallback.guid = "SMS-FALLBACK-0001".into();
    fallback.service = ir::Transport::Sms;
    fallback.message_kind = MessageKind::Sms;
    fallback.imessage = None;
    fallback.text = "hello fallback".into();
    imessage.messages.push(fallback);
    // A Mac `chat.db` row with no service is read as an SMS by its kind.
    let mut no_service = imessage.messages[0].clone();
    no_service.guid = "NO-SERVICE-0001".into();
    no_service.service = ir::Transport::Unknown;
    no_service.message_kind = MessageKind::Sms;
    no_service.imessage = None;
    no_service.text = "hello no service".into();
    imessage.messages.push(no_service);
    let documents = [
        ir::testutil::sample_document("hello ir"),
        imessage,
        ir::testutil::sample_whatsapp_document("hello whatsapp"),
    ];

    let tmp = tempfile::tempdir().unwrap();
    let mut report = message_crate_core::ExportReport::default();
    SbrArchive
        .write(tmp.path(), &documents, &mut report)
        .unwrap();
    let path = sbr::default_backup_path(tmp.path());
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"count="3""#), "{text}");
    assert!(text.contains("hello ir"));
    assert!(text.contains("hello fallback"));
    assert!(text.contains("hello no service"), "{text}");
    assert!(!text.contains("hello imessage"), "{text}");
    assert!(!text.contains("Loved a message"), "{text}");
    assert!(!text.contains("hello whatsapp"), "{text}");
    // The WhatsApp conversation had nothing left to write, so no element
    // names its address.
    assert!(!text.contains("+15555550102"), "{text}");
    // Two iMessage rows and one WhatsApp row.
    assert_eq!(report.extra(NOT_SMS_OR_MMS_LEFT_OUT), 3);
}

#[test]
fn the_archive_restores_source_fields_as_attrs() {
    let mut document = ir::testutil::sample_document("hello ir");
    // SyncTech-shaped bag (the same shape the reader stores).
    if let Some(source) = document.messages[0].source.as_mut() {
        let mut attrs = Map::new();
        attrs.insert("protocol".into(), json!("0"));
        attrs.insert("address".into(), json!("+15555550101"));
        attrs.insert("date".into(), json!("1400773261000"));
        attrs.insert("type".into(), json!("1"));
        attrs.insert("body".into(), json!("hello ir"));
        attrs.insert("service_center".into(), json!("+15555550114"));
        attrs.insert("contact_name".into(), json!("Sam"));
        source.fields = {
            let mut m = Map::new();
            m.insert("kind".into(), json!("sms"));
            m.insert("attrs".into(), Value::Object(attrs));
            m
        };
    }
    let tmp = tempfile::tempdir().unwrap();
    SbrArchive
        .write(
            tmp.path(),
            std::slice::from_ref(&document),
            &mut message_crate_core::ExportReport::default(),
        )
        .unwrap();
    let path = sbr::default_backup_path(tmp.path());
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"service_center="+15555550114""#));
    assert!(text.contains("hello ir"));
}

#[test]
fn the_archive_counts_the_characters_xml_cannot_carry() {
    let document = ir::testutil::sample_document("bell\u{7} and escape\u{1b}");
    let tmp = tempfile::tempdir().unwrap();
    let mut report = message_crate_core::ExportReport::default();
    SbrArchive
        .write(tmp.path(), std::slice::from_ref(&document), &mut report)
        .unwrap();
    assert_eq!(report.extra(CHARACTERS_LEFT_OUT), 2);
    let path = sbr::default_backup_path(tmp.path());
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"body="bell and escape""#), "{text}");
}

/// The file's one `backup_date` is the newest Backup Date of the messages
/// it writes: of two SMS dated by two backups the newer stands, and a
/// WhatsApp message, which it does not write, does not move it.
#[test]
fn the_backup_date_comes_from_the_messages_written() {
    let mut sms = ir::testutil::sample_document("hello ir");
    let mut older = sms.messages[0].clone();
    older.guid = "older".into();
    older.backup_taken_at_unix_ms = Some(1_780_000_000_000);
    sms.messages[0].backup_taken_at_unix_ms = Some(1_788_256_800_000);
    sms.messages.push(older);
    let mut whatsapp = ir::testutil::sample_whatsapp_document("hello whatsapp");
    whatsapp.messages[0].backup_taken_at_unix_ms = Some(1_790_793_912_000);

    let tmp = tempfile::tempdir().unwrap();
    let mut report = message_crate_core::ExportReport::default();
    SbrArchive
        .write(tmp.path(), &[sms, whatsapp], &mut report)
        .unwrap();
    let path = sbr::default_backup_path(tmp.path());
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"backup_date="1788256800000""#), "{text}");
}
