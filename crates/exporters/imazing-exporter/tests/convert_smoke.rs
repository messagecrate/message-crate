use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_crate_core::testutil::{assert_csv_row, assert_jsonl_resumes, csv_rows};
use message_crate_core::{ExportReport, ExportTransforms, OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};

fn convert(input: &Path, output: &Path) -> Result<ExportReport> {
    convert_export(ConvertExportArgs {
        input,
        output,
        timezone: Some("UTC"),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
        issues: None,
    })
}

#[test]
fn convert_messages_keys_the_chat_by_its_number() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let messages = fixture.join("messages.csv");
    assert!(messages.is_file(), "missing {}", messages.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let report = convert(&messages, tmp.path()).expect("convert");

    assert_eq!(report.conversations, 1);
    assert_eq!(report.messages, 8);
    assert_eq!(report.extra(crate::emit::MESSAGES_FILES), 1);
    assert_eq!(report.extra(crate::emit::WHATSAPP_FILES), 0);
    assert_eq!(report.extra(message_crate_core::NAME_ONLY_CHAT), 0);

    let out = tmp.path().join("+13215550100.csv");
    let body = fs::read_to_string(&out).expect("read csv");
    assert!(body.contains("imazing"));
    assert!(body.contains("iMazing"));
    assert!(body.contains("3.5.5"));

    // The first three messages, read back by column. The substring assertions above
    // are satisfied by the export metadata, so they hold even if every row was
    // dropped.
    assert_csv_row(
        &out,
        &[
            ("text", "Hello from Bob"),
            ("direction", "incoming"),
            ("service", "sms"),
            ("sender_display_name", "Bob Sample"),
            // iMazing writes whole seconds only.
            ("time_precision", "seconds"),
        ],
    );
    assert_csv_row(
        &out,
        &[
            ("text", "Hi Bob"),
            ("direction", "outgoing"),
            ("time_precision", "seconds"),
        ],
    );
    // The third row is an iMessage carrying an attachment, so it proves both
    // that the service column follows the source and that the attachment file
    // name reached the row rather than only the directory.
    let rows = csv_rows(&out);
    let photo = rows
        .iter()
        .find(|r| r.get("text").is_some_and(|t| t == "Photo"))
        .expect("the attachment message must be in the export");
    assert_eq!(photo.get("service").map(String::as_str), Some("imessage"));
    assert!(
        photo
            .get("attachments_json")
            .is_some_and(|a| a.contains("image000000.jpg")),
        "the attachment must be recorded on its own message, not merely \
         somewhere in the file: {photo:#?}"
    );
}

/// The fixture's reaction, reply, deleted and edited rows reach the
/// message's own fields (#2030): each reaction under its reactor's display
/// name, `Me` being the account holder; a reply linked to the message of
/// the same chat whose Message Date it quotes, and a reply quoting a date
/// the export lacks left a reply with no link; a Deleted Date marking the
/// message Deleted in the source app, never Unsent; and an Edited Date
/// giving one earlier version with no text and the edit's time. None of the
/// four cells is a vendor leftover any more.
#[test]
fn reactions_replies_deletions_and_edits_reach_the_message() {
    use message_ir::{Deletion, EarlierVersion, Reaction, ReplyTo};

    let messages = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/messages.csv");
    let tmp = tempfile::tempdir().expect("tempdir");
    convert_export(ConvertExportArgs {
        input: &messages,
        output: tmp.path(),
        timezone: Some("UTC"),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");
    let doc = message_ir_format::read_conversation_json(&tmp.path().join("+13215550100.json"))
        .expect("read the conversation back");
    let by_text = |text: &str| {
        doc.messages
            .iter()
            .find(|m| m.text == text)
            .unwrap_or_else(|| panic!("no message reads {text:?}"))
    };

    let lunch = by_text("Lunch tomorrow?");
    let reaction = |emoji: &str, name: &str, is_from_me: bool| Reaction {
        part_index: 0,
        kind: "emoji".into(),
        emoji: Some(emoji.into()),
        is_from_me,
        reactor_identity: None,
        reactor_display_name: Some(name.into()),
    };
    assert_eq!(
        lunch.reactions,
        vec![
            reaction("\u{2764}\u{fe0f}", "Me", true),
            reaction("\u{1f44d}", "Bob Sample", false),
        ]
    );
    assert_eq!(
        by_text("Yes at noon").reply_to,
        Some(ReplyTo {
            guid: Some(lunch.guid.clone()),
            part_index: None,
        })
    );
    assert_eq!(
        by_text("That was before this export").reply_to,
        Some(ReplyTo {
            guid: None,
            part_index: None,
        })
    );
    assert_eq!(
        by_text("Deleted later").deletion,
        Some(Deletion::DeletedInSourceApp)
    );
    assert_eq!(
        by_text("See you at noon").edits,
        vec![EarlierVersion {
            part_index: 0,
            text: None,
            // 2020-01-01 12:10:00 UTC, the Edited Date.
            edited_at_unix_ms: Some(1_577_880_600_000),
        }]
    );
    for message in &doc.messages {
        assert_eq!(message.deletion.is_some(), message.text == "Deleted later");
        assert_eq!(message.edits.is_empty(), message.text != "See you at noon");
        assert_eq!(
            message.reactions.is_empty(),
            message.text != "Lunch tomorrow?"
        );
        assert_eq!(
            message.reply_to.is_some(),
            ["Yes at noon", "That was before this export"].contains(&message.text.as_str()),
            "{}",
            message.text
        );
        let fields = message.source.as_ref().map(|source| &source.fields);
        for key in ["reactions", "replying_to", "edited_date", "deleted_date"] {
            assert!(
                fields.is_none_or(|fields| !fields.contains_key(key)),
                "{key} is the message's own field, not a vendor leftover: {}",
                message.text
            );
        }
    }
}

#[test]
fn convert_whatsapp_csv_direct() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let whatsapp = fixture.join("whatsapp.csv");
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = convert(&whatsapp, tmp.path()).expect("convert");

    assert_eq!(report.conversations, 1);
    assert_eq!(report.messages, 3);
    assert_eq!(report.extra(crate::emit::WHATSAPP_FILES), 1);
    let out = tmp.path().join("+13215550100__whatsapp.csv");
    let body = fs::read_to_string(&out).expect("read csv");
    assert!(body.contains("WhatsApp"));

    // `contains("forwarded")` matched the `forwarded` key that every row's
    // `source_fields_json` carries whatever its value, so it passed even when
    // no message was actually marked forwarded. The flag is read off the row
    // that should carry it instead, together with the other two messages, so a
    // parse that lost the column or put the flag on the wrong message fails.
    assert_csv_row(
        &out,
        &[
            ("text", "Hello on WhatsApp"),
            ("direction", "incoming"),
            ("time_precision", "seconds"),
        ],
    );
    assert_csv_row(
        &out,
        &[("text", "Reply on WhatsApp"), ("direction", "outgoing")],
    );
    let rows = csv_rows(&out);
    let forwarded = rows
        .iter()
        .find(|r| r.get("text").is_some_and(|t| t == "Forwarded photo"))
        .expect("the forwarded message must be in the export");
    let source: serde_json::Value =
        serde_json::from_str(forwarded.get("source_fields_json").expect("source fields"))
            .expect("source_fields_json is JSON");
    assert_eq!(
        source.get("forwarded").and_then(|v| v.as_str()),
        Some("Yes"),
        "the forwarded flag belongs to this message: {forwarded:#?}"
    );
    assert_eq!(
        source.get("attachment_info").and_then(|v| v.as_str()),
        Some("12.34 KB")
    );
}

#[test]
fn convert_export_root_recursively_keeps_services_separate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/export_root");
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = convert(&root, tmp.path()).expect("convert");

    assert_eq!(report.extra(crate::emit::MESSAGES_FILES), 2);
    assert_eq!(report.extra(crate::emit::WHATSAPP_FILES), 1);
    assert!(report.conversations >= 3);
    assert!(tmp.path().join("+13215550100.csv").is_file());
    assert!(tmp.path().join("+13215550100__whatsapp.csv").is_file());
    // Silent Carol never sent a message, so the source records no address for
    // her. She is reported rather than given an invented number.
    let group = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.ends_with(".csv") && n.starts_with("group") && !n.contains("whatsapp"))
        .expect("group csv");
    let body = fs::read_to_string(tmp.path().join(group)).unwrap();
    assert!(body.contains("group"));
    assert!(body.contains("Notification") || body.contains("notification"));
    assert!(report.extra(crate::emit::UNRESOLVED_GROUP_PARTICIPANTS) >= 1);
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let messages = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/messages.csv");
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = assert_jsonl_resumes(tmp.path(), |resume| {
        convert_export(ConvertExportArgs {
            input: &messages,
            output: tmp.path(),
            timezone: Some("UTC"),
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
            issues: None,
        })
    });
    assert_eq!(report.conversations, 1);
}

/// An iMazing export records no date inside it, so the conversation file
/// says when iMazing wrote the CSV: the export date.
#[test]
fn the_backup_date_is_the_export_date_of_the_csv() {
    use message_crate_core::testutil::{
        TEST_BACKUP_TAKEN_AT_UNIX_MS, jsonl_backup_dates, set_modified_unix_ms,
    };
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let input = tempfile::tempdir().expect("tempdir");
    let messages = input.path().join("messages.csv");
    fs::copy(fixture.join("messages.csv"), &messages).expect("copy fixture");
    set_modified_unix_ms(&messages, TEST_BACKUP_TAKEN_AT_UNIX_MS);

    let output = tempfile::tempdir().expect("tempdir");
    convert_export(ConvertExportArgs {
        input: &messages,
        output: output.path(),
        timezone: Some("UTC"),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Jsonl,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");
    assert_eq!(
        jsonl_backup_dates(output.path()),
        vec![Some(TEST_BACKUP_TAKEN_AT_UNIX_MS)]
    );
}
