use super::*;
use message_ir::{IrImessage, UnsupportedSchemaVersion};

use crate::test_support::{MessageLine, conversation_header, message_line};

/// An incoming SMS from Sam, "hello", sent at 1400773261000.
fn from_sam(guid: &str) -> MessageLine {
    message_line(guid, "hello")
        .at(1_400_773_261_000)
        .sms()
        .sender("+15555550101")
        .sender_display_name("Sam")
}

#[test]
fn parses_ir_sms_without_imessage_bag() {
    let header = conversation_header("sms-backup-restore", "+15555550101")
        .participant("+15555550101", Some("Sam"))
        .to_string();
    let lines = [header, from_sam("g1").to_string()];
    let records = parse_ir_lines(lines).unwrap();
    assert_eq!(records.len(), 2);
    match &records[1] {
        ExportRecord::Message(m) => {
            assert_eq!(m.guid, "g1");
            assert!(!m.is_from_me);
            assert_eq!(m.text.as_deref(), Some("hello"));
            assert_eq!(m.service.as_deref(), Some("sms"));
            assert!(m.tapbacks.is_empty());
            assert!(m.reply_to.is_none());
        }
        _ => panic!("expected message"),
    }
}

/// The header and one incoming iMessage whose `subject` is `subject`,
/// or `null` for `None`.
fn message_with(subject: Option<&str>) -> MessageRecord {
    let header = conversation_header("imessage", "+15555550101")
        .participant("+15555550101", Some("Sam"))
        .to_string();
    let mut message = message_line("g1", "hello")
        .at(1_400_773_261_000)
        .sender("+15555550101")
        .sender_display_name("Sam");
    if let Some(subject) = subject {
        message = message.subject(subject);
    }
    let mut records = parse_ir_lines([header, message.to_string()]).unwrap();
    match records.pop() {
        Some(ExportRecord::Message(m)) => m,
        _ => panic!("expected message"),
    }
}

#[test]
fn a_subject_is_kept_and_an_empty_one_is_none() {
    assert_eq!(
        message_with(Some("Dinner on Friday")).subject.as_deref(),
        Some("Dinner on Friday")
    );
    assert_eq!(message_with(Some("")).subject, None);
    assert_eq!(message_with(None).subject, None);
}

/// A reaction names its reactor in `reactor_identity` and says in
/// `is_from_me` whether the owner reacted. Neither is taken from the
/// message reacted to (#1213).
#[test]
fn a_reaction_keeps_the_reactor_it_names() {
    let header = conversation_header("imessage", "+15555550101")
        .participant("+15555550101", Some("Ada"))
        .to_string();
    let reacted_to = |message: MessageLine| {
        message
            .at(1_400_773_261_000)
            .reaction(Reaction {
                part_index: 0,
                kind: "loved".into(),
                emoji: None,
                is_from_me: false,
                reactor_identity: Some("+15555550110".into()),
                reactor_display_name: Some("Sam".into()),
            })
            .reaction(Reaction {
                part_index: 2,
                kind: "emoji".into(),
                emoji: Some("🔥".into()),
                is_from_me: true,
                reactor_identity: None,
                reactor_display_name: Some("Me".into()),
            })
            .to_string()
    };
    // The owner's own message and Ada's, each reacted to by Sam and then
    // by the owner.
    let records = parse_ir_lines([
        header,
        reacted_to(message_line("g-mine", "hello").outgoing()),
        reacted_to(message_line("g-adas", "hello").sender("+15555550101")),
    ])
    .unwrap();
    let messages: Vec<_> = records
        .iter()
        .filter_map(|r| match r {
            ExportRecord::Message(m) => Some(m),
            ExportRecord::Conversation(_) => None,
        })
        .collect();
    assert_eq!(messages.len(), 2);
    for m in messages {
        let rows: Vec<_> = m
            .tapbacks
            .iter()
            .map(|t| {
                (
                    t.part_index,
                    t.kind.as_str(),
                    t.emoji.as_deref(),
                    t.is_from_me,
                    t.sender.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (0, "loved", None, false, Some("+15555550110")),
                (2, "emoji", Some("🔥"), true, None),
            ],
            "{}: Sam's reaction is Sam's and the owner's is the owner's",
            m.guid
        );
    }
}

/// The Apple Messages reader writes each reaction as a row of its own as
/// well as in the `reactions` of the message reacted to. The row is
/// not a message, and it carries no reaction of its own (#1213).
#[test]
fn a_reaction_row_is_not_a_message() {
    let header = conversation_header("imessage", "+15555550101")
        .participant("+15555550101", Some("Sam"))
        .to_string();
    let target = message_line("g-hi", "hi")
        .at(1_400_773_261_000)
        .outgoing()
        .to_string();
    let row = |guid: &str, kind: IrMessageKind, text: &str, action: &str| {
        message_line(guid, text)
            .at(1_400_773_262_000)
            .kind(kind)
            .sender("+15555550101")
            .sender_display_name("Sam")
            .imessage(IrImessage {
                associated_guid: Some("g-hi".into()),
                associated_part: Some(0),
                tapback_kind: Some("loved".into()),
                tapback_action: Some(action.into()),
                ..IrImessage::default()
            })
            .to_string()
    };
    let records = parse_ir_lines([
        header,
        target,
        row("g-love", IrMessageKind::Tapback, "Loved a message", "add"),
        row(
            "g-unlove",
            IrMessageKind::Tapback,
            "Removed Heart",
            "remove",
        ),
        row(
            "g-sticker",
            IrMessageKind::StickerTapback,
            "Reacted with a sticker",
            "add",
        ),
    ])
    .unwrap();
    let messages: Vec<_> = records
        .iter()
        .filter_map(|r| match r {
            ExportRecord::Message(m) => Some(m),
            ExportRecord::Conversation(_) => None,
        })
        .collect();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0].guid, "g-hi");
    assert!(messages[0].tapbacks.is_empty());
}

#[test]
fn parses_concatenated_ir_conversations() {
    let header = |chat: &str| {
        conversation_header("sms-backup-restore", chat)
            .participant(chat, None)
            .to_string()
    };
    let msg = |guid: &str, handle: &str| {
        message_line(guid, "hi")
            .at(1_400_773_261_000)
            .sms()
            .sender(handle)
            .to_string()
    };
    let records = parse_ir_lines([
        header("+15555550101"),
        msg("g1", "+15555550101"),
        header("+15555550102"),
        msg("g2", "+15555550102"),
    ])
    .unwrap();
    assert_eq!(records.len(), 4);
    match &records[0] {
        ExportRecord::Conversation(c) => assert_eq!(c.chat_identifier, "+15555550101"),
        _ => panic!("expected conversation"),
    }
    match &records[2] {
        ExportRecord::Conversation(c) => assert_eq!(c.chat_identifier, "+15555550102"),
        _ => panic!("expected conversation"),
    }
}

#[test]
fn parse_ir_lines_refuses_schema_3_as_a_failure() {
    let header = r#"{"schema_version":3,"export":{"source":"whatsapp","tool":"t","owner_identity":"+1","owner_display_name":"Me"},"conversation":{"chat_identifier":"+2","conversation_type":"individual","participants":[]}}"#;
    let failure = parse_ir_lines([header]).unwrap_err();
    assert_eq!(
        failure,
        ImportFailure::SchemaVersion {
            refusal: UnsupportedSchemaVersion { found: 3 },
            line: 1
        }
    );
}

#[test]
fn parse_ir_lines_reports_a_non_json_line_as_a_failure() {
    let failure = parse_ir_lines(["this is not json"]).unwrap_err();
    match failure {
        ImportFailure::NotJson { line, .. } => assert_eq!(line, 1),
        other => panic!("expected NotJson, got {other:?}"),
    }
}

#[test]
fn parse_ir_lines_reports_a_message_before_any_header_as_a_failure() {
    let failure = parse_ir_lines([r#"{"guid":"m1"}"#]).unwrap_err();
    match failure {
        ImportFailure::Invalid { line, detail } => {
            assert_eq!(line, 1);
            assert!(
                detail.contains("before the conversation header"),
                "{detail}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn parse_ir_lines_reports_a_message_with_an_impossible_timestamp_as_a_failure() {
    let header = conversation_header("sms-backup-restore", "+15555550101")
        .participant("+15555550101", Some("Sam"))
        .to_string();
    // The timestamp is the input under test, so the line stays written out.
    let msg = r#"{"guid":"g1","timestamp_unix_ms":9223372036854775807,"time_precision":"milliseconds","direction":"incoming","service":"sms","message_kind":"sms","sender_identity":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}"#;
    let failure = parse_ir_lines([header, msg.to_string()]).unwrap_err();
    match failure {
        ImportFailure::Invalid { line, .. } => assert_eq!(line, 2),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// A message the guid index cannot see would be stored again by every
/// retried batch (#1162), so the whole file is refused, naming every
/// line that has no guid, before anything is staged.
#[test]
fn parse_ir_lines_refuses_messages_without_a_guid_naming_every_line() {
    let header = conversation_header("sms-backup-restore", "+15555550101")
        .participant("+15555550101", Some("Sam"))
        .to_string();
    // The guids are the input under test, so the lines stay written out.
    let msg = |guid: &str| {
        format!(
            r#"{{"guid":"{guid}","timestamp_unix_ms":1400773261000,"time_precision":"milliseconds","direction":"incoming","service":"sms","message_kind":"sms","sender_identity":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}}"#
        )
    };
    let lines = [header.to_string(), msg("g1"), msg(""), msg("   ")];
    let failure = parse_ir_lines(lines).unwrap_err();
    assert_eq!(
        failure,
        ImportFailure::MissingGuid {
            lines: vec![3, 4],
            total: 2
        }
    );
}
