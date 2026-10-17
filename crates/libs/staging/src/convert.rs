//! The convert step every exporter but Apple Messages runs: open the
//! writer, read the backup, project each conversation it read, and finish
//! the writer. Each exporter supplies only its read step and its
//! [`Project`] step.
//!
//! It lives here rather than in `message-crate-core` because it opens an
//! [`ExportWriter`], and this crate depends on that one.

use crate::export_writer::ExportWriter;
use crate::write_queue::AttachmentSource;
use anyhow::Result;
use ir::{Attachment, ConversationDocument};
use ir_format::MergedArchive;
use message_crate_core::{CancelFlag, ConvertRun, ExportReport, check_cancel};
use std::path::Path;

/// How [`convert_backup`] opens the run's [`ExportWriter`], beyond what
/// [`ConvertRun`] says.
#[derive(Default)]
pub struct WriterSetup {
    /// Give the writer an attachment spool under the run's Scratch Directory
    /// ([`ConvertRun::scratch_dir`], [`ExportWriter::with_spool`]), for an
    /// exporter whose attachments arrive as bytes.
    pub spool: bool,
    /// Write the documents through this archive instead of one file per
    /// conversation ([`ExportWriter::with_archive`]).
    pub archive: Option<Box<dyn MergedArchive>>,
}

/// What an exporter's read step hands back to [`convert_backup`]: the
/// conversations it read, the step that turns each into its document, and
/// the report so far.
pub struct ReadBackup<P: Project> {
    /// Every conversation read, in the order their documents are written.
    pub conversations: Vec<P::Conversation>,
    /// Turns each of `conversations` into its document.
    pub project: P,
    /// The report the read step was given, with what it counted.
    pub report: ExportReport,
}

/// An exporter's step from one conversation it read to its document, and
/// from each attachment of the documents to where its bytes come from.
pub trait Project {
    /// One conversation as the read step left it.
    type Conversation;

    /// The document for `conversation`, or `None` when it has nothing to
    /// write. Called once per conversation, in order.
    fn project(
        &mut self,
        conversation: Self::Conversation,
        report: &mut ExportReport,
    ) -> Option<ConversationDocument>;

    /// Where one attachment's bytes come from, and its size for the
    /// progress totals: the `source_for` hook of [`ExportWriter::finish`],
    /// called once per attachment not in the spool, in document order. The
    /// default takes the bytes held on the attachment
    /// ([`AttachmentSource::take_bytes`]).
    fn attachment_source(
        &mut self,
        attachment: &mut Attachment,
    ) -> (AttachmentSource, Option<u64>) {
        AttachmentSource::take_bytes(attachment)
    }

    /// Called with every document once all are projected, before any is
    /// written. Does nothing by default.
    fn before_write(&mut self, _documents: &[ConversationDocument], _writer: &ExportWriter) {}

    /// Called once the writer has written every document, with the
    /// finished report. Does nothing by default.
    ///
    /// # Errors
    ///
    /// Returns what the exporter's own last step fails with.
    fn after_write(self, _report: &ExportReport) -> Result<()>
    where
        Self: Sized,
    {
        Ok(())
    }
}

/// Convert one backup into `output_dir`: open the writer (which cleans the
/// directory, or keeps it when the run resumes), run `read` with the writer,
/// the cancel flag and an empty report, project each conversation it read,
/// stamp each document with the run's Export Date
/// ([`ConvertRun::exported_at_unix_ms`]), and write every document.
///
/// Cancellation is checked after `read` and before each conversation is
/// projected; `read` checks it as often as its backup needs.
///
/// # Errors
///
/// Returns an error when the output directory cannot be prepared, `read`
/// or the exporter's [`Project::after_write`] fails, a write fails, or the
/// user cancels.
pub fn convert_backup<P: Project>(
    output_dir: &Path,
    run: ConvertRun,
    setup: WriterSetup,
    read: impl FnOnce(&ExportWriter, &CancelFlag, ExportReport) -> Result<ReadBackup<P>>,
) -> Result<ExportReport> {
    let ConvertRun {
        transforms,
        output_format,
        cancel,
        resume,
        issues,
        scratch_dir,
        exported_at_unix_ms,
    } = run;
    let mut writer = ExportWriter::open(output_dir, output_format, transforms, resume)?;
    if setup.spool {
        writer = writer.with_spool(&scratch_dir);
    }
    if let Some(archive) = setup.archive {
        writer = writer.with_archive(archive);
    }
    let ReadBackup {
        conversations,
        mut project,
        mut report,
    } = read(&writer, &cancel, ExportReport::with_issues(issues))?;
    check_cancel(&cancel)?;

    let mut documents = Vec::new();
    for conversation in conversations {
        check_cancel(&cancel)?;
        if let Some(mut document) = project.project(conversation, &mut report) {
            document.export.exported_at_unix_ms = exported_at_unix_ms;
            documents.push(document);
        }
    }

    project.before_write(&documents, &writer);
    writer.finish(
        documents,
        &mut |attachment| project.attachment_source(attachment),
        &cancel,
        &mut report,
    )?;
    project.after_write(&report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_crate_core::OutputFormat;

    /// Projects each document it is handed as it is.
    struct AsRead;

    impl Project for AsRead {
        type Conversation = ConversationDocument;

        fn project(
            &mut self,
            conversation: ConversationDocument,
            _report: &mut ExportReport,
        ) -> Option<ConversationDocument> {
            Some(conversation)
        }
    }

    /// Every file a convert writes carries the Export Date its caller gave,
    /// whatever the exporter's projection put in the header, and each
    /// message keeps the Backup Date the exporter gave it.
    #[test]
    fn every_file_carries_the_export_date_the_caller_gave() {
        let output = tempfile::tempdir().unwrap();
        let mut document = ir::testutil::sample_document("hello");
        document.messages[0].backup_taken_at_unix_ms = Some(1_790_000_000_000);
        convert_backup(
            output.path(),
            ConvertRun {
                output_format: OutputFormat::Jsonl,
                exported_at_unix_ms: Some(1_791_000_000_000),
                ..ConvertRun::default()
            },
            WriterSetup::default(),
            |_writer, _cancel, report| {
                Ok(ReadBackup {
                    conversations: vec![document],
                    project: AsRead,
                    report,
                })
            },
        )
        .unwrap();
        let written = std::fs::read_dir(output.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|e| e == "jsonl"))
            .unwrap();
        let read = ir_format::read_conversation_jsonl(&written).unwrap();
        assert_eq!(read.export.exported_at_unix_ms, Some(1_791_000_000_000));
        assert_eq!(
            read.messages[0].backup_taken_at_unix_ms,
            Some(1_790_000_000_000)
        );
    }
}
