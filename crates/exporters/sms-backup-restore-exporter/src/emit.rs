//! Read SMS Backup & Restore XML into the shared conversation structure, then
//! write the chosen output format through [`convert_backup`].

use crate::read::{ReadOptions, ReadReport, read_backup};
use crate::write::SbrArchive;
use anyhow::Result;
use message_crate_core::{
    ConvertRun, DUPLICATES_DROPPED, ExportReport, IssueSink, ItemKind, OutputFormat,
    SKIPPED_INVALID_DATE, unreadable_parts_note,
};
use message_ir::ConversationDocument;
use message_ir_format::MergedArchive;
use message_staging::{Project, Read, WriterSetup, convert_backup};
use std::path::Path;

/// Map the reader's [`ReadReport`] onto the shared [`ExportReport`] shape,
/// moving reader-specific counters into `extra` with the words and in the
/// order Convert's log gives them, and send each error to `issues` as an
/// Import Error and each message kept with something left out of it as a
/// note naming the file and the message.
fn to_core_report(report: ReadReport, issues: &IssueSink) -> ExportReport {
    let mut out = ExportReport {
        conversations: report.conversations,
        // Every message in a produced document is either sent or received.
        messages: report.sent + report.received,
        sent: report.sent,
        received: report.received,
        skipped_invalid_date: report.skipped_invalid_date,
        duplicates_dropped: report.duplicates_dropped,
        ..ExportReport::with_issues(issues.clone())
    };
    for error in &report.errors {
        out.error(ItemKind::File, &error.file, &error.what_happened());
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
    pub owner_phones: &'a [String],
    pub convert_run: ConvertRun,
}

/// Convert SMS Backup & Restore XML into the shared conversation structure,
/// then write the chosen output format.
///
/// # Errors
///
/// Returns an error when the XML cannot be read, a conversation cannot be
/// written, or the user cancels.
pub(crate) fn convert_export(args: ConvertExportArgs<'_>) -> Result<ExportReport> {
    let setup = WriterSetup {
        spool: true,
        // This crate owns the backup format, so a round trip back to
        // `smses.xml` goes through its own archive writer.
        archive: (args.convert_run.output_format == OutputFormat::Xml)
            .then(|| Box::new(SbrArchive) as Box<dyn MergedArchive>),
    };
    convert_backup(
        args.output_dir,
        args.convert_run,
        setup,
        |writer, cancel, report| {
            let (documents, read_report) = read_backup(
                args.input,
                ReadOptions {
                    owner_phones: args.owner_phones,
                    attachments_dir: Some(writer.attachments_dir()),
                    spool: writer.spool(),
                    files: writer.files(),
                    sinks: writer.sinks(cancel),
                },
            )?;
            // The reader already counted conversations; zero the
            // conversation counter so the shared write tail's fold counts
            // only the documents it actually writes.
            let mut report = to_core_report(read_report, &report.issues);
            report.conversations = 0;
            Ok(Read {
                conversations: documents,
                project: ReadDocuments,
                report,
            })
        },
    )
}

/// The reader's documents, which need no projection, for [`convert_backup`].
struct ReadDocuments;

impl Project for ReadDocuments {
    type Conversation = ConversationDocument;

    fn project(
        &mut self,
        document: ConversationDocument,
        _report: &mut ExportReport,
    ) -> Option<ConversationDocument> {
        Some(document)
    }
}
