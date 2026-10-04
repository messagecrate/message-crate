use super::*;
use media::MediaMode;
use message_crate_core::{ExportReport, OutputFormat};
use message_ir::{
    ConversationMeta, ConversationStats, ExportMeta, HandleType, IrConversationType, IrImessage,
    IrMessage, IrMessageKind, IrParticipant, IrService, IrSource, MessageGuid, MessageIdentity,
    Reaction, SCHEMA_VERSION,
};
use serde_json::{Value, json};
use std::fs;

fn doc_with_image_attachment() -> ConversationDocument {
    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: "test".into(),
            tool: "test".into(),
            tool_version: "0".into(),
            owner_identity: None,
            owner_display_name: None,
        },
        conversation: ConversationMeta {
            chat_identifier: "+15555550101".into(),
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: vec![IrParticipant {
                identity: Some("+15555550101".into()),
                display_name: Some("Sam".into()),
                identity_type: None,
            }],
            stats: ConversationStats::default(),
        },
        messages: vec![IrMessage {
            guid: "guid-1".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Sms,
            sender_identity: Some("+15555550101".into()),
            sender_display_name: Some("Sam".into()),
            owner_identity: None,
            subject: None,
            text: "hi".into(),
            attachments: vec![IrAttachment {
                path: Some("photo.jpg".into()),
                original_name: Some("photo.jpg".into()),
                mime_type: Some("image/jpeg".into()),
                digest_sha256: None,
                is_sticker: false,
                transcription: None,
                sticker_effect: None,
                size_bytes: None,
                missing_reason: None,
                bytes: None,
            }],
            reactions: vec![],
            deletion: None,
            imessage: None,
            source: None,
        }],
        packaging_stem_suffix: None,
    }
}

/// Obfuscate one document as an export of it alone.
fn obfuscate_one(doc: &mut ConversationDocument, anon: &mut Obfuscator) {
    obfuscate_documents(std::slice::from_mut(doc), anon);
}

#[test]
fn obfuscate_drops_the_vendor_bag() {
    let mut doc = message_ir::testutil::sample_document("secret");
    assert!(doc.messages[0].source.is_some());
    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_one(&mut doc, &mut anon);
    assert!(
        doc.messages[0].source.is_none(),
        "obfuscated output must not carry vendor fields"
    );
}

#[test]
fn obfuscate_replaces_the_owner_address_on_each_message() {
    let mut doc = message_ir::testutil::sample_document("secret");
    doc.messages[0].owner_identity = Some("+15555550100".into());
    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_one(&mut doc, &mut anon);
    let owner = doc.messages[0].owner_identity.as_deref();
    assert_ne!(owner, Some("+15555550100"));
    assert_eq!(
        owner,
        doc.export.owner_identity.as_deref(),
        "one address becomes one fake address wherever it appears"
    );
}

/// "Me" is the label exports give the owner's own messages and names nobody,
/// so it stays. A real name on a sent message is the owner's and must go.
#[test]
fn obfuscate_keeps_me_on_a_sent_message_and_replaces_a_real_name() {
    let mut doc = doc_with_image_attachment();
    let mut labelled = doc.messages[0].clone();
    labelled.direction = IrDirection::Outgoing;
    labelled.sender_display_name = Some("Me".into());
    let mut named = labelled.clone();
    named.sender_display_name = Some("Alex Rivera".into());
    doc.messages = vec![labelled, named];

    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_one(&mut doc, &mut anon);

    assert_eq!(doc.messages[0].sender_display_name.as_deref(), Some("Me"));
    let replaced = doc.messages[1].sender_display_name.as_deref().unwrap();
    assert_ne!(replaced, "Alex Rivera");
    assert_ne!(replaced, "Me");
}

#[test]
fn obfuscate_skips_staged_media_and_writes_placeholders() {
    let tmp = crate::export_dir();
    let att = tmp.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    // Pretend an exporter staged a real file (should be removed).
    fs::write(att.join("real-photo.jpg"), b"REAL_JPEG_BYTES_SHOULD_GO").unwrap();

    let mut docs = vec![doc_with_image_attachment()];
    let transforms = ExportTransforms {
        media: MediaMode::Convert,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    let outcome = apply_transforms(&mut docs, tmp.path(), &transforms).unwrap();
    assert_eq!(outcome.obfuscated_docs, 1);

    assert!(!att.join("real-photo.jpg").exists());
    assert!(att.join("placeholder.jpg").is_file());
    assert!(att.join("placeholder.mp4").is_file());
    assert!(att.join("placeholder.bin").is_file());
    assert_eq!(
        docs[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/placeholder.jpg")
    );
}

#[test]
fn obfuscate_keeps_mime_when_media_disabled() {
    let tmp = crate::export_dir();
    let mut docs = vec![doc_with_image_attachment()];
    let transforms = ExportTransforms {
        media: MediaMode::Disabled,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    apply_transforms(&mut docs, tmp.path(), &transforms).unwrap();
    assert_eq!(
        docs[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/placeholder.jpg")
    );
}

/// The real file's size describes the file an obfuscated export leaves
/// out, as its digest does, so it goes with the digest (#1564).
#[test]
fn obfuscate_drops_the_real_files_size() {
    let tmp = crate::export_dir();
    let mut docs = vec![doc_with_image_attachment()];
    docs[0].messages[0].attachments[0].size_bytes = Some(5);
    let transforms = ExportTransforms {
        media: MediaMode::Disabled,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    apply_transforms(&mut docs, tmp.path(), &transforms).unwrap();
    assert_eq!(docs[0].messages[0].attachments[0].size_bytes, None);
}

#[test]
fn convert_at_finish_leaves_cloned_file() {
    let tmp = crate::export_dir();
    let att = tmp.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    fs::write(att.join("keep.bin"), b"already-cloned").unwrap();
    let mut docs = vec![doc_with_image_attachment()];
    docs[0].messages[0].attachments[0].path = Some("attachments/keep.bin".into());
    let transforms = ExportTransforms {
        media: MediaMode::Convert,
        ..ExportTransforms::none()
    };
    let outcome = apply_transforms(&mut docs, tmp.path(), &transforms).unwrap();
    assert_eq!(outcome.obfuscated_docs, 0);
    assert_eq!(
        docs[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/keep.bin")
    );
    assert_eq!(fs::read(att.join("keep.bin")).unwrap(), b"already-cloned");
}

/// A group chat in which every string holds its own `LEAK-<n>` marker, except
/// the values listed in [`KEPT_AS_IS`]. Every `Option` is set, so a field
/// added to the IR later has to be filled in here and the walk then checks it.
fn doc_with_a_marker_in_every_field() -> ConversationDocument {
    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: "sms-backup-restore".into(),
            tool: "SMS Backup & Restore".into(),
            tool_version: "10.20".into(),
            owner_identity: Some("LEAK-01".into()),
            owner_display_name: Some("LEAK-02".into()),
        },
        conversation: ConversationMeta {
            chat_identifier: "LEAK-03".into(),
            conversation_type: IrConversationType::Group,
            group_title: Some("LEAK-04".into()),
            participants: vec![IrParticipant {
                identity: Some("LEAK-05".into()),
                display_name: Some("LEAK-06".into()),
                identity_type: Some(HandleType::Phone),
            }],
            stats: ConversationStats {
                message_count: 1,
                attachment_count: 1,
                first_timestamp_unix_ms: Some(1_400_773_261_000),
                last_timestamp_unix_ms: Some(1_400_773_261_000),
            },
        },
        messages: vec![IrMessage {
            guid: "LEAK-27".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::IMessage,
            message_kind: IrMessageKind::IMessage,
            sender_identity: Some("LEAK-07".into()),
            sender_display_name: Some("LEAK-08".into()),
            owner_identity: Some("LEAK-09".into()),
            subject: Some("LEAK-10".into()),
            text: "LEAK-11".into(),
            attachments: vec![IrAttachment {
                path: Some("attachments/LEAK-12.jpg".into()),
                original_name: Some("LEAK-13.jpg".into()),
                mime_type: Some("image/jpeg".into()),
                digest_sha256: Some("LEAK-14".into()),
                is_sticker: false,
                transcription: Some("LEAK-15".into()),
                sticker_effect: Some("shiny".into()),
                size_bytes: Some(4),
                missing_reason: Some("file_missing".into()),
                bytes: None,
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
            imessage: Some(IrImessage {
                is_reply: true,
                in_reply_to_guid: Some("LEAK-28".into()),
                thread_originator_part: Some(0),
                num_replies: Some(1),
                send_effect: Some("slam".into()),
                shared_location: Some("LEAK-16".into()),
                announcement: Some("LEAK-17".into()),
                read_receipt_rfc3339: Some("2014-05-22T12:21:01Z".into()),
                parts: Some(json!([{ "text": "LEAK-18", "attachment_indices": [0] }])),
                edits: Some(json!([{ "text": "LEAK-19", "timestamp_unix_ms": 1 }])),
                app: Some(json!({ "url": "https://LEAK-24.example/", "title": "LEAK-25" })),
                balloon_bundle_id: Some("com.apple.DigitalTouchBalloonProvider".into()),
                balloon_kind: Some("sketch".into()),
                associated_guid: Some("LEAK-29".into()),
                associated_part: Some(0),
                tapback_kind: Some("emoji".into()),
                tapback_emoji: Some("👍".into()),
                tapback_action: Some("added".into()),
            }),
            source: Some(IrSource {
                android_type: Some(1),
                fields: serde_json::Map::from_iter([("address".into(), json!("LEAK-26"))]),
            }),
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
    ("conversation.participants[].identity_type", "enum value"),
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
    let doc = doc_with_a_marker_in_every_field();
    let mut leaves = Vec::new();
    string_leaves(&serde_json::to_value(&doc).unwrap(), "", &mut leaves);
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
        media: MediaMode::Disabled,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    let mut sink = crate::FormatSink::open(tmp.path(), OutputFormat::Json, transforms).unwrap();
    sink.write_document(doc).unwrap();
    sink.finish(&mut ExportReport::default()).unwrap();

    let mut written = String::new();
    for entry in fs::read_dir(tmp.path()).unwrap() {
        let path = entry.unwrap().path();
        // A group chat's file is named after its title.
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
    let mut doc = doc_with_a_marker_in_every_field();
    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_one(&mut doc, &mut anon);

    let [reaction] = doc.messages[0].reactions.as_slice() else {
        panic!("one reaction is kept: {:?}", doc.messages[0].reactions);
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
    let mut docs = vec![doc_with_image_attachment()];
    let att = &mut docs[0].messages[0].attachments[0];
    att.bytes = Some(vec![1, 2, 3]);
    att.digest_sha256 = Some("ab".repeat(32));
    let transforms = ExportTransforms {
        media: MediaMode::Disabled,
        ..ExportTransforms::none()
    };
    apply_transforms(&mut docs, tmp.path(), &transforms).unwrap();

    let att = &docs[0].messages[0].attachments[0];
    assert_eq!(att.path, None);
    assert_eq!(att.bytes, None);
    assert_eq!(att.digest_sha256, None);
    assert_eq!(
        att.mime_type.as_deref(),
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
    let mut doc = doc_with_image_attachment();
    doc.messages[0].attachments.clear();
    doc.messages[0].guid = original("hi", doc.messages[0].timestamp_unix_ms);
    let mut reply = doc.messages[0].clone();
    reply.guid = original("hi", reply.timestamp_unix_ms + 1);
    reply.timestamp_unix_ms += 1;
    reply.imessage = Some(IrImessage {
        is_reply: true,
        in_reply_to_guid: Some(doc.messages[0].guid.clone()),
        ..IrImessage::default()
    });
    doc.messages.push(reply);
    let before: Vec<String> = doc.messages.iter().map(|m| m.guid.clone()).collect();

    let obfuscated = |doc: &ConversationDocument| {
        let mut doc = doc.clone();
        obfuscate_one(&mut doc, &mut Obfuscator::new([7u8; 32]));
        doc
    };
    let after = obfuscated(&doc);
    for (old, msg) in before.iter().zip(&after.messages) {
        assert_ne!(&msg.guid, old, "the original guid is kept");
        assert_eq!(msg.guid.len(), 64);
    }
    assert_ne!(after.messages[0].guid, after.messages[1].guid);
    assert_eq!(
        after.messages[1]
            .imessage
            .as_ref()
            .unwrap()
            .in_reply_to_guid
            .as_deref(),
        Some(after.messages[0].guid.as_str()),
        "the reply points at its target's new guid"
    );
    assert_eq!(
        obfuscated(&doc).messages[0].guid,
        after.messages[0].guid,
        "one seed gives one guid"
    );
    let mut other_seed = doc.clone();
    obfuscate_one(&mut other_seed, &mut Obfuscator::new([8u8; 32]));
    assert_ne!(other_seed.messages[0].guid, after.messages[0].guid);
}

/// Two conversations for the cross-conversation tests: A holds one message,
/// and B holds one whose iMessage extension is `link` with A's message's id.
fn two_conversations(link: impl FnOnce(String) -> IrImessage) -> Vec<ConversationDocument> {
    let mut a = doc_with_image_attachment();
    a.messages[0].attachments.clear();
    a.messages[0].guid = "a-target".into();
    let mut b = doc_with_image_attachment();
    b.conversation.chat_identifier = "+15555550102".into();
    b.messages[0].attachments.clear();
    b.messages[0].guid = "b-link".into();
    b.messages[0].imessage = Some(link("a-target".into()));
    vec![a, b]
}

fn obfuscate_all(docs: &mut [ConversationDocument]) {
    let tmp = crate::export_dir();
    let transforms = ExportTransforms {
        media: MediaMode::Disabled,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    apply_transforms(docs, tmp.path(), &transforms).unwrap();
}

/// A reply (`in_reply_to_guid`) or a tapback (`associated_guid`).
#[derive(Clone, Copy, Debug)]
enum Link {
    Reply,
    Tapback,
}

impl Link {
    /// The extension that makes this link name `target`.
    fn to(self, target: &str) -> IrImessage {
        let target = Some(target.to_owned());
        match self {
            Link::Reply => IrImessage {
                is_reply: true,
                in_reply_to_guid: target,
                ..IrImessage::default()
            },
            Link::Tapback => IrImessage {
                associated_guid: target,
                ..IrImessage::default()
            },
        }
    }

    /// The id this link names in `msg`.
    fn target(self, msg: &IrMessage) -> Option<&str> {
        let im = msg.imessage.as_ref()?;
        match self {
            Link::Reply => im.in_reply_to_guid.as_deref(),
            Link::Tapback => im.associated_guid.as_deref(),
        }
    }
}

/// Apple Messages can reply or react to a message in another conversation,
/// and each conversation is its own document, so the target is found across
/// every document of the export, not only its own.
#[test]
fn obfuscate_points_a_reply_or_tapback_at_its_target_in_another_conversation() {
    for link in [Link::Reply, Link::Tapback] {
        let mut docs = two_conversations(|target| link.to(&target));
        obfuscate_all(&mut docs);
        assert_ne!(docs[0].messages[0].guid, "a-target", "{link:?}");
        assert_eq!(
            link.target(&docs[1].messages[0]),
            Some(docs[0].messages[0].guid.as_str()),
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
            let mut docs = two_conversations(|_| IrImessage::default());
            let mut copy = docs[0].messages[0].clone();
            copy.imessage = None;
            docs[1].messages.insert(0, copy);
            docs[1].messages[1].imessage = Some(link.to("a-target"));
            if linking_first {
                docs.reverse();
            }
            obfuscate_all(&mut docs);
            let (linking, other) = if linking_first {
                (&docs[0], &docs[1])
            } else {
                (&docs[1], &docs[0])
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
    let mut docs = two_conversations(|_| IrImessage {
        is_reply: true,
        in_reply_to_guid: Some("not-exported".into()),
        ..IrImessage::default()
    });
    obfuscate_all(&mut docs);
    let target = docs[1].messages[0]
        .imessage
        .as_ref()
        .unwrap()
        .in_reply_to_guid
        .clone()
        .unwrap();
    assert_ne!(target, "not-exported");
    assert_eq!(target.len(), 64);
    assert!(
        docs.iter()
            .flat_map(|d| &d.messages)
            .all(|m| m.guid != target)
    );
}
