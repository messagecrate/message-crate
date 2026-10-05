//! Everything an Upload says while it runs: the on-disk log, the live progress
//! callback, and the batched progress line that keeps big imports readable.
//!
//! [`Reporter`] is the one object the rest of the crate talks to. It owns the
//! log file, the optional progress callback, and the [`ProgressBatcher`], so
//! callers never juggle three separate borrows to say one thing.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use message_crate_core::count_of;

use crate::report::{
    FileResult, PushReport, UploadProfile, elapsed_ms, format_ms_seconds, format_profile_line,
};

/// How the Upload ended with one conversation file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// Sent now.
    Ok,
    /// Not sent; the log says why.
    Failed,
    /// The journal records it as sent by an earlier attempt, so it is not
    /// sent again.
    Skipped,
}

impl FileStatus {
    /// The word the desktop app's `extract:file-done` event carries.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// Events the desktop app can show while an Upload is running.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// One line for the log panel.
    Log(String),
    /// The token was accepted; the run knows which account it imports into.
    Auth {
        /// Account id the token resolved to.
        account_id: i64,
        /// Username the server reports for that account.
        username: String,
    },
    /// Work on one conversation file began.
    FileStart {
        /// Position of this file in the run, starting at 1.
        index: usize,
        /// Files in the run.
        total: usize,
        /// File name relative to the input directory.
        file: String,
    },
    /// Work on one conversation file ended. A conversation a stop left
    /// unsent gets no `FileDone`; its `cancelled` row is in the report.
    FileDone {
        /// File name relative to the input directory.
        file: String,
        /// How it ended.
        status: FileStatus,
    },
    /// Structured skip/error for Import Errors (e.g. oversized attachment).
    Issue {
        /// `skip` when the item was left out, `error` when it failed.
        kind: String,
        /// The Stage that raised it, e.g. `upload`.
        step: String,
        /// What was affected: an attachment path or a conversation file.
        item: String,
        /// Why, in one sentence.
        reason: String,
        /// The conversation file the row is about. A row about the whole
        /// conversation has it as its `item` too. A resumed Upload reads
        /// again only the conversations the journal does not list, so this
        /// says which rows a resume reports again.
        conversation: String,
    },
    /// The run finished; the report is final.
    Finished(PushReport),
}

/// Callback type for live progress (desktop log panel, tests).
pub type ProgressFn<'a> = dyn FnMut(ProgressEvent) + Send + 'a;

/// How many finished conversations are grouped into one progress line.
/// Printing every single chat would flood the log on a big import.
const PROGRESS_BATCH_SIZE: usize = 10;

/// One attachment omitted from upload but kept as metadata on the message.
#[derive(Debug, Clone)]
pub(crate) struct AttachmentSkip {
    /// The conversation file the attachment is in.
    pub conversation: String,
    pub item: String,
    pub reason: String,
}

/// Append-only log file next to the export (also mirrored to progress callbacks).
struct LogWriter {
    file: File,
}

impl LogWriter {
    /// Create or open the log file, making parent directories if needed.
    fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("open log {}", path.display()))?;
        Ok(Self { file })
    }

    /// Write one line and flush so a crash still leaves the last message on disk.
    fn line(&mut self, msg: &str) {
        let _ = writeln!(self.file, "{msg}");
        let _ = self.file.flush();
    }
}

/// Collects finished conversations and writes one progress line every
/// [`PROGRESS_BATCH_SIZE`] of them.
struct ProgressBatcher {
    total: usize,
    done: usize,
    /// Conversations sent in this window.
    window_sent: u64,
    /// Conversations in this window the journal says were sent before.
    window_sent_before: u64,
    window_messages: u64,
    window_bytes: u64,
    window_import_ms: u64,
    /// When this window began: when the batcher was made, or when it wrote
    /// its previous line. The window's wall time is measured from here, so it
    /// covers every conversation the line counts.
    window_started: Instant,
    window_count: usize,
}

impl ProgressBatcher {
    /// Start a batcher that writes a line when a window of conversations is full.
    fn new(total: usize) -> Self {
        Self {
            total,
            done: 0,
            window_sent: 0,
            window_sent_before: 0,
            window_messages: 0,
            window_bytes: 0,
            window_import_ms: 0,
            window_started: Instant::now(),
            window_count: 0,
        }
    }

    /// Record one conversation the Upload sent. Returns a log line when the
    /// window is full.
    fn note_ok(&mut self, messages: u64, profile: &UploadProfile) -> Option<String> {
        self.done = self.done.saturating_add(1);
        self.window_count = self.window_count.saturating_add(1);
        self.window_sent = self.window_sent.saturating_add(1);
        self.window_messages = self.window_messages.saturating_add(messages);
        self.window_bytes = self.window_bytes.saturating_add(profile.asset_bytes);
        self.window_import_ms = self
            .window_import_ms
            .saturating_add(profile.message_import_ms);
        self.line_if_full()
    }

    /// Record a conversation skipped because the journal says it was sent before.
    fn note_skipped(&mut self) -> Option<String> {
        self.done = self.done.saturating_add(1);
        self.window_count = self.window_count.saturating_add(1);
        self.window_sent_before = self.window_sent_before.saturating_add(1);
        self.line_if_full()
    }

    /// Count a failure toward "done" without adding it to the window's totals.
    fn note_failed(&mut self) {
        self.done = self.done.saturating_add(1);
    }

    /// The window's line when this window is full or the run is complete.
    fn line_if_full(&mut self) -> Option<String> {
        (self.window_count >= PROGRESS_BATCH_SIZE || self.done >= self.total)
            .then(|| self.take_window_line())
    }

    /// Write any leftover partial window at the end of the run.
    fn flush_remainder(&mut self) -> Option<String> {
        (self.window_count > 0).then(|| self.take_window_line())
    }

    /// Format the current window's line, then start the next window.
    fn take_window_line(&mut self) -> String {
        let mut line = format!(
            "Finished {} of {}.",
            self.done,
            count_of(self.total as u64, "conversation", "conversations"),
        );
        if self.window_sent > 0 {
            // The import time is each conversation's own clock added up, so it
            // can exceed the window's wall time when conversations share an
            // import request.
            line.push_str(&format!(
                " In the last {} the Upload sent {} with {} and {} of Assets. \
                 Importing their messages took {}, added up across the conversations.",
                format_ms_seconds(elapsed_ms(self.window_started)),
                count_of(self.window_sent, "conversation", "conversations"),
                count_of(self.window_messages, "message", "messages"),
                media::format_bytes(self.window_bytes),
                format_ms_seconds(self.window_import_ms),
            ));
        }
        if self.window_sent_before > 0 {
            line.push_str(&format!(
                " {} had been sent before.",
                count_of(self.window_sent_before, "conversation", "conversations"),
            ));
        }
        *self = Self {
            total: self.total,
            done: self.done,
            ..Self::new(self.total)
        };
        line
    }
}

/// The single outlet for everything an Upload says.
///
/// Quiet per-file detail goes to the log file only (`log`). Anything a person
/// watching the desktop app should see goes to both the log and the progress
/// callback (`show`). Structured events (`event`) go to the callback only.
pub(crate) struct Reporter<'p, 'f> {
    log: LogWriter,
    progress: Option<&'p mut ProgressFn<'f>>,
    batcher: ProgressBatcher,
}

impl<'p, 'f> Reporter<'p, 'f> {
    /// Open the log file. The progress line's counter starts at zero until
    /// [`Reporter::expect_files`] says how many conversations the run has.
    ///
    /// # Errors
    ///
    /// Returns an error when the log file cannot be created.
    pub(crate) fn open(log_path: &Path, progress: Option<&'p mut ProgressFn<'f>>) -> Result<Self> {
        Ok(Self {
            log: LogWriter::open(log_path)?,
            progress,
            batcher: ProgressBatcher::new(0),
        })
    }

    /// Tell the progress line how many conversations this run covers.
    pub(crate) fn expect_files(&mut self, total: usize) {
        self.batcher = ProgressBatcher::new(total);
    }

    /// Write to the log file only.
    pub(crate) fn log(&mut self, line: &str) {
        self.log.line(line);
    }

    /// Write to the log file and mirror the same text to the progress callback.
    pub(crate) fn show(&mut self, line: String) {
        self.log.line(&line);
        self.event(ProgressEvent::Log(line));
    }

    /// Send a structured event to the progress callback, if there is one.
    pub(crate) fn event(&mut self, event: ProgressEvent) {
        if let Some(cb) = self.progress.as_mut() {
            cb(event);
        }
    }

    /// Announce that a conversation is starting.
    pub(crate) fn file_start(&mut self, index: usize, total: usize, file: &str) {
        self.event(ProgressEvent::FileStart {
            index,
            total,
            file: file.to_string(),
        });
    }

    /// Announce how the Upload ended with a conversation.
    pub(crate) fn file_done(&mut self, file: &str, status: FileStatus) {
        self.event(ProgressEvent::FileDone {
            file: file.to_string(),
            status,
        });
    }

    /// Record a successful conversation in the progress line.
    pub(crate) fn note_ok(&mut self, messages: u64, profile: &UploadProfile) {
        if let Some(line) = self.batcher.note_ok(messages, profile) {
            self.show(line);
        }
    }

    /// Record a skipped conversation in the progress line.
    pub(crate) fn note_skipped(&mut self) {
        if let Some(line) = self.batcher.note_skipped() {
            self.show(line);
        }
    }

    /// Record a failed conversation: flush the pending progress line first so
    /// failure text is not mixed into it, then log the failure and, when
    /// known, where its time went so slow failures stay diagnosable.
    pub(crate) fn note_failed(&mut self, name: &str, error: &str, profile: Option<&UploadProfile>) {
        self.flush_file_counter();
        self.batcher.note_failed();
        self.show(format!("{name} failed: {error}"));
        if let Some(profile) = profile {
            self.show(format_profile_line(name, profile));
        }
    }

    /// Write any partial progress line (end of run, or before an error line).
    pub(crate) fn flush_file_counter(&mut self) {
        if let Some(line) = self.batcher.flush_remainder() {
            self.show(line);
        }
    }

    /// Write Import Errors skip rows for attachments that were not uploaded.
    pub(crate) fn attachment_skips(&mut self, skips: &[AttachmentSkip]) {
        for skip in skips {
            self.show(format!("Did not upload {}: {}", skip.item, skip.reason));
            self.event(ProgressEvent::Issue {
                kind: "skip".into(),
                step: "upload".into(),
                item: skip.item.clone(),
                reason: skip.reason.clone(),
                conversation: skip.conversation.clone(),
            });
        }
    }

    /// Send one Import Errors row per conversation that failed or was left
    /// unsent by a stop, so any consumer can list them without reading the
    /// report.
    /// A `skipped` conversation gets no row: the Upload's journal skips only a
    /// conversation an earlier part of the same run sent, so it is on the
    /// server and is not a problem.
    /// The log already carries the "… failed: …" line for each failure, so this
    /// goes to the callback only.
    pub(crate) fn conversation_issues(&mut self, results: &[FileResult]) {
        for result in results {
            let (kind, fallback) = match result.status.as_str() {
                "failed" => ("error", "upload failed"),
                "cancelled" => ("skip", "the Upload ended before this conversation was sent"),
                _ => continue,
            };
            self.event(ProgressEvent::Issue {
                kind: kind.into(),
                step: "upload".into(),
                item: result.file.clone(),
                reason: result.error.clone().unwrap_or_else(|| fallback.to_string()),
                conversation: result.file.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_batcher_emits_every_ten_and_on_completion() {
        let mut batcher = ProgressBatcher::new(25);
        let profile = UploadProfile {
            message_import_ms: 3_300,
            total_ms: 5_500,
            asset_bytes: 700_000,
            ..UploadProfile::default()
        };
        let mut lines = Vec::new();
        for _ in 0..9 {
            assert!(batcher.note_ok(2, &profile).is_none());
        }
        let tenth = batcher.note_ok(2, &profile).unwrap();
        assert!(
            tenth.starts_with("Finished 10 of 25 conversations. In the last "),
            "{tenth}"
        );
        assert!(
            tenth.ends_with(
                " the Upload sent 10 conversations with 20 messages and 7.0 MB of Assets. \
                 Importing their messages took 33.0s, added up across the conversations."
            ),
            "{tenth}"
        );
        // The window's time is its wall clock, not the sum of profile.total_ms.
        assert!(!tenth.contains("last 55.0s"), "{tenth}");
        assert!(!tenth.contains('='));
        lines.push(tenth);
        for _ in 0..15 {
            if let Some(line) = batcher.note_ok(1, &profile) {
                lines.push(line);
            }
        }
        assert_eq!(lines.len(), 3);
        assert!(lines[1].starts_with("Finished 20 of 25 conversations. In the last "));
        assert!(lines[1].contains(" sent 10 conversations with 10 messages "));
        assert!(lines[2].starts_with("Finished 25 of 25 conversations. In the last "));
        assert!(lines[2].contains(" sent 5 conversations with 5 messages "));
    }

    /// A window of one conversation with one message words both counts
    /// singular (#1825).
    #[test]
    fn progress_line_counts_one_conversation_and_one_message() {
        let mut batcher = ProgressBatcher::new(1);
        let profile = UploadProfile {
            message_import_ms: 200,
            asset_bytes: 500,
            ..UploadProfile::default()
        };
        let line = batcher.note_ok(1, &profile).unwrap();
        assert!(
            line.starts_with("Finished 1 of 1 conversation. In the last "),
            "{line}"
        );
        assert!(
            line.ends_with(
                " the Upload sent 1 conversation with 1 message and 500 B of Assets. \
                 Importing their messages took 0.2s, added up across the conversations."
            ),
            "{line}"
        );
    }

    /// The window's clock starts before its first conversation is worked on,
    /// so a window of one conversation counts that conversation's time rather
    /// than reading 0.0s.
    #[test]
    fn the_window_time_covers_its_first_conversation() {
        let mut batcher = ProgressBatcher::new(1);
        std::thread::sleep(std::time::Duration::from_millis(150));
        let line = batcher.note_ok(1, &UploadProfile::default()).unwrap();
        let seconds: f64 = line
            .strip_prefix("Finished 1 of 1 conversation. In the last ")
            .and_then(|rest| rest.split_once('s'))
            .and_then(|(seconds, _)| seconds.parse().ok())
            .unwrap_or_else(|| panic!("{line}"));
        assert!(seconds >= 0.1, "{line}");
    }

    /// Conversations the journal says were sent before are named apart, so
    /// the line never says they were sent again, and a failure in the window
    /// counts toward "Finished" only.
    #[test]
    fn conversations_sent_before_are_named_apart_from_those_sent() {
        let mut batcher = ProgressBatcher::new(3);
        assert!(batcher.note_skipped().is_none());
        batcher.note_failed();
        let line = batcher.note_skipped().unwrap();
        assert_eq!(
            line,
            "Finished 3 of 3 conversations. 2 conversations had been sent before."
        );

        let mut batcher = ProgressBatcher::new(2);
        assert!(batcher.note_ok(4, &UploadProfile::default()).is_none());
        let line = batcher.note_skipped().unwrap();
        assert!(
            line.contains(" the Upload sent 1 conversation with 4 messages "),
            "{line}"
        );
        assert!(
            line.ends_with(" 1 conversation had been sent before."),
            "{line}"
        );
    }

    #[test]
    fn reporter_mirrors_shown_lines_to_callback_and_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("push.log");
        let mut seen = Vec::new();
        {
            let mut cb = |event: ProgressEvent| seen.push(event);
            let mut reporter = Reporter::open(&log_path, Some(&mut cb)).unwrap();
            reporter.log("quiet");
            reporter.show("loud".into());
        }
        let text = fs::read_to_string(&log_path).unwrap();
        assert_eq!(text, "quiet\nloud\n");
        let shown: Vec<String> = seen
            .into_iter()
            .map(|event| match event {
                ProgressEvent::Log(line) => line,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(shown, ["loud"]);
    }

    #[test]
    fn conversation_issues_cover_failed_and_cancelled_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("push.log");
        let mut seen = Vec::new();
        {
            let mut cb = |event: ProgressEvent| seen.push(event);
            let mut reporter = Reporter::open(&log_path, Some(&mut cb)).unwrap();
            reporter.conversation_issues(&[
                FileResult {
                    file: "ok.jsonl".into(),
                    status: "ok".into(),
                    error: None,
                    messages: 3,
                    attachments: 0,
                    profile: None,
                },
                FileResult::failed("bad.jsonl", "attachment exceeds limit"),
                FileResult::skipped("done.jsonl"),
                FileResult::cancelled("unsent.jsonl"),
                FileResult {
                    file: "silent.jsonl".into(),
                    status: "failed".into(),
                    error: None,
                    messages: 0,
                    attachments: 0,
                    profile: None,
                },
            ]);
        }
        let rows: Vec<(String, String, String)> = seen
            .into_iter()
            .map(|event| match event {
                ProgressEvent::Issue {
                    kind, item, reason, ..
                } => (kind, item, reason),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "error".to_string(),
                    "bad.jsonl".to_string(),
                    "attachment exceeds limit".to_string()
                ),
                (
                    "skip".to_string(),
                    "unsent.jsonl".to_string(),
                    "the Upload ended before this conversation was sent".to_string()
                ),
                (
                    "error".to_string(),
                    "silent.jsonl".to_string(),
                    "upload failed".to_string()
                ),
            ]
        );
        assert_eq!(
            fs::read_to_string(&log_path).unwrap(),
            "",
            "conversation issues are callback-only; the log already has the fail lines"
        );
    }
}
