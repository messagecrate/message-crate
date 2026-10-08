use std::fs;
use std::path::Path;

use message_crate_core::{RunLogAccount, RunLogLevel};

use super::{LinesQuery, Reader, list, read_lines, read_whole};
use crate::app_directories::{RunLog, import_run_log};

const SERVER: &str = "http://127.0.0.1:8080";

/// The run whose directory is `staging-<label>`, its log started for
/// `account_id` on `server`, with one line of its own.
fn run_log(logs: &Path, label: &str, account_id: i64, server: &str) -> String {
    let run_dir = Path::new("/staging").join(format!("staging-{label}"));
    let log = RunLog::open(logs, &run_dir);
    log.account(&RunLogAccount {
        import_run_id: account_id * 10,
        account_id,
        server: server.into(),
    });
    log.line(&format!("Conversations: {account_id}"));
    import_run_log(logs, &run_dir)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn account(account_id: i64) -> Reader {
    Reader {
        server: SERVER.into(),
        account_id,
        owner: false,
    }
}

fn owner() -> Reader {
    Reader {
        server: SERVER.into(),
        account_id: 1,
        owner: true,
    }
}

fn names(reader: &Reader, logs: &Path) -> Vec<String> {
    let mut names: Vec<_> = list(logs, reader)
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    names
}

#[test]
fn an_account_lists_its_own_run_logs_and_the_owner_lists_every_one() {
    let logs = tempfile::tempdir().unwrap();
    let alice = run_log(logs.path(), "whatsapp-261004-143000", 2, SERVER);
    let bob = run_log(logs.path(), "iphone-ios-261005-090000", 3, SERVER);
    // Account 2 of another server is another account.
    let elsewhere = run_log(
        logs.path(),
        "sms-261006-120000",
        2,
        "http://192.168.1.20:8080",
    );
    // A log that names no account, left before the account line was written.
    fs::write(
        logs.path().join("import-old-261001-080000.log"),
        "Conversations: 1\n",
    )
    .unwrap();

    assert_eq!(
        names(&account(2), logs.path()),
        std::slice::from_ref(&alice)
    );
    assert_eq!(names(&account(3), logs.path()), std::slice::from_ref(&bob));
    let mut every = vec![
        alice,
        bob,
        elsewhere,
        "import-old-261001-080000.log".to_string(),
    ];
    every.sort();
    assert_eq!(names(&owner(), logs.path()), every);
}

#[test]
fn an_account_cannot_read_another_account_s_run_log() {
    let logs = tempfile::tempdir().unwrap();
    let bob = run_log(logs.path(), "iphone-ios-261005-090000", 3, SERVER);
    let query = LinesQuery {
        limit: 10,
        ..LinesQuery::default()
    };

    let refused = read_lines(logs.path(), &account(2), &bob, &query).unwrap_err();
    assert!(refused.contains("another account"), "{refused}");
    assert!(read_whole(logs.path(), &account(2), &bob).is_err());
    assert_eq!(
        read_lines(logs.path(), &owner(), &bob, &query)
            .unwrap()
            .items
            .len(),
        2
    );
}

#[test]
fn a_name_outside_the_logs_directory_is_refused() {
    let logs = tempfile::tempdir().unwrap();
    for name in ["../import-x.log", "import-x.txt", "notes.log"] {
        assert!(read_whole(logs.path(), &owner(), name).is_err(), "{name}");
    }
}

#[test]
fn lines_come_newest_first_filtered_by_level_and_text_a_page_at_a_time() {
    let logs = tempfile::tempdir().unwrap();
    let run_dir = Path::new("/staging/staging-sms-261006-120000");
    let log = RunLog::open(logs.path(), run_dir);
    for n in 1..=5 {
        log.line(&format!("Uploaded chat-{n}.jsonl"));
    }
    log.issue(&message_crate_core::RunIssue {
        kind: "skip".into(),
        step: "attachments".into(),
        item: "a.jpg".into(),
        reason: "the file is missing".into(),
        conversation: None,
    });
    log.error(&anyhow::anyhow!("the backup could not be read"));
    let name = "import-sms-261006-120000.log";

    let page = |query: LinesQuery| read_lines(logs.path(), &owner(), name, &query).unwrap();
    let all = page(LinesQuery {
        limit: 3,
        ..LinesQuery::default()
    });
    let texts: Vec<_> = all.items.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "the backup could not be read",
            "a.jpg: the file is missing",
            "Uploaded chat-5.jsonl"
        ]
    );
    assert!(all.has_more);

    let older = page(LinesQuery {
        limit: 3,
        after: Some(all.items[2].id),
        ..LinesQuery::default()
    });
    let texts: Vec<_> = older.items.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Uploaded chat-4.jsonl",
            "Uploaded chat-3.jsonl",
            "Uploaded chat-2.jsonl"
        ]
    );
    assert!(older.has_more);

    let warnings = page(LinesQuery {
        level: Some(RunLogLevel::Warn),
        limit: 10,
        ..LinesQuery::default()
    });
    let levels: Vec<_> = warnings.items.iter().map(|line| line.level).collect();
    assert_eq!(levels, [RunLogLevel::Error, RunLogLevel::Warn]);
    assert!(!warnings.has_more);

    let errors = page(LinesQuery {
        level: Some(RunLogLevel::Error),
        limit: 10,
        ..LinesQuery::default()
    });
    assert_eq!(errors.items.len(), 1);

    let found = page(LinesQuery {
        text: Some("CHAT-3".into()),
        limit: 10,
        ..LinesQuery::default()
    });
    let texts: Vec<_> = found.items.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["Uploaded chat-3.jsonl"]);
}

#[test]
fn a_run_log_is_still_listed_and_read_after_the_run_s_directory_is_deleted() {
    let scratch = crate::run_directories::Scratch::new();
    let run_dir = scratch
        .directories
        .create("whatsapp", "261004-143000")
        .unwrap();
    let logs = crate::app_directories::logs_dir_in(scratch.app_data.path());
    let log = RunLog::open(&logs, &run_dir);
    log.account(&RunLogAccount {
        import_run_id: 42,
        account_id: 2,
        server: SERVER.into(),
    });
    log.line("Conversations: 3");
    drop(log);

    // The run ended: its directory in the Staging Directory goes.
    scratch
        .directories
        .delete(run_dir.to_str().unwrap())
        .unwrap();
    assert!(!run_dir.exists());

    let listed = list(&logs, &account(2)).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "import-whatsapp-261004-143000.log");
    assert_eq!(
        listed[0].account.as_ref().map(|a| a.import_run_id),
        Some(42)
    );
    let page = read_lines(
        &logs,
        &account(2),
        &listed[0].name,
        &LinesQuery {
            limit: 10,
            ..LinesQuery::default()
        },
    )
    .unwrap();
    assert_eq!(page.items[0].text, "Conversations: 3");
}

#[test]
fn a_line_still_being_written_is_left_out_of_the_page() {
    let logs = tempfile::tempdir().unwrap();
    let run_dir = Path::new("/staging/staging-sms-261006-120000");
    RunLog::open(logs.path(), run_dir).line("Uploaded chat-1.jsonl");
    let path = import_run_log(logs.path(), run_dir);
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("2026-10-08T12:00:00.123456Z  INFO Uploaded cha");
    fs::write(&path, text).unwrap();

    let page = read_lines(
        logs.path(),
        &owner(),
        "import-sms-261006-120000.log",
        &LinesQuery {
            limit: 10,
            ..LinesQuery::default()
        },
    )
    .unwrap();
    let texts: Vec<_> = page.items.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["Uploaded chat-1.jsonl"]);
}
