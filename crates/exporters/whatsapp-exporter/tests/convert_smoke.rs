use crate::emit::{ConvertRequest, convert_json};
use message_crate_core::testutil::assert_jsonl_resumes;
use message_crate_core::{ExportTransforms, OutputFormat};
use std::fs;
use std::path::PathBuf;

#[test]
fn convert_fixture_json_individual_and_group() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/result.json");
    assert!(fixture.is_file(), "missing {}", fixture.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let report = convert_json(ConvertRequest {
        json_path: &fixture,
        output: tmp.path(),
        transforms: ExportTransforms::none(),
        media_search_roots: &[],
        owner_identity: None,
        backup_taken_at_unix_ms: None,
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");

    assert_eq!(report.conversations, 2);
    assert_eq!(report.messages, 4);
    assert_eq!(report.sent, 2);
    assert_eq!(report.received, 2);

    let individual = tmp.path().join("+15555550122__whatsapp.csv");
    assert!(individual.is_file(), "missing {}", individual.display());
    let body = fs::read_to_string(&individual).expect("read individual");
    assert!(body.contains("chat_identifier"));
    assert!(body.contains("whatsapp"));
    assert!(body.contains("Hello from Sam"));
    assert!(body.contains("WhatsApp Chat Exporter"));

    let group = tmp.path().join("Family_Chat__whatsapp.csv");
    assert!(group.is_file(), "missing {}", group.display());
    let gbody = fs::read_to_string(&group).expect("read group");
    assert!(gbody.contains("group"));
    assert!(gbody.contains("Family Chat"));
    assert!(gbody.contains("+15555550133") || gbody.contains("15555550133"));
}

#[test]
fn copies_ios_style_media_true_data_paths() {
    let media_root = tempfile::tempdir().expect("media root");
    let media_base = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";
    let rel = "Message/Media/chat/a/b/photo.jpg";
    let src = media_root.path().join(media_base).join(rel);
    fs::create_dir_all(src.parent().unwrap()).expect("mkdir");
    fs::write(&src, b"fake-jpeg").expect("write media");

    let json = serde_json::json!({
        "15555550167@s.whatsapp.net": {
            "name": "Media Peer",
            "type": "ios",
            "media_base": format!("{media_base}/"),
            "members": null, "messages": {
                "M1": {
                    "from_me": false,
                    "timestamp": 1609459200,
                    "time": "00:00",
                    "key_id": "M1",
                    "data": rel,
                    "sender": null,
                    "media": true,
                    "mime": "image/jpeg",
                    "caption": "look at this",
                    "sticker": false,
                    "reply": null,
                    "reactions": {},
                    "sender_jid": null,
                    "sender_lid": null,
                    "sender_contact_name": null,
                    "sender_push_name": null
                }
            }
        }
    });
    let json_path = media_root.path().join("result.json");
    fs::write(&json_path, json.to_string()).expect("write json");

    let out = tempfile::tempdir().expect("out");
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: out.path(),
        transforms: ExportTransforms::none(),
        media_search_roots: &[media_root.path().to_path_buf()],
        owner_identity: None,
        backup_taken_at_unix_ms: None,
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");

    assert_eq!(report.attachments_saved, 1);
    assert_eq!(report.extra(message_crate_core::ATTACHMENTS_MISSING), 0);
    let csv = out.path().join("+15555550167__whatsapp.csv");
    let body = fs::read_to_string(&csv).expect("csv");
    assert!(body.contains("look at this"));
    assert!(
        !body.contains(rel),
        "media path must not become message text"
    );
    assert!(body.contains("attachments/"));
    let att_dir = out.path().join("attachments");
    let files: Vec<_> = fs::read_dir(&att_dir)
        .expect("attachments dir")
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(fs::read(&files[0]).unwrap(), b"fake-jpeg");
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/result.json");
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = assert_jsonl_resumes(tmp.path(), |resume| {
        convert_json(ConvertRequest {
            json_path: &fixture,
            output: tmp.path(),
            transforms: ExportTransforms::none(),
            media_search_roots: &[],
            owner_identity: None,
            backup_taken_at_unix_ms: None,
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
            issues: None,
        })
    });
    assert_eq!(report.conversations, 2, "one individual chat and one group");
}

/// Convert `json` into a JSON export and read each conversation back,
/// keyed by chat identifier.
fn convert_to_documents(
    json: &serde_json::Value,
) -> (
    message_crate_core::ExportReport,
    std::collections::BTreeMap<String, message_ir::ConversationDocument>,
) {
    convert_with_owner(json, None)
}

/// [`convert_to_documents`], with the owner's number as the run knows it.
fn convert_with_owner(
    json: &serde_json::Value,
    owner_identity: Option<&str>,
) -> (
    message_crate_core::ExportReport,
    std::collections::BTreeMap<String, message_ir::ConversationDocument>,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("result.json");
    fs::write(&json_path, json.to_string()).expect("write json");
    let out = dir.path().join("out");
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: &out,
        transforms: ExportTransforms::none(),
        media_search_roots: &[dir.path().to_path_buf()],
        owner_identity: owner_identity.map(str::to_string),
        backup_taken_at_unix_ms: None,
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");
    let documents = fs::read_dir(&out)
        .expect("out")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| message_ir_format::read_conversation_json(&path).expect("read back"))
        .map(|doc| (doc.conversation.chat_identifier.clone(), doc))
        .collect();
    (report, documents)
}

/// One wtsexporter message row with the fields every row carries.
fn row(key_id: &str, timestamp: serde_json::Value, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "from_me": false,
        "timestamp": timestamp,
        "key_id": key_id,
        "data": data,
        "sender": null,
        "media": false,
        "mime": null,
        "caption": null,
        "sticker": false,
        "reply": null,
        "reactions": {},
        "sender_jid": null,
        "sender_lid": null,
        "sender_contact_name": null,
        "sender_push_name": null
    })
}

/// Each message keeps its exact time, whether wtsexporter wrote seconds or
/// milliseconds, and a time no calendar can hold is skipped. Two messages
/// sent in the same second with the same text are two messages, told
/// apart by their `key_id`.
#[test]
fn messages_keep_their_time_and_their_identity() {
    let mut ok_twice = row("K2", 1_609_459_300.into(), "ok".into());
    ok_twice["from_me"] = true.into();
    let mut ok_again = ok_twice.clone();
    ok_again["key_id"] = "K3".into();
    let json = serde_json::json!({
        "15555550122@s.whatsapp.net": {
            "name": "Sam Example",
            "members": null, "messages": {
                "K1": row("K1", 1_609_459_200.into(), "Hello".into()),
                "K2": ok_twice,
                "K3": ok_again,
                "K4": row("K4", 1_609_459_400_123_i64.into(), "in milliseconds".into()),
                "K5": row("K5", 1e18.into(), "never".into()),
                // The largest stamp read as seconds, in 2286.
                "K6": row("K6", 9_999_999_999_i64.into(), "last second".into()),
            }
        }
    });

    let (report, documents) = convert_to_documents(&json);
    assert_eq!(report.skipped_invalid_date, 1);
    let doc = &documents["+15555550122"];
    let times: Vec<(i64, &str)> = doc
        .messages
        .iter()
        .map(|m| (m.timestamp_unix_ms, m.text.as_str()))
        .collect();
    assert_eq!(
        times,
        vec![
            (1_609_459_200_000, "Hello"),
            (1_609_459_300_000, "ok"),
            (1_609_459_300_000, "ok"),
            (1_609_459_400_123, "in milliseconds"),
            (9_999_999_999_000, "last second"),
        ]
    );
    // WhatsApp stores milliseconds, and the seconds wtsexporter writes carry
    // them as a fraction, so every time has milliseconds, whole or not.
    assert!(
        doc.messages
            .iter()
            .all(|m| m.time_precision == message_ir::TimePrecision::Milliseconds),
        "{:?}",
        doc.messages
            .iter()
            .map(|m| m.time_precision)
            .collect::<Vec<_>>()
    );
    assert_ne!(
        doc.messages[1].guid, doc.messages[2].guid,
        "the same text in the same second is two messages"
    );
}

/// `tests/fixtures/senders.json`, converted with the owner `+15555550100`,
/// keyed by chat identifier.
fn senders_fixture() -> std::collections::BTreeMap<String, message_ir::ConversationDocument> {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/senders.json");
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fixture).expect("read fixture"))
            .expect("parse fixture");
    convert_with_owner(&json, Some("+15555550100")).1
}

/// Each participant as (identity, type, name).
fn roster(
    doc: &message_ir::ConversationDocument,
) -> Vec<(Option<&str>, Option<message_ir::IdentityType>, Option<&str>)> {
    doc.conversation
        .participants
        .iter()
        .map(|p| {
            (
                p.identity.as_deref(),
                p.identity_type,
                p.display_name.as_deref(),
            )
        })
        .collect()
}

/// The message in `doc` whose text is `text`, as (sender identity, sender
/// name).
fn sender_of<'a>(
    doc: &'a message_ir::ConversationDocument,
    text: &str,
) -> (Option<&'a str>, Option<&'a str>) {
    let msg = message_of(doc, text);
    (
        msg.sender_identity.as_deref(),
        msg.sender_display_name.as_deref(),
    )
}

/// A group sender's number comes only from a `sender_jid` that is a phone
/// id, and `sender` is never read as a number: its 15 digits on a message
/// from an `@lid` id are not a phone number. The name is the first of
/// `sender_contact_name`, `sender` when it is a name, and
/// `sender_push_name`. A sender whose `sender_jid` is still an `@lid` id
/// has no number, and is written under that id, typed `other` (#1092), so
/// a sender with no name keeps their sender.
#[test]
fn a_group_sender_has_the_number_of_a_phone_id_and_the_first_name_present() {
    let documents = senders_fixture();
    let club = &documents["120363042222222222@g.us"];

    assert_eq!(
        sender_of(club, "From a member with a contact name"),
        (Some("+15555550133"), Some("Ada Lovelace")),
        "the contact name comes before sender and the push name"
    );
    assert_eq!(
        sender_of(club, "From someone who is not a member"),
        (Some("+15555550155"), Some("Cy")),
        "digits in sender are not a name, so the push name is"
    );
    assert_eq!(
        sender_of(club, "From an @lid id the backup maps to a phone id"),
        (Some("+15555550166"), None),
        "the phone id behind an @lid id gives the number, and the lid digits in sender give none"
    );
    assert_eq!(
        sender_of(club, "From an @lid id the backup cannot map"),
        (Some("123456789012345@lid"), Some("Lid Person")),
        "an @lid id is no phone number"
    );
    assert_eq!(
        sender_of(club, "From an @lid id with no name"),
        (Some("555555555555555@lid"), None)
    );
    assert_eq!(
        sender_of(club, "From a sender named in sender"),
        (Some("+15555550177"), Some("Dee")),
        "a name in sender comes before the push name"
    );
    assert_eq!(
        sender_of(club, "From nobody the backup names"),
        (None, None)
    );
}

/// A group has one participant per member, with a number and a name where
/// the JSON has them, a member who never wrote included. Each sender who is
/// not a member is added. The owner's own member entry is left out, because
/// the owner is never a participant of their own conversation.
#[test]
fn a_group_has_its_members_and_its_other_senders_as_participants() {
    use message_ir::IdentityType::{Other, Phone};
    let documents = senders_fixture();
    let club = &documents["120363042222222222@g.us"];
    assert_eq!(
        roster(club),
        vec![
            (Some("+15555550133"), Some(Phone), Some("Ada Lovelace")),
            (Some("+15555550144"), Some(Phone), Some("Benny")),
            (Some("123456789012345@lid"), Some(Other), Some("Lid Person")),
            (Some("+15555550155"), Some(Phone), Some("Cy")),
            (Some("+15555550166"), Some(Phone), None),
            (Some("+15555550177"), Some(Phone), Some("Dee")),
            (Some("555555555555555@lid"), Some(Other), None),
            // Another person with the same name stays another participant.
            (Some("666666666666666@lid"), Some(Other), Some("Lid Person")),
        ]
    );
}

/// A group whose `members` is `null` had no member table to read, so its
/// senders are all its participants.
#[test]
fn a_group_with_no_member_table_has_its_senders_as_participants() {
    use message_ir::IdentityType::Phone;
    let documents = senders_fixture();
    assert_eq!(
        roster(&documents["120363042333333333@g.us"]),
        vec![(Some("+15555550188"), Some(Phone), Some("Ed Example"))]
    );
}

/// A one-to-one chat's participant and the sender of its incoming messages
/// is the peer, named by the chat.
#[test]
fn a_one_to_one_participant_has_the_chat_name() {
    use message_ir::IdentityType::Phone;
    let documents = senders_fixture();
    let sam = &documents["+15555550122"];
    assert_eq!(
        roster(sam),
        vec![(Some("+15555550122"), Some(Phone), Some("Sam Example"))]
    );
    assert_eq!(
        sender_of(sam, "Hello from Sam"),
        (Some("+15555550122"), Some("Sam Example"))
    );
}

/// A row with a time no calendar can hold is dropped whole, so its sender
/// doesn't join the group's participants either.
#[test]
fn a_dropped_row_adds_no_participant() {
    let mut kept = row("G1", 1_609_632_000.into(), "Group hello".into());
    kept["sender_jid"] = "15555550133@s.whatsapp.net".into();
    let mut dropped = row("G2", 1e18.into(), "never".into());
    dropped["sender_jid"] = "15555550144@s.whatsapp.net".into();
    let json = serde_json::json!({
        "120363042111111111@g.us": {
            "name": "Family Chat",
            "members": null, "messages": { "G1": kept, "G2": dropped }
        }
    });

    let (_, documents) = convert_to_documents(&json);
    let identities: Vec<_> = roster(&documents["120363042111111111@g.us"])
        .into_iter()
        .map(|(identity, _, _)| identity)
        .collect();
    assert_eq!(identities, vec![Some("+15555550133")]);
}

/// A `result.json` from upstream WhatsApp Chat Exporter has no sender ids,
/// names or group members, and is refused with a message that names the
/// fork, rather than read the old way. A file that lacks only `members`, or
/// only one sender field, is refused the same way.
#[test]
fn a_json_without_the_forks_fields_is_refused() {
    let mut upstream_row = row("K1", 1_609_459_200.into(), "Hello".into());
    for field in [
        "sender_jid",
        "sender_lid",
        "sender_contact_name",
        "sender_push_name",
    ] {
        upstream_row.as_object_mut().unwrap().remove(field);
    }
    let mut no_push_name = row("K1", 1_609_459_200.into(), "Hello".into());
    no_push_name
        .as_object_mut()
        .unwrap()
        .remove("sender_push_name");
    let cases = [
        serde_json::json!({
            "15555550122@s.whatsapp.net": { "name": "Sam", "messages": { "K1": upstream_row } }
        }),
        serde_json::json!({
            "15555550122@s.whatsapp.net": {
                "name": "Sam",
                "messages": { "K1": row("K1", 1_609_459_200.into(), "Hello".into()) }
            }
        }),
        serde_json::json!({
            "15555550122@s.whatsapp.net": {
                "name": "Sam", "members": null, "messages": { "K1": no_push_name }
            }
        }),
    ];
    for json in cases {
        let dir = tempfile::tempdir().expect("tempdir");
        let json_path = dir.path().join("result.json");
        fs::write(&json_path, json.to_string()).expect("write json");
        let err = convert_json(ConvertRequest {
            json_path: &json_path,
            output: &dir.path().join("out"),
            transforms: ExportTransforms::none(),
            media_search_roots: &[],
            owner_identity: None,
            backup_taken_at_unix_ms: None,
            output_format: OutputFormat::Json,
            cancel: None,
            resume: false,
            issues: None,
        })
        .expect_err("refused");
        let text = format!("{err:#}");
        assert!(
            text.contains("messagecrate/WhatsApp-Chat-Exporter"),
            "{json}: {text}"
        );
    }
}

/// A number or a boolean in `data` is the message's text, a caption the
/// text doesn't already hold is added below it, and the text wtsexporter
/// writes for media that was not in the backup is neither text nor an
/// attachment.
#[test]
fn a_body_that_is_not_a_string_is_kept_and_missing_media_is_dropped() {
    let mut missing_flag = row("K3", 1_609_459_202.into(), "The media is missing".into());
    missing_flag["media"] = true.into();
    let mut missing_path = row("K4", 1_609_459_203.into(), "caption".into());
    missing_path["media"] = "The media is missing".into();
    let mut captioned = row("K5", 1_609_459_204.into(), "Look".into());
    captioned["caption"] = "the view".into();
    let mut caption_in_text = row("K6", 1_609_459_205.into(), "Look at the view".into());
    caption_in_text["caption"] = "the view".into();
    let json = serde_json::json!({
        "15555550122@s.whatsapp.net": {
            "name": "Sam Example",
            "members": null, "messages": {
                "K1": row("K1", 1_609_459_200.into(), 42.into()),
                "K2": row("K2", 1_609_459_201.into(), true.into()),
                "K3": missing_flag,
                "K4": missing_path,
                "K5": captioned,
                "K6": caption_in_text,
            }
        }
    });

    let (report, documents) = convert_to_documents(&json);
    let messages = &documents["+15555550122"].messages;
    let texts: Vec<&str> = messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "42",
            "true",
            "",
            "caption",
            "Look\nthe view",
            "Look at the view"
        ]
    );
    assert!(messages.iter().all(|m| m.attachments.is_empty()));
    assert_eq!(report.extra(message_crate_core::ATTACHMENTS_MISSING), 0);
}

/// Older dumps put the media path straight in `media` rather than setting
/// `media: true` and putting it in `data`; the file is copied all the same.
#[test]
fn a_media_path_in_the_media_field_is_copied() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("photo.jpg"), b"fake-jpeg").expect("write media");
    let mut photo = row("K1", 1_609_459_200.into(), serde_json::Value::Null);
    photo["media"] = "photo.jpg".into();
    let json = serde_json::json!({
        "15555550122@s.whatsapp.net": { "name": "Sam Example", "members": null, "messages": { "K1": photo } }
    });
    let json_path = dir.path().join("result.json");
    fs::write(&json_path, json.to_string()).expect("write json");

    let out = dir.path().join("out");
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: &out,
        transforms: ExportTransforms::none(),
        media_search_roots: &[dir.path().to_path_buf()],
        owner_identity: None,
        backup_taken_at_unix_ms: None,
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");
    assert_eq!(report.attachments_saved, 1);
}

/// A photo-only message, as wtsexporter writes it, with its file at
/// `photo.jpg` under the conversion's search root.
fn photo_only_chat() -> serde_json::Value {
    let mut photo = row("K1", 1_609_459_200.into(), "photo.jpg".into());
    photo["media"] = true.into();
    photo["mime"] = "image/jpeg".into();
    serde_json::json!({
        "15555550122@s.whatsapp.net": { "name": "Sam Example", "members": null, "messages": { "K1": photo } }
    })
}

/// A media file that is not found stays on the message, marked
/// `file_missing`, so a photo-only message does not become an empty one.
/// Finding the file later gives the message the same GUID, so a second
/// import with the media in place lands on the same message.
#[test]
fn a_media_file_not_found_is_kept_as_file_missing_and_the_guid_does_not_change() {
    let (report, documents) = convert_to_documents(&photo_only_chat());
    let missing = &documents["+15555550122"].messages[0];
    assert_eq!(missing.attachments.len(), 1);
    assert_eq!(
        missing.attachments[0].missing_reason.as_deref(),
        Some("file_missing")
    );
    assert_eq!(
        missing.attachments[0].original_name.as_deref(),
        Some("photo.jpg")
    );
    assert_eq!(report.extra(message_crate_core::ATTACHMENTS_MISSING), 1);

    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("photo.jpg"), b"fake-jpeg").expect("write media");
    let json_path = dir.path().join("result.json");
    fs::write(&json_path, photo_only_chat().to_string()).expect("write json");
    let out = dir.path().join("out");
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: &out,
        transforms: ExportTransforms::none(),
        media_search_roots: &[dir.path().to_path_buf()],
        owner_identity: None,
        backup_taken_at_unix_ms: None,
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");
    assert_eq!(report.attachments_saved, 1);
    let found = message_ir_format::read_conversation_json(&out.join("+15555550122__whatsapp.json"))
        .expect("read back");
    let found = &found.messages[0];
    assert_eq!(found.attachments[0].missing_reason, None);
    assert_eq!(found.guid, missing.guid);
}

/// A one-to-one chat keyed by an internal `@lid` id has no phone number to
/// write. Its one participant is the raw id typed `other`, so the server
/// does not read the `@` in it as an email address (#1141).
#[test]
fn a_lid_chat_has_its_id_as_an_other_participant() {
    let json = serde_json::json!({
        "123456@lid": {
            "name": "Lid Peer",
            "members": null, "messages": { "L1": row("L1", 1_609_459_200.into(), "Hello".into()) }
        }
    });

    let (_, documents) = convert_to_documents(&json);
    let doc = &documents["123456@lid"];
    let participants: Vec<_> = doc
        .conversation
        .participants
        .iter()
        .map(|p| (p.identity.as_deref(), p.identity_type))
        .collect();
    assert_eq!(
        participants,
        vec![(Some("123456@lid"), Some(message_ir::IdentityType::Other))]
    );
    assert_eq!(
        doc.messages[0].sender_identity.as_deref(),
        Some("123456@lid")
    );
}

/// Status updates (`status@broadcast`) and Channel posts (`@newsletter`) are
/// not conversations, so neither is written. Each skipped message is counted
/// in the run summary, so none is dropped silently (#1141).
#[test]
fn status_updates_and_channel_posts_are_skipped_and_counted() {
    let json = serde_json::json!({
        "status@broadcast": {
            "name": null,
            "members": null, "messages": {
                "S1": row("S1", 1_609_459_200.into(), "my status".into()),
                "S2": row("S2", 1_609_459_201.into(), "another status".into()),
            }
        },
        "120363000000000001@newsletter": {
            "name": "A Channel",
            "members": null, "messages": { "N1": row("N1", 1_609_459_200.into(), "a post".into()) }
        },
        "15555550122@s.whatsapp.net": {
            "name": "Sam Example",
            "members": null, "messages": { "K1": row("K1", 1_609_459_200.into(), "Hello".into()) }
        }
    });

    let (report, documents) = convert_to_documents(&json);
    assert_eq!(documents.keys().collect::<Vec<_>>(), vec!["+15555550122"]);
    assert_eq!(report.conversations, 1);
    assert_eq!(report.messages, 1);
    assert_eq!(report.extra(crate::emit::SKIPPED_STATUS_UPDATES), 2);
    assert_eq!(report.extra(crate::emit::SKIPPED_CHANNEL_POSTS), 1);
}

/// The guid of the message in `doc` whose text is `text`.
fn guid_of<'a>(doc: &'a message_ir::ConversationDocument, text: &str) -> &'a str {
    &message_of(doc, text).guid
}

/// The message in `doc` whose text is `text`.
fn message_of<'a>(
    doc: &'a message_ir::ConversationDocument,
    text: &str,
) -> &'a message_ir::IrMessage {
    doc.messages
        .iter()
        .find(|m| m.text == text)
        .unwrap_or_else(|| panic!("no message {text:?}"))
}

/// A quoted reply links to the message it quotes in the same chat, found by
/// `reply_key_id` equal to that message's `full_key_id`. On an iPhone two
/// messages can share the 17-character `key_id`, so the whole key decides
/// which one is quoted. A reply whose quoted message is not in the chat, or
/// one from a JSON with no `reply_key_id`, is still a reply, with no
/// link, and a key that names a message in another chat links nothing.
#[test]
fn a_reply_links_to_the_message_it_quotes_in_the_same_chat() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replies.json");
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fixture).expect("read fixture"))
            .expect("parse fixture");

    let (_, documents) = convert_to_documents(&json);
    let sam = &documents["+15555550122"];
    let saturday = guid_of(sam, "Or Saturday");

    assert_eq!(message_of(sam, "Lunch on Friday?").reply_to, None);
    assert_eq!(message_of(sam, "Or Saturday").reply_to, None);
    assert_eq!(
        message_of(sam, "Saturday works").reply_to,
        Some(message_ir::ReplyTo {
            guid: Some(saturday.to_string()),
            part_index: None,
        }),
        "the whole key picks the second of two messages that share a key_id"
    );
    let fields: Vec<&str> = message_of(sam, "Saturday works")
        .source
        .as_ref()
        .expect("source")
        .fields
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        vec!["jid", "key_id"],
        "reply_to records the reply, so source.fields carries no copy of it"
    );
    let no_link = Some(message_ir::ReplyTo {
        guid: None,
        part_index: None,
    });
    assert_eq!(
        message_of(sam, "About that photo from last year").reply_to,
        no_link,
        "the quoted message is not in the export"
    );
    assert_eq!(
        message_of(sam, "From an older export").reply_to,
        no_link,
        "a JSON with no reply_key_id"
    );

    let ada = &documents["+15555550133"];
    assert_eq!(
        message_of(ada, "Did Sam say Saturday?").reply_to,
        no_link,
        "a key from another chat links nothing"
    );
}
