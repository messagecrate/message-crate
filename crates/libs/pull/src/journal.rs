//! Local log of which attachments an Export from a server already downloaded.
//!
//! The file is `.message-crate-pull-state.jsonl`. JSON Lines means one JSON object per
//! line. A later Export Run can skip attachments that are already on disk.

use std::collections::HashSet;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Filename of the journal, written in the output directory.
pub const PULL_JOURNAL_NAME: &str = ".message-crate-pull-state.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
/// One row in `.message-crate-pull-state.jsonl`.
pub enum PullJournalEvent {
    /// One attachment is on disk, so a later run can skip downloading it.
    AssetOk {
        /// Server base URL the attachment came from.
        url: String,
        /// Account username the run logged in as.
        username: String,
        /// Hex SHA-256 fingerprint of the attachment bytes; the skip key.
        sha256: String,
    },
    /// An Export Run finished, with its counts.
    ExportComplete {
        /// Server base URL the Export Run read from.
        url: String,
        /// Account username the run logged in as.
        username: String,
        /// Conversations written.
        conversations: u64,
        /// Messages written.
        messages: u64,
        /// Attachments downloaded, plus those already on disk according to
        /// the journal.
        assets: u64,
    },
}

#[derive(Debug, Default)]
/// Skip sets rebuilt from the journal for one server URL and username.
pub struct PullJournalState {
    /// SHA-256 fingerprints (hex of the file bytes) of attachments already on disk.
    pub assets: HashSet<String>,
    /// True if the last run finished cleanly (an `export_complete` event was written).
    pub export_complete: bool,
}

/// Path of `.message-crate-pull-state.jsonl` inside the output directory.
pub fn journal_path(out_dir: &Path) -> PathBuf {
    out_dir.join(PULL_JOURNAL_NAME)
}

/// Read the journal and keep events that match this server URL and username.
///
/// A missing file is treated as an empty journal. A line that cannot be parsed
/// is skipped so a newer event type does not break an older client.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or a line cannot be read.
pub fn load(path: &Path, url: &str, username: &str) -> Result<PullJournalState> {
    let mut state = PullJournalState::default();
    let events: Vec<PullJournalEvent> =
        jsonl_journal::load_events("pull journal", path, &mut |_, _| {})?;
    for event in events.into_iter().filter(|e| e.is_for(url, username)) {
        match event {
            PullJournalEvent::AssetOk { sha256, .. } => {
                state.assets.insert(sha256);
            }
            PullJournalEvent::ExportComplete { .. } => state.export_complete = true,
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
pub fn append(path: &Path, event: &PullJournalEvent) -> Result<()> {
    jsonl_journal::append("pull journal", path, event)
}

/// Rewrite the lines of one server URL and username from in-memory `state`,
/// and keep every line of every other server URL and username as it was.
///
/// One output directory can hold the journal of Export runs from several
/// servers, or several accounts on one server. Each pair's lines are its own
/// skip list, so finishing a run for one pair must not drop another's.
///
/// # Errors
///
/// Returns an error when the temporary file cannot be written or the rename fails.
pub fn compact(path: &Path, url: &str, username: &str, state: &PullJournalState) -> Result<()> {
    jsonl_journal::compact_with::<PullJournalEvent, _>("pull journal", path, |read| {
        let mut events: Vec<PullJournalEvent> = read
            .into_iter()
            .filter(|event| !event.is_for(url, username))
            .collect();
        let mut assets: Vec<_> = state.assets.iter().collect();
        assets.sort_unstable();
        for sha in assets {
            events.push(PullJournalEvent::AssetOk {
                url: url.to_string(),
                username: username.to_string(),
                sha256: sha.clone(),
            });
        }
        if state.export_complete {
            // Counts are unused on resume; an `export_complete` row only means the last run finished.
            events.push(PullJournalEvent::ExportComplete {
                url: url.to_string(),
                username: username.to_string(),
                conversations: 0,
                messages: 0,
                assets: 0,
            });
        }
        events
    })
}

impl PullJournalEvent {
    /// Whether this line was written by a run against server `url` as `username`.
    fn is_for(&self, url: &str, username: &str) -> bool {
        let (u, a) = match self {
            Self::AssetOk { url, username, .. } | Self::ExportComplete { url, username, .. } => {
                (url, username)
            }
        };
        u == url && a == username
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_asset_and_export_complete_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PULL_JOURNAL_NAME);
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

        let state = load(&path, "http://server", "alice").unwrap();

        assert!(state.assets.contains("aaabbbccc"));
        assert!(state.assets.contains("dddeeefff"));
        assert!(state.export_complete);
    }

    #[test]
    fn filters_by_url_and_username() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PULL_JOURNAL_NAME);
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

        let state = load(&path, "http://server-a", "alice").unwrap();
        assert!(state.assets.contains("aaa"));
        assert!(!state.assets.contains("bbb"));
    }

    #[test]
    fn compact_sorts_assets_and_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PULL_JOURNAL_NAME);
        let mut state = PullJournalState::default();
        state.assets.insert("ccc".into());
        state.assets.insert("aaa".into());
        state.assets.insert("bbb".into());
        state.export_complete = true;

        compact(&path, "http://server", "alice", &state).unwrap();

        let reloaded = load(&path, "http://server", "alice").unwrap();
        assert_eq!(reloaded.assets.len(), 3);
        assert!(reloaded.assets.contains("aaa"));
        assert!(reloaded.assets.contains("bbb"));
        assert!(reloaded.assets.contains("ccc"));
        assert!(reloaded.export_complete);
    }

    /// `append` is what a pull actually calls, once per asset, and nothing
    /// called it: every test here wrote the file by hand or went through
    /// `compact`. Replacing it with a no-op made a pull that resumed from
    /// nothing and downloaded every asset again, with the suite green.
    #[test]
    fn appended_events_are_on_disk_and_load_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PULL_JOURNAL_NAME);

        append(
            &path,
            &PullJournalEvent::AssetOk {
                url: "http://server".into(),
                username: "alice".into(),
                sha256: "aaa".into(),
            },
        )
        .unwrap();

        // Loading between the two appends is the resume case: a pull that was
        // interrupted after one asset must find that one asset.
        let after_first = load(&path, "http://server", "alice").unwrap();
        assert!(after_first.assets.contains("aaa"));
        assert!(!after_first.export_complete);

        append(
            &path,
            &PullJournalEvent::AssetOk {
                url: "http://server".into(),
                username: "alice".into(),
                sha256: "bbb".into(),
            },
        )
        .unwrap();

        let after_second = load(&path, "http://server", "alice").unwrap();
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

    /// The parent directory is created on the way. A pull writing its first
    /// journal into a fresh output directory would otherwise fail on the very
    /// first asset.
    #[test]
    fn appending_creates_the_directory_it_needs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-yet").join(PULL_JOURNAL_NAME);

        append(
            &path,
            &PullJournalEvent::AssetOk {
                url: "http://server".into(),
                username: "alice".into(),
                sha256: "aaa".into(),
            },
        )
        .unwrap();

        assert!(path.is_file());
        assert!(
            load(&path, "http://server", "alice")
                .unwrap()
                .assets
                .contains("aaa")
        );
    }

    /// One output directory used by Export for two servers, or two accounts
    /// on one server: finishing a run for one rewrites only its own lines,
    /// so the next run for another still skips what it downloaded (#1532).
    #[test]
    fn compact_keeps_the_lines_of_every_other_server_and_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PULL_JOURNAL_NAME);
        let asset = |url: &str, username: &str, sha256: &str| PullJournalEvent::AssetOk {
            url: url.into(),
            username: username.into(),
            sha256: sha256.into(),
        };
        let complete = |url: &str, username: &str| PullJournalEvent::ExportComplete {
            url: url.into(),
            username: username.into(),
            conversations: 1,
            messages: 1,
            assets: 1,
        };
        for event in [
            asset("http://server-a", "alice", "aaa"),
            complete("http://server-a", "alice"),
            asset("http://server-a", "bob", "bbb"),
            asset("http://server-b", "alice", "ccc"),
            asset("http://server-b", "alice", "ccc"),
        ] {
            append(&path, &event).unwrap();
        }
        let mut state = PullJournalState::default();
        state.assets.insert("ccc".into());
        state.assets.insert("ddd".into());
        state.export_complete = true;

        compact(&path, "http://server-b", "alice", &state).unwrap();

        let server_a = load(&path, "http://server-a", "alice").unwrap();
        assert!(server_a.assets.contains("aaa"));
        assert!(server_a.export_complete);
        let other_account = load(&path, "http://server-a", "bob").unwrap();
        assert!(other_account.assets.contains("bbb"));
        assert!(!other_account.export_complete);
        let server_b = load(&path, "http://server-b", "alice").unwrap();
        assert_eq!(server_b.assets.len(), 2);
        assert!(server_b.export_complete);
        // Server B's duplicate line is gone: one line per attachment.
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 6, "{text}");
    }
}
