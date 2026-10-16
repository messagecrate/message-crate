//! Writes `tests/fixtures/desktop-events/events.json`: payloads of every event
//! the window reads as a typed object, built from the structs in
//! [`super`] and the app's own server's [`Notice`], keyed by the channel each
//! is sent on.
//! `web/src/lib/desktopEvents.test.ts` reads the file back and checks every
//! payload against its TypeScript type in `web/src/lib/types.ts`, so a field
//! renamed or removed on either side fails a test (#2109).
//!
//! The committed file is generated, not authored. This test fails when it
//! drifts from what the structs send today, the same way
//! `web/src/lib/searchQuery.test.ts` fails when
//! `tests/fixtures/search/web-queries.txt` drifts from the web app's builders.
//!
//! Every optional field is set in at least one payload, and every value of
//! each enum a payload carries appears in one, so the web test sees each
//! field and each word the window can receive. An issue row's `step` is the
//! exception: Rust carries it as a `String`, not an enum, so its one word
//! here is a sample and the stages' other words are not checked.

use std::path::PathBuf;

use import::FileStatus;
use message_crate_core::{Canceled, ProgressEvent, RunIssue, RunIssueKind, WriteStatus};
use serde_json::Value;

use super::{
    DesktopJobErrorEvent, ERROR, FILE_DONE, FILE_WRITTEN, ISSUE, ImportFileDoneEvent,
    ImportIssueEvent, ImportProgressEvent, PROGRESS, Step, WindowEvent,
};
use crate::commands::local_server::NOTICE;
use crate::local_server::{Notice, Stop};
use crate::tool_downloads::Program;

/// The committed fixture, from the repository root.
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/desktop-events/events.json")
}

/// `every!(Enum: A, B, C)` is `[Enum::A, Enum::B, Enum::C]`, from one list
/// of variants. Its exhaustive match stops compiling when a variant is
/// added, so the new variant is added to the list too.
macro_rules! every {
    ($ty:ident: $($variant:ident),+ $(,)?) => {{
        let all = [$($ty::$variant),+];
        for value in all {
            match value {
                $($ty::$variant)|+ => {}
            }
        }
        all
    }};
}

fn to_json(payload: impl serde::Serialize) -> Value {
    serde_json::to_value(payload).expect("an event payload serializes")
}

/// The payloads, keyed by channel.
fn payloads() -> Value {
    // One per step with every field set. The struct literal stops compiling
    // when a field is added, so the new field is set here too.
    let progress = every!(Step: Setup, Parse, Attachments, Prepare, Check, Media, Upload)
        .into_iter()
        .map(|step| ImportProgressEvent {
            step,
            done: 2,
            total: 5,
            bytes_done: Some(100),
            bytes_total: Some(500),
            status: Some("Deriving backup keys".into()),
            waiting: Some(Program::Ffmpeg),
        })
        .chain(
            every!(Program: Ffmpeg, Ffprobe, Wtsexporter)
                .into_iter()
                .map(|program| ImportProgressEvent::waiting(Step::Media, program, 1, Some(2))),
        )
        .map(to_json);

    let issue = every!(RunIssueKind: Skip, Error, Note, Resolved)
        .into_iter()
        .map(|kind| {
            ImportIssueEvent::from(&RunIssue {
                kind,
                step: "attachments".into(),
                item: "/backup/IMG_0001.MOV".into(),
                reason: "could not be decrypted".into(),
                conversation: Some("c.jsonl".into()),
            })
        })
        .map(to_json);

    let file_done = every!(FileStatus: Ok, Failed, Skipped)
        .into_iter()
        .map(|status| ImportFileDoneEvent::new("c.jsonl".into(), status))
        .map(to_json);

    let file_written = every!(WriteStatus: Written, Skipped)
        .into_iter()
        .map(|status| {
            match WindowEvent::from(ProgressEvent::FileWritten {
                file: "c.jsonl".into(),
                status,
            }) {
                WindowEvent::FileWritten(written) => written,
                other => panic!("not a finished file: {other:?}"),
            }
        })
        .map(to_json);

    // A Cancel, and a failure with a sentence for the person: between them
    // every field, and both values of `canceled`.
    let error = [
        DesktopJobErrorEvent::from(anyhow::Error::from(Canceled)),
        DesktopJobErrorEvent::from(
            anyhow::Error::from(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "No space left on device",
            ))
            .context("write c.jsonl"),
        ),
    ]
    .into_iter()
    .map(to_json);

    // The app's own server stopped each way, answered again, and failed.
    let notice = every!(Stop: Unanswered, Exited)
        .into_iter()
        .map(|cause| Notice::Restarting { cause })
        .chain([
            Notice::Answering,
            Notice::Failed {
                message: "Message Crate stopped 4 times within 10 minutes.".into(),
                details: "database is locked".into(),
            },
        ])
        .map(to_json);

    let channels: [(&str, Vec<Value>); 6] = [
        (PROGRESS, progress.collect()),
        (ISSUE, issue.collect()),
        (FILE_DONE, file_done.collect()),
        (FILE_WRITTEN, file_written.collect()),
        (ERROR, error.collect()),
        (NOTICE, notice.collect()),
    ];
    Value::Object(
        channels
            .into_iter()
            .map(|(channel, payloads)| (channel.to_string(), Value::Array(payloads)))
            .collect(),
    )
}

/// The committed fixture is what the structs send today. With
/// `UPDATE_FIXTURES` set, the test writes the file first.
#[test]
fn the_desktop_events_fixture_matches_the_structs() {
    let want = format!(
        "{}\n",
        serde_json::to_string_pretty(&payloads()).expect("the payloads serialize")
    );
    let path = fixture_path();
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &want).expect("write the fixture");
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "tests/fixtures/desktop-events/events.json is out of date with the event structs in \
         src-tauri/src/commands/events.rs. Regenerate it with: \
         UPDATE_FIXTURES=1 cargo test --manifest-path src-tauri/Cargo.toml desktop_events_fixture"
    );
}
