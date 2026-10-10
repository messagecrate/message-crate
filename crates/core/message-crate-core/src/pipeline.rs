//! Helpers shared by the exporter crates and the desktop app's in-process runners.
//!
//! This module keeps its dependency surface small (only `anyhow` for
//! context-rich path errors) so the desktop app stays lightweight. Callers map
//! `String` errors at the edge when needed.

use crate::config::OutputFormat;
use crate::counter::{
    ATTACHMENTS_SAVED, CONVERSATIONS_OBFUSCATED, CONVERSATIONS_RESUMED, Counter,
    DUPLICATES_DROPPED, ItemKind, NOT_SMS_OR_MMS_LEFT_OUT, NOTIFICATIONS, SKIPPED_INVALID_DATE,
    count_of_files, error_and_note_lines, item_line, item_reason,
};
use anyhow::{Context, bail};
use media::MediaReport;
pub use message_crate_api_types::RunIssueKind;
use message_ir::{
    ConversationDocument, PendingConversation, ProjectionHooks, ProjectionTally,
    pending_to_document, prepare_conversation,
};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Recursively walk `root`, collecting files that match `predicate`.
/// Skips symlinks (both files and directories). Directories are
/// traversed depth-first with no explicit depth limit (callers
/// should use this on trusted local input trees).
///
/// # Errors
///
/// Returns an I/O error when `root` cannot be read.
pub fn discover_files(
    root: &Path,
    predicate: &dyn Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut out = Vec::new();
    discover_files_into(root, predicate, &mut out)?;
    Ok(out)
}

/// Append matching files under `dir` onto `out`.
///
/// # Errors
///
/// Returns an I/O error when a directory cannot be read.
fn discover_files_into(
    dir: &Path,
    predicate: &dyn Fn(&Path) -> bool,
    out: &mut Vec<PathBuf>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            discover_files_into(&path, predicate, out)?;
        } else if ft.is_file() && predicate(&path) {
            out.push(path);
        }
    }
    Ok(())
}

/// Result of a successful exporter `run`: human-readable log lines and the
/// counts from the run's [`ExportReport`].
#[derive(Debug, Default)]
pub struct RunResult {
    /// Human-readable log lines (summary lines plus mid-run notes).
    pub messages: Vec<String>,
    /// Conversations exported, as [`ExportReport::conversations`] counted them.
    pub conversations: u64,
    /// Messages exported, as [`ExportReport::messages`] counted them.
    pub message_count: u64,
}

impl RunResult {
    /// The log lines `messages` with the counts of `report`.
    pub fn new(messages: Vec<String>, report: &ExportReport) -> Self {
        Self {
            messages,
            conversations: report.conversations,
            message_count: report.messages,
        }
    }
}

/// One row a run reports for its Import Run's record, sent through an
/// [`IssueSink`] as the run records it. The fields are the ones the upload
/// reports its own issues with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunIssue {
    /// What the row is: an Import Error ([`RunIssueKind::Skip`] or
    /// [`RunIssueKind::Error`]), a note, or [`RunIssueKind::Resolved`] when
    /// an earlier row with the same step and item no longer holds.
    pub kind: RunIssueKind,
    /// The step that raised it, such as `attachments`.
    pub step: String,
    /// What was affected, such as an attachment's path in the backup.
    pub item: String,
    /// Why, in one sentence; for a note, what the run did.
    pub reason: String,
    /// The conversation file the row is about, by the name the write queue
    /// announces it with ([`crate::ProgressEvent::FileWritten`]), for a row
    /// recorded while the write queue writes that conversation. A resumed
    /// Staging skips a conversation already written without reading it
    /// again, so such a row is reported again only while its conversation
    /// is not yet written. `None` for every other row, such as one recorded
    /// while the run reads the backup, which every resumed run reads again
    /// in full (#1688).
    pub conversation: Option<String>,
}

/// The note an exporter sends for a chat that names its person with no
/// address, which it keeps under the name alone, counted as
/// [`crate::NAME_ONLY_CHAT`].
pub const NAME_ONLY_CHAT_NOTE: &str = "This chat names its person with no phone number or email \
     address, so the conversation is kept under the name alone.";

/// The note an exporter sends for a message it kept with `n` parts left out
/// because they could not be read.
pub fn unreadable_parts_note(n: u64) -> String {
    match n {
        1 => "1 part of this message could not be read and was left out. The message itself is \
              kept."
            .into(),
        n => format!(
            "{n} parts of this message could not be read and were left out. The message itself \
             is kept."
        ),
    }
}

/// What an exporter says happened to a CSV it cannot read, before the
/// parser's own words, as the `what_happened` of an [`ExportReport::error`]
/// of [`ItemKind::Csv`].
pub const CSV_NOT_READ: &str = "could not be read and was left out";

/// The [`RunIssue::step`] of a row an exporter records while it reads the
/// backup.
const READ_STEP: &str = "parse";

/// Callback that receives each [`RunIssue`] the moment a run records it.
///
/// The desktop app sets one and sends each row on to its window, which
/// writes it into the Import Run's record at once: an app that closes
/// mid-run keeps every row that had arrived. A row's `kind` says what it
/// is, so a new kind of row travels through the same sink. A run without a
/// sink reports its rows nowhere but the log lines it writes beside them.
#[derive(Clone)]
pub struct IssueSink(Arc<dyn Fn(RunIssue) + Send + Sync>);

impl IssueSink {
    /// Wrap a callback that receives one row at a time.
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(RunIssue) + Send + Sync + 'static,
    {
        Self(Arc::new(f))
    }

    /// Send one row to the callback.
    pub fn emit(&self, issue: RunIssue) {
        (self.0)(issue);
    }
}

impl fmt::Debug for IssueSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IssueSink")
    }
}

/// Send `issue` to `sink` when one is set.
pub fn emit_issue(sink: Option<&IssueSink>, issue: RunIssue) {
    if let Some(sink) = sink {
        sink.emit(issue);
    }
}

/// Export run statistics: what was counted while parsing and what the
/// write tail did. Per-exporter extension counters (PDU counts, dedupe
/// counts, etc.) are stored in the `extra` list, in the order they were first
/// counted.
#[derive(Debug, Default, Clone)]
pub struct ExportReport {
    /// Conversations exported.
    pub conversations: u64,
    /// Conversations a resumed run found already written and did not write
    /// again. Counted in `conversations` as well, because they are part of
    /// the export; this says how much of it this run did not have to do.
    pub conversations_skipped: u64,
    /// Messages exported.
    pub messages: u64,
    /// Outgoing messages exported.
    pub sent: u64,
    /// Incoming messages exported.
    pub received: u64,
    /// Rows skipped because their date could not be parsed.
    pub skipped_invalid_date: u64,
    /// Duplicate rows dropped during dedupe.
    pub duplicates_dropped: u64,
    /// Attachment files saved to the output.
    pub attachments_saved: u64,
    /// What media conversion did to the staged attachments.
    pub media: MediaReport,
    /// Documents whose handles, names and bodies were obfuscated.
    pub obfuscated_docs: u64,
    /// What failed, one sentence for each item that names it, such as an
    /// [`crate::item_line`] (capped by each exporter). The summary gives
    /// them under the Import Errors heading.
    pub errors: Vec<String>,
    /// What the run did that is worth knowing but did not fail, such as a
    /// choice it made between two rows, one sentence for each item that
    /// names it. The summary gives them under a heading of their own, apart
    /// from `errors`, so a note never reads as a failure.
    pub notes: Vec<String>,
    /// Per-exporter extension counters, each with the words its log line
    /// uses, in the order the run first counted them, so the summary gives
    /// them in the order the exporter does.
    pub extra: Vec<(Counter, u64)>,
    /// Where [`ExportReport::error`], [`ExportReport::note`] and
    /// [`ExportReport::caveat`] send each row the moment the run records it.
    /// `None` sends the rows nowhere, and the lines in `errors` and `notes`
    /// are all that is kept.
    pub issues: Option<IssueSink>,
}

impl ExportReport {
    /// An empty report that sends its rows to `issues`.
    pub fn with_issues(issues: Option<IssueSink>) -> Self {
        Self {
            issues,
            ..Self::default()
        }
    }

    /// Record that the run could not read `item`, of `kind`, and what
    /// happened to it, `what_happened` worded to follow the item: its
    /// [`item_line`] in `errors`, and an Import Error with its
    /// [`item_reason`] sent to `issues` at once.
    pub fn error(&mut self, kind: ItemKind, item: impl Into<String>, what_happened: &str) {
        let item = item.into();
        self.errors.push(item_line(kind, &item, what_happened));
        self.send(RunIssueKind::Error, item, item_reason(kind, what_happened));
    }

    /// Record something the run did with `item`, of `kind`, that is worth
    /// knowing but did not fail, `what_happened` worded to follow the item:
    /// its [`item_line`] in `notes`, and a note with its [`item_reason`]
    /// sent to `issues` at once.
    pub fn note(&mut self, kind: ItemKind, item: impl Into<String>, what_happened: &str) {
        let item = item.into();
        self.notes.push(item_line(kind, &item, what_happened));
        self.send(RunIssueKind::Note, item, item_reason(kind, what_happened));
    }

    /// Count `by` under `counter` for one item the run kept with a caveat,
    /// and send a note that names the item to `issues`. The log keeps the
    /// count, which says as much as a line per item would.
    pub fn caveat(
        &mut self,
        counter: Counter,
        by: u64,
        item: impl Into<String>,
        text: impl Into<String>,
    ) {
        self.bump(counter, by);
        self.caveat_note(item, text);
    }

    /// Send a note that names one item the run kept with a caveat to
    /// `issues`, for an exporter that counts the caveat's total itself.
    pub fn caveat_note(&self, item: impl Into<String>, text: impl Into<String>) {
        self.send(RunIssueKind::Note, item.into(), text.into());
    }

    /// Send one row about an item of the backup to `issues`.
    fn send(&self, kind: RunIssueKind, item: String, reason: String) {
        emit_issue(
            self.issues.as_ref(),
            RunIssue {
                kind,
                step: READ_STEP.into(),
                item,
                reason,
                conversation: None,
            },
        );
    }

    /// The run's log line for [`NOT_SMS_OR_MMS_LEFT_OUT`]: how many messages
    /// were left out of `format`, the format that holds only SMS and MMS, and
    /// why. `None` when none were left out.
    pub fn not_sms_or_mms_line(&self, format: &str) -> Option<String> {
        let left_out = self.extra(NOT_SMS_OR_MMS_LEFT_OUT);
        (left_out > 0).then(|| {
            format!(
                "{}, because {format} holds only SMS and MMS",
                NOT_SMS_OR_MMS_LEFT_OUT.line(left_out)
            )
        })
    }

    /// Refuse a run whose Media stage failed on every file it tried, when
    /// the mode needed ffmpeg: that is a missing tool, not a bad file.
    ///
    /// # Errors
    ///
    /// Returns an error when `needs_tools` is set, the Media stage reported
    /// errors, and it processed nothing.
    pub fn check_media(&self, needs_tools: bool) -> anyhow::Result<()> {
        if needs_tools && !self.media.errors.is_empty() && self.media.processed == 0 {
            bail!("media processing failed for all candidate files");
        }
        Ok(())
    }

    /// Human-readable lines about the write tail: the Media stage, with each
    /// of its failures as the sentence it was written as, and obfuscation.
    /// Empty when neither did anything.
    pub fn media_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.media.processed > 0 || self.media.skipped > 0 || !self.media.errors.is_empty() {
            lines.push(format!(
                "Media: processed {}, skipped {}",
                count_of_files(self.media.processed as u64),
                count_of_files(self.media.skipped as u64)
            ));
            for err in self.media.errors.iter().take(10) {
                lines.push(format!("  {err}"));
            }
            if self.media.errors.len() > 10 {
                lines.push(format!("  …and {} more", self.media.errors.len() - 10));
            }
        }
        if self.obfuscated_docs > 0 {
            lines.push(CONVERSATIONS_OBFUSCATED.line(self.obfuscated_docs));
        }
        lines
    }

    /// Append the summary lines to `out`: where the export went, then each
    /// resume, skip, duplicate, attachment, and extension count that is not
    /// zero, each in its [`Counter`]'s words, then the Import Errors and
    /// the notes, each under its heading ([`error_and_note_lines`]).
    pub fn summary_lines(
        &self,
        format: OutputFormat,
        output: &std::path::Path,
        out: &mut Vec<String>,
    ) {
        out.push(format!(
            "Wrote {} export under {}",
            format.as_str(),
            output.display()
        ));
        let counts = [
            (CONVERSATIONS_RESUMED, self.conversations_skipped),
            (SKIPPED_INVALID_DATE, self.skipped_invalid_date),
            (DUPLICATES_DROPPED, self.duplicates_dropped),
            (ATTACHMENTS_SAVED, self.attachments_saved),
        ];
        for (counter, count) in counts.into_iter().chain(self.extra.iter().copied()) {
            if count > 0 {
                out.push(format!("  {}", counter.line(count)));
            }
        }
        for line in error_and_note_lines(&self.errors, &self.notes) {
            out.push(format!("  {line}"));
        }
    }

    /// Bump a per-exporter extension counter in the `extra` list, adding it
    /// at the end the first time it is counted.
    pub fn bump(&mut self, counter: Counter, by: u64) {
        match self.extra.iter_mut().find(|(c, _)| *c == counter) {
            Some((_, count)) => *count += by,
            None => self.extra.push((counter, by)),
        }
    }

    /// Read a per-exporter extension counter from the `extra` list (0 when unset).
    pub fn extra(&self, counter: Counter) -> u64 {
        self.extra
            .iter()
            .find(|(c, _)| *c == counter)
            .map_or(0, |(_, count)| *count)
    }

    /// Fold the counts from one projected conversation into this report.
    pub fn absorb_tally(&mut self, tally: ProjectionTally) {
        self.messages += tally.messages;
        self.sent += tally.sent;
        self.received += tally.received;
        self.duplicates_dropped += tally.duplicates;
        if tally.notifications > 0 {
            self.bump(NOTIFICATIONS, tally.notifications);
        }
    }
}

/// Turn one pending conversation into a document, or drop it.
///
/// This is the step every exporter runs after parsing: sort the messages the
/// way the exporter's hooks say, drop rows whose timestamp cannot be
/// represented, project the rest with [`pending_to_document`], and fold the
/// counts into `report`. Returns `None` when no message survives.
pub fn project_conversation<H: ProjectionHooks + ?Sized>(
    chat_id: &str,
    convo: &mut PendingConversation,
    hooks: &H,
    report: &mut ExportReport,
) -> Option<ConversationDocument> {
    let unit = hooks.sort_key_unit();
    let (keep, skipped) = prepare_conversation(
        convo,
        |a, b| hooks.message_order(a, b),
        |key| unit.to_secs(key),
    );
    report.skipped_invalid_date += skipped;
    if !keep {
        return None;
    }
    let (doc, tally) = pending_to_document(chat_id, convo, hooks);
    report.absorb_tally(tally);
    Some(doc)
}

/// Create and canonicalize the output directory, canonicalize every input,
/// and bail when the output is the same as, or contains, an input.
///
/// Returns the canonicalized `(inputs, output)` paths.
///
/// # Errors
///
/// Returns an error when the output directory cannot be created, a path
/// cannot be resolved, or the output overlaps an input.
pub fn prepare_outputs(
    inputs: &[std::path::PathBuf],
    output: &std::path::Path,
) -> anyhow::Result<(Vec<std::path::PathBuf>, std::path::PathBuf)> {
    fs::create_dir_all(output).with_context(|| format!("create {}", output.display()))?;
    let output =
        fs::canonicalize(output).with_context(|| format!("resolve {}", output.display()))?;
    let mut resolved = Vec::with_capacity(inputs.len());
    for input in inputs {
        let input =
            fs::canonicalize(input).with_context(|| format!("resolve {}", input.display()))?;
        if output == input || input.starts_with(&output) {
            bail!(
                "output {} must not be the same as, or contain, the input {}",
                output.display(),
                input.display()
            );
        }
        resolved.push(input);
    }
    Ok((resolved, output))
}

/// Standard export metadata: source / tool / version, the owner identity,
/// and when the backup was made.
pub fn export_meta(
    source: &str,
    tool: &str,
    tool_version: &str,
    owner_identity: Option<String>,
    owner_display_name: Option<String>,
    backup_taken_at_unix_ms: Option<i64>,
) -> message_ir::ExportMeta {
    message_ir::ExportMeta {
        source: source.to_string(),
        tool: tool.to_string(),
        tool_version: tool_version.to_string(),
        owner_identity,
        owner_display_name,
        backup_taken_at_unix_ms,
    }
}

/// When a file was last changed, in Unix milliseconds, or `None` when it
/// cannot be read or its time is before 1970. The backup date of a source
/// whose files record none of their own: the file was written when the
/// backup was made, or copied with its time kept.
pub fn file_modified_unix_ms(path: &std::path::Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

/// The latest [`file_modified_unix_ms`] of `paths`, or `None` when none has
/// one: the backup date of a source read from several files, which is as
/// new as the newest of them.
pub fn newest_file_modified_unix_ms<'a>(
    paths: impl IntoIterator<Item = &'a std::path::Path>,
) -> Option<i64> {
    paths.into_iter().filter_map(file_modified_unix_ms).max()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source with no date of its own is as new as its newest file, and
    /// a file that is not there adds nothing.
    #[test]
    fn a_backup_read_from_files_is_as_new_as_the_newest() {
        use crate::testutil::{TEST_BACKUP_TAKEN_AT_UNIX_MS, set_modified_unix_ms};
        let dir = tempfile::tempdir().unwrap();
        let older = dir.path().join("older.csv");
        let newer = dir.path().join("newer.csv");
        fs::write(&older, "a").unwrap();
        fs::write(&newer, "b").unwrap();
        set_modified_unix_ms(&older, TEST_BACKUP_TAKEN_AT_UNIX_MS - 86_400_000);
        set_modified_unix_ms(&newer, TEST_BACKUP_TAKEN_AT_UNIX_MS);

        assert_eq!(
            file_modified_unix_ms(&older),
            Some(TEST_BACKUP_TAKEN_AT_UNIX_MS - 86_400_000)
        );
        let missing = dir.path().join("missing.csv");
        assert_eq!(file_modified_unix_ms(&missing), None);
        assert_eq!(
            newest_file_modified_unix_ms([older.as_path(), newer.as_path(), missing.as_path()]),
            Some(TEST_BACKUP_TAKEN_AT_UNIX_MS)
        );
        assert_eq!(newest_file_modified_unix_ms([missing.as_path()]), None);
    }

    #[test]
    fn the_not_sms_or_mms_line_names_the_format_and_the_count() {
        let mut report = ExportReport::default();
        assert_eq!(report.not_sms_or_mms_line("SMS Backup+"), None);
        report.bump(NOT_SMS_OR_MMS_LEFT_OUT, 3);
        assert_eq!(
            report.not_sms_or_mms_line("SMS Backup+").as_deref(),
            Some(
                "Left out 3 messages that are not SMS or MMS, because SMS Backup+ holds only SMS \
                 and MMS"
            )
        );
    }

    /// A note is printed under its own heading, apart from the Import
    /// Errors, so a run that did what it says never reads as a failure
    /// (#1414). Each is a sentence naming its item, and the Import Run gets
    /// the item and the same words said of "this" item (#1920).
    #[test]
    fn summary_lines_print_notes_apart_from_errors() {
        let rows = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&rows);
        let mut report = ExportReport::with_issues(Some(IssueSink::new(move |issue| {
            sink.lock().unwrap().push(issue);
        })));
        report.note(ItemKind::Picture, "a.jpg", "is named by 2 rows");
        report.error(
            ItemKind::Csv,
            "b.csv",
            "could not be read and was left out: cut off",
        );
        let mut lines = Vec::new();
        report.summary_lines(OutputFormat::Jsonl, Path::new("out"), &mut lines);
        assert_eq!(
            lines,
            [
                "Wrote jsonl export under out",
                "  Import Errors",
                "    The CSV b.csv could not be read and was left out: cut off",
                "  Notes",
                "    The picture a.jpg is named by 2 rows",
            ]
        );
        let rows: Vec<(RunIssueKind, String, String)> = rows
            .lock()
            .unwrap()
            .iter()
            .map(|i| (i.kind, i.item.clone(), i.reason.clone()))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    RunIssueKind::Note,
                    "a.jpg".into(),
                    "This picture is named by 2 rows".into()
                ),
                (
                    RunIssueKind::Error,
                    "b.csv".into(),
                    "This CSV could not be read and was left out: cut off".into()
                ),
            ]
        );
    }

    /// Every count is a line in its counter's words, singular for one, and a
    /// count of zero is no line at all, whether the exporter set it or not
    /// (#1700).
    #[test]
    fn summary_lines_word_each_count_and_leave_out_zero() {
        const SEEN: Counter = Counter::new("seen", "Read 1 SMS", "Read {n} SMS");
        let mut report = ExportReport {
            conversations_skipped: 2,
            skipped_invalid_date: 1,
            duplicates_dropped: 3,
            attachments_saved: 1,
            ..ExportReport::default()
        };
        report.bump(crate::SKIPPED_UNKNOWN_ADDRESS, 1);
        report.bump(SEEN, 0);
        let mut lines = Vec::new();
        report.summary_lines(OutputFormat::Jsonl, Path::new("out"), &mut lines);
        assert_eq!(
            lines,
            [
                "Wrote jsonl export under out",
                "  Resumed past 2 conversations that were already written",
                "  Skipped 1 message with an invalid date",
                "  Dropped 3 repeated copies of messages",
                "  Saved 1 attachment",
                "  Skipped 1 message with no usable address",
            ]
        );
    }

    #[test]
    fn discover_files_walks_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.xml"), b"<x/>").unwrap();
        std::fs::write(root.join("b.txt"), b"x").unwrap();
        std::fs::write(root.join("sub").join("c.xml"), b"<x/>").unwrap();
        std::fs::write(root.join("sub").join("d.eml"), b"").unwrap();
        let files = discover_files(root, &|p| {
            p.extension().and_then(|e| e.to_str()) == Some("xml")
        })
        .unwrap();
        let mut names: Vec<PathBuf> = files
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_path_buf())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![PathBuf::from("a.xml"), PathBuf::from("sub").join("c.xml")]
        );
    }

    fn report_with_media(processed: usize, errors: &[&str]) -> ExportReport {
        ExportReport {
            media: MediaReport {
                processed,
                errors: errors.iter().map(|e| (*e).to_string()).collect(),
                ..MediaReport::default()
            },
            ..ExportReport::default()
        }
    }

    #[test]
    fn check_media_refuses_a_pass_that_failed_on_every_file() {
        let err = report_with_media(0, &["a.heic: ffmpeg not found"])
            .check_media(true)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "media processing failed for all candidate files"
        );
    }

    #[test]
    fn check_media_accepts_a_pass_that_processed_a_file() {
        report_with_media(1, &["a.heic: bad file"])
            .check_media(true)
            .unwrap();
    }

    #[test]
    fn check_media_accepts_a_pass_with_no_errors() {
        report_with_media(0, &[]).check_media(true).unwrap();
    }

    /// The Media stage's failures are sentences of their own, so the log
    /// shows each as it is, with no `media warning:` before it (#1888).
    #[test]
    fn media_lines_show_each_failure_as_its_own_sentence() {
        let failure = "1 file could not be converted; its conversation entry says why";
        assert_eq!(
            report_with_media(1, &[failure]).media_lines(),
            [
                "Media: processed 1 file, skipped 0 files".to_string(),
                format!("  {failure}"),
            ]
        );
    }

    #[test]
    fn check_media_accepts_failures_when_the_mode_needs_no_tools() {
        report_with_media(0, &["a.heic: ffmpeg not found"])
            .check_media(false)
            .unwrap();
    }

    #[test]
    fn absorb_tally_adds_every_count_to_the_report() {
        let mut report = ExportReport {
            messages: 10,
            sent: 4,
            received: 6,
            ..ExportReport::default()
        };
        report.bump(NOTIFICATIONS, 2);

        report.absorb_tally(ProjectionTally {
            messages: 5,
            sent: 2,
            received: 3,
            notifications: 3,
            duplicates: 4,
        });

        assert_eq!(report.messages, 15);
        assert_eq!(report.sent, 6);
        assert_eq!(report.received, 9);
        assert_eq!(report.extra(NOTIFICATIONS), 5);
        assert_eq!(report.duplicates_dropped, 4);
    }

    #[test]
    fn absorb_tally_adds_no_notifications_key_when_there_were_none() {
        let mut report = ExportReport::default();
        report.absorb_tally(ProjectionTally {
            messages: 1,
            sent: 1,
            received: 0,
            notifications: 0,
            duplicates: 0,
        });
        assert!(report.extra.is_empty());
    }

    #[test]
    fn bump_adds_to_a_counter_and_extra_reads_zero_when_unset() {
        let mut report = ExportReport::default();
        const PDU: Counter = Counter::new("pdu", "Read 1 PDU", "Read {n} PDUs");
        assert_eq!(report.extra(PDU), 0);
        report.bump(PDU, 3);
        report.bump(PDU, 4);
        assert_eq!(report.extra(PDU), 7);
    }

    #[test]
    fn discover_files_missing_root_errors() {
        let err = discover_files(Path::new("/no/such/dir"), &|_| true).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    /// Cleaning the output before a write would delete the backup being read.
    #[test]
    fn prepare_outputs_refuses_an_output_that_is_or_contains_an_input() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("backup");
        std::fs::create_dir_all(&input).unwrap();
        let inputs = [input.clone()];

        for output in [input.clone(), tmp.path().to_path_buf()] {
            let err = prepare_outputs(&inputs, &output).unwrap_err().to_string();
            assert!(
                err.contains("must not be the same as, or contain, the input"),
                "{}: {err}",
                output.display()
            );
        }

        let output = tmp.path().join("export");
        let (resolved, out) = prepare_outputs(&inputs, &output).unwrap();
        assert!(output.is_dir(), "the output directory is created");
        assert_eq!(out, std::fs::canonicalize(&output).unwrap());
        assert_eq!(resolved, [std::fs::canonicalize(&input).unwrap()]);
    }
}
