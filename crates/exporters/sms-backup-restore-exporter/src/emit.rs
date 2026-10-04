//! Read SMS Backup & Restore XML into the shared conversation structure, then
//! write the chosen output format via [`ExportWriter`].

use crate::read::{ReadOptions, ReadReport, read_backup};
use crate::write::SbrArchive;
use anyhow::Result;
use message_crate_core::{CancelFlag, ExportReport, ExportTransforms, IssueSink, OutputFormat};
use message_staging::{AttachmentSource, ExportWriter};
use std::path::Path;

/// Map the reader's [`ReadReport`] onto the shared [`ExportReport`] shape,
/// moving reader-specific counters into `extra`, and send each error to
/// `issues` as an Import Error.
fn to_core_report(report: ReadReport, issues: Option<&IssueSink>) -> ExportReport {
    let mut out = ExportReport {
        conversations: report.conversations,
        // Every message in a produced document is either sent or received.
        messages: report.sent + report.received,
        sent: report.sent,
        received: report.received,
        skipped_invalid_date: report.skipped_invalid_date,
        skipped_out_of_range: report.skipped_out_of_range,
        duplicates_dropped: report.duplicates_dropped,
        ..ExportReport::with_issues(issues.cloned())
    };
    for error in report.errors {
        out.error(
            error.file,
            format!("This file could not be read in full: {}", error.reason),
        );
    }
    out.extra.insert("sms_seen".into(), report.sms_seen);
    out.extra.insert("mms_seen".into(), report.mms_seen);
    out.extra.insert(
        "skipped_unknown_address".into(),
        report.skipped_unknown_address,
    );
    out.extra
        .insert("skipped_unknown_type".into(), report.skipped_unknown_type);
    out.extra.insert(
        "skipped_draft_or_outbox".into(),
        report.skipped_draft_or_outbox,
    );
    out.extra.insert(
        "skipped_empty_participants".into(),
        report.skipped_empty_participants,
    );
    out.extra.insert(
        "skipped_unreadable_part".into(),
        report.skipped_unreadable_part,
    );
    out.extra.insert(
        "dropped_character_references".into(),
        report.dropped_character_references,
    );
    out
}

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a> {
    pub input: &'a Path,
    pub output_dir: &'a Path,
    /// The app's cache folder, which the run's attachment spool goes under.
    pub cache_dir: &'a Path,
    pub owner_phones: &'a [String],
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
    /// Where each Import Error goes as the run records it.
    pub issues: Option<&'a IssueSink>,
}

/// Convert SMS Backup & Restore XML into the shared conversation structure,
/// then write the chosen output format.
///
/// # Errors
///
/// Returns an error when the XML cannot be read, a conversation cannot be
/// written, or the user cancels.
pub(crate) fn convert_export(args: ConvertExportArgs<'_>) -> Result<ExportReport> {
    // The read options still need the compress settings after `transforms`
    // moves into the writer.
    let compress = args.transforms.compress.clone();
    let mut writer = ExportWriter::open(
        args.output_dir,
        args.output_format,
        args.transforms,
        args.resume,
    )?
    .with_spool(args.cache_dir);
    if args.output_format == OutputFormat::Xml {
        // This crate owns the backup format, so a round trip back to
        // `smses.xml` goes through its own archive writer.
        writer = writer.with_archive(Box::new(SbrArchive));
    }
    let (documents, report) = read_backup(
        args.input,
        ReadOptions {
            owner_phones: args.owner_phones,
            attachments_dir: Some(writer.attachments_dir()),
            spool: writer.spool(),
            exclude_dir: Some(args.output_dir),
            media: writer.media_mode(),
            compress,
            log: writer.log(),
            progress: writer.progress(),
            cancel: args.cancel,
        },
    )?;

    // The reader already counted conversations; zero the conversation
    // counter so the shared write tail's fold counts only the documents it
    // actually writes.
    let mut core = to_core_report(report, args.issues);
    core.conversations = 0;
    writer.finish(
        documents,
        &mut AttachmentSource::take_bytes,
        args.cancel,
        &mut core,
    )?;
    Ok(core)
}
