//! Local log of which conversations and files were already uploaded.
//!
//! The file is `.import-state.jsonl`. JSON Lines means one JSON object per
//! line. A later Upload can skip work that already succeeded. The Message Crate
//! HTTP server still ignores true duplicates if a line is sent again.

use std::collections::HashSet;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Filename of the local upload log, written next to the conversation files.
pub const JOURNAL_NAME: &str = ".import-state.jsonl";
/// Filename of the JSON summary written at the end of an Upload.
pub const REPORT_NAME: &str = "message-crate-push-report.json";
/// Filename of the human-readable Upload log.
pub const LOG_NAME: &str = "message-crate-push.log";

/// One message identity recorded in the journal: conversation file plus guid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalMessage {
    pub file: String,
    pub guid: String,
}

/// The server and account a journal records progress for: the server's URL
/// and the username the session resolved to. One export directory can be
/// uploaded to more than one, so every event names its target.
///
/// On disk the two are a line's own `url` and `username` keys, beside the
/// event's other fields, so each line reads whole on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerTarget {
    pub url: String,
    pub username: String,
}

impl ServerTarget {
    /// The target for `url` and `username`.
    pub fn new(url: impl Into<String>, username: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            username: username.into(),
        }
    }
}

/// One row in `.import-state.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JournalEvent {
    AssetOk {
        #[serde(flatten)]
        target: ServerTarget,
        sha256: String,
    },
    MessageBatchOk {
        #[serde(flatten)]
        target: ServerTarget,
        source: String,
        messages: Vec<JournalMessage>,
    },
    FileOk {
        #[serde(flatten)]
        target: ServerTarget,
        source: String,
        file: String,
    },
    Fail {
        #[serde(flatten)]
        target: ServerTarget,
        source: String,
        file: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        guid: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
        stage: String,
        error: String,
    },
}

impl JournalEvent {
    /// The server and account this event belongs to.
    fn target(&self) -> &ServerTarget {
        match self {
            Self::AssetOk { target, .. }
            | Self::MessageBatchOk { target, .. }
            | Self::FileOk { target, .. }
            | Self::Fail { target, .. } => target,
        }
    }
}

/// In-memory skip sets rebuilt from the journal for one [`ServerTarget`].
#[derive(Debug, Default)]
pub struct JournalState {
    pub assets: HashSet<String>,
    pub messages: HashSet<String>,
    pub files: HashSet<String>,
}

impl JournalState {
    /// Join a conversation filename and message guid into one set key.
    pub fn message_key(file: &str, guid: &str) -> String {
        format!("{file}\0{guid}")
    }
}

/// The Upload's log line for a journal line that could not be read.
///
/// The line may have recorded nothing this Upload needs (a failure, another
/// server's events, or a conversation a later line records), so the sentence
/// says only that what it recorded may be sent again.
fn unreadable_line_sentence(path: &Path, line: usize, error: &serde_json::Error) -> String {
    format!(
        "Line {line} of the Upload's journal {} could not be read ({}), \
         so the Upload skips it and may send again what it recorded. The server skips \
         what it already holds.",
        path.display(),
        jsonl_journal::unreadable_line_reason(error)
    )
}

/// Path of `.import-state.jsonl` inside the export directory.
pub fn journal_path(input: &Path) -> PathBuf {
    input.join(JOURNAL_NAME)
}

/// Read the journal and keep the events for `target`.
///
/// A missing file is treated as an empty journal. An unreadable line is skipped,
/// and `on_unreadable` gets one sentence for the Upload's log saying so. What
/// the line recorded may be sent again, and the server skips what it already
/// holds.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or a line cannot be read.
pub fn load(
    path: &Path,
    target: &ServerTarget,
    on_unreadable: &mut dyn FnMut(String),
) -> Result<JournalState> {
    let mut state = JournalState::default();
    let events: Vec<JournalEvent> = jsonl_journal::load_events("journal", path, &mut |i, e| {
        on_unreadable(unreadable_line_sentence(path, i, e));
    })?;
    for event in events.into_iter().filter(|event| event.target() == target) {
        match event {
            JournalEvent::AssetOk { sha256, .. } => {
                state.assets.insert(sha256);
            }
            JournalEvent::MessageBatchOk { messages, .. } => {
                for message in messages {
                    state
                        .messages
                        .insert(JournalState::message_key(&message.file, &message.guid));
                }
            }
            JournalEvent::FileOk { file, .. } => {
                state.files.insert(file);
            }
            JournalEvent::Fail { .. } => {}
        }
    }
    Ok(state)
}

/// Append one event as a JSON Lines row and flush it to disk.
///
/// # Errors
///
/// Returns an error when the parent directory cannot be created, the file cannot
/// be opened, or the write fails.
pub fn append(path: &Path, event: &JournalEvent) -> Result<()> {
    jsonl_journal::append("journal", path, event)
}

/// Rewrite the journal from in-memory `state` for `target`.
///
/// Events for other targets are kept, so one export directory can resume
/// against more than one server and account.
///
/// # Errors
///
/// Returns an error when the existing file cannot be read, the temporary file
/// cannot be written, or the rename fails.
pub fn compact(path: &Path, target: &ServerTarget, state: &JournalState) -> Result<()> {
    jsonl_journal::compact_with::<JournalEvent, _>("journal", path, |mut events| {
        // Preserve other server targets so one export directory can resume against
        // multiple servers without wiping their skip state.
        events.retain(|event| event.target() != target);
        let mut assets: Vec<_> = state.assets.iter().collect();
        assets.sort_unstable();
        for sha in assets {
            events.push(JournalEvent::AssetOk {
                target: target.clone(),
                sha256: sha.clone(),
            });
        }
        let messages = messages_from_state_keys(state);
        for batch in messages.chunks(1_000) {
            events.push(JournalEvent::MessageBatchOk {
                target: target.clone(),
                source: String::new(),
                messages: batch.to_vec(),
            });
        }
        let mut files: Vec<_> = state.files.iter().collect();
        files.sort_unstable();
        for file in files {
            events.push(JournalEvent::FileOk {
                target: target.clone(),
                source: String::new(),
                file: file.clone(),
            });
        }
        events
    })
}

/// The journal of one Upload: the in-memory skip sets plus the file they
/// are appended to, bound to one [`ServerTarget`].
///
/// Every write goes through here so callers never repeat the target and path
/// that every [`JournalEvent`] carries. Successful events update the
/// in-memory sets *and* append to disk; failures are best-effort diagnostics
/// and never fail the run.
#[derive(Debug)]
pub struct RunJournal {
    state: JournalState,
    path: PathBuf,
    target: ServerTarget,
}

impl RunJournal {
    /// Load the journal for this server target, or start empty when `fresh` is
    /// set (force mode and replace mode both ignore earlier progress). Each
    /// line that could not be read goes to `on_unreadable` as a sentence for the
    /// Upload's log.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing journal file cannot be read.
    pub fn open(
        path: PathBuf,
        target: ServerTarget,
        fresh: bool,
        on_unreadable: &mut dyn FnMut(String),
    ) -> Result<Self> {
        let state = if fresh {
            JournalState::default()
        } else {
            load(&path, &target, on_unreadable)?
        };
        Ok(Self {
            state,
            path,
            target,
        })
    }

    /// True when this conversation file fully imported on an earlier run.
    pub fn has_file(&self, file: &str) -> bool {
        self.state.files.contains(file)
    }

    /// True when this message id was imported on an earlier run.
    ///
    /// A blank guid is never one: the server accepts and skips a tapback row
    /// with a blank guid, so one can be recorded, but a message with a blank
    /// guid must be sent every time so the server refuses it.
    pub fn has_message(&self, file: &str, guid: &str) -> bool {
        !guid.trim().is_empty()
            && self
                .state
                .messages
                .contains(&JournalState::message_key(file, guid))
    }

    /// True when this attachment fingerprint was uploaded on an earlier run.
    pub fn has_asset(&self, sha256: &str) -> bool {
        self.state.assets.contains(sha256)
    }

    /// Record that the server now holds this attachment.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal file cannot be appended to.
    pub fn asset_ok(&mut self, sha256: &str) -> Result<()> {
        self.state.assets.insert(sha256.to_string());
        append(
            &self.path,
            &JournalEvent::AssetOk {
                target: self.target.clone(),
                sha256: sha256.to_string(),
            },
        )
    }

    /// Record that every message in one import request was accepted.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal file cannot be appended to.
    pub fn message_batch_ok(&mut self, source: &str, messages: Vec<JournalMessage>) -> Result<()> {
        for message in &messages {
            self.state
                .messages
                .insert(JournalState::message_key(&message.file, &message.guid));
        }
        append(
            &self.path,
            &JournalEvent::MessageBatchOk {
                target: self.target.clone(),
                source: source.to_string(),
                messages,
            },
        )
    }

    /// Record that a whole conversation file finished importing.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal file cannot be appended to.
    pub fn file_ok(&mut self, source: &str, file: &str) -> Result<()> {
        self.state.files.insert(file.to_string());
        append(
            &self.path,
            &JournalEvent::FileOk {
                target: self.target.clone(),
                source: source.to_string(),
                file: file.to_string(),
            },
        )
    }

    /// Note a failure for later diagnosis. Best effort: a journal write error
    /// here is swallowed because the failure itself is already being reported.
    pub fn record_failure(&self, source: &str, file: &str, stage: &str, error: &str) {
        let _ = append(
            &self.path,
            &JournalEvent::Fail {
                target: self.target.clone(),
                source: source.to_string(),
                file: file.to_string(),
                guid: None,
                sha256: None,
                stage: stage.to_string(),
                error: error.to_string(),
            },
        );
    }

    /// Rewrite the journal file from the in-memory sets (see [`compact`]).
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be rewritten.
    pub fn compact(&self) -> Result<()> {
        compact(&self.path, &self.target, &self.state)
    }
}

/// Split stored `file\0guid` keys back into journal message records, sorted.
fn messages_from_state_keys(state: &JournalState) -> Vec<JournalMessage> {
    let mut messages: Vec<JournalMessage> = Vec::new();
    for key in &state.messages {
        let Some((file, guid)) = key.split_once('\0') else {
            continue;
        };
        messages.push(JournalMessage {
            file: file.to_string(),
            guid: guid.to_string(),
        });
    }
    messages.sort_unstable_by(|a, b| (&a.file, &a.guid).cmp(&(&b.file, &b.guid)));
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// The target most tests upload to.
    fn alice() -> ServerTarget {
        ServerTarget::new("http://server", "alice")
    }

    #[test]
    fn loads_batch_message_success_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_NAME);
        fs::write(
            &path,
            concat!(
                "{\"event\":\"message_batch_ok\",\"url\":\"http://server\",\"username\":\"alice\",",
                "\"source\":\"sms\",\"messages\":[{\"file\":\"first.jsonl\",\"guid\":\"guid-1\"},",
                "{\"file\":\"second.jsonl\",\"guid\":\"guid-2\"}]}\n"
            ),
        )
        .unwrap();

        let state = load(&path, &alice(), &mut |_| {}).unwrap();

        assert!(
            state
                .messages
                .contains(&JournalState::message_key("first.jsonl", "guid-1"))
        );
        assert!(
            state
                .messages
                .contains(&JournalState::message_key("second.jsonl", "guid-2"))
        );
    }

    /// One event of each success kind, all for `target`.
    fn success_events(target: &ServerTarget, tag: &str) -> Vec<JournalEvent> {
        vec![
            JournalEvent::AssetOk {
                target: target.clone(),
                sha256: format!("sha-{tag}"),
            },
            JournalEvent::MessageBatchOk {
                target: target.clone(),
                source: "sms".into(),
                messages: vec![JournalMessage {
                    file: "chat.jsonl".into(),
                    guid: format!("batch-{tag}"),
                }],
            },
            JournalEvent::FileOk {
                target: target.clone(),
                source: "sms".into(),
                file: format!("file-{tag}.jsonl"),
            },
        ]
    }

    /// An unreadable line is skipped while the line before it is kept, and the
    /// sentence counts it among every line of the file (#1889). serde's
    /// position is checked by `jsonl_journal`'s own tests, and the rest of the
    /// sentence by
    /// `an_unreadable_journal_line_is_a_sentence_in_the_uploads_log`.
    #[test]
    fn an_unreadable_line_is_skipped_and_named_by_its_line_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_NAME);
        append(&path, &success_events(&alice(), "kept")[2]).unwrap();
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("{not json\n");
        fs::write(&path, text).unwrap();

        let mut lines = Vec::new();
        let state = load(&path, &alice(), &mut |line| lines.push(line)).unwrap();

        assert!(state.files.contains("file-kept.jsonl"));
        assert_eq!(lines.len(), 1, "{lines:?}");
        let prefix = format!(
            "Line 2 of the Upload's journal {} could not be read (",
            path.display()
        );
        assert!(
            lines[0].starts_with(&prefix) && lines[0].contains("), so the Upload skips it"),
            "{}",
            lines[0]
        );
    }

    /// A line names its target as its own `url` and `username` keys, beside
    /// the event's other fields, as `ServerTarget`'s doc says.
    #[test]
    fn an_event_writes_its_target_as_url_and_username_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_NAME);
        let mut journal = RunJournal::open(path.clone(), alice(), false, &mut |_| {}).unwrap();
        journal.file_ok("sms", "chat.jsonl").unwrap();

        let line: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&path).unwrap().trim()).unwrap();
        assert_eq!(
            line,
            serde_json::json!({
                "event": "file_ok",
                "url": "http://server",
                "username": "alice",
                "source": "sms",
                "file": "chat.jsonl"
            })
        );
    }

    #[test]
    fn load_keeps_only_events_for_this_server_and_username() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_NAME);
        let events = [
            success_events(&ServerTarget::new("http://other", "alice"), "other-url"),
            success_events(&ServerTarget::new("http://server", "bob"), "other-user"),
            success_events(&alice(), "mine"),
        ];
        for event in events.iter().flatten() {
            append(&path, event).unwrap();
        }

        let state = load(&path, &alice(), &mut |_| {}).unwrap();

        let expected_assets: HashSet<String> = ["sha-mine".to_string()].into();
        assert_eq!(state.assets, expected_assets);
        let expected_messages: HashSet<String> =
            [JournalState::message_key("chat.jsonl", "batch-mine")].into();
        assert_eq!(state.messages, expected_messages);
        let expected_files: HashSet<String> = ["file-mine.jsonl".to_string()].into();
        assert_eq!(state.files, expected_files);
    }

    #[test]
    fn a_recorded_asset_is_skipped_after_reopening_the_journal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_NAME);
        let mut journal = RunJournal::open(path.clone(), alice(), false, &mut |_| {}).unwrap();
        assert!(!journal.has_asset("sha-1"));
        journal.asset_ok("sha-1").unwrap();
        drop(journal);

        let reopened = RunJournal::open(path, alice(), false, &mut |_| {}).unwrap();
        assert!(reopened.has_asset("sha-1"));
        assert!(!reopened.has_asset("sha-2"));
    }

    #[test]
    fn compact_preserves_other_server_target_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_NAME);
        fs::write(
            &path,
            concat!(
                r#"{"event":"asset_ok","url":"http://a","username":"alice","sha256":"aaa"}"#,
                "\n",
                r#"{"event":"file_ok","url":"http://b","username":"bob","source":"sms","file":"chat.jsonl"}"#,
                "\n",
            ),
        )
        .unwrap();

        let mut state = JournalState::default();
        state.assets.insert("bbb".into());
        let alice = ServerTarget::new("http://a", "alice");
        let bob = ServerTarget::new("http://b", "bob");
        compact(&path, &bob, &state).unwrap();

        let a = load(&path, &alice, &mut |_| {}).unwrap();
        assert!(a.assets.contains("aaa"));
        let b = load(&path, &bob, &mut |_| {}).unwrap();
        assert!(b.assets.contains("bbb"));
        assert!(!b.files.contains("chat.jsonl"));
    }

    /// Compacting for one account keeps another account's entries on the
    /// same server, and the same account's entries on another server.
    #[test]
    fn compact_keeps_other_accounts_on_the_same_server() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_NAME);
        let alice_a = ServerTarget::new("http://a", "alice");
        let bob_a = ServerTarget::new("http://a", "bob");
        let alice_b = ServerTarget::new("http://b", "alice");
        for (target, file) in [
            (&alice_a, "alice-a.jsonl"),
            (&bob_a, "bob-a.jsonl"),
            (&alice_b, "alice-b.jsonl"),
        ] {
            append(
                &path,
                &JournalEvent::FileOk {
                    target: target.clone(),
                    source: "sms".into(),
                    file: file.into(),
                },
            )
            .unwrap();
        }

        let mut state = JournalState::default();
        state.files.insert("alice-a.jsonl".into());
        compact(&path, &alice_a, &state).unwrap();

        let files = |target: &ServerTarget| load(&path, target, &mut |_| {}).unwrap().files;
        assert_eq!(files(&alice_a), ["alice-a.jsonl".into()].into());
        assert_eq!(files(&bob_a), ["bob-a.jsonl".into()].into());
        assert_eq!(files(&alice_b), ["alice-b.jsonl".into()].into());
    }

    #[test]
    fn append_writes_complete_lines_under_contention() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join(JOURNAL_NAME));
        let mut handles = Vec::new();
        for i in 0..8 {
            let path = Arc::clone(&path);
            handles.push(thread::spawn(move || {
                for j in 0..50 {
                    let guid = format!("g-{i}-{j}");
                    let messages: Vec<_> = (0..200)
                        .map(|k| JournalMessage {
                            file: format!("f{i}.jsonl"),
                            guid: format!("{guid}-{k}"),
                        })
                        .collect();
                    append(
                        &path,
                        &JournalEvent::MessageBatchOk {
                            target: alice(),
                            source: "sms".into(),
                            messages,
                        },
                    )
                    .unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let text = fs::read_to_string(&*path).unwrap();
        let mut lines = 0usize;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            serde_json::from_str::<JournalEvent>(line).expect("torn line");
            lines += 1;
        }
        assert_eq!(lines, 8 * 50);
    }
}
