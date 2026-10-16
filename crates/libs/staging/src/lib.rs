//! Staging: the resumable path that writes a run's conversations and
//! attachments into the Staging Directory.
//!
//! Parse finishes before anything is written. Every exporter but Apple
//! Messages runs its convert step through [`convert_backup`], which opens
//! the writer, reads, projects, and hands the documents to
//! [`ExportWriter`], which either drains the bounded write
//! queue (JSON Lines, the import path: a conversation file lands only after
//! everything it references, so an interrupted run resumes by skipping what
//! is already there) or stages attachments and writes through
//! `ir_format::FormatSink` for every other format. An exporter whose
//! attachments arrive as bytes writes each payload to the writer's
//! [`AttachmentSpool`], under the Scratch Directory, as it parses, so parse
//! never holds the backup's attachments in memory, and the write reads them
//! back a file at a time. Every write is checked for room first, against
//! the disk that holds it ([`check_headroom`]).
//! [`transcode_staged`] converts or compresses staged attachments
//! afterwards, on its own and resumable. The desktop app runs it as an
//! Import Run's Media Stage, and a local export runs it through the write
//! queue. [`summarize_staging`] reads a run directory back for the Staging
//! Review. The Staging Review and the desktop app's Media Stage both work to
//! the media settings Staging recorded in the directory
//! ([`write_media_settings`], [`read_media_settings`]); a local export passes
//! its own.
//!
//! Formats live in `ir-format`; the run model in
//! `message-crate-core`. Why this is its own crate:
//! `docs/adr/0012-four-crates-in-the-export-pipeline.md`.

mod attachment_paths;
mod convert;
mod counted_attachments;
mod export_writer;
mod headroom;
mod media_settings;
mod spool;
mod staging_summary;
mod transcode;
mod write_queue;

pub use attachment_paths::AttachmentPaths;
pub use convert::{Project, ReadBackup, WriterSetup, convert_backup};
pub use counted_attachments::{CountedAttachments, PathSources};
pub use export_writer::{ExportWriter, ExportWriterParts};
pub use headroom::{
    Disk, LOW_DISK_SPACE_BYTES, bytes_embedded, bytes_to_copy, bytes_to_write, check_headroom,
    low_disk_space, scratch_disk_full,
};
pub use media_settings::{MEDIA_SETTINGS_FILE, read_media_settings, write_media_settings};
pub use spool::AttachmentSpool;
pub use staging_summary::{AttachmentForecast, StagingSummary, SummaryProgress, summarize_staging};
pub use transcode::{TranscodeOptions, TranscodeProgress, TranscodeReport, transcode_staged};
pub use write_queue::{
    AttachmentSource, ConversationUnit, UnitAttachment, WriteQueueOptions, WriteQueueReport,
    default_writer_count, drain_units, drain_write_queue, drain_write_queue_with_loader,
    load_attachment_source, unreadable_attachment_line,
};
