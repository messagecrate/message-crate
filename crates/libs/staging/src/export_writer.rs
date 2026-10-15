//! The shared write tail every exporter used to copy: sink opening, the
//! queue-or-sink decision, and both drain arms.

use crate::counted_attachments::{CountedAttachments, PathSources};
use crate::headroom::{Disk, check_headroom};
use crate::spool::AttachmentSpool;
use crate::write_queue::{
    AttachmentSource, ConversationUnit, WriteQueueOptions, drain_units, load_attachment_source,
};
use anyhow::Result;
use message_crate_core::{
    AttachmentFiles, CancelFlag, ExportReport, ExportTransforms, LogSink, OutputFormat,
    ProgressSink, Sinks,
};
use message_ir::{ConversationDocument, IrAttachment};
use message_ir_format::{FormatSink, MergedArchive, write_documents_through_sink};
use std::path::{Path, PathBuf};

/// Owns the write tail of an exporter run: output preparation, the
/// queue-or-sink decision, attachment staging, and the format write.
///
/// [`open`](Self::open) runs before parse (it cleans or resumes the output
/// directory, so the exporter can query [`files`](Self::files),
/// [`use_queue`](Self::use_queue), and [`attachments_dir`](Self::attachments_dir)
/// while collecting messages); [`finish`](Self::finish) takes the collected
/// documents and writes everything. An exporter whose drain cannot go
/// through [`finish`](Self::finish) (iMessage's encrypted-backup loader is
/// not `Sync`) takes [`into_parts`](Self::into_parts) instead and keeps its
/// own tail.
#[derive(Debug)]
pub struct ExportWriter {
    output_dir: PathBuf,
    format: OutputFormat,
    sink: FormatSink,
    attachments_dir: PathBuf,
    files: AttachmentFiles,
    log: LogSink,
    progress: ProgressSink,
    resume: bool,
    use_queue: bool,
    /// Set by [`with_spool`](Self::with_spool), for an exporter whose
    /// attachments arrive as bytes.
    spool: Option<AttachmentSpool>,
}

/// The opened sink and the decisions [`ExportWriter::open`] made, for an
/// exporter that writes its own tail instead of calling
/// [`ExportWriter::finish`].
#[derive(Debug)]
pub struct ExportWriterParts {
    /// The opened (cleaned or resumed) format sink.
    pub sink: FormatSink,
    /// Directory attachment files are staged into (`<output>/attachments`).
    pub attachments_dir: PathBuf,
    /// Whether the run should drain the write queue instead of the sink.
    pub use_queue: bool,
    /// Whether this run stages the original attachment files
    /// ([`AttachmentFiles::Skip`] when the transforms do not copy them).
    pub files: AttachmentFiles,
    /// Log sink captured from the transforms.
    pub log: LogSink,
    /// Progress sink captured from the transforms.
    pub progress: ProgressSink,
}

impl ExportWriter {
    /// Prepare `output_dir` and open the sink for one export run.
    ///
    /// When `resume` is set the previous run's output is kept and its
    /// conversations are skipped ([`FormatSink::open_resume`]); otherwise the
    /// directory is cleaned first ([`FormatSink::open_prepared`]). When the
    /// transforms do not copy attachments, the run stages
    /// [`AttachmentFiles::Skip`].
    ///
    /// # Errors
    ///
    /// Returns an error when the output directory cannot be prepared, or a
    /// resumed run points at a directory that is not a run directory.
    pub fn open(
        output_dir: &Path,
        format: OutputFormat,
        transforms: ExportTransforms,
        resume: bool,
    ) -> Result<Self> {
        let files = if transforms.copies_attachments() {
            AttachmentFiles::Originals
        } else {
            AttachmentFiles::Skip
        };
        let log = transforms.log.clone();
        let progress = transforms.progress.clone();
        // The queue path is for the import, which is JSONL and never
        // obfuscated. Obfuscation is stateful across documents and the other
        // formats merge or embed at finish, so those keep the sink path.
        let use_queue = format == OutputFormat::Jsonl && !transforms.obfuscate;
        let (sink, attachments_dir) = if resume {
            FormatSink::open_resume(output_dir, format, transforms)
        } else {
            FormatSink::open_prepared(output_dir, format, transforms)
        }?;
        Ok(Self {
            output_dir: output_dir.to_path_buf(),
            format,
            sink,
            attachments_dir,
            files,
            log,
            progress,
            resume,
            use_queue,
            spool: None,
        })
    }

    /// Give the run an attachment spool under `scratch_dir`, the app's cache
    /// directory, when it copies attachments; see [`spool`](Self::spool).
    #[must_use]
    pub fn with_spool(mut self, scratch_dir: &Path) -> Self {
        if self.files.copies() {
            self.spool = Some(AttachmentSpool::new(scratch_dir).with_copy_dir(&self.output_dir));
        }
        self
    }

    /// Write the documents through `archive` instead of one file per
    /// conversation; see [`FormatSink::with_archive`]. The queue arm never
    /// applies, since a merged archive is not JSON Lines.
    #[must_use]
    pub fn with_archive(mut self, archive: Box<dyn MergedArchive>) -> Self {
        self.sink = self.sink.with_archive(archive);
        self
    }

    /// Directory attachment files are staged into (`<output>/attachments`).
    pub fn attachments_dir(&self) -> &Path {
        &self.attachments_dir
    }

    /// Whether [`finish`](Self::finish) will drain the write queue instead of
    /// the buffered sink.
    pub fn use_queue(&self) -> bool {
        self.use_queue
    }

    /// The run's attachment spool, under the Scratch Directory and never
    /// in the output directory; `None` when the writer was not given one
    /// or the run copies no attachments. An exporter whose attachments
    /// arrive as bytes writes each payload here as it parses it, so no
    /// payload waits in memory for the write. [`finish`](Self::finish) reads
    /// every spooled attachment back from its file, and the spool is removed
    /// when the writer is done, whether the run succeeded or failed.
    pub fn spool(&self) -> Option<&AttachmentSpool> {
        self.spool.as_ref()
    }

    /// Whether this run stages the original attachment files
    /// ([`AttachmentFiles::Skip`] when obfuscating or when the transforms do
    /// not copy them). Exporters check this while parsing to decide whether
    /// to keep payloads around.
    pub fn files(&self) -> AttachmentFiles {
        self.files
    }

    /// Log sink captured from the transforms.
    pub fn log(&self) -> &LogSink {
        &self.log
    }

    /// Progress sink captured from the transforms.
    pub fn progress(&self) -> &ProgressSink {
        &self.progress
    }

    /// Give up the writer and take the opened sink plus the decisions
    /// [`open`](Self::open) made, for a custom drain (see
    /// [`ExportWriterParts`]).
    pub fn into_parts(self) -> ExportWriterParts {
        ExportWriterParts {
            sink: self.sink,
            attachments_dir: self.attachments_dir,
            use_queue: self.use_queue,
            files: self.files,
            log: self.log,
            progress: self.progress,
        }
    }

    /// Write the collected documents: the queue arm drains
    /// [`ConversationUnit`]s (attachments before each conversation file, so
    /// an interrupted run can resume); the sink arm stages attachments and
    /// writes every document through the buffered sink.
    ///
    /// An attachment whose digest is in the [`spool`](Self::spool) is read
    /// from its spooled file. For every other attachment, `source_for` is
    /// the per-exporter hook. It is called once per attachment, in document
    /// order, and returns where that attachment's bytes come from plus a
    /// size hint for the progress totals. Exporters carrying bytes on the document move them out with
    /// `att.bytes.take()`; path-backed exporters return
    /// [`AttachmentSource::Path`] from their own source list.
    ///
    /// Folds conversation, attachment and obfuscation counts into `report`.
    ///
    /// # Errors
    ///
    /// Returns an error when a write fails, the staging disk is too small
    /// for the attachments it copies (checked before anything is written,
    /// in every format), or the user cancels.
    pub fn finish(
        self,
        documents: Vec<ConversationDocument>,
        source_for: &mut dyn FnMut(&mut IrAttachment) -> (AttachmentSource, Option<u64>),
        cancel: &CancelFlag,
        report: &mut ExportReport,
    ) -> Result<()> {
        let sinks = Sinks {
            log: &self.log,
            progress: &self.progress,
            cancel,
        };
        let spool = self.spool.as_ref();
        let mut source_for = |att: &mut IrAttachment| match spool.and_then(|s| s.source(att)) {
            Some(spooled) => spooled,
            None => source_for(att),
        };
        if self.use_queue {
            let units: Vec<ConversationUnit> = documents
                .into_iter()
                .map(|doc| ConversationUnit::from_doc(doc, |_, att| source_for(att)))
                .collect();
            let options = WriteQueueOptions {
                files: self.files,
                resume: self.resume,
                writer_count: 0,
            };
            return drain_units(&self.output_dir, units, &options, sinks, report);
        }

        let mut documents = documents;
        // Every attachment known now to have no file gets no hint, so
        // neither the disk check nor the progress total counts it, and the
        // total never drops when the run reaches it, as in the queue arm
        // (#1701).
        let counted = CountedAttachments::new(
            message_crate_core::document_messages(&mut documents),
            self.files,
            PathSources::OnDisk,
            &mut source_for,
            &self.log,
        );
        // The same check the queue arm makes, before anything is written:
        // the staged copies, and for a mail or merged archive every
        // embedded copy too, need room on the disk that holds the output. A
        // spool on that disk has already taken its share of what is free.
        if self.files.copies() {
            check_headroom(
                &self.output_dir,
                counted.bytes_to_write(self.format),
                Disk::Staging,
            )?;
        }
        report.attachments_saved +=
            counted.stage(&self.attachments_dir, load_attachment_source, sinks)?;

        write_documents_through_sink(documents, self.sink, sinks, report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_crate_core::{ProgressEvent, testutil::names_in};
    use std::fs;
    use std::sync::{Arc, Mutex};

    fn transforms(obfuscate: bool) -> ExportTransforms {
        ExportTransforms {
            obfuscate,
            ..ExportTransforms::none()
        }
    }

    #[test]
    fn only_unobfuscated_jsonl_takes_the_queue() {
        let tmp = tempfile::tempdir().unwrap();
        let open = |format, obfuscate| {
            ExportWriter::open(tmp.path(), format, transforms(obfuscate), false).unwrap()
        };

        let jsonl = open(OutputFormat::Jsonl, false);
        assert!(jsonl.use_queue());
        assert_eq!(jsonl.files(), AttachmentFiles::Originals);

        assert!(!open(OutputFormat::Csv, false).use_queue());
        assert!(!open(OutputFormat::Json, false).use_queue());

        let obfuscated = open(OutputFormat::Jsonl, true);
        assert!(!obfuscated.use_queue());
        assert_eq!(obfuscated.files(), AttachmentFiles::Skip);
    }

    /// One conversation whose only message carries one attachment's bytes.
    fn document_with_bytes() -> ConversationDocument {
        let mut doc = message_ir::testutil::sample_document("with a photo");
        doc.messages[0].attachments = vec![IrAttachment {
            bytes: Some(b"\xff\xd8\xffphoto".to_vec()),
            ..message_ir::testutil::sample_attachment()
        }];
        doc
    }

    /// Both paths stage the attachment's bytes, write the conversation, and
    /// count both in the report.
    #[test]
    fn finish_stages_attachments_and_counts_them_on_both_paths() {
        for format in [OutputFormat::Jsonl, OutputFormat::Csv] {
            let tmp = tempfile::tempdir().unwrap();
            let writer = ExportWriter::open(tmp.path(), format, transforms(false), false).unwrap();
            let mut report = ExportReport::default();

            writer
                .finish(
                    vec![document_with_bytes()],
                    &mut AttachmentSource::take_bytes,
                    &CancelFlag::default(),
                    &mut report,
                )
                .unwrap();

            assert_eq!(report.conversations, 1, "{format:?}");
            assert_eq!(report.attachments_saved, 1, "{format:?}");
            let staged: Vec<Vec<u8>> = fs::read_dir(tmp.path().join("attachments"))
                .unwrap()
                .map(|e| fs::read(e.unwrap().path()).unwrap())
                .collect();
            assert_eq!(staged, [b"\xff\xd8\xffphoto".to_vec()], "{format:?}");
            let written = fs::read_dir(tmp.path())
                .unwrap()
                .filter(|e| {
                    e.as_ref()
                        .unwrap()
                        .path()
                        .extension()
                        .and_then(|x| x.to_str())
                        == Some(format.as_str())
                })
                .count();
            assert_eq!(written, 1, "{format:?}: one conversation file");
        }
    }

    /// An export to a format other than JSON Lines knows before it starts
    /// which attachments have no file: a source that says so, a path with
    /// nothing there and a source with no bytes. Its byte total leaves them
    /// out from the first event and never drops mid-run, as the write
    /// queue's does (#1701).
    #[test]
    fn the_byte_total_of_a_csv_export_stays_the_same_when_a_file_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));
        let sink_seen = Arc::clone(&seen);
        let progress = ProgressSink::unpaced(move |event| sink_seen.lock().unwrap().push(event));
        let writer = ExportWriter::open(
            &tmp.path().join("out"),
            OutputFormat::Csv,
            ExportTransforms {
                progress,
                ..transforms(false)
            },
            false,
        )
        .unwrap();
        let mut doc = document_with_bytes();
        let attachment = doc.messages[0].attachments[0].clone();
        // The real attachment comes first, so the first event is sent before
        // the run reaches a missing one and could take its size off.
        doc.messages[0].attachments = [5, 700, 1_000, 30]
            .map(|size| IrAttachment {
                size_bytes: Some(size),
                ..attachment.clone()
            })
            .to_vec();
        let mut sources = vec![
            AttachmentSource::Bytes(b"xxxxx".to_vec()),
            AttachmentSource::Bytes(Vec::new()),
            AttachmentSource::Path(tmp.path().join("gone.jpg")),
            AttachmentSource::Missing,
        ]
        .into_iter();

        writer
            .finish(
                vec![doc],
                &mut |att: &mut IrAttachment| (sources.next().unwrap(), att.size_bytes),
                &CancelFlag::default(),
                &mut ExportReport::default(),
            )
            .unwrap();

        let totals: Vec<(usize, u64, u64)> = seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match *event {
                ProgressEvent::Attachments {
                    done,
                    bytes_done,
                    bytes_total,
                    ..
                } => Some((done, bytes_done, bytes_total)),
                _ => None,
            })
            .collect();
        assert!(totals.len() > 1, "{totals:?}");
        assert!(
            totals.iter().all(|&(_, _, bytes_total)| bytes_total == 5),
            "every total: {totals:?}"
        );
        assert_eq!(
            totals
                .last()
                .map(|&(done, bytes_done, _)| (done, bytes_done)),
            Some((4, 5))
        );
    }

    /// Every format that is not JSON Lines checks the staging disk for room
    /// before it writes, as the JSON Lines queue does: an attachment no disk
    /// could hold stops the run with the space it needs, and the output
    /// directory holds nothing but the export's mark (#1421).
    #[test]
    fn every_format_refuses_a_backup_the_staging_disk_cannot_hold_before_writing() {
        for format in [
            OutputFormat::Csv,
            OutputFormat::Json,
            OutputFormat::Eml,
            OutputFormat::Mbox,
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let writer = ExportWriter::open(tmp.path(), format, transforms(false), false).unwrap();
            let mut doc = document_with_bytes();
            doc.messages[0].attachments[0].size_bytes = Some(u64::MAX / 2);

            let err = writer
                .finish(
                    vec![doc],
                    &mut AttachmentSource::take_bytes,
                    &CancelFlag::default(),
                    &mut ExportReport::default(),
                )
                .unwrap_err();

            let text = err.to_string();
            assert!(
                text.starts_with("Not enough space on the staging disk: this backup needs about "),
                "{format:?}: {text}"
            );
            assert_eq!(
                names_in(tmp.path()),
                [".message-crate-export", "attachments"],
                "{format:?}"
            );
            assert!(
                names_in(&tmp.path().join("attachments")).is_empty(),
                "{format:?}"
            );
        }
    }

    /// The spool sits under the Scratch Directory the writer is given, never in
    /// the output directory, and is gone once the run ends (#1421).
    #[test]
    fn the_spool_is_under_the_scratch_directory_and_never_in_the_output_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out");
        let cache = tmp.path().join("cache");
        let writer = ExportWriter::open(&out, OutputFormat::Csv, transforms(false), false)
            .unwrap()
            .with_spool(&cache);
        let spool = writer
            .spool()
            .expect("a run that copies attachments spools");
        let digest = spool.put(b"\xff\xd8\xffphoto").unwrap();
        let spooled = spool.path(&digest).unwrap();

        assert!(
            spooled.starts_with(cache.join(message_crate_core::ATTACHMENT_SPOOL_DIRECTORY)),
            "{}",
            spooled.display()
        );
        assert_eq!(names_in(&out), [".message-crate-export", "attachments"]);

        let mut doc = document_with_bytes();
        let att = &mut doc.messages[0].attachments[0];
        att.bytes = None;
        att.digest_sha256 = Some(digest);
        let mut report = ExportReport::default();
        writer
            .finish(
                vec![doc],
                &mut AttachmentSource::take_bytes,
                &CancelFlag::default(),
                &mut report,
            )
            .unwrap();
        assert_eq!(report.attachments_saved, 1);
        assert!(!spooled.exists(), "the spool is removed when the run ends");
    }
}
