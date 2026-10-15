//! JSON Lines state journals: append-only logs rewritten by sorted compaction.
//!
//! A journal is one JSON object per line. Readers rebuild skip-sets from the
//! events; writers append rows and periodically rewrite the file compacted.
//! Events are opaque to this crate. Callers bring their own serde type. The
//! one shape this crate names is [`ServerTarget`], the server and account an
//! event belongs to, which the Upload's journal and the Export's both carry.
//! An event that names it through [`TargetedEvent`] can be loaded and
//! compacted one target at a time.
//!
//! All writes run under one process-wide lock, so a rewrite can never mix
//! bytes with a concurrent append.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The server and account a journal records progress for: the server's URL
/// and the username the session resolved to. One directory can be uploaded
/// to more than one server and account, or be exported into from more than
/// one, so every event names its target.
///
/// An event holds it as `#[serde(flatten)]`, so on disk the two are a line's
/// own `url` and `username` keys, beside the event's other fields, and each
/// line reads whole on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerTarget {
    /// Base URL of the server.
    pub url: String,
    /// Username of the account the run logged in as.
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

/// An event that belongs to one [`ServerTarget`].
///
/// A journal can hold the events of several servers and accounts, and a run
/// reads and rewrites only its own: [`load_for_target`] keeps the events of
/// one target, and [`compact_for_target`] keeps every other target's.
pub trait TargetedEvent {
    /// The server and account this event belongs to.
    fn target(&self) -> &ServerTarget;
}

/// One lock for append and rewrite so two threads cannot mix bytes on a line
/// or rewrite the file while another thread is appending.
static JOURNAL_WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Append one event as a single JSON Lines row and sync it to disk.
///
/// The event is serialized to a buffer first so a serialization failure cannot
/// tear a half-written row. When a crash cut the last row short, the new row
/// starts on a line of its own: joined to the partial row it would be one
/// unreadable line, skipped on every later load.
///
/// # Errors
///
/// Returns an error when the parent directory cannot be created, the file cannot
/// be opened, read or synced, the event cannot be serialized, or the write
/// fails.
pub fn append<E: Serialize>(label: &str, path: &Path, event: &E) -> Result<()> {
    let _guard = JOURNAL_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {label} for append {}", path.display()))?;
    let mut buf = Vec::new();
    if ends_in_a_torn_row(&mut file)
        .with_context(|| format!("read the end of {label} {}", path.display()))?
    {
        buf.push(b'\n');
    }
    serde_json::to_writer(&mut buf, event).context("serialize journal event")?;
    buf.push(b'\n');
    file.write_all(&buf)?;
    file.sync_data()
        .with_context(|| format!("sync {label} {}", path.display()))?;
    Ok(())
}

/// Whether the journal is not empty and its last byte is not a newline.
fn ends_in_a_torn_row(file: &mut File) -> std::io::Result<bool> {
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    let mut last = [0_u8; 1];
    file.seek(SeekFrom::End(-1))?;
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

/// Parse every event from a journal file, of every target.
///
/// A missing file is treated as an empty journal. Each line that cannot be
/// parsed is reported to `on_unreadable(line_number, parse_error)` and
/// skipped, as [`load_for_target`] says.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or read.
fn load_events<E: DeserializeOwned>(
    label: &str,
    path: &Path,
    on_unreadable: &mut dyn FnMut(usize, &serde_json::Error),
) -> Result<Vec<E>> {
    let mut events = Vec::new();
    if !path.is_file() {
        return Ok(events);
    }
    let file = File::open(path).with_context(|| format!("open {label} {}", path.display()))?;
    for (i, line) in BufReader::new(file).split(b'\n').enumerate() {
        let line = line.with_context(|| format!("read {label} line {}", i + 1))?;
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice(&line) {
            Ok(event) => events.push(event),
            Err(error) => on_unreadable(i + 1, &error),
        }
    }
    Ok(events)
}

/// Read the journal, keep the events of `target`, and fold them in file order
/// into a state that starts from `S::default()`.
///
/// A missing file folds nothing. Each line that cannot be parsed is reported
/// to `on_unreadable(line_number, parse_error)` and skipped. The caller
/// decides whether to warn or stay silent. A line that is not valid UTF-8,
/// such as a row a crash cut inside a character, is one of those lines, so it
/// cannot stop every later run from reading the file.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or read.
pub fn load_for_target<E, S>(
    label: &str,
    path: &Path,
    target: &ServerTarget,
    on_unreadable: &mut dyn FnMut(usize, &serde_json::Error),
    mut fold: impl FnMut(&mut S, E),
) -> Result<S>
where
    E: DeserializeOwned + TargetedEvent,
    S: Default,
{
    let mut state = S::default();
    for event in load_events::<E>(label, path, on_unreadable)? {
        if event.target() == target {
            fold(&mut state, event);
        }
    }
    Ok(state)
}

/// serde's text for why a journal line did not parse, for a sentence that
/// names the line by its number in the file.
///
/// serde names a position within the line's JSON, whose line is always 1, so
/// only its column is kept. An error with no position (line 0, which a
/// missing or mistyped field gives) keeps serde's text whole.
pub fn unreadable_line_reason(error: &serde_json::Error) -> String {
    let text = error.to_string();
    if error.line() == 0 {
        return text;
    }
    let position = format!(" at line {} column {}", error.line(), error.column());
    match text.strip_suffix(&position) {
        Some(reason) => format!("{reason} at column {}", error.column()),
        None => text,
    }
}

/// Rewrite the lines of `target` as `events`, and keep every line of every
/// other target as it was. The kept lines stay in their order, and `events`
/// follow them.
///
/// One directory can hold the progress of runs against several servers, or
/// several accounts on one server. Each target's lines are its own skip list,
/// so compacting one target must not drop another's. Every event in `events`
/// belongs to `target`.
///
/// Unreadable lines are dropped silently, in the Upload's journal and the
/// Export's alike.
///
/// # Errors
///
/// Returns an error when the file cannot be read, the temporary file cannot
/// be written, or the rename fails.
pub fn compact_for_target<E>(
    label: &str,
    path: &Path,
    target: &ServerTarget,
    events: Vec<E>,
) -> Result<()>
where
    E: Serialize + DeserializeOwned + TargetedEvent,
{
    debug_assert!(events.iter().all(|event| event.target() == target));
    compact_with::<E, _>(label, path, |mut kept| {
        kept.retain(|event| event.target() != target);
        kept.extend(events);
        kept
    })
}

/// One `asset_ok` event per Asset fingerprint in `assets`, in sorted order,
/// each built by `asset_ok` from its fingerprint.
///
/// The Upload's journal and the Export's both start a compacted journal with
/// these events, so a compacted file comes out the same whatever order the
/// skip set held its fingerprints in.
pub fn asset_ok_events<'a, E>(
    assets: impl IntoIterator<Item = &'a String>,
    asset_ok: impl FnMut(String) -> E,
) -> Vec<E> {
    let mut assets: Vec<&String> = assets.into_iter().collect();
    assets.sort_unstable();
    assets.into_iter().cloned().map(asset_ok).collect()
}

/// Read the journal, transform the surviving events with `rebuild`, and
/// rewrite the file, all under one write-lock acquisition, so a concurrent
/// [`append`] either lands before the read or after the rewrite, never between
/// them.
///
/// Unreadable lines are skipped silently during the read.
///
/// # Errors
///
/// Returns an error when the file cannot be read, the temporary file cannot
/// be written, or the rename fails.
fn compact_with<E, F>(label: &str, path: &Path, rebuild: F) -> Result<()>
where
    E: Serialize + DeserializeOwned,
    F: FnOnce(Vec<E>) -> Vec<E>,
{
    let _guard = JOURNAL_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Unreadable lines are skipped silently.
    let events = load_events::<E>(label, path, &mut |_, _| {})?;
    let events = rebuild(events);
    write_unlocked(path, &events)
}

/// Write `events` to a temp file and rename it over the journal (lock held),
/// synced, so a power loss leaves the old journal or the whole new one.
fn write_unlocked<E: Serialize>(path: &Path, events: &[E]) -> Result<()> {
    file_io::write_atomic(path, |out| {
        for event in events {
            serde_json::to_writer(&mut *out, event).context("serialize journal event")?;
            out.write_all(b"\n")?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestEvent {
        url: String,
        user: String,
        key: String,
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        let events: Vec<TestEvent> = load_events("journal", &path, &mut |_, _| {}).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn unreadable_lines_are_reported_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"url\":\"http://server\",\"user\":\"alice\",\"key\":\"a\"}\n",
                "{not json}\n",
                "{\"url\":\"http://server\",\"user\":\"alice\",\"key\":\"b\"}\n",
            ),
        )
        .unwrap();
        let mut reported = Vec::new();
        let events: Vec<TestEvent> = load_events("journal", &path, &mut |line, error| {
            reported.push((line, error.to_string()));
        })
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].key, "b");
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].0, 2);
    }

    /// A line that is not valid UTF-8 is reported and skipped like any other
    /// unreadable line, rather than failing the whole read.
    #[test]
    fn a_line_that_is_not_utf8_is_reported_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        let mut bytes = b"{\"url\":\"http://caf\xc3".to_vec();
        bytes.extend_from_slice(
            b"\n{\"url\":\"http://server\",\"user\":\"alice\",\"key\":\"b\"}\r\n",
        );
        fs::write(&path, bytes).unwrap();
        let mut reported = Vec::new();
        let events: Vec<TestEvent> = load_events("journal", &path, &mut |line, _| {
            reported.push(line);
        })
        .unwrap();
        assert_eq!(reported, [1]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].key, "b");
    }

    fn parse_error(line: &str) -> serde_json::Error {
        serde_json::from_str::<TestEvent>(line).unwrap_err()
    }

    /// serde's position is cut to its column, so a sentence naming the
    /// journal's own line number names only one line.
    #[test]
    fn the_reason_keeps_only_serdes_column() {
        let reason = unreadable_line_reason(&parse_error("{not json"));
        assert!(reason.ends_with(" at column 2"), "{reason}");
        assert!(!reason.contains(" at line "), "{reason}");
    }

    /// An event tagged by a field, as both journals' events are.
    #[derive(Debug, Deserialize)]
    #[serde(tag = "event", rename_all = "snake_case")]
    enum TaggedEvent {
        #[allow(dead_code)]
        Seen { url: String, key: String },
    }

    /// A line of a tagged event that misses a field gives serde no position,
    /// and its text is kept whole, even when a value in it holds the words
    /// " at line ".
    #[test]
    fn a_reason_with_no_position_keeps_serdes_text_whole() {
        let error =
            serde_json::from_slice::<TaggedEvent>(br#"{"event":"seen","url":"sms at line 2"}"#)
                .unwrap_err();
        assert_eq!(error.line(), 0, "{error}");
        assert_eq!(unreadable_line_reason(&error), error.to_string());
    }

    #[test]
    fn append_writes_complete_lines_under_contention() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(dir.path().join("state.jsonl"));
        let mut handles = Vec::new();
        for i in 0..8 {
            let path = std::sync::Arc::clone(&path);
            handles.push(std::thread::spawn(move || {
                for j in 0..50 {
                    append(
                        "journal",
                        &path,
                        &TestEvent {
                            url: "http://server".into(),
                            user: "alice".into(),
                            key: format!("g-{i}-{j}"),
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
            serde_json::from_str::<TestEvent>(line).expect("torn line");
            lines += 1;
        }
        assert_eq!(lines, 8 * 50);
    }

    #[test]
    fn compact_with_rebuilds_under_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        append(
            "journal",
            &path,
            &TestEvent {
                url: "http://a".into(),
                user: "alice".into(),
                key: "old".into(),
            },
        )
        .unwrap();
        compact_with::<TestEvent, _>("journal", &path, |mut events| {
            events.push(TestEvent {
                url: "http://a".into(),
                user: "alice".into(),
                key: "new".into(),
            });
            events
        })
        .unwrap();
        let loaded: Vec<TestEvent> = load_events("journal", &path, &mut |_, _| {}).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[1].key, "new");
        assert!(!path.with_extension("jsonl.tmp").exists());
    }

    #[test]
    fn appends_concurrent_with_compact_are_never_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(dir.path().join("state.jsonl"));
        let mut handles = Vec::new();
        for i in 0..4 {
            let path = std::sync::Arc::clone(&path);
            handles.push(std::thread::spawn(move || {
                for j in 0..25 {
                    append(
                        "journal",
                        &path,
                        &TestEvent {
                            url: "http://server".into(),
                            user: "alice".into(),
                            key: format!("c-{i}-{j}"),
                        },
                    )
                    .unwrap();
                }
            }));
        }
        for _ in 0..3 {
            compact_with::<TestEvent, _>("journal", &path, |events| events).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for h in handles {
            h.join().unwrap();
        }
        let loaded: Vec<TestEvent> = load_events("journal", &path, &mut |_, _| {}).unwrap();
        assert_eq!(loaded.len(), 4 * 25);
        let text = fs::read_to_string(&*path).unwrap();
        let mut lines = 0usize;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            serde_json::from_str::<TestEvent>(line).expect("torn line");
            lines += 1;
        }
        assert_eq!(lines, 4 * 25);
    }

    /// An event that names its target, as both journals' events do.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Seen {
        #[serde(flatten)]
        target: ServerTarget,
        key: String,
    }

    impl TargetedEvent for Seen {
        fn target(&self) -> &ServerTarget {
            &self.target
        }
    }

    fn seen(url: &str, username: &str, key: &str) -> Seen {
        Seen {
            target: ServerTarget::new(url, username),
            key: key.into(),
        }
    }

    /// Only the events of the asked target are folded: another server's, and
    /// another account's on the same server, are left out.
    #[test]
    fn load_for_target_folds_only_that_targets_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        for event in [
            seen("http://a", "alice", "1"),
            seen("http://b", "alice", "2"),
            seen("http://a", "bob", "3"),
            seen("http://a", "alice", "4"),
        ] {
            append("journal", &path, &event).unwrap();
        }
        let keys: Vec<String> = load_for_target(
            "journal",
            &path,
            &ServerTarget::new("http://a", "alice"),
            &mut |_, _| {},
            |keys: &mut Vec<String>, event: Seen| keys.push(event.key),
        )
        .unwrap();
        assert_eq!(keys, ["1", "4"]);
    }

    /// A missing file folds nothing, and an unreadable line is reported and
    /// skipped.
    #[test]
    fn load_for_target_reports_unreadable_lines_and_starts_from_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        let target = ServerTarget::new("http://a", "alice");
        let empty: Vec<String> = load_for_target(
            "journal",
            &path,
            &target,
            &mut |_, _| panic!("a missing file has no lines"),
            |keys: &mut Vec<String>, event: Seen| keys.push(event.key),
        )
        .unwrap();
        assert!(empty.is_empty());

        fs::write(
            &path,
            "{not json}\n{\"url\":\"http://a\",\"username\":\"alice\",\"key\":\"1\"}\n",
        )
        .unwrap();
        let mut reported = Vec::new();
        let keys: Vec<String> = load_for_target(
            "journal",
            &path,
            &target,
            &mut |line, _| reported.push(line),
            |keys: &mut Vec<String>, event: Seen| keys.push(event.key),
        )
        .unwrap();
        assert_eq!(reported, [1]);
        assert_eq!(keys, ["1"]);
    }

    /// Compacting one target replaces that target's lines and keeps every
    /// other target's lines as they were.
    #[test]
    fn compact_for_target_keeps_every_other_targets_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        for event in [
            seen("http://a", "alice", "old"),
            seen("http://b", "alice", "b"),
            seen("http://a", "bob", "bob"),
        ] {
            append("journal", &path, &event).unwrap();
        }
        compact_for_target(
            "journal",
            &path,
            &ServerTarget::new("http://a", "alice"),
            vec![seen("http://a", "alice", "new")],
        )
        .unwrap();
        let loaded: Vec<Seen> = load_events("journal", &path, &mut |_, _| {}).unwrap();
        assert_eq!(
            loaded,
            [
                seen("http://b", "alice", "b"),
                seen("http://a", "bob", "bob"),
                seen("http://a", "alice", "new"),
            ]
        );
    }

    /// The events come out sorted by fingerprint, whatever order the
    /// fingerprints went in, each built by the caller's closure.
    #[test]
    fn asset_ok_events_are_sorted_by_fingerprint() {
        let assets = ["ccc".to_string(), "aaa".to_string(), "bbb".to_string()];
        let events = asset_ok_events(&assets, |sha| seen("http://a", "alice", &sha));
        assert_eq!(
            events,
            [
                seen("http://a", "alice", "aaa"),
                seen("http://a", "alice", "bbb"),
                seen("http://a", "alice", "ccc"),
            ]
        );
    }

    #[test]
    fn an_append_after_a_torn_last_line_is_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("j.jsonl");
        // A crash cut the previous row short: no closing brace, no newline.
        fs::write(&path, b"{\"url\":\"u\",\"us").unwrap();
        let event = TestEvent {
            url: "u".into(),
            user: "me".into(),
            key: "file-1".into(),
        };
        append("test", &path, &event).unwrap();
        let events: Vec<TestEvent> = load_events("test", &path, &mut |_, _| {}).unwrap();
        assert_eq!(
            events,
            vec![event],
            "the event appended after the torn row is lost"
        );
    }
}
