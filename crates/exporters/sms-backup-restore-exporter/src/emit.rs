//! Read SMS Backup & Restore XML into the shared conversation structure, then
//! write the chosen output format via [`ExportWriter`].

use crate::read::{ReadOptions, ReadReport, read_backup};
use crate::write::SbrArchive;
use anyhow::Result;
use message_crate_core::{
    CancelFlag, DUPLICATES_DROPPED, ExportReport, ExportTransforms, IssueSink, OutputFormat,
    SKIPPED_INVALID_DATE, unreadable_parts_note,
};
use message_staging::{AttachmentSource, ExportWriter};
use std::path::Path;

/// Map the reader's [`ReadReport`] onto the shared [`ExportReport`] shape,
/// moving reader-specific counters into `extra` with the words and in the
/// order Convert's log gives them, and send each error to `issues` as an
/// Import Error and each message kept with something left out of it as a
/// note naming the file and the message.
fn to_core_report(report: ReadReport, issues: Option<&IssueSink>) -> ExportReport {
    let mut out = ExportReport {
        conversations: report.conversations,
        // Every message in a produced document is either sent or received.
        messages: report.sent + report.received,
        sent: report.sent,
        received: report.received,
        skipped_invalid_date: report.skipped_invalid_date,
        duplicates_dropped: report.duplicates_dropped,
        ..ExportReport::with_issues(issues.cloned())
    };
    for error in &report.errors {
        out.error(&error.file, error.explanation());
    }
    // The invalid dates and the repeated copies have fields of their own;
    // every other count goes to `extra` in the order Convert's log gives it.
    for (counter, count) in report.counts() {
        if count > 0 && counter != SKIPPED_INVALID_DATE && counter != DUPLICATES_DROPPED {
            out.bump(counter, count);
        }
    }
    // Counted in the totals above; each message is a note of its own.
    for left_out in report.left_out {
        let item = format!("{} ({})", left_out.file, left_out.message);
        if left_out.counts.unreadable_parts > 0 {
            out.caveat_note(
                item.as_str(),
                unreadable_parts_note(left_out.counts.unreadable_parts),
            );
        }
        if left_out.counts.dropped_character_references > 0 {
            out.caveat_note(
                item,
                dropped_references_note(left_out.counts.dropped_character_references),
            );
        }
    }
    out
}

/// The note for a message kept with `n` character references left out
/// because they are not a character.
fn dropped_references_note(n: u64) -> String {
    match n {
        1 => "1 character reference in this message is not a character and was left out. The \
              message itself is kept."
            .into(),
        n => format!(
            "{n} character references in this message are not characters and were left out. \
             The message itself is kept."
        ),
    }
}

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a> {
    pub input: &'a Path,
    pub output_dir: &'a Path,
    /// the Scratch Directory, which the run's attachment spool goes under.
    pub scratch_dir: &'a Path,
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
    .with_spool(args.scratch_dir);
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
