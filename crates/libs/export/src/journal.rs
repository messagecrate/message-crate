//! Local log of which Assets an Export from a server already fetched.
//!
//! The file is `.message-crate-export-state.jsonl`. JSON Lines means one JSON object per
//! line. A later Export Run can skip Assets that are already on disk.

use std::collections::HashSet;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
pub use jsonl_journal::ServerTarget;
use serde::{Deserialize, Serialize};

/// Filename of the journal, written in the output directory.
pub const EXPORT_JOURNAL_NAME: &str = ".message-crate-export-state.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
/// One row in `.message-crate-export-state.jsonl`.
pub enum ExportJournalEvent {
    /// One Asset is on disk, so a later run can skip fetching it.
    AssetOk {
        /// Server and account the Asset came from.
        #[serde(flatten)]
        target: ServerTarget,
        /// Hex SHA-256 fingerprint of the Asset's bytes; the skip key.
        sha256: String,
    },
    /// An Export Run finished, with its counts.
    ExportComplete {
        /// Server and account the Export Run read from.
        #[serde(flatten)]
        target: ServerTarget,
        /// Conversations written.
        conversations: u64,
        /// Messages written.
        messages: u64,
        /// Assets fetched, plus those kept because they were already on
        /// disk.
        assets: u64,
    },
}

#[derive(Debug, Default)]
/// Skip sets rebuilt from the journal for one [`ServerTarget`].
pub struct ExportJournalState {
    /// SHA-256 fingerprints (hex of the file bytes) of attachments already on disk.
    pub assets: HashSet<String>,
    /// True if the last run finished cleanly (an `export_complete` event was written).
    pub export_complete: bool,
}

/// Path of `.message-crate-export-state.jsonl` inside the output directory.
pub fn journal_path(out_dir: &Path) -> PathBuf {
    out_dir.join(EXPORT_JOURNAL_NAME)
}

/// The Export's log line for a line of the journal that could not be read.
///
/// Skipping the line costs no fetch: an Asset whose file is in the directory
/// is kept whether the journal names it or not, so the sentence says so.
fn unreadable_line_sentence(path: &Path, line: usize, error: &serde_json::Error) -> String {
    format!(
        "Line {line} of {}, the record of fetched Assets, could not be read ({}), \
         so the Export skips it. An Asset that line recorded is not fetched again \
         while its file is in the directory.",
        path.display(),
        jsonl_journal::unreadable_line_reason(error)
    )
}

/// Read the journal and keep the events for `target`.
///
/// A missing file is treated as an empty journal. A line that cannot be
/// parsed is skipped, and `on_unreadable` gets one sentence for the Export's
/// log naming it.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or a line cannot be read.
pub fn load(
    path: &Path,
    target: &ServerTarget,
    on_unreadable: &mut dyn FnMut(String),
) -> Result<ExportJournalState> {
    let mut state = ExportJournalState::default();
    let events: Vec<ExportJournalEvent> =
        jsonl_journal::load_events("export journal", path, &mut |line, error| {
            on_unreadable(unreadable_line_sentence(path, line, error));
        })?;
    for event in events.into_iter().filter(|event| event.target() == target) {
        match event {
            ExportJournalEvent::AssetOk { sha256, .. } => {
                state.assets.insert(sha256);
            }
            ExportJournalEvent::ExportComplete { .. } => state.export_complete = true,
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
pub fn append(path: &Path, event: &ExportJournalEvent) -> Result<()> {
    jsonl_journal::append("export journal", path, event)
}

/// Rewrite the lines of `target` from in-memory `state`, and keep every line
/// of every other target as it was.
///
/// One output directory can hold the journal of Export runs from several
/// servers, or several accounts on one server. Each target's lines are its
/// own skip list, so finishing a run for one target must not drop another's.
///
/// # Errors
///
/// Returns an error when the temporary file cannot be written or the rename fails.
pub fn compact(path: &Path, target: &ServerTarget, state: &ExportJournalState) -> Result<()> {
    jsonl_journal::compact_with::<ExportJournalEvent, _>("export journal", path, |read| {
        let mut events: Vec<ExportJournalEvent> = read
            .into_iter()
            .filter(|event| event.target() != target)
            .collect();
        let mut assets: Vec<_> = state.assets.iter().collect();
        assets.sort_unstable();
        for sha in assets {
            events.push(ExportJournalEvent::AssetOk {
                target: target.clone(),
                sha256: sha.clone(),
            });
        }
        if state.export_complete {
            // A later Export Run ignores the counts; an `export_complete` row
            // only means the last Export Run finished.
            events.push(ExportJournalEvent::ExportComplete {
                target: target.clone(),
                conversations: 0,
                messages: 0,
                assets: 0,
            });
        }
        events
    })
}

impl ExportJournalEvent {
    /// The server and account of the run that wrote this line.
    fn target(&self) -> &ServerTarget {
        match self {
            Self::AssetOk { target, .. } | Self::ExportComplete { target, .. } => target,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The target most tests export from.
    fn alice() -> ServerTarget {
        ServerTarget::new("http://server", "alice")
    }

    #[test]
    fn loads_asset_and_export_complete_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXPORT_JOURNAL_NAME);
        fs::write(
            &path,
            concat!(
                "{\"event\":\"asset_ok\",\"url\":\"http://server\",\"username\":\"alice\",",
                "\"sha256\":\"aaabbbccc\",\"path\":\"attachments/aaabbbccc\",\"size_bytes\":12345}\n",
                "{\"event\":\"asset_ok\",\"url\":\"http://server\",\"username\":\"alice\",",
                "\"sha256\":\"dddeeefff\",\"path\":\"attachments/dddeeefff\",\"size_bytes\":67890}\n",
                "{\"event\":\"export_complete\",\"url\":\"http://server\",\"username\":\"alice\",",
                "\"conversations\":2,\"messages\":100,\"assets\":2}\n",
            ),
        )
        .unwrap();

        let state = load(&path, &alice(), &mut |_| {}).unwrap();

        assert!(state.assets.contains("aaabbbccc"));
        assert!(state.assets.contains("dddeeefff"));
        assert!(state.export_complete);
    }

    #[test]
    fn filters_by_url_and_username() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXPORT_JOURNAL_NAME);
        fs::write(
            &path,
            concat!(
                "{\"event\":\"asset_ok\",\"url\":\"http://server-a\",\"username\":\"alice\",",
                "\"sha256\":\"aaa\",\"path\":\"attachments/aaa\",\"size_bytes\":1}\n",
                "{\"event\":\"asset_ok\",\"url\":\"http://server-b\",\"username\":\"bob\",",
                "\"sha256\":\"bbb\",\"path\":\"attachments/bbb\",\"size_bytes\":2}\n",
            ),
        )
        .unwrap();

        let state = load(
            &path,
            &ServerTarget::new("http://server-a", "alice"),
            &mut |_| {},
        )
        .unwrap();
        assert!(state.assets.contains("aaa"));
        assert!(!state.assets.contains("bbb"));
    }

    #[test]
    fn compact_sorts_assets_and_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXPORT_JOURNAL_NAME);
        let mut state = ExportJournalState::default();
        state.assets.insert("ccc".into());
        state.assets.insert("aaa".into());
        state.assets.insert("bbb".into());
        state.export_complete = true;

        compact(&path, &alice(), &state).unwrap();

        let reloaded = load(&path, &alice(), &mut |_| {}).unwrap();
        assert_eq!(reloaded.assets.len(), 3);
        assert!(reloaded.assets.contains("aaa"));
        assert!(reloaded.assets.contains("bbb"));
        assert!(reloaded.assets.contains("ccc"));
        assert!(reloaded.export_complete);
    }

    /// `append` is what an Export Run actually calls, once per Asset, and nothing
    /// called it: every test here wrote the file by hand or went through
    /// `compact`. Replacing it with a no-op made a later Export Run start
    /// from nothing and fetch every Asset again, with the suite green.
    #[test]
    fn appended_events_are_on_disk_and_load_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXPORT_JOURNAL_NAME);

        append(
            &path,
            &ExportJournalEvent::AssetOk {
                target: alice(),
                sha256: "aaa".into(),
            },
        )
        .unwrap();

        // Loading between the two appends stands for a later Export Run: after
        // a run interrupted after one Asset, it must find that Asset.
        let after_first = load(&path, &alice(), &mut |_| {}).unwrap();
        assert!(after_first.assets.contains("aaa"));
        assert!(!after_first.export_complete);

        append(
            &path,
            &ExportJournalEvent::AssetOk {
                target: alice(),
                sha256: "bbb".into(),
            },
        )
        .unwrap();

        let after_second = load(&path, &alice(), &mut |_| {}).unwrap();
        assert!(
            after_second.assets.contains("aaa"),
            "the second append must not have replaced the first"
        );
        assert!(after_second.assets.contains("bbb"));

        // One JSON Lines row per append, not one document overwritten.
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text.lines().count(),
            2,
            "each event is its own line: {text:?}"
        );
    }

    /// The parent directory is created on the way. An Export Run writing its first
    /// journal into a fresh output directory would otherwise fail on the very
    /// first asset.
    #[test]
    fn appending_creates_the_directory_it_needs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-yet").join(EXPORT_JOURNAL_NAME);

        append(
            &path,
            &ExportJournalEvent::AssetOk {
                target: alice(),
                sha256: "aaa".into(),
            },
        )
        .unwrap();

        assert!(path.is_file());
        assert!(
            load(&path, &alice(), &mut |_| {})
                .unwrap()
                .assets
                .contains("aaa")
        );
    }

    /// An unreadable line is skipped while the lines around it are kept, and
    /// the sentence counts it among every line of the file (#1910). serde's
    /// position is checked by `jsonl_journal`'s own tests, and the rest of the
    /// sentence by
    /// `an_unreadable_journal_line_is_a_sentence_in_the_exports_log`.
    #[test]
    fn an_unreadable_line_is_skipped_and_named_by_its_line_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXPORT_JOURNAL_NAME);
        fs::write(
            &path,
            concat!(
                "{\"event\":\"asset_ok\",\"url\":\"http://server\",\"username\":\"alice\",",
                "\"sha256\":\"aaa\"}\n",
                "{not json\n",
                "{\"event\":\"asset_ok\",\"url\":\"http://server\",\"username\":\"alice\",",
                "\"sha256\":\"bbb\"}\n",
            ),
        )
        .unwrap();

        let mut lines = Vec::new();
        let state = load(&path, &alice(), &mut |line| lines.push(line)).unwrap();

        assert!(state.assets.contains("aaa"));
        assert!(state.assets.contains("bbb"));
        assert_eq!(lines.len(), 1, "{lines:?}");
        let prefix = format!(
            "Line 2 of {}, the record of fetched Assets, could not be read (",
            path.display()
        );
        assert!(
            lines[0].starts_with(&prefix) && lines[0].contains("), so the Export skips it"),
            "{}",
            lines[0]
        );
    }

    /// One output directory used by Export for two servers, or two accounts
    /// on one server: finishing a run for one rewrites only its own lines,
    /// so the next run for another still skips what it fetched (#1532).
    #[test]
    fn compact_keeps_the_lines_of_every_other_server_and_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXPORT_JOURNAL_NAME);
        let alice_a = ServerTarget::new("http://server-a", "alice");
        let bob_a = ServerTarget::new("http://server-a", "bob");
        let alice_b = ServerTarget::new("http://server-b", "alice");
        let asset = |target: &ServerTarget, sha256: &str| ExportJournalEvent::AssetOk {
            target: target.clone(),
            sha256: sha256.into(),
        };
        let complete = |target: &ServerTarget| ExportJournalEvent::ExportComplete {
            target: target.clone(),
            conversations: 1,
            messages: 1,
            assets: 1,
        };
        for event in [
            asset(&alice_a, "aaa"),
            complete(&alice_a),
            asset(&bob_a, "bbb"),
            asset(&alice_b, "ccc"),
            asset(&alice_b, "ccc"),
        ] {
            append(&path, &event).unwrap();
        }
        let mut state = ExportJournalState::default();
        state.assets.insert("ccc".into());
        state.assets.insert("ddd".into());
        state.export_complete = true;

        compact(&path, &alice_b, &state).unwrap();

        let server_a = load(&path, &alice_a, &mut |_| {}).unwrap();
        assert!(server_a.assets.contains("aaa"));
        assert!(server_a.export_complete);
        let other_account = load(&path, &bob_a, &mut |_| {}).unwrap();
        assert!(other_account.assets.contains("bbb"));
        assert!(!other_account.export_complete);
        let server_b = load(&path, &alice_b, &mut |_| {}).unwrap();
        assert_eq!(server_b.assets.len(), 2);
        assert!(server_b.export_complete);
        // Server B's duplicate line is gone: one line per attachment.
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 6, "{text}");
    }

    /// Each line names its target as its own `url` and `username` keys,
    /// beside the event's other fields, as [`ServerTarget`]'s doc says. A
    /// change to the shape on disk would make a later Export Run read none of
    /// the lines an earlier one wrote.
    #[test]
    fn each_event_writes_its_target_as_url_and_username_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(EXPORT_JOURNAL_NAME);
        append(
            &path,
            &ExportJournalEvent::AssetOk {
                target: alice(),
                sha256: "aaa".into(),
            },
        )
        .unwrap();
        append(
            &path,
            &ExportJournalEvent::ExportComplete {
                target: alice(),
                conversations: 2,
                messages: 3,
                assets: 1,
            },
        )
        .unwrap();

        let lines: Vec<serde_json::Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            lines,
            [
                serde_json::json!({
                    "event": "asset_ok",
                    "url": "http://server",
                    "username": "alice",
                    "sha256": "aaa"
                }),
                serde_json::json!({
                    "event": "export_complete",
                    "url": "http://server",
                    "username": "alice",
                    "conversations": 2,
                    "messages": 3,
                    "assets": 1
                }),
            ]
        );
    }
}
