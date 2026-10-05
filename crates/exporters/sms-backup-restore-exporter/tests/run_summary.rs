//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or a report that dropped a skip count or a file's parse
//! error, passed them.

use crate::emit::{ConvertExportArgs, convert_export};
use message_crate_core::testutil::{assert_run_wrote_jsonl, collect_issues, jsonl_run_config};
use message_crate_core::{ExportTransforms, OutputFormat, SmsBackupRestoreConfig, SourceConfig};
use std::fs;
use std::path::Path;

/// One row for every reason a record is skipped, around two good messages,
/// one of them repeated.
const SKIPS_XML: &str = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
<smses count="9">
  <sms address="+15555550101" date="1400773261000" type="1" body="hello" contact_name="Sam" />
  <sms address="+15555550101" date="1400773261000" type="1" body="hello" contact_name="Sam" />
  <sms address="+15555550101" date="1400773321000" type="2" body="hey&#0;" contact_name="Sam" />
  <sms address="+15555550101" date="soon" type="1" body="bad date" />
  <sms address="" date="1400773500000" type="1" body="no address" />
  <sms address="+15555550101" date="1400773600000" type="9" body="unknown type" />
  <sms address="+15555550101" date="1400773700000" type="3" body="draft" />
  <mms date="1400773800000" msg_box="1">
    <parts><part seq="0" ct="text/plain" text="nobody" /></parts>
  </mms>
  <mms date="1400773900000" msg_box="1" address="+15555550101" contact_name="Sam">
    <parts>
      <part seq="0" ct="text/plain" text="broken photo" />
      <part seq="1" ct="image/jpeg" name="pic.jpg" data="%%% not base64 %%%" />
    </parts>
    <addrs><addr address="+15555550101" type="137" charset="106" /></addrs>
  </mms>
</smses>
"#;

/// A backup directory holding `SKIPS_XML` and a file cut off mid-element.
fn backup_directory(root: &Path) -> std::path::PathBuf {
    let input = root.join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("sms-1.xml"), SKIPS_XML).unwrap();
    fs::write(
        input.join("sms-2-broken.xml"),
        "<smses count=\"1\">\n  <sms address=\"+15555550102\" date=\"1\" body=\"cut",
    )
    .unwrap();
    input
}

#[test]
fn run_writes_the_conversation_and_reports_every_skip_and_error() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_directory(tmp.path());
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(
        &[&input],
        &output,
        SourceConfig::SmsBackupRestore(SmsBackupRestoreConfig {
            owner_phones: vec!["+15555550100".into()],
        }),
    );
    let issues = collect_issues(&mut config);

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("\"hello\""), "{written}");
    assert!(written.contains("\"hey\""), "{written}");
    assert!(written.contains("broken photo"), "{written}");
    for line in [
        "  Skipped 1 message with an invalid date",
        "  Dropped 1 repeated copy of a message",
        "  Read 2 MMS",
        "  Left out 1 message part that could not be read",
        "  Left out 1 character reference that is not a character",
        "  Skipped 1 draft or unsent message",
        "  Skipped 1 MMS with no participants",
        "  Skipped 1 message with no usable address",
        "  Skipped 1 message of an unknown type",
        "  Read 7 SMS",
    ] {
        assert!(
            result.messages.iter().any(|l| l == line),
            "{line:?} missing from {:?}",
            result.messages
        );
    }
    let errors: Vec<&String> = result
        .messages
        .iter()
        .filter(|l| l.starts_with("  error: "))
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", result.messages);
    assert!(errors[0].contains("sms-2-broken.xml"), "{}", errors[0]);
    // The file it could not read is an Import Error naming it (#1626), and
    // each message kept with something left out is a note naming the file
    // and the message (#1707).
    let issues = issues.lock().unwrap();
    let rows: Vec<(&str, &str, &str, &str)> = issues
        .iter()
        .map(|i| {
            (
                i.kind.as_str(),
                i.step.as_str(),
                i.item.as_str(),
                i.reason.as_str(),
            )
        })
        .collect();
    let file = |name: &str| input.join(name).display().to_string();
    let (sms_1, broken) = (file("sms-1.xml"), file("sms-2-broken.xml"));
    let hey = format!("{sms_1} (message of 2014-05-22T15:42:01Z with +15555550101)");
    let photo = format!("{sms_1} (message of 2014-05-22T15:51:40Z with +15555550101)");
    assert_eq!(
        rows,
        [
            ("error", "parse", broken.as_str(), rows[0].3),
            (
                "note",
                "parse",
                hey.as_str(),
                "1 character reference in this message is not a character and was left out. \
                 The message itself is kept."
            ),
            (
                "note",
                "parse",
                photo.as_str(),
                "1 part of this message could not be read and was left out. The message \
                 itself is kept."
            ),
        ],
    );
}

/// Convert logs what the read counted the moment the read returns, and an
/// import's summary gives the same counts after its write. Both give every
/// count and every error in the same words and the same order (#1700).
#[test]
fn convert_and_import_word_every_count_and_error_alike() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_directory(tmp.path());
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[&input],
        &output,
        SourceConfig::SmsBackupRestore(SmsBackupRestoreConfig {
            owner_phones: vec!["+15555550100".into()],
        }),
    );
    let imported = crate::run(&config).expect("run").messages;
    let owner = ["+15555550100".to_string()];
    let (_, report) = crate::read_backup(
        &input,
        crate::ReadOptions {
            owner_phones: &owner,
            attachments_dir: None,
            spool: None,
            exclude_dir: None,
            media: media::MediaMode::Disabled,
            compress: media::CompressOptions::default(),
            log: None,
            progress: None,
            cancel: None,
        },
    )
    .expect("read");
    let converted = report.log_lines();
    assert_eq!(converted.len(), 11, "{converted:?}");
    // Every line Convert logs is a line of the import's summary, in the same
    // order.
    let mut rest = imported.iter().map(|l| l.trim_start());
    for line in &converted {
        assert!(
            rest.any(|l| l == line),
            "{line:?} missing from, or out of order in, {imported:?}"
        );
    }
}

#[test]
fn the_report_counts_conversations_and_directions() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_directory(tmp.path());
    let cache = tempfile::tempdir().unwrap();
    let report = convert_export(ConvertExportArgs {
        input: &input,
        output_dir: &tmp.path().join("out"),
        scratch_dir: cache.path(),
        owner_phones: &["+15555550100".into()],
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Jsonl,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");

    assert_eq!(report.conversations, 1);
    assert_eq!(report.sent, 1);
    assert_eq!(report.received, 2);
    assert_eq!(report.skipped_invalid_date, 1);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
}
