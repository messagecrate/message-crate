//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or dropped its summary, passed them.

use message_crate_core::testutil::{assert_run_wrote_jsonl, collect_issues, jsonl_run_config};
use message_crate_core::{RunIssue, SmsBackupPlusConfig, SourceConfig};
use std::fs;
use std::path::Path;

/// A flat SMS Backup+ message from Alice sent at `date_ms`.
fn eml(date_ms: &str, text: &str) -> String {
    format!(
        "From: alice@unknown.email\n\
         To: me@example.com\n\
         Subject: SMS with Alice\n\
         X-smssync-type: 1\n\
         X-smssync-address: 4075550107\n\
         X-smssync-date: {date_ms}\n\
         Content-Type: text/plain; charset=utf-8\n\
         \n\
         {text}\n"
    )
}

/// A directory with one message and one dated past any calendar.
fn backup_directory(root: &Path) -> std::path::PathBuf {
    let input = root.join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("1.eml"),
        eml("1609459200000", "Hello from Alice"),
    )
    .unwrap();
    fs::write(input.join("2.eml"), eml("99999999999999999", "Far future")).unwrap();
    input
}

fn source(include_summary: bool) -> SourceConfig {
    SourceConfig::SmsBackupPlus(SmsBackupPlusConfig {
        owner_phones: vec!["+15555550100".into()],
        owner_emails: vec!["me@example.com".into()],
        verbose: false,
        include_summary,
    })
}

#[test]
fn run_writes_the_conversation_and_counts_the_bad_date_row() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_directory(tmp.path());
    let output = tmp.path().join("out");

    let result = crate::run(&jsonl_run_config(&[&input], &output, source(true))).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Alice"), "{written}");
    assert!(!written.contains("Far future"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  Skipped 1 message with an invalid date"),
        "{:?}",
        result.messages
    );
}

#[test]
fn run_without_a_summary_still_writes_the_export() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_directory(tmp.path());
    let output = tmp.path().join("out");

    let result = crate::run(&jsonl_run_config(&[&input], &output, source(false))).expect("run");

    assert!(result.messages.is_empty(), "{:?}", result.messages);
    let files = fs::read_dir(&output)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .count();
    assert_eq!(files, 1);
}

/// A call-log mail SMS Backup+ wrote into its "Call log" label.
const CALL_LOG_EML: &str = "From: alice@unknown.email\n\
     To: me@example.com\n\
     Subject: Call with Alice\n\
     X-smssync-datatype: CALLLOG\n\
     X-smssync-type: 1\n\
     X-smssync-duration: 123\n\
     X-smssync-address: 4075550107\n\
     X-smssync-date: 1609459300000\n\
     Content-Type: text/plain; charset=utf-8\n\
     \n\
     123s (00:02:03)\n\
     4075550107 (incoming call)\n";

/// SMS Backup+ has no text in a call-log mail, only the call's length and
/// number, and Message Crate has no model for a call.
#[test]
fn a_call_log_mail_is_skipped_and_counted_in_the_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("1.eml"),
        eml("1609459200000", "Hello from Alice"),
    )
    .unwrap();
    fs::write(input.join("2.eml"), CALL_LOG_EML).unwrap();
    let output = tmp.path().join("out");

    let result = crate::run(&jsonl_run_config(&[&input], &output, source(true))).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Alice"), "{written}");
    assert!(!written.contains("123s"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  Skipped 1 call log entry"),
        "{:?}",
        result.messages
    );
}

/// A received group MMS whose `From` names nobody in the group is written
/// with no sender, and the summary and the Import Run's issues count it once,
/// even when the archive holds two copies of the mail.
#[test]
fn a_group_message_with_no_readable_sender_is_kept_and_counted_once() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let mail = "From: Bob <bob@example.org>\n\
         To: me@example.com\n\
         Subject: SMS with group\n\
         X-smssync-type: 132\n\
         X-smssync-address: 4075550150~4075550108\n\
         X-smssync-date: 1609459200000\n\
         Content-Type: text/plain; charset=utf-8\n\
         \n\
         Hello group\n";
    fs::write(input.join("1.eml"), mail).unwrap();
    fs::write(input.join("2.eml"), mail).unwrap();
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(&[&input], &output, source(true));
    let issues = collect_issues(&mut config);

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello group"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  Kept 1 group message with no sender"),
        "{:?}",
        result.messages
    );
    let issues = issues.lock().unwrap();
    assert_eq!(issues.len(), 1, "{issues:?}");
    let issue = &issues[0];
    assert_eq!(
        (
            issue.kind.as_str(),
            issue.step.as_str(),
            issue.item.as_str()
        ),
        ("skip", "parse", "1.eml (sender)")
    );
    assert!(issue.reason.contains("left out"), "{}", issue.reason);
    assert!(issue.reason.contains("kept"), "{}", issue.reason);
}

/// A received group MMS whose `To` names none of the owner's addresses is
/// filed one-to-one under its `From`, and the summary counts it once, even
/// when the archive holds two copies of the mail. Its sender was read, so it
/// is no Import Error: the run sends one note naming the mail kept (#1535).
#[test]
fn a_group_message_not_naming_the_owner_is_counted_once() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let mail = "From: carol@example.org\n\
         To: <me@icloud.example>, <+14075550150@unknown.email>\n\
         Subject: SMS with Alice\n\
         X-smssync-type: 132\n\
         X-smssync-address: 4075550150\n\
         X-smssync-date: 1609459200000\n\
         Content-Type: text/plain; charset=utf-8\n\
         \n\
         Hello from Carol\n";
    fs::write(input.join("1.eml"), mail).unwrap();
    fs::write(input.join("2.eml"), mail).unwrap();
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(&[&input], &output, source(true));
    let issues = collect_issues(&mut config);

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Carol"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  Kept 1 group message that names none of the owner's numbers or email addresses as a one-to-one message"),
        "{:?}",
        result.messages
    );
    // Carol's mail gives her no number, so she is kept by her address too.
    let issues = issues.lock().unwrap();
    assert_eq!(
        *issues,
        [
            note(
                "carol@example.org",
                "The backup gives this group member no phone number, so they are kept by this \
                 email address."
            ),
            note(
                "1.eml",
                "This group message names none of your phone numbers or email addresses, so it \
                 is kept as a one-to-one message from its sender."
            )
        ]
    );
}

/// The note the run sends about `item`.
fn note(item: &str, text: &str) -> RunIssue {
    RunIssue {
        kind: "note".into(),
        step: "parse".into(),
        item: item.into(),
        reason: text.into(),
    }
}

/// A mail with no address for the other person is kept in a conversation
/// under the name the mail gives, and the run sends a note naming the mail
/// (#1535).
#[test]
fn a_message_with_no_address_for_the_other_person_is_a_note() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let mail = "From: alice@unknown.email\n\
         To: me@example.com\n\
         Subject: SMS with Alice\n\
         X-smssync-type: 1\n\
         X-smssync-address: \n\
         X-smssync-date: 1609459200000\n\
         Content-Type: text/plain; charset=utf-8\n\
         \n\
         No number on this one\n";
    fs::write(input.join("1.eml"), mail).unwrap();
    fs::write(input.join("2.eml"), mail).unwrap();
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(&[&input], &output, source(true));
    let issues = collect_issues(&mut config);

    let result = crate::run(&config).expect("run");

    assert!(
        result.messages.iter().any(|l| l
            == "  Kept 1 message that names no phone number or email address for the other person"),
        "{:?}",
        result.messages
    );
    assert_eq!(
        *issues.lock().unwrap(),
        [note(
            "1.eml",
            "This message records no phone number or email address for the other person, so \
             it is kept in a conversation under the name the message gives, or with nobody."
        )]
    );
}

/// A group member the archive names only by email address keeps the address
/// as their key, and the summary counts them once, however many mails name
/// them (#1545).
#[test]
fn a_group_member_with_no_number_in_the_archive_is_counted_once() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    for (name, date) in [("1.eml", "1609459200000"), ("2.eml", "1609459300000")] {
        let mail = format!(
            "From: me@example.com\n\
             To: \"Dave\" <dave@example.com>, <+14075550108@unknown.email>\n\
             Subject: SMS with Dave\n\
             X-smssync-type: 128\n\
             X-smssync-address: 4075550108\n\
             X-smssync-date: {date}\n\
             Content-Type: text/plain; charset=utf-8\n\
             \n\
             Hello {name}\n"
        );
        fs::write(input.join(name), mail).unwrap();
    }
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(&[&input], &output, source(true));
    let issues = collect_issues(&mut config);

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("dave@example.com"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  Kept 1 group member by an email address, with no phone number"),
        "{:?}",
        result.messages
    );
    assert_eq!(
        *issues.lock().unwrap(),
        [note(
            "dave@example.com",
            "The backup gives this group member no phone number, so they are kept by this email \
             address."
        )]
    );
}

/// An email address the archive gives two numbers, as a contact card two
/// people share does, is neither person's: a group member named by it keeps
/// the address, and the summary counts them (#1545 review).
#[test]
fn a_group_member_whose_address_has_two_numbers_keeps_the_address() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let one_to_one = |number: &str, date: &str| {
        format!(
            "From: \"Smiths\" <smiths@example.com>\n\
             To: me@example.com\n\
             Subject: SMS with Smiths\n\
             X-smssync-type: 1\n\
             X-smssync-address: {number}\n\
             X-smssync-date: {date}\n\
             Content-Type: text/plain; charset=utf-8\n\
             \n\
             Hello from {number}\n"
        )
    };
    // Two mails give Mom's number and one gives Dad's: a majority still
    // names neither.
    for (name, number, date) in [
        ("1.eml", "4075550111", "1609459100000"),
        ("2.eml", "4075550111", "1609459150000"),
        ("3.eml", "4075550112", "1609459170000"),
    ] {
        fs::write(input.join(name), one_to_one(number, date)).unwrap();
    }
    fs::write(
        input.join("4.eml"),
        "From: me@example.com\n\
         To: <smiths@example.com>, <+14075550108@unknown.email>\n\
         Subject: SMS with Smiths\n\
         X-smssync-type: 128\n\
         X-smssync-address: 4075550108\n\
         X-smssync-date: 1609459200000\n\
         Content-Type: text/plain; charset=utf-8\n\
         \n\
         Hello group\n",
    )
    .unwrap();
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(&[&input], &output, source(true));
    let issues = collect_issues(&mut config);

    let result = crate::run(&config).expect("run");

    let group = fs::read_dir(&output)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| fs::read_to_string(p).is_ok_and(|t| t.contains("Hello group")))
        .expect("the group's file");
    let written = fs::read_to_string(group).unwrap();
    assert!(written.contains("smiths@example.com"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  Kept 1 group member by an email address the backup gives more than one phone number"),
        "{:?}",
        result.messages
    );
    assert_eq!(
        *issues.lock().unwrap(),
        [note(
            "smiths@example.com",
            "The backup gives this email address more than one phone number, so the group \
             member is kept by the email address."
        )]
    );
}

/// A mail kept with a part that could not be decoded is a note naming the
/// mail, so the person learns which message lost a part (#1535 review).
#[test]
fn a_message_kept_without_a_part_it_could_not_read_is_a_note() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("1.eml"),
        "From: x@unknown.email\nTo: me@example.com\nSubject: SMS with X\nX-smssync-type: 1\n\
         X-smssync-address: 4075550107\nX-smssync-date: 1609459200000\nMIME-Version: 1.0\n\
         Content-Type: multipart/mixed; boundary=\"b\"\n\n\
         --b\nContent-Type: text/plain; charset=utf-8\n\nhi\n\
         --b\nContent-Type: image/jpeg\nContent-Transfer-Encoding: base64\n\n@@@@\n--b--\n",
    )
    .unwrap();
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(&[&input], &output, source(true));
    let issues = collect_issues(&mut config);

    let result = crate::run(&config).expect("run");

    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  Left out 1 message part that could not be read"),
        "{:?}",
        result.messages
    );
    assert_eq!(
        *issues.lock().unwrap(),
        [note(
            "1.eml",
            "1 part of this message could not be read and was left out. The message itself is \
             kept."
        )]
    );
}
