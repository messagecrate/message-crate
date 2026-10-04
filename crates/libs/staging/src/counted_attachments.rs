//! The one way a run that holds its conversations stages their attachments
//! outside the write queue: every attachment paired with its source and
//! counted by the rule the queue counts by, before any total is summed.

use crate::headroom::bytes_to_write;
use crate::write_queue::{AttachmentSource, counted_source, missing_if_no_file};
use media::MediaMode;
use message_crate_core::{
    AttachmentJob, CancelFlag, LoadError, LogSink, MediaConfig, OutputFormat, ProgressSink,
    attachment_jobs, stage_attachment_jobs,
};
use message_ir::{IrAttachment, IrMessage};
use std::path::Path;

/// Where a `Path` source's file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSources {
    /// On disk, read with a plain `fs::read`: a path with no file there is
    /// known before the run to have no file.
    OnDisk,
    /// Somewhere only the caller's loader can reach, such as inside an
    /// encrypted iPhone backup, so the path is left to the loader.
    ReadByLoader,
}

/// Every attachment of a run's messages with its source, counted before the
/// run so that its byte total, and the disk check made from it, leave out
/// every attachment known to have no file (#1701, #1727).
///
/// An attachment whose source is `Missing`, is bytes that are empty, or,
/// with [`PathSources::OnDisk`], is a path with nothing there, gets no size
/// hint and stages as `file_missing`. The write queue counts its sources
/// the same way.
#[derive(Debug)]
pub struct CountedAttachments<'a> {
    jobs: Vec<AttachmentJob<'a>>,
    sources: Vec<AttachmentSource>,
    media: MediaConfig,
}

impl<'a> CountedAttachments<'a> {
    /// Pair every attachment across `messages`, in the order given, with the
    /// source and size hint `source_for` returns for it, and count each.
    ///
    /// A path checked on disk and found empty is logged as the read would
    /// have logged it. With the media turned off nothing is read, so no path
    /// is checked.
    pub fn new(
        messages: impl IntoIterator<Item = &'a mut IrMessage>,
        media: MediaConfig,
        paths: PathSources,
        mut source_for: impl FnMut(&mut IrAttachment) -> (AttachmentSource, Option<u64>),
        log: Option<&LogSink>,
    ) -> Self {
        let check_paths = paths == PathSources::OnDisk && media.mode != MediaMode::Disabled;
        let mut jobs = attachment_jobs(messages);
        let mut sources = Vec::with_capacity(jobs.len());
        for job in &mut jobs {
            let mut counted = counted_source(source_for(job.attachment));
            if check_paths {
                counted = missing_if_no_file(counted, log);
            }
            let (source, hint) = counted;
            job.size_hint = hint;
            sources.push(source);
        }
        Self {
            jobs,
            sources,
            media,
        }
    }

    /// The bytes staging these attachments puts in an output of `format`,
    /// for the disk check: [`bytes_to_write`] over every attachment with a
    /// file.
    pub fn bytes_to_write(&self, format: OutputFormat) -> u64 {
        let sizes: Vec<(Option<&str>, u64)> = self
            .jobs
            .iter()
            .zip(&self.sources)
            .filter(|(_, source)| !matches!(source, AttachmentSource::Missing))
            .map(|(job, _)| {
                (
                    job.attachment.digest_sha256.as_deref(),
                    job.size_hint.unwrap_or(0),
                )
            })
            .collect();
        bytes_to_write(format, &sizes)
    }

    /// Stage every attachment into `attachments_dir` through
    /// [`stage_attachment_jobs`], loading each source with `load`, and
    /// return how many distinct files were written.
    /// [`load_attachment_source`](crate::load_attachment_source) is the
    /// loader for sources on disk or in memory.
    ///
    /// # Errors
    ///
    /// As [`stage_attachment_jobs`].
    pub fn stage(
        self,
        attachments_dir: &Path,
        mut load: impl FnMut(&mut AttachmentSource) -> Result<Option<Vec<u8>>, LoadError>,
        log: Option<&LogSink>,
        progress: Option<&ProgressSink>,
        cancel: Option<&CancelFlag>,
    ) -> Result<u64, String> {
        let Self {
            jobs,
            mut sources,
            media,
        } = self;
        stage_attachment_jobs(
            jobs,
            attachments_dir,
            &media,
            |i| match sources.get_mut(i) {
                Some(source) => load(source),
                None => Ok(None),
            },
            log,
            progress,
            cancel,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_attachment_source;

    fn attachment(size: u64) -> IrAttachment {
        IrAttachment {
            path: None,
            original_name: Some("photo.jpg".into()),
            mime_type: Some("image/jpeg".into()),
            digest_sha256: None,
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: Some(size),
            missing_reason: None,
            bytes: None,
        }
    }

    /// The byte totals of one staging run in which only the first of four
    /// attachments has a file, each given the size its record names.
    fn totals(paths: PathSources, gone: &Path) -> (Vec<(usize, u64, u64)>, u64) {
        let out = tempfile::tempdir().unwrap();
        let mut doc = message_ir::testutil::sample_document("with a photo");
        doc.messages[0].attachments = [5, 700, 1_000, 30].map(attachment).to_vec();
        let mut sources = vec![
            AttachmentSource::Bytes(b"xxxxx".to_vec()),
            AttachmentSource::Path(gone.to_path_buf()),
            AttachmentSource::Missing,
            AttachmentSource::Bytes(Vec::new()),
        ]
        .into_iter();
        let (progress, totals) = message_crate_core::testutil::attachment_totals();

        let counted = CountedAttachments::new(
            doc.messages.iter_mut(),
            MediaConfig::default(),
            paths,
            |att| (sources.next().unwrap(), att.size_bytes),
            None,
        );
        let needed = counted.bytes_to_write(OutputFormat::Csv);
        counted
            .stage(
                &out.path().join("attachments"),
                load_attachment_source,
                None,
                Some(&progress),
                None,
            )
            .unwrap();

        let totals = totals.lock().unwrap().clone();
        (totals, needed)
    }

    /// Every attachment known before the run to have no file is left out of
    /// the byte total from the first event, and out of the disk check, so
    /// the total never drops when the run reaches it (#1727).
    #[test]
    fn the_byte_total_and_the_disk_check_leave_out_every_attachment_with_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (totals, needed) = totals(PathSources::OnDisk, &tmp.path().join("gone.jpg"));
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
        assert_eq!(needed, 5);
    }

    /// A path only the caller's loader can read is counted at its hint: its
    /// file may be inside a backup, not at that path on disk.
    #[test]
    fn a_path_the_loader_reads_is_counted_at_its_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let (totals, needed) = totals(PathSources::ReadByLoader, &tmp.path().join("inside.jpg"));
        assert_eq!(totals.first().map(|&(_, _, total)| total), Some(705));
        assert_eq!(needed, 705);
    }
}
