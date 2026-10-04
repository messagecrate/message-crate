//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_json` directly, so a `run()` that
//! wrote nothing, or a report that dropped the bad-date count, passed them.

use message_crate_core::testutil::{assert_run_wrote_jsonl, collect_issues, jsonl_run_config};
use message_crate_core::{RunIssue, SourceConfig, WhatsappConfig};
use std::fs;

/// A message with the given `timestamp` JSON value.
fn message(key: &str, timestamp: &str, text: &str) -> String {
    format!(
        r#""{key}": {{
        "from_me": false, "timestamp": {timestamp}, "time": "00:00", "key_id": "{key}",
        "data": "{text}", "sender": null, "media": false, "mime": null,
        "caption": null, "sticker": false, "reply": null, "reactions": {{}}
      }}"#
    )
}

#[test]
fn run_writes_the_conversation_and_counts_the_bad_date_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let json = tmp.path().join("result.json");
    // One message without a timestamp, and one whose timestamp no calendar
    // can show.
    let messages = [
        message("AAA", "1609459200", "Hello from Sam"),
        message("BBB", "null", "No time"),
        message("CCC", "1e300", "Far future"),
    ]
    .join(",\n");
    fs::write(
        &json,
        format!(
            r#"{{ "15555550122@s.whatsapp.net": {{
    "name": "Sam Example", "type": "ANDROID",
    "messages": {{ {messages} }}
  }} }}"#
        ),
    )
    .unwrap();
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[],
        &output,
        SourceConfig::Whatsapp(WhatsappConfig {
            json: Some(json),
            ..WhatsappConfig::default()
        }),
    );

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Sam"), "{written}");
    assert!(!written.contains("No time"), "{written}");
    assert!(!written.contains("Far future"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  skipped 2 invalid-date rows"),
        "{:?}",
        result.messages
    );
    // No owner was given, so the header records none.
    assert!(written.contains(r#""owner_identity":null"#), "{written}");
}

/// The number from the form is stamped on the export header, under the
/// server's handle key, so every message is held at it (ADR 0015). It is a
/// header value, not a participant: the holder is never listed there.
#[test]
fn run_records_the_form_owner_on_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    let json = tmp.path().join("result.json");
    fs::write(
        &json,
        format!(
            r#"{{ "15555550122@s.whatsapp.net": {{
    "name": "Sam Example", "type": "ANDROID",
    "messages": {{ {} }}
  }} }}"#,
            message("AAA", "1609459200", "Hello from Sam")
        ),
    )
    .unwrap();
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[],
        &output,
        SourceConfig::Whatsapp(WhatsappConfig {
            json: Some(json),
            owner_phone: Some("+1 555 555 0100".into()),
            ..WhatsappConfig::default()
        }),
    );

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(
        written.contains(r#""owner_identity":"+15555550100""#),
        "{written}"
    );
    let header = written.lines().next().unwrap();
    assert!(!header.contains(r#""identity":"+15555550100""#), "{header}");
}

/// A message whose media file is not in the backup is kept without it, and
/// the run sends a note naming the file (#1535).
#[test]
fn run_sends_a_note_for_a_media_file_the_backup_does_not_hold() {
    let tmp = tempfile::tempdir().unwrap();
    let json = tmp.path().join("result.json");
    fs::write(
        &json,
        r#"{ "15555550122@s.whatsapp.net": {
    "name": "Sam Example", "type": "ANDROID",
    "messages": { "AAA": {
        "from_me": false, "timestamp": 1609459200, "time": "00:00", "key_id": "AAA",
        "data": "Media/WhatsApp Images/IMG-1.jpg", "sender": null, "media": true,
        "mime": "image/jpeg", "caption": "A photo", "sticker": false, "reply": null,
        "reactions": {}
    } }
  } }"#,
    )
    .unwrap();
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(
        &[],
        &output,
        SourceConfig::Whatsapp(WhatsappConfig {
            json: Some(json),
            ..WhatsappConfig::default()
        }),
    );
    config.media.mode = media::MediaMode::Clone;
    let issues = collect_issues(&mut config);

    let result = crate::run(&config).expect("run");

    assert_run_wrote_jsonl(&result, &output, 1);
    assert_eq!(
        *issues.lock().unwrap(),
        [RunIssue {
            kind: "note".into(),
            step: "parse".into(),
            item: "Media/WhatsApp Images/IMG-1.jpg".into(),
            reason:
                "This attachment's file is not in the backup, so its message is kept without it."
                    .into(),
        }]
    );
}
