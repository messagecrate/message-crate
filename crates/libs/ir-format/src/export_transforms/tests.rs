use super::*;
use ir::{
    ConversationMeta, ConversationStats, ConversationType, EarlierVersion, ExportMeta,
    ImessageExtensions, Message, MessageGuid, MessageIdentity, MessageKind, Participant, Reaction,
    ReplyTo, SCHEMA_VERSION, Transport, VendorFields,
};
use message_crate_core::AttachmentFiles;
use message_crate_core::{ExportReport, OutputFormat};
use serde_json::{Value, json};
use std::fs;

/// `ir`'s sample conversation, its one message `hi` with a JPEG
/// attached.
fn document_with_image_attachment() -> ConversationDocument {
    let mut document = ir::testutil::sample_document("hi");
    document.messages[0].attachments = vec![Attachment {
        path: Some("photo.jpg".into()),
        ..ir::testutil::sample_attachment()
    }];
    document
}

/// Obfuscate one document as an export of it alone.
fn obfuscate_one(document: &mut ConversationDocument, anon: &mut Obfuscator) {
    obfuscate_documents(std::slice::from_mut(document), anon);
}

#[test]
fn obfuscate_drops_the_vendor_bag() {
    let mut document = ir::testutil::sample_document("secret");
    assert!(document.messages[0].source.is_some());
    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_one(&mut document, &mut anon);
    assert!(
        document.messages[0].source.is_none(),
        "obfuscated output must not carry vendor fields"
    );
}

#[test]
fn obfuscate_replaces_the_owner_address_on_each_message() {
    let mut document = ir::testutil::sample_document("secret");
    document.messages[0].owner_identity = Some("+15555550100".into());
    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_one(&mut document, &mut anon);
    let owner = document.messages[0].owner_identity.as_deref();
    assert_ne!(owner, Some("+15555550100"));
    assert_eq!(
        owner,
        document.export.owner_identity.as_deref(),
        "one address becomes one fake address wherever it appears"
    );
}

/// "Me" is the label exports give the owner's own messages and names nobody,
/// so it stays. A real name on a sent message is the owner's and must go.
#[test]
fn obfuscate_keeps_me_on_a_sent_message_and_replaces_a_real_name() {
    let mut document = document_with_image_attachment();
    let mut labeled = document.messages[0].clone();
    labeled.direction = Direction::Outgoing;
    labeled.sender.display_name = Some("Me".into());
    let mut named = labeled.clone();
    named.sender.display_name = Some("Alex Rivera".into());
    document.messages = vec![labeled, named];

    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_one(&mut document, &mut anon);

    assert_eq!(
        document.messages[0].sender.display_name.as_deref(),
        Some("Me")
    );
    let replaced = document.messages[1].sender.display_name.as_deref().unwrap();
    assert_ne!(replaced, "Alex Rivera");
    assert_ne!(replaced, "Me");
}

#[test]
fn obfuscate_skips_staged_media_and_writes_placeholders() {
    let tmp = crate::export_dir();
    let attachment = tmp.path().join("attachments");
    fs::create_dir_all(&attachment).unwrap();
    // Pretend an exporter staged a real file (should be removed).
    fs::write(
        attachment.join("real-photo.jpg"),
        b"REAL_JPEG_BYTES_SHOULD_GO",
    )
    .unwrap();

    let mut documents = vec![document_with_image_attachment()];
    let transforms = ExportTransforms {
        files: AttachmentFiles::Originals,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    let outcome = apply_transforms(&mut documents, tmp.path(), &transforms).unwrap();
    assert_eq!(outcome.obfuscated_documents, 1);

    assert!(!attachment.join("real-photo.jpg").exists());
    assert!(attachment.join("placeholder.jpg").is_file());
    assert!(attachment.join("placeholder.mp4").is_file());
    assert!(attachment.join("placeholder.bin").is_file());
    assert_eq!(
        documents[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/placeholder.jpg")
    );
}

#[test]
fn obfuscate_keeps_mime_when_media_disabled() {
    let tmp = crate::export_dir();
    let mut documents = vec![document_with_image_attachment()];
    let transforms = ExportTransforms {
        files: AttachmentFiles::Skip,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    apply_transforms(&mut documents, tmp.path(), &transforms).unwrap();
    assert_eq!(
        documents[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/placeholder.jpg")
    );
}

/// The real file's size describes the file an obfuscated export leaves
/// out, as its digest does, so it goes with the digest (#1564).
#[test]
fn obfuscate_drops_the_real_files_size() {
    let tmp = crate::export_dir();
    let mut documents = vec![document_with_image_attachment()];
    documents[0].messages[0].attachments[0].size_bytes = Some(5);
    let transforms = ExportTransforms {
        files: AttachmentFiles::Skip,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    apply_transforms(&mut documents, tmp.path(), &transforms).unwrap();
    assert_eq!(documents[0].messages[0].attachments[0].size_bytes, None);
}

#[test]
fn finish_leaves_a_copied_file_as_it_is() {
    let tmp = crate::export_dir();
    let attachment = tmp.path().join("attachments");
    fs::create_dir_all(&attachment).unwrap();
    fs::write(attachment.join("keep.bin"), b"already-cloned").unwrap();
    let mut documents = vec![document_with_image_attachment()];
    documents[0].messages[0].attachments[0].path = Some("attachments/keep.bin".into());
    let transforms = ExportTransforms {
        files: AttachmentFiles::Originals,
        ..ExportTransforms::none()
    };
    let outcome = apply_transforms(&mut documents, tmp.path(), &transforms).unwrap();
    assert_eq!(outcome.obfuscated_documents, 0);
    assert_eq!(
        documents[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/keep.bin")
    );
    assert_eq!(
        fs::read(attachment.join("keep.bin")).unwrap(),
        b"already-cloned"
    );
}

/// A group conversation in which every string holds its own `LEAK-<n>`
/// marker, except the values listed in [`KEPT_AS_IS`]. Every `Option` is set,
/// so a field added to the IR later has to be filled in here and the walk then
/// checks it.
fn document_with_a_marker_in_every_field() -> ConversationDocument {
    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: "sms-backup-restore".into(),
            tool: "SMS Backup & Restore".into(),
            tool_version: "10.20".into(),
            owner_identity: Some("LEAK-01".into()),
            owner_display_name: Some("LEAK-02".into()),
            exported_at_unix_ms: Some(1_791_000_000_000),
        },
        conversation: ConversationMeta {
            chat_identifier: "LEAK-03".into(),
            conversation_type: ConversationType::Group,
            group_title: Some("LEAK-04".into()),
            participants: vec![Participant {
                identity: Some("LEAK-05".into()),
                display_name: Some("LEAK-06".into()),
                country: None,
            }],
            stats: ConversationStats {
                message_count: 1,
                attachment_count: 1,
                first_timestamp_unix_ms: Some(1_400_773_261_000),
                last_timestamp_unix_ms: Some(1_400_773_261_000),
            },
        },
        messages: vec![Message {
            guid: "LEAK-27".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            time_precision: ir::TimePrecision::Milliseconds,
            direction: Direction::Incoming,
            service: Transport::IMessage,
            message_kind: MessageKind::IMessage,
            sender: ir::Sender {
                identity: Some("LEAK-07".into()),
                display_name: Some("LEAK-08".into()),
                country: None,
            },
            owner_identity: Some("LEAK-09".into()),
            subject: Some("LEAK-10".into()),
            text: "LEAK-11".into(),
            attachments: vec![Attachment {
                path: Some("attachments/LEAK-12.jpg".into()),
                original_name: Some("LEAK-13.jpg".into()),
                digest_sha256: Some("LEAK-14".into()),
                transcription: Some("LEAK-15".into()),
                sticker_effect: Some("shiny".into()),
                size_bytes: Some(4),
                missing_reason: Some("file_missing".into()),
                ..ir::testutil::sample_attachment()
            }],
            reactions: vec![Reaction {
                part_index: 0,
                kind: "emoji".into(),
                emoji: Some("👍".into()),
                is_from_me: false,
                reactor_identity: Some("LEAK-20".into()),
                reactor_display_name: Some("LEAK-21".into()),
            }],
            deletion: None,
            edits: vec![EarlierVersion {
                part_index: 0,
                text: Some("LEAK-19".into()),
                edited_at_unix_ms: Some(1),
            }],
            reply_to: Some(ReplyTo {
                guid: Some("LEAK-28".into()),
                part_index: Some(0),
            }),
            imessage: Some(ImessageExtensions {
                send_effect: Some("slam".into()),
                shared_location: Some("LEAK-16".into()),
                announcement: Some("LEAK-17".into()),
                read_receipt_rfc3339: Some("2014-05-22T12:21:01Z".into()),
                parts: Some(json!([{ "text": "LEAK-18", "attachment_indices": [0] }])),
                app: Some(json!({ "url": "https://LEAK-24.example/", "title": "LEAK-25" })),
                balloon_bundle_id: Some("com.apple.DigitalTouchBalloonProvider".into()),
                balloon_kind: Some("sketch".into()),
                associated_guid: Some("LEAK-29".into()),
                associated_part: Some(0),
                tapback_kind: Some("emoji".into()),
                tapback_emoji: Some("👍".into()),
                tapback_action: Some("added".into()),
            }),
            source: Some(VendorFields {
                android_type: Some(1),
                fields: serde_json::Map::from_iter([("address".into(), json!("LEAK-26"))]),
            }),
            backup_taken_at_unix_ms: Some(1_790_793_912_000),
        }],
        packaging_stem_suffix: None,
    }
}

/// The strings obfuscation passes through unchanged, by JSON path, each with
/// the reason that is safe. Every other string must be replaced or dropped.
const KEPT_AS_IS: &[(&str, &str)] = &[
    ("export.source", "names the backup tool, not the person"),
    ("export.tool", "names the backup tool, not the person"),
    (
        "export.tool_version",
        "names the backup tool, not the person",
    ),
    ("conversation.conversation_type", "enum value"),
    ("messages[].time_precision", "enum value"),
    ("messages[].direction", "enum value"),
    ("messages[].service", "enum value"),
    ("messages[].message_kind", "enum value"),
    (
        "messages[].attachments[].mime_type",
        "picks the placeholder file",
    ),
    (
        "messages[].attachments[].sticker_effect",
        "Apple effect label",
    ),
    (
        "messages[].attachments[].missing_reason",
        "closed set of reasons",
    ),
    ("messages[].imessage.send_effect", "Apple effect label"),
    (
        "messages[].imessage.read_receipt_rfc3339",
        "a time, and times are kept",
    ),
    ("messages[].imessage.balloon_bundle_id", "Apple bundle id"),
    ("messages[].imessage.balloon_kind", "Apple balloon label"),
    ("messages[].imessage.tapback_kind", "tapback label"),
    (
        "messages[].imessage.tapback_emoji",
        "the reaction, not who made it",
    ),
    ("messages[].imessage.tapback_action", "tapback label"),
    ("messages[].reactions[].kind", "reaction label"),
    (
        "messages[].reactions[].emoji",
        "the reaction, not who made it",
    ),
];

/// Push every string in `value` with its path (`a.b[].c`) onto `out`.
///
/// # Panics
///
/// Panics on a null, because the fixture must set every field.
fn string_leaves(value: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::String(s) => out.push((path.to_string(), s.clone())),
        Value::Array(items) => {
            for item in items {
                string_leaves(item, &format!("{path}[]"), out);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                string_leaves(v, &child, out);
            }
        }
        Value::Null => panic!("{path} is null; the fixture must set every field"),
        Value::Bool(_) | Value::Number(_) => {}
    }
}

#[test]
fn obfuscated_export_keeps_no_string_from_the_source() {
    let document = document_with_a_marker_in_every_field();
    let mut leaves = Vec::new();
    string_leaves(&serde_json::to_value(&document).unwrap(), "", &mut leaves);
    for (path, value) in &leaves {
        let kept = KEPT_AS_IS.iter().any(|(p, _)| p == path);
        assert_eq!(
            value.contains("LEAK-"),
            !kept,
            "{path} = {value:?}: a string not in KEPT_AS_IS must hold a LEAK marker"
        );
    }

    let tmp = crate::export_dir();
    let transforms = ExportTransforms {
        files: AttachmentFiles::Skip,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    let mut sink = crate::FormatSink::open(tmp.path(), OutputFormat::Json, transforms).unwrap();
    sink.write_document(document).unwrap();
    sink.finish(&mut ExportReport::default()).unwrap();

    let mut written = String::new();
    for entry in fs::read_dir(tmp.path()).unwrap() {
        let path = entry.unwrap().path();
        // A group conversation's file is named after its title.
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(!name.contains("LEAK-"), "obfuscated export wrote {name}");
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            written.push_str(&fs::read_to_string(&path).unwrap());
        }
    }
    assert!(!written.is_empty(), "the export wrote no JSON file");
    let leaked: Vec<&str> = leaves
        .iter()
        .filter(|(_, value)| value.contains("LEAK-") && written.contains(value.as_str()))
        .map(|(path, _)| path.as_str())
        .collect();
    assert!(leaked.is_empty(), "obfuscated export kept {leaked:?}");
}

/// Reactions are imported, so an obfuscated export must still say what each
/// reaction was; only who reacted is replaced.
#[test]
fn obfuscate_keeps_each_reaction_and_replaces_only_who_reacted() {
    let mut document = document_with_a_marker_in_every_field();
    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_one(&mut document, &mut anon);

    let [reaction] = document.messages[0].reactions.as_slice() else {
        panic!("one reaction is kept: {:?}", document.messages[0].reactions);
    };
    assert_eq!(reaction.part_index, 0);
    assert_eq!(reaction.kind, "emoji");
    assert_eq!(reaction.emoji.as_deref(), Some("👍"));
    assert!(!reaction.is_from_me);
    let reactor = reaction
        .reactor_identity
        .as_deref()
        .expect("reactor_identity is kept");
    assert!(!reactor.is_empty());
    assert!(
        !reactor.contains("LEAK-"),
        "reactor_identity kept {reactor}"
    );
    let name = reaction
        .reactor_display_name
        .as_deref()
        .expect("reactor_display_name is kept");
    assert!(!name.contains("LEAK-"), "reactor_display_name kept {name}");
}

#[test]
fn media_disabled_clears_each_attachment_path_bytes_and_fingerprint() {
    let tmp = crate::export_dir();
    let mut documents = vec![document_with_image_attachment()];
    let attachment = &mut documents[0].messages[0].attachments[0];
    attachment.bytes = Some(vec![1, 2, 3]);
    attachment.digest_sha256 = Some("ab".repeat(32));
    let transforms = ExportTransforms {
        files: AttachmentFiles::Skip,
        ..ExportTransforms::none()
    };
    apply_transforms(&mut documents, tmp.path(), &transforms).unwrap();

    let attachment = &documents[0].messages[0].attachments[0];
    assert_eq!(attachment.path, None);
    assert_eq!(attachment.bytes, None);
    assert_eq!(attachment.digest_sha256, None);
    assert_eq!(
        attachment.mime_type.as_deref(),
        Some("image/jpeg"),
        "the metadata stays"
    );
}

/// For a source without ids of its own, the `guid` is a hash of content the
/// obfuscated output partly keeps, so a kept `guid` would check a guess at
/// the original text. The obfuscated `guid` is made again from the
/// obfuscated message, and a reply still points at its target.
#[test]
fn obfuscate_makes_each_guid_again_and_keeps_replies_pointing_at_their_target() {
    let original = |text: &str, ms: i64| {
        MessageGuid::new(&MessageIdentity {
            chat: "+15555550101",
            is_from_me: false,
            sender: Some("+15555550101"),
            timestamp_unix_ms: ms,
            text,
            attachment_digests: &[],
            vendor_key: None,
        })
        .into_string()
    };
    let mut document = document_with_image_attachment();
    document.messages[0].attachments.clear();
    document.messages[0].guid = original("hi", document.messages[0].timestamp_unix_ms);
    let mut reply = document.messages[0].clone();
    reply.guid = original("hi", reply.timestamp_unix_ms + 1);
    reply.timestamp_unix_ms += 1;
    reply.reply_to = Some(ReplyTo {
        guid: Some(document.messages[0].guid.clone()),
        part_index: None,
    });
    document.messages.push(reply);
    let before: Vec<String> = document.messages.iter().map(|m| m.guid.clone()).collect();

    let obfuscated = |document: &ConversationDocument| {
        let mut document = document.clone();
        obfuscate_one(&mut document, &mut Obfuscator::new([7u8; 32]));
        document
    };
    let after = obfuscated(&document);
    for (old, message) in before.iter().zip(&after.messages) {
        assert_ne!(&message.guid, old, "the original guid is kept");
        assert_eq!(message.guid.len(), 64);
    }
    assert_ne!(after.messages[0].guid, after.messages[1].guid);
    assert_eq!(
        after.messages[1]
            .reply_to
            .as_ref()
            .and_then(|r| r.guid.as_deref()),
        Some(after.messages[0].guid.as_str()),
        "the reply points at its target's new guid"
    );
    assert_eq!(
        obfuscated(&document).messages[0].guid,
        after.messages[0].guid,
        "one seed gives one guid"
    );
    let mut other_seed = document.clone();
    obfuscate_one(&mut other_seed, &mut Obfuscator::new([8u8; 32]));
    assert_ne!(other_seed.messages[0].guid, after.messages[0].guid);
}

/// Two conversations for the cross-conversation tests: A holds one message,
/// and B holds one that `link` points at A's message's id.
fn two_conversations(link: impl FnOnce(&mut Message, &str)) -> Vec<ConversationDocument> {
    let mut a = document_with_image_attachment();
    a.messages[0].attachments.clear();
    a.messages[0].guid = "a-target".into();
    let mut b = document_with_image_attachment();
    b.conversation.chat_identifier = "+15555550102".into();
    b.messages[0].attachments.clear();
    b.messages[0].guid = "b-link".into();
    link(&mut b.messages[0], "a-target");
    vec![a, b]
}

fn obfuscate_all(documents: &mut [ConversationDocument]) {
    let tmp = crate::export_dir();
    let transforms = ExportTransforms {
        files: AttachmentFiles::Skip,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    apply_transforms(documents, tmp.path(), &transforms).unwrap();
}

/// A reply (`reply_to.guid`) or a tapback (`associated_guid`).
#[derive(Clone, Copy, Debug)]
enum Link {
    Reply,
    Tapback,
}

impl Link {
    /// Make `message` name `target` by this link.
    fn set(self, message: &mut Message, target: &str) {
        let target = Some(target.to_owned());
        match self {
            Link::Reply => {
                message.reply_to = Some(ReplyTo {
                    guid: target,
                    part_index: None,
                });
            }
            Link::Tapback => {
                message.imessage = Some(ImessageExtensions {
                    associated_guid: target,
                    ..ImessageExtensions::default()
                });
            }
        }
    }

    /// The id this link names in `message`.
    fn target(self, message: &Message) -> Option<&str> {
        match self {
            Link::Reply => message.reply_to.as_ref()?.guid.as_deref(),
            Link::Tapback => message.imessage.as_ref()?.associated_guid.as_deref(),
        }
    }
}

/// Apple Messages can reply or react to a message in another conversation,
/// and each conversation is its own document, so the target is found across
/// every document of the export, not only its own.
#[test]
fn obfuscate_points_a_reply_or_tapback_at_its_target_in_another_conversation() {
    for link in [Link::Reply, Link::Tapback] {
        let mut documents = two_conversations(|message, target| link.set(message, target));
        obfuscate_all(&mut documents);
        assert_ne!(documents[0].messages[0].guid, "a-target", "{link:?}");
        assert_eq!(
            link.target(&documents[1].messages[0]),
            Some(documents[0].messages[0].guid.as_str()),
            "{link:?} points at its target's new guid"
        );
    }
}

/// One source message can sit in two documents, and each copy gets its own
/// new `guid`. A reply or tapback keeps pointing at the copy in its own
/// document, whichever document the export holds first.
#[test]
fn obfuscate_points_a_reply_or_tapback_at_the_copy_in_its_own_conversation() {
    for link in [Link::Reply, Link::Tapback] {
        for linking_first in [true, false] {
            let mut documents = two_conversations(|message, _| {
                message.imessage = Some(ImessageExtensions::default())
            });
            let mut copy = documents[0].messages[0].clone();
            copy.imessage = None;
            copy.reply_to = None;
            documents[1].messages.insert(0, copy);
            link.set(&mut documents[1].messages[1], "a-target");
            if linking_first {
                documents.reverse();
            }
            obfuscate_all(&mut documents);
            let (linking, other) = if linking_first {
                (&documents[0], &documents[1])
            } else {
                (&documents[1], &documents[0])
            };
            let case = format!("{link:?}, linking_first={linking_first}");
            assert_ne!(
                linking.messages[0].guid, other.messages[0].guid,
                "{case}: each copy gets its own guid"
            );
            assert_eq!(
                link.target(&linking.messages[1]),
                Some(linking.messages[0].guid.as_str()),
                "{case}: the link points at the copy in its own conversation"
            );
        }
    }
}

/// A target in no document of the export, such as one the date range left
/// out, keeps the keyed stand-in: its original id does not survive.
#[test]
fn obfuscate_replaces_a_target_outside_the_export_with_its_stand_in() {
    let mut documents = two_conversations(|message, _| Link::Reply.set(message, "not-exported"));
    obfuscate_all(&mut documents);
    let target = Link::Reply
        .target(&documents[1].messages[0])
        .unwrap()
        .to_owned();
    assert_ne!(target, "not-exported");
    assert_eq!(target.len(), 64);
    assert!(
        documents
            .iter()
            .flat_map(|d| &d.messages)
            .all(|m| m.guid != target)
    );
}
