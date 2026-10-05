//! Drain a queue of conversations onto disk, one conversation at a time.
//!
//! Parse finishes before anything is written: every exporter buffers its
//! documents and collects attachment sources in one pass, then hands the
//! result here as a queue of [`ConversationUnit`]s. A worker writes a unit's
//! attachments first and its conversation file last, so a conversation file
//! on disk means everything it references is on disk too. That invariant is
//! what makes an interrupted write resumable: a resumed run skips any unit
//! whose conversation file it finds written to the end, and rewrites one
//! whose file is empty or cut off.
//!
//! Writers never transcode. Convert and compress stage the originals here and
//! run afterwards, on their own and resumable, through [`transcode_staged`].
//!
//! Only non-obfuscated JSONL exports are routed here. Obfuscation is stateful
//! across documents and the other formats merge or embed at finish, so those
//! keep the `FormatSink` path.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anyhow::{Context, Result};
use media::{CompressOptions, MediaMode};
use message_crate_core::{
    AttachmentJob, CONVERSATION_FILES_PREPARING, CONVERSATIONS_RESUMED, CancelFlag, Counter,
    LoadError, LogSink, MediaConfig, OutputFormat, ProgressEvent, ProgressSink,
    attachment_size_hint, emit_log, emit_progress, run_attachment_jobs,
};
use message_ir::{ConversationDocument, IrAttachment, give_each_document_its_own_file};

use crate::headroom::{Disk, bytes_to_copy, check_headroom};
use crate::transcode::{TranscodeOptions, transcode_staged};
use message_ir_format::{is_complete_file, write_format};

/// Where a unit's attachment bytes come from at write time.
#[derive(Debug, Default)]
pub enum AttachmentSource {
    /// Read this file when the attachment is written. Worker-safe: a plain
    /// `fs::read`, no shared handle.
    Path(PathBuf),
    /// Bytes the exporter already holds (SBR blobs, handwriting SVG).
    Bytes(Vec<u8>),
    /// Nothing to read; the attachment becomes `file_missing` under a mode
    /// that copies files.
    #[default]
    Missing,
}

impl AttachmentSource {
    /// The attachment's in-memory bytes as its source, taken out of the
    /// record, with the size to report; `Missing` when the exporter held
    /// none. The `source_for` hook of every exporter whose attachments
    /// arrive as bytes rather than files.
    pub fn take_bytes(att: &mut IrAttachment) -> (Self, Option<u64>) {
        let hint = attachment_size_hint(att);
        match att.bytes.take() {
            Some(bytes) => (Self::Bytes(bytes), hint),
            None => (Self::Missing, hint),
        }
    }
}

/// One attachment of a unit, pinned to its place in the document.
#[derive(Debug)]
pub struct UnitAttachment {
    /// Index into `doc.messages`.
    pub message_index: usize,
    /// Index into that message's `attachments`.
    pub attachment_index: usize,
    /// Where the bytes come from.
    pub source: AttachmentSource,
    /// Message timestamp, which dates the staged filename.
    pub timestamp_unix_ms: i64,
    /// Size from the backup when known; byte totals grow as unhinted files load.
    /// Always `None` for a `Missing` source, which is never copied, even when
    /// the backup knew its size. [`ConversationUnit::from_doc`] makes an
    /// empty `Bytes` source `Missing`, and [`drain_write_queue`] does the same
    /// for a `Path` with no file there, before either sums a total.
    pub size_hint: Option<u64>,
}

/// One conversation and everything it references: the queue's unit of work.
#[derive(Debug)]
pub struct ConversationUnit {
    /// The conversation to write.
    pub doc: ConversationDocument,
    /// Its attachments, in message order.
    pub attachments: Vec<UnitAttachment>,
}

impl ConversationUnit {
    /// Pair every attachment in `doc` with a source and a size hint.
    ///
    /// The closure sees each attachment in message order and receives it as
    /// `&mut`, so an exporter carrying bytes on the document can move them
    /// out with `att.bytes.take()` instead of copying them.
    pub fn from_doc(
        mut doc: ConversationDocument,
        mut source_for: impl FnMut(
            usize,
            &mut message_ir::IrAttachment,
        ) -> (AttachmentSource, Option<u64>),
    ) -> Self {
        let mut attachments = Vec::new();
        let mut flat = 0usize;
        for (message_index, msg) in doc.messages.iter_mut().enumerate() {
            let timestamp_unix_ms = msg.timestamp_unix_ms;
            for (attachment_index, att) in msg.attachments.iter_mut().enumerate() {
                let (source, size_hint) = counted_source(source_for(flat, att));
                attachments.push(UnitAttachment {
                    message_index,
                    attachment_index,
                    source,
                    timestamp_unix_ms,
                    size_hint,
                });
                flat += 1;
            }
        }
        Self { doc, attachments }
    }
}

/// How a drain stages files.
#[derive(Debug, Clone)]
pub struct WriteQueueOptions {
    /// The mode the user asked for. Convert and compress stage originals here
    /// and transcode afterwards — writers do not transcode.
    pub media: MediaMode,
    /// Compress settings, used by the convert/compress run after the write.
    pub compress: CompressOptions,
    /// Skip units whose conversation file is already on disk.
    pub resume: bool,
    /// 0 picks a count from the machine. The sequential drain ignores it.
    pub writer_count: usize,
}

/// What a drain did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WriteQueueReport {
    /// Conversation files written by this run.
    pub conversations_written: usize,
    /// Conversation files a resumed run found already written.
    pub conversations_skipped: usize,
    /// Attachment records staged with a path and a digest, duplicates included.
    pub attachments_saved: usize,
    /// Filled by the convert/compress run after the write; default otherwise.
    pub media: media::MediaReport,
}

impl WriteQueueReport {
    /// Add this drain's counts to the run's report. Skipped conversations
    /// count as conversations too: they are part of the export, and
    /// `conversations_skipped` says how many this run did not have to write.
    pub fn fold_into(&self, report: &mut message_crate_core::ExportReport) {
        report.conversations += (self.conversations_written + self.conversations_skipped) as u64;
        report.conversations_skipped += self.conversations_skipped as u64;
        report.attachments_saved += self.attachments_saved as u64;
        report.media = self.media.clone();
    }
}

/// Read one attachment source.
///
/// `Bytes` are moved out of the source rather than copied — every source is
/// loaded at most once, so taking them is safe and spares a full copy of the
/// payload. `Missing` reads as an absent file, which Staging turns
/// into `file_missing`.
///
/// # Errors
///
/// Returns [`LoadError::Unreadable`] with the read error when a `Path`
/// source cannot be read: one unreadable file is that attachment's problem.
pub fn load_attachment_source(source: &mut AttachmentSource) -> Result<Option<Vec<u8>>, LoadError> {
    match source {
        AttachmentSource::Path(path) => fs::read(&*path)
            .map(Some)
            .map_err(|e| LoadError::Unreadable(format!("read {}: {e}", path.display()))),
        AttachmentSource::Bytes(bytes) => Ok(Some(std::mem::take(bytes))),
        AttachmentSource::Missing => Ok(None),
    }
}

/// What one attachment added to the drain's totals.
///
/// Deltas, not running counts: both drains fold them into one
/// [`AttachmentTotals`], so the per-unit body does not need to know the
/// global picture.
struct UnitProgress {
    done: usize,
    bytes_done: u64,
    /// How far this attachment moved the byte total: up or down by the
    /// difference between its file and its size hint.
    bytes_total_change: i64,
}

/// The drain's running attachment counts, which every
/// [`ProgressEvent::Attachments`] reports.
///
/// The counts change together: a parallel drain keeps them behind one lock
/// and sends each event while it holds it, so every event is one moment's
/// counts and the events never go back. Three counters updated one at a
/// time let two writers mix their counts, and the bar could end on 3 of 4
/// attachments (#1536).
struct AttachmentTotals {
    done: usize,
    total: usize,
    bytes_done: u64,
    bytes_total: u64,
}

impl AttachmentTotals {
    /// Nothing done yet, out of every attachment in `units` and the sizes
    /// their sources claim.
    fn new(units: &[ConversationUnit]) -> Self {
        let attachments = || units.iter().flat_map(|u| u.attachments.iter());
        Self {
            done: 0,
            total: attachments().count(),
            bytes_done: 0,
            bytes_total: attachments().filter_map(|a| a.size_hint).sum(),
        }
    }

    /// Fold in what one attachment changed, then report the new counts: a
    /// log line for people and a [`ProgressEvent::Attachments`] for the bar.
    fn add_and_report(
        &mut self,
        p: UnitProgress,
        log: Option<&LogSink>,
        progress: Option<&ProgressSink>,
    ) {
        self.done += p.done;
        self.bytes_done += p.bytes_done;
        self.bytes_total = self.bytes_total.saturating_add_signed(p.bytes_total_change);
        let Self {
            done,
            total,
            bytes_done,
            bytes_total,
        } = *self;
        let due = emit_progress(
            progress,
            ProgressEvent::Attachments {
                done,
                total,
                bytes_done,
                bytes_total,
            },
        );
        if due {
            emit_log(
                log,
                format!("  attachments {done}/{total} {bytes_done}/{bytes_total}"),
            );
        }
    }
}

/// What one unit did. Byte and file counts travel through the progress
/// callback instead, into the drain's [`AttachmentTotals`].
struct UnitOutcome {
    written: bool,
    attachments_saved: usize,
}

/// A byte count as a signed number, for the difference between two of them.
fn signed(bytes: u64) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}

/// Loads one attachment's bytes by source; `Ok(None)` or
/// [`LoadError::Unreadable`] marks it missing, and [`LoadError::Fatal`]
/// stops the drain.
pub type AttachmentLoader<'a> =
    dyn FnMut(&mut AttachmentSource) -> Result<Option<Vec<u8>>, LoadError> + 'a;

/// Drain `units` with a caller-supplied loader.
///
/// Exporters whose attachment loader cannot cross threads — an encrypted iOS
/// backup holds a SQLite connection that is not `Sync` — use this and get one
/// writer. Everyone else wants `drain_write_queue`.
///
/// # Errors
///
/// Returns the first unit error, which stops the drain. A cancel surfaces as
/// `"cancelled"`.
pub fn drain_write_queue_with_loader(
    output_dir: &Path,
    mut units: Vec<ConversationUnit>,
    options: &WriteQueueOptions,
    load: &mut AttachmentLoader<'_>,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
) -> Result<WriteQueueReport> {
    give_each_unit_its_own_file(&mut units)?;
    check_units_headroom(output_dir, &units, options.media)?;
    let attachments_dir = output_dir.join("attachments");
    let mut report = WriteQueueReport::default();

    let unit_count = units.len();
    let totals = RefCell::new(AttachmentTotals::new(&units));

    announce_start(log, progress, unit_count);

    let report_progress = |p: UnitProgress| {
        totals.borrow_mut().add_and_report(p, log, progress);
    };

    for unit in units {
        let outcome = write_one_unit(
            output_dir,
            &attachments_dir,
            unit,
            options,
            load,
            &report_progress,
            cancel,
        )?;
        report.attachments_saved += outcome.attachments_saved;
        if outcome.written {
            report.conversations_written += 1;
        } else {
            report.conversations_skipped += 1;
        }
        emit_progress(
            progress,
            ProgressEvent::Prepare {
                done: report.conversations_written + report.conversations_skipped,
                total: unit_count,
            },
        );
    }

    report.media = run_media_post_pass(output_dir, options, log, progress, cancel)?;
    announce_finish(log, &report, options.resume);
    Ok(report)
}

/// Make every `Path` source with no file there `Missing`, with no size
/// hint, so the byte totals and the disk check leave it out from the start
/// rather than drop when the run reaches it (#1581). Each one is logged as
/// the read would have logged it. With media turned off nothing is read,
/// so the caller skips this. A unit a resumed run will skip, because its
/// conversation file is already written to the end, reads nothing either,
/// so its paths are not checked or logged again.
///
/// Only [`drain_write_queue`] calls this: its loader reads a `Path` from
/// disk. A caller's own loader in [`drain_write_queue_with_loader`] decides
/// what a path means (an encrypted iPhone backup's path names a file inside
/// the backup, not on disk), so those paths are left to it.
fn leave_out_files_that_are_gone(
    output_dir: &Path,
    units: &mut [ConversationUnit],
    resume: bool,
    log: Option<&LogSink>,
) {
    for unit in units {
        if written_by_an_earlier_run(output_dir, &unit.doc, resume) {
            continue;
        }
        for attachment in &mut unit.attachments {
            let source = std::mem::take(&mut attachment.source);
            (attachment.source, attachment.size_hint) =
                missing_if_no_file((source, attachment.size_hint), log);
        }
    }
}

/// One attachment's source and size hint as a run counts them: a source
/// known before the run to have no file, `Missing` or bytes that are empty,
/// becomes `Missing` with no hint. An attachment with no file is never
/// copied, so its hint counts toward no byte total, not the progress and
/// not the disk check, and saying so before any total is summed keeps the
/// total from dropping when the run reaches it. Both the write queue and
/// [`CountedAttachments`](crate::CountedAttachments), which every run
/// outside the queue stages through, count sources this way.
pub(crate) fn counted_source(
    (source, size_hint): (AttachmentSource, Option<u64>),
) -> (AttachmentSource, Option<u64>) {
    match source {
        AttachmentSource::Bytes(bytes) if bytes.is_empty() => (AttachmentSource::Missing, None),
        AttachmentSource::Missing => (AttachmentSource::Missing, None),
        found => (found, size_hint),
    }
}

/// One attachment's source and size hint, with a `Path` that has no file
/// there made `Missing` with no hint and logged as the read would have
/// logged it (#1581). Only for a loader that reads a path from disk, as
/// [`load_attachment_source`] does.
pub(crate) fn missing_if_no_file(
    (source, size_hint): (AttachmentSource, Option<u64>),
    log: Option<&LogSink>,
) -> (AttachmentSource, Option<u64>) {
    match source {
        AttachmentSource::Path(path) if !path.is_file() => {
            emit_log(log, unreadable_attachment_line(&path, "no file there"));
            (AttachmentSource::Missing, None)
        }
        found => (found, size_hint),
    }
}

/// The log line for an attachment file that could not be read, naming the
/// file and why.
fn unreadable_attachment_line(path: &Path, why: impl std::fmt::Display) -> String {
    format!(
        "warning: attachment {} could not be read: {why}",
        path.display()
    )
}

/// The conversation file a unit is written to.
fn conversation_file(output_dir: &Path, doc: &ConversationDocument) -> PathBuf {
    output_dir.join(format!("{}.jsonl", doc.filename_stem()))
}

/// Whether a resumed run skips this conversation: an earlier run wrote its
/// file to the end, attachments and all. An empty or cut-off file left by a
/// power loss is written again.
fn written_by_an_earlier_run(output_dir: &Path, doc: &ConversationDocument, resume: bool) -> bool {
    resume && is_complete_file(&conversation_file(output_dir, doc))
}

/// Give every unit a file name no other unit in the run has; see
/// [`give_each_document_its_own_file`].
fn give_each_unit_its_own_file(units: &mut [ConversationUnit]) -> Result<()> {
    let mut docs: Vec<&mut ConversationDocument> = units.iter_mut().map(|u| &mut u.doc).collect();
    give_each_document_its_own_file(&mut docs).map_err(anyhow::Error::msg)
}

/// Writers scale with the machine: writing is IO and hashing, and past a
/// handful of threads the disk, not the CPU, sets the pace.
pub fn default_writer_count() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, 8)
}

/// Drain `units` through the write queue, fold the written/skipped counts
/// into `report`. The shared tail of every exporter's queue arm.
pub fn drain_units(
    output_dir: &Path,
    units: Vec<ConversationUnit>,
    options: &WriteQueueOptions,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
    report: &mut message_crate_core::ExportReport,
) -> Result<()> {
    drain_write_queue(output_dir, units, options, log, progress, cancel)?.fold_into(report);
    Ok(())
}

/// Drain `units` across a pool of writer threads.
///
/// Each worker pops the next conversation, stages its attachments, and writes
/// its conversation file. The first error stops the pool and is what the
/// caller sees.
///
/// # Errors
///
/// Returns the first unit error, or the headroom error when the staging disk
/// cannot hold what the backup needs. A cancel surfaces as `"cancelled"`.
pub fn drain_write_queue(
    output_dir: &Path,
    mut units: Vec<ConversationUnit>,
    options: &WriteQueueOptions,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
) -> Result<WriteQueueReport> {
    give_each_unit_its_own_file(&mut units)?;
    if options.media != MediaMode::Disabled {
        leave_out_files_that_are_gone(output_dir, &mut units, options.resume, log);
    }
    check_units_headroom(output_dir, &units, options.media)?;

    let attachments_dir = output_dir.join("attachments");
    // Idempotent, but doing it once here keeps every worker's first write
    // from racing the same create.
    fs::create_dir_all(&attachments_dir)
        .with_context(|| format!("create {}", attachments_dir.display()))?;

    let unit_count = units.len();
    let totals = Mutex::new(AttachmentTotals::new(&units));

    announce_start(log, progress, unit_count);

    let attachments_saved = AtomicUsize::new(0);
    let written = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    // Behind a lock for the same reason as `totals`: each prepare event is
    // sent while it is held, so the count never goes back.
    let units_done: Mutex<usize> = Mutex::new(0);
    let abort = AtomicBool::new(false);
    let first_error: Mutex<Option<String>> = Mutex::new(None);
    let queue: Mutex<VecDeque<ConversationUnit>> = Mutex::new(VecDeque::from(units));

    let report_progress = |p: UnitProgress| {
        totals
            .lock()
            .expect("write queue totals")
            .add_and_report(p, log, progress);
    };

    let writer_count = if options.writer_count == 0 {
        default_writer_count()
    } else {
        options.writer_count
    }
    .min(unit_count.max(1));

    std::thread::scope(|scope| {
        for _ in 0..writer_count {
            scope.spawn(|| {
                loop {
                    if abort.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some(unit) = queue.lock().expect("write queue lock").pop_front() else {
                        return;
                    };
                    let mut load = |source: &mut AttachmentSource| {
                        // Name the file before the failure turns into a chip:
                        // otherwise a systemic problem (a revoked permission, a
                        // failing disk) reads as a run's worth of unexplained
                        // missing attachments.
                        let named = match source {
                            AttachmentSource::Path(path) => Some(path.clone()),
                            _ => None,
                        };
                        load_attachment_source(source).inspect_err(|e| {
                            if let Some(path) = named {
                                emit_log(log, unreadable_attachment_line(&path, e));
                            }
                        })
                    };
                    match write_one_unit(
                        output_dir,
                        &attachments_dir,
                        unit,
                        options,
                        &mut load,
                        &report_progress,
                        cancel,
                    ) {
                        Ok(outcome) => {
                            attachments_saved
                                .fetch_add(outcome.attachments_saved, Ordering::Relaxed);
                            if outcome.written {
                                written.fetch_add(1, Ordering::Relaxed);
                            } else {
                                skipped.fetch_add(1, Ordering::Relaxed);
                            }
                            let mut finished = units_done.lock().expect("write queue units done");
                            *finished += 1;
                            emit_progress(
                                progress,
                                ProgressEvent::Prepare {
                                    done: *finished,
                                    total: unit_count,
                                },
                            );
                        }
                        Err(err) => {
                            let mut slot = first_error.lock().expect("write queue error slot");
                            if slot.is_none() {
                                *slot = Some(format!("{err:#}"));
                            }
                            abort.store(true, Ordering::Relaxed);
                            return;
                        }
                    }
                }
            });
        }
    });

    if let Some(msg) = first_error.into_inner().expect("write queue error slot") {
        anyhow::bail!(msg);
    }

    let mut report = WriteQueueReport {
        conversations_written: written.load(Ordering::Relaxed),
        conversations_skipped: skipped.load(Ordering::Relaxed),
        attachments_saved: attachments_saved.load(Ordering::Relaxed),
        media: media::MediaReport::default(),
    };
    report.media = run_media_post_pass(output_dir, options, log, progress, cancel)?;
    announce_finish(log, &report, options.resume);
    Ok(report)
}

/// Convert or compress the staged originals, once every writer is done.
///
/// Writers stage originals and nothing else, so this is where convert and
/// compress actually happen. Running it on its own gives per-file commits,
/// so an interruption keeps every derivative already finished, and progress
/// worth reporting.
fn run_media_post_pass(
    output_dir: &Path,
    options: &WriteQueueOptions,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
) -> Result<media::MediaReport> {
    if !matches!(options.media, MediaMode::Convert | MediaMode::Compress) {
        return Ok(media::MediaReport::default());
    }

    let transcode_options = TranscodeOptions {
        mode: options.media,
        compress: options.compress.clone(),
        // No server limit applies to a local export, so nothing here is
        // written off as too large. The desktop's own Media stage, which does
        // enforce the real limit, never reaches this code: it stages with
        // Clone and converts on its own.
        asset_max_bytes: u64::MAX,
    };
    // The desktop never runs this branch (it stages with Clone and converts
    // on its own after the Staging Review); the events are for any other consumer.
    let report = transcode_staged(output_dir, &transcode_options, cancel, None, &mut |p| {
        let due = emit_progress(
            progress,
            ProgressEvent::Media {
                done: p.done,
                total: p.total,
            },
        );
        if due {
            emit_log(log, format!("  media {}/{}", p.done, p.total));
        }
    })?;

    let mut media = media::MediaReport {
        processed: report.converted,
        skipped: report.skipped + report.repointed,
        bytes_before: report.bytes_before,
        bytes_after: report.bytes_after,
        errors: Vec::new(),
    };
    if report.failed > 0 {
        // The per-file reasons are already on the attachments themselves.
        media.errors.push(NOT_CONVERTED.line(report.failed as u64));
    }
    emit_log(log, media::done_line(options.media, &media));
    Ok(media)
}

/// Refuse a drain the staging disk plainly cannot hold.
///
/// `needed` counts the originals the run will copy. Peak usage is those plus
/// one in-flight derivative, since the convert/compress run commits per file, so the
/// sum plus a fixed slack is the honest requirement.
///
/// With media turned off nothing is copied, so nothing is counted. A
/// `Missing` source is never copied either, and [`ConversationUnit::from_doc`]
/// already drops its hint, so the sum leaves it out.
fn check_units_headroom(
    output_dir: &Path,
    units: &[ConversationUnit],
    media: MediaMode,
) -> Result<()> {
    if media == MediaMode::Disabled {
        return Ok(());
    }
    // Summed before any resume skip: over-asking on a resumed run is the
    // conservative direction, and such a run usually has most of those bytes
    // on disk already.
    let needed = bytes_to_copy(units.iter().flat_map(|unit| {
        unit.attachments.iter().filter_map(|a| {
            let digest = unit.doc.messages[a.message_index].attachments[a.attachment_index]
                .digest_sha256
                .as_deref();
            Some((digest, a.size_hint?))
        })
    }));
    check_headroom(output_dir, needed, Disk::Staging)
}

/// The line that ends the write queue's work.
const PREPARED: Counter = Counter::new(
    "conversation_files_prepared",
    "Prepared 1 conversation file",
    "Prepared {n} conversation files",
);

/// Files Media could not convert, each of whose conversation entries says
/// why.
const NOT_CONVERTED: Counter = Counter::new(
    "media_not_converted",
    "1 file could not be converted; its conversation entry says why",
    "{n} files could not be converted; their conversation entries say why",
);

/// Say that the write queue is starting on `units` conversations: a log
/// line for people and a zero-of-`units` prepare event for the bar.
fn announce_start(log: Option<&LogSink>, progress: Option<&ProgressSink>, units: usize) {
    emit_log(log, "");
    emit_log(log, CONVERSATION_FILES_PREPARING.line(units as u64));
    emit_progress(
        progress,
        ProgressEvent::Prepare {
            done: 0,
            total: units,
        },
    );
}

/// Log the write queue's totals, noting resumed work.
fn announce_finish(log: Option<&LogSink>, report: &WriteQueueReport, resume: bool) {
    emit_log(log, PREPARED.line(report.conversations_written as u64));
    if resume && report.conversations_skipped > 0 {
        emit_log(
            log,
            CONVERSATIONS_RESUMED.line(report.conversations_skipped as u64),
        );
    }
}

/// Stage one conversation's attachments, then write the conversation file.
///
/// The order is the engine's whole contract: the conversation file lands last,
/// so its presence on disk vouches for everything it points at.
fn write_one_unit(
    output_dir: &Path,
    attachments_dir: &Path,
    unit: ConversationUnit,
    options: &WriteQueueOptions,
    load: &mut AttachmentLoader<'_>,
    on_progress: &dyn Fn(UnitProgress),
    cancel: Option<&CancelFlag>,
) -> Result<UnitOutcome> {
    if cancel.is_some_and(|f| f.load(Ordering::Relaxed)) {
        anyhow::bail!("cancelled");
    }

    let ConversationUnit {
        mut doc,
        attachments,
    } = unit;
    let attachment_count = attachments.len();
    let hint_sum: u64 = attachments.iter().filter_map(|a| a.size_hint).sum();

    let path = conversation_file(output_dir, &doc);
    if written_by_an_earlier_run(output_dir, &doc, options.resume) {
        // Count its attachments and their bytes as done — progress describes
        // the whole import, not just this run's share of it — and load
        // nothing.
        on_progress(UnitProgress {
            done: attachment_count,
            bytes_done: hint_sum,
            bytes_total_change: 0,
        });
        return Ok(UnitOutcome {
            written: false,
            attachments_saved: 0,
        });
    }

    // Writers copy originals; convert and compress run later on their own.
    let stage_mode = match options.media {
        MediaMode::Disabled => MediaMode::Disabled,
        _ => MediaMode::Clone,
    };

    let timestamps: Vec<i64> = doc.messages.iter().map(|m| m.timestamp_unix_ms).collect();
    let mut slots: HashMap<(usize, usize), UnitAttachment> = attachments
        .into_iter()
        .map(|a| ((a.message_index, a.attachment_index), a))
        .collect();

    let mut sources: Vec<AttachmentSource> = Vec::with_capacity(attachment_count);
    let mut jobs: Vec<AttachmentJob<'_>> = Vec::new();
    for (message_index, msg) in doc.messages.iter_mut().enumerate() {
        let fallback_ts = timestamps.get(message_index).copied().unwrap_or(0);
        for (attachment_index, att) in msg.attachments.iter_mut().enumerate() {
            let slot = slots.remove(&(message_index, attachment_index));
            let (source, size_hint, timestamp_unix_ms) = match slot {
                Some(a) => (a.source, a.size_hint, a.timestamp_unix_ms),
                None => (AttachmentSource::Missing, None, fallback_ts),
            };
            sources.push(source);
            jobs.push(AttachmentJob {
                attachment: att,
                timestamp_unix_ms,
                size_hint,
            });
        }
    }

    let mut unit_bytes_done = 0_u64;
    let mut unit_total_change = 0_i64;
    let mut reported_done = 0_usize;
    {
        let sources = &mut sources;
        run_attachment_jobs(
            &mut jobs,
            attachments_dir,
            &MediaConfig {
                mode: stage_mode,
                compress: options.compress.clone(),
            },
            |i| match sources.get_mut(i) {
                Some(source) => load(source),
                None => Ok(None),
            },
            |p| {
                // run_attachment_jobs reports this unit's running totals; the
                // drain wants what each attachment changed.
                let total_change = signed(p.bytes_total) - signed(hint_sum);
                on_progress(UnitProgress {
                    done: p.done.saturating_sub(reported_done),
                    bytes_done: p.bytes_done.saturating_sub(unit_bytes_done),
                    bytes_total_change: total_change - unit_total_change,
                });
                reported_done = p.done;
                unit_bytes_done = p.bytes_done;
                unit_total_change = total_change;
            },
            None,
            cancel.map(|flag| flag.as_ref()),
        )
        .map_err(anyhow::Error::msg)?;
    }

    let attachments_saved = jobs
        .iter()
        .filter(|j| j.attachment.path.is_some() && j.attachment.digest_sha256.is_some())
        .count();
    drop(jobs);

    message_ir_format::clear_attachments_when_disabled(&mut doc, options.media);
    for msg in &mut doc.messages {
        for att in &mut msg.attachments {
            att.bytes = None;
        }
    }

    write_format(output_dir, OutputFormat::Jsonl, doc)
        .with_context(|| format!("write {}", path.display()))?;

    Ok(UnitOutcome {
        written: true,
        attachments_saved,
    })
}

#[cfg(test)]
mod tests;
