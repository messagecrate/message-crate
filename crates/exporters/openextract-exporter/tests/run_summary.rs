//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or a report that dropped the bad-date count, passed them.

use message_crate_core::testutil::{assert_run_wrote_jsonl, collect_issues, jsonl_run_config};
use message_crate_core::{OpenExtractConfig, RunIssue, SourceConfig};
use std::fs;

const ALL_CONVERSATIONS_CSV: &str = "\
Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments
2020-01-01T17:00:00+00:00,Sam Example,Received,+15555550122,Hello from Sam,False,False
yesterday,Sam Example,Received,+15555550122,Bad date,False,False
2020-01-01T17:01:00+00:00,Sam Example,Sent,me,Hi Sam,True,False
";

#[test]
fn run_writes_the_conversation_and_counts_the_bad_date_row() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("all_conversations.csv");
    fs::write(&input, ALL_CONVERSATIONS_CSV).unwrap();
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[&input],
        &output,
        SourceConfig::OpenExtract(OpenExtractConfig {}),
    );

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Sam"), "{written}");
    assert!(written.contains("Hi Sam"), "{written}");
    assert!(!written.contains("Bad date"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  skipped 1 invalid-date rows"),
        "{:?}",
        result.messages
    );
}

/// A CSV the run cannot read is an Import Error naming it, and a chat kept
/// under a name alone is a note naming it (#1626, #1535).
#[test]
fn run_sends_an_import_error_for_an_unreadable_csv_and_a_note_for_a_name_only_chat() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("in");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("all_conversations.csv"),
        "Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T17:00:00+00:00,Mystery Person,Received,Mystery Person,Hello,False,False\n",
    )
    .unwrap();
    fs::write(
        input.join("broken.csv"),
        "Date,Conversation\n\"unterminated",
    )
    .unwrap();
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(
        &[&input],
        &output,
        SourceConfig::OpenExtract(OpenExtractConfig {}),
    );
    let issues = collect_issues(&mut config);

    crate::run(&config).expect("run");

    let issues = issues.lock().unwrap();
    let broken = input.join("broken.csv").display().to_string();
    let error = issues
        .iter()
        .find(|i| i.kind == "error")
        .expect("an error row");
    assert_eq!(
        (error.step.as_str(), error.item.as_str()),
        ("parse", broken.as_str())
    );
    let notes: Vec<&RunIssue> = issues.iter().filter(|i| i.kind == "note").collect();
    assert_eq!(
        notes,
        [&RunIssue {
            kind: "note".into(),
            step: "parse".into(),
            item: format!("{} (Mystery Person)", input.join("all_conversations.csv").display()),
            reason: "This chat names its person with no phone number or email address, so the conversation is kept under the name alone.".into(),
        }]
    );
}
