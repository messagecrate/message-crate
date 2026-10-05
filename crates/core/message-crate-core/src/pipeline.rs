//! Helpers shared by the exporter crates and the desktop app's in-process runners.
//!
//! This module keeps its dependency surface small (only `anyhow` for
//! context-rich path errors) so the desktop app stays lightweight. Callers map
//! `String` errors at the edge when needed.

use crate::config::OutputFormat;
use anyhow::{Context, bail};
use media::MediaReport;
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
    /// What the row is: `skip` when the item was left out, `error` when it
    /// failed, [`NOTE`] when the run did something with it worth knowing that
    /// is not a failure. `resolved` says an earlier row with the same step
    /// and item no longer holds, as when Media converts a file on a later try.
    pub kind: String,
    /// The step that raised it, such as `attachments`.
    pub step: String,
    /// What was affected, such as an attachment's path in the backup.
    pub item: String,
    /// Why, in one sentence; for a note, what the run did.
    pub reason: String,
}

/// The [`RunIssue::kind`] of a note: something the run did with an item that
/// is worth knowing but did not fail, such as a message it kept with a
/// caveat. The Import Run lists notes apart from its Import Errors.
pub const NOTE: &str = "note";

/// The report counter for chats that name their person with no address,
/// each kept under the name alone and sent as a [`NAME_ONLY_CHAT_NOTE`].
pub const NAME_ONLY_CHAT: &str = "name_only_chat";

/// The note an exporter sends for a chat that names its person with no
/// address, which it keeps under the name alone.
pub const NAME_ONLY_CHAT_NOTE: &str = "This chat names its person with no phone number or email \
     address, so the conversation is kept under the name alone.";

/// The reason an exporter gives for a CSV it cannot read, before the
/// parser's own words.
pub const CSV_NOT_READ: &str = "This CSV could not be read and was left out";

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
/// counts, etc.) are stored in the `extra` map.
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
    /// Rows skipped because they fell outside the date range.
    pub skipped_out_of_range: u64,
    /// Duplicate rows dropped during dedupe.
    pub duplicates_dropped: u64,
    /// Attachment files saved to the output.
    pub attachments_saved: u64,
    /// The convert or compress pass over the staged attachments.
    pub media: MediaReport,
    /// Documents whose handles, names and bodies were obfuscated.
    pub obfuscated_docs: u64,
    /// Human-readable lines about what failed (capped by each exporter).
    pub errors: Vec<String>,
    /// Human-readable lines about what the run did that is worth knowing
    /// but did not fail, such as a choice it made between two rows. Shown
    /// apart from `errors`, so a note never reads as a failure.
    pub notes: Vec<String>,
    /// Per-exporter extension counters keyed by name.
    pub extra: std::collections::BTreeMap<String, u64>,
    /// Where [`ExportReport::error`], [`ExportReport::note`] and
    /// [`ExportReport::caveat`] send each row the moment the run records it.
    /// `None` sends the rows nowhere, and the lines in `errors` and `notes`
    /// are all that is kept.
    pub issues: Option<IssueSink>,
}

/// The report counter for messages an export left out because its format
/// holds only SMS and MMS: an iMessage or a WhatsApp message written as an
/// SMS would come back from a re-import as an SMS under a new id (ADR 0021).
pub const NOT_SMS_OR_MMS_LEFT_OUT: &str = "messages_not_sms_or_mms_left_out";

/// The report counter for attachments an export could not write because
/// their file was gone. Convert's log says how many.
pub const ATTACHMENTS_MISSING: &str = "attachments_missing";

impl ExportReport {
    /// An empty report that sends its rows to `issues`.
    pub fn with_issues(issues: Option<IssueSink>) -> Self {
        Self {
            issues,
            ..Self::default()
        }
    }

    /// Record that the run could not read `item` and why: a line in
    /// `errors`, and an Import Error sent to `issues` at once.
    pub fn error(&mut self, item: impl Into<String>, reason: impl Into<String>) {
        let (item, reason) = (item.into(), reason.into());
        self.errors.push(format!("{item}: {reason}"));
        self.send("error", item, reason);
    }

    /// Record something the run did with `item` that is worth knowing but
    /// did not fail: a line in `notes`, and a note sent to `issues` at once.
    pub fn note(&mut self, item: impl Into<String>, text: impl Into<String>) {
        let (item, text) = (item.into(), text.into());
        self.notes.push(format!("{item}: {text}"));
        self.send(NOTE, item, text);
    }

    /// Count `by` under `counter` for one item the run kept with a caveat,
    /// and send a note that names the item to `issues`. The log keeps the
    /// count, which says as much as a line per item would.
    pub fn caveat(
        &mut self,
        counter: &str,
        by: u64,
        item: impl Into<String>,
        text: impl Into<String>,
    ) {
        self.bump(counter, by);
        self.send(NOTE, item.into(), text.into());
    }

    /// Send one row about an item of the backup to `issues`.
    fn send(&self, kind: &str, item: String, reason: String) {
        emit_issue(
            self.issues.as_ref(),
            RunIssue {
                kind: kind.into(),
                step: READ_STEP.into(),
                item,
                reason,
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
                "Left out {left_out} message(s) that are not SMS or MMS, because {format} holds \
                 only SMS and MMS"
            )
        })
    }

    /// Refuse a run whose media pass failed on every file it tried, when
    /// the mode needed ffmpeg: that is a missing tool, not a bad file.
    ///
    /// # Errors
    ///
    /// Returns an error when `needs_tools` is set, the media pass reported
    /// errors, and it processed nothing.
    pub fn check_media(&self, needs_tools: bool) -> anyhow::Result<()> {
        if needs_tools && !self.media.errors.is_empty() && self.media.processed == 0 {
            bail!("media processing failed for all candidate files");
        }
        Ok(())
    }

    /// Human-readable lines about the write tail: the media pass and
    /// obfuscation. Empty when neither did anything.
    pub fn media_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.media.processed > 0 || self.media.skipped > 0 || !self.media.errors.is_empty() {
            lines.push(format!(
                "Media: processed {} file(s), skipped {}",
                self.media.processed, self.media.skipped
            ));
            for err in self.media.errors.iter().take(10) {
                lines.push(format!("  media warning: {err}"));
            }
            if self.media.errors.len() > 10 {
                lines.push(format!("  …and {} more", self.media.errors.len() - 10));
            }
        }
        if self.obfuscated_docs > 0 {
            lines.push(format!(
                "Obfuscated {} conversation(s)",
                self.obfuscated_docs
            ));
        }
        lines
    }

    /// Append the summary lines to `out`: where the export went, then each
    /// resume, skip, duplicate, attachment, and extension count that is not
    /// zero, then the notes, then the errors.
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
        if self.conversations_skipped > 0 {
            out.push(format!(
                "  resumed: {} conversation(s) were already written",
                self.conversations_skipped
            ));
        }
        if self.skipped_invalid_date > 0 {
            out.push(format!(
                "  skipped {} invalid-date rows",
                self.skipped_invalid_date
            ));
        }
        if self.skipped_out_of_range > 0 {
            out.push(format!(
                "  skipped {} out-of-range rows",
                self.skipped_out_of_range
            ));
        }
        if self.duplicates_dropped > 0 {
            out.push(format!(
                "  dropped {} duplicate rows",
                self.duplicates_dropped
            ));
        }
        if self.attachments_saved > 0 {
            out.push(format!("  saved {} attachments", self.attachments_saved));
        }
        for (key, count) in &self.extra {
            out.push(format!("  {key}: {count}"));
        }
        for note in &self.notes {
            out.push(format!("  note: {note}"));
        }
        for err in &self.errors {
            out.push(format!("  error: {err}"));
        }
    }

    /// Bump a per-exporter extension counter in the `extra` map.
    pub fn bump(&mut self, key: &str, by: u64) {
        *self.extra.entry(key.to_string()).or_insert(0) += by;
    }

    /// Read a per-exporter extension counter from the `extra` map (0 when unset).
    pub fn extra(&self, key: &str) -> u64 {
        self.extra.get(key).copied().unwrap_or(0)
    }

    /// Fold the counts from one projected conversation into this report.
    pub fn absorb_tally(&mut self, tally: ProjectionTally) {
        self.messages += tally.messages;
        self.sent += tally.sent;
        self.received += tally.received;
        self.duplicates_dropped += tally.duplicates;
        if tally.notifications > 0 {
            self.bump("notifications", tally.notifications);
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

/// Drop messages with unrepresentable timestamps and finalize a pending
/// conversation. Returns whether any message remains.
///
/// `to_secs` converts a message sort key to Unix seconds (exporters that
/// store milliseconds pass `|k| k / 1000`).
pub fn prune_and_finish_conversation(
    convo: &mut PendingConversation,
    report: &mut ExportReport,
    to_secs: impl Fn(i64) -> i64,
) -> bool {
    convo.messages.retain(|m| {
        if message_csv::format_local_ts(to_secs(m.sort_key)).is_some() {
            true
        } else {
            report.skipped_invalid_date += 1;
            false
        }
    });
    convo.has_attachments = convo.messages.iter().any(|m| !m.attachments.is_empty());
    !convo.messages.is_empty()
}

/// Standard export metadata: source / tool / version plus the owner identity.
pub fn export_meta(
    source: &str,
    tool: &str,
    tool_version: &str,
    owner_identity: Option<String>,
    owner_display_name: Option<String>,
) -> message_ir::ExportMeta {
    message_ir::ExportMeta {
        source: source.to_string(),
        tool: tool.to_string(),
        tool_version: tool_version.to_string(),
        owner_identity,
        owner_display_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_not_sms_or_mms_line_names_the_format_and_the_count() {
        let mut report = ExportReport::default();
        assert_eq!(report.not_sms_or_mms_line("SMS Backup+"), None);
        report.bump(NOT_SMS_OR_MMS_LEFT_OUT, 3);
        assert_eq!(
            report.not_sms_or_mms_line("SMS Backup+").as_deref(),
            Some(
                "Left out 3 message(s) that are not SMS or MMS, because SMS Backup+ holds only \
                 SMS and MMS"
            )
        );
    }

    /// A note is printed as a note, apart from the errors, so a run that did
    /// what it says never reads as a failure (#1414).
    #[test]
    fn summary_lines_print_notes_apart_from_errors() {
        let report = ExportReport {
            notes: vec!["a.jpg: 2 rows name this picture".into()],
            errors: vec!["b.csv: unreadable".into()],
            ..ExportReport::default()
        };
        let mut lines = Vec::new();
        report.summary_lines(OutputFormat::Jsonl, Path::new("out"), &mut lines);
        assert_eq!(
            lines,
            [
                "Wrote jsonl export under out",
                "  note: a.jpg: 2 rows name this picture",
                "  error: b.csv: unreadable",
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

    #[test]
    fn check_media_accepts_failures_when_the_mode_needs_no_tools() {
        report_with_media(0, &["a.heic: ffmpeg not found"])
            .check_media(false)
            .unwrap();
    }

    fn pending_message(sort_key: i64, attachment: bool) -> message_ir::PendingMessage {
        message_ir::PendingMessage {
            sort_key,
            is_from_me: false,
            sender_identity: "+15555550100".to_string(),
            sender_display_name: None,
            text: "hi".to_string(),
            attachments: if attachment {
                vec![message_ir::PendingAttachment {
                    rel_path: "attachments/a.jpg".to_string(),
                    content_type: "image/jpeg".to_string(),
                    digest_sha256: None,
                    name_hint: None,
                    size_bytes: None,
                }]
            } else {
                Vec::new()
            },
            extra: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn prune_drops_and_counts_invalid_dates_and_keeps_the_rest() {
        let mut convo = PendingConversation::new("chat", false, None, Vec::new());
        convo.messages = vec![
            pending_message(i64::MAX, true),
            pending_message(1_700_000_000, false),
            pending_message(i64::MAX, false),
        ];
        convo.has_attachments = true;
        let mut report = ExportReport {
            skipped_invalid_date: 5,
            ..ExportReport::default()
        };

        assert!(prune_and_finish_conversation(
            &mut convo,
            &mut report,
            |k| k
        ));
        assert_eq!(report.skipped_invalid_date, 7);
        assert_eq!(convo.messages.len(), 1);
        assert_eq!(convo.messages[0].sort_key, 1_700_000_000);
        // The only attachment went with a dropped message.
        assert!(!convo.has_attachments);
    }

    #[test]
    fn prune_reports_a_conversation_with_no_valid_message_as_empty() {
        let mut convo = PendingConversation::new("chat", false, None, Vec::new());
        convo.messages = vec![pending_message(i64::MAX, false)];
        let mut report = ExportReport::default();

        assert!(!prune_and_finish_conversation(
            &mut convo,
            &mut report,
            |k| k
        ));
        assert_eq!(report.skipped_invalid_date, 1);
        assert!(convo.messages.is_empty());
    }

    #[test]
    fn prune_passes_sort_keys_through_to_secs() {
        // Milliseconds that are valid only once divided by 1000.
        let mut convo = PendingConversation::new("chat", false, None, Vec::new());
        convo.messages = vec![pending_message(i64::MAX / 10, true)];
        let mut report = ExportReport::default();

        assert!(prune_and_finish_conversation(
            &mut convo,
            &mut report,
            |_| 1_700_000_000
        ));
        assert_eq!(report.skipped_invalid_date, 0);
        assert!(convo.has_attachments);
    }

    #[test]
    fn absorb_tally_adds_every_count_to_the_report() {
        let mut report = ExportReport {
            messages: 10,
            sent: 4,
            received: 6,
            ..ExportReport::default()
        };
        report.bump("notifications", 2);

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
        assert_eq!(report.extra("notifications"), 5);
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
        assert_eq!(report.extra("pdu"), 0);
        report.bump("pdu", 3);
        report.bump("pdu", 4);
        assert_eq!(report.extra("pdu"), 7);
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
