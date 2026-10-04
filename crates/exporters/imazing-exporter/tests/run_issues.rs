//! The rows `run()` sends to the issue sink as it reads an export: a CSV it
//! cannot read is an Import Error naming the file, and a Live Photo video two
//! rows name is a note naming the picture (#1626). Without a sink the desktop
//! app never heard of either.

use message_crate_core::testutil::{collect_issues, jsonl_run_config};
use message_crate_core::{ImazingConfig, RunIssue, SourceConfig};
use std::fs;
use std::path::Path;

/// `run()` over `input` into `output`, copying attachments, and the rows it sent.
fn run_collecting(input: &Path, output: &Path) -> Vec<RunIssue> {
    let mut config = jsonl_run_config(&[input], output, SourceConfig::Imazing(ImazingConfig {}));
    config.timezone = Some("UTC".into());
    config.media.mode = media::MediaMode::Clone;
    let issues = collect_issues(&mut config);
    crate::run(&config).expect("run");
    issues.lock().unwrap().clone()
}

#[test]
fn a_csv_the_run_cannot_read_is_an_import_error_naming_it() {
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unreadable_csv");
    let tmp = tempfile::tempdir().unwrap();

    let rows = run_collecting(&input, &tmp.path().join("out"));

    let broken =
        input.join("Messages/2020-01-02 09 00 00 - Carol Sample/Messages - Carol Sample.csv");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, "error");
    assert_eq!(rows[0].step, "parse");
    assert_eq!(rows[0].item, broken.display().to_string());
    assert!(rows[0].reason.contains("type"), "{rows:?}");
}

#[test]
fn a_live_photo_video_two_rows_name_is_a_note_naming_the_picture() {
    let tmp = tempfile::tempdir().unwrap();
    let chat = tmp.path().join("in/2020-01-01 12 00 00 - Bob");
    fs::create_dir_all(&chat).unwrap();
    let header = "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n";
    for (csv, text) in [("Messages.csv", "first"), ("Messages_2.csv", "second")] {
        fs::write(
            chat.join(csv),
            format!(
                "{header}Bob,2020-01-01 12:05:00,iMessage,Incoming,+15555550100,Bob,Read,,,{text},,IMG_0002.jpg,Image\n"
            ),
        )
        .unwrap();
    }
    fs::write(
        chat.join("2020-01-01 12 05 00 - Bob - IMG_0002.jpg"),
        "picture",
    )
    .unwrap();
    fs::write(
        chat.join("2020-01-01 12 05 00 - Bob - IMG_0002.MOV"),
        "video",
    )
    .unwrap();

    let rows = run_collecting(&tmp.path().join("in"), &tmp.path().join("out"));

    let picture = chat.join("2020-01-01 12 05 00 - Bob - IMG_0002.jpg");
    assert_eq!(
        rows,
        [RunIssue {
            kind: "note".into(),
            step: "parse".into(),
            item: picture.display().to_string(),
            reason: "2 rows name this picture; its Live Photo video goes to the first of them in the CSV".into(),
        }]
    );
}

/// Write one Messages CSV holding `rows` under a temporary input directory,
/// run over it, and return the CSV's path with the rows the run sent.
fn run_messages_csv(rows: &str) -> (String, Vec<RunIssue>) {
    let tmp = tempfile::tempdir().unwrap();
    let csv = tmp.path().join("in/Messages.csv");
    fs::create_dir_all(csv.parent().unwrap()).unwrap();
    fs::write(
        &csv,
        format!("Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n{rows}"),
    )
    .unwrap();
    let issues = run_collecting(&tmp.path().join("in"), &tmp.path().join("out"));
    (csv.display().to_string(), issues)
}

/// A chat that names its person with no address is kept under the name
/// alone, and the run says so for that chat (#1535).
#[test]
fn a_chat_kept_under_a_name_alone_is_a_note() {
    let (csv, rows) = run_messages_csv(
        "Mystery Person,2020-01-01 12:00:00,SMS,Incoming,,,Read,,,Hello,,,\n\
Mystery Person,2020-01-01 12:01:00,SMS,Outgoing,,,Read,,,Hi,,,\n",
    );
    assert_eq!(
        rows,
        [RunIssue {
            kind: "note".into(),
            step: "parse".into(),
            item: format!("{csv} (Mystery Person)"),
            reason: "This chat names its person with no phone number or email address, so the conversation is kept under the name alone.".into(),
        }]
    );
}

/// A group member the chat's name lists, whom no row pairs with an address,
/// is kept by name, and the run names each such member (#1535).
#[test]
fn a_group_member_kept_by_name_is_a_note_per_member() {
    let (csv, rows) = run_messages_csv(
        "Alice Example & Bob Example & Carol Silent,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice Example,Read,,,Hi,,,\n\
Alice Example & Bob Example & Carol Silent,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob Example,Read,,,Hey,,,\n",
    );
    assert_eq!(
        rows,
        [RunIssue {
            kind: "note".into(),
            step: "parse".into(),
            item: format!("{csv} (Carol Silent)"),
            reason: "The group's name lists this member, but no message gives their phone number or email address, so they are kept by name.".into(),
        }]
    );
}
