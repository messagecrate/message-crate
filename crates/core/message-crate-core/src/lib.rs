//! Shared exporter forms, typed config, and background-job helpers for desktop apps.
//!
//! The desktop app's commands and the exporter crates share this crate so every
//! backup type validates the same way before a job starts.

pub mod attachment_jobs;
pub mod attachments;
mod config;
mod counter;
mod exporters;
mod pipeline;
mod process;
mod progress;
mod run;
mod scratch;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
mod transforms;

pub use attachment_jobs::{
    AttachmentJob, AttachmentProgress, LoadError, attachment_jobs, attachment_size_hint,
    document_messages, mime_for_rel, report_attachment_progress, run_attachment_jobs,
    stage_attachment_jobs,
};
pub use attachments::{attachment_date_prefix, attachment_dest_name, digest_prefix};
pub use config::{
    AppleConfig, ExporterConfig, FormatConfig, GoSmsProConfig, ImazingConfig, MediaConfig,
    ObfuscateConfig, OpenExtractConfig, OutputFormat, SmsBackupPlusConfig, SmsBackupRestoreConfig,
    SourceConfig, WhatsappConfig,
};
pub use counter::{
    ATTACHMENTS_MISSING, ATTACHMENTS_SAVED, CONVERSATION_FILES_PREPARING, CONVERSATIONS_OBFUSCATED,
    CONVERSATIONS_RESUMED, Counter, DUPLICATES_DROPPED, NAME_ONLY_CHAT, NOT_SMS_OR_MMS_LEFT_OUT,
    NOTIFICATIONS, SKIPPED_INVALID_DATE, SKIPPED_UNKNOWN_ADDRESS, SKIPPED_UNKNOWN_TYPE,
    SKIPPED_UNREADABLE_PART, count_of, count_of_files, error_line, note_line,
};
pub use exporters::{
    ApplePlatform, AttachmentMedia, CONVERT_COMPRESS_FFMPEG_REQUIRED, Exporter, Form,
    WhatsappPlatform, ensure_output_dir,
};
pub use pipeline::{
    CSV_NOT_READ, ExportReport, IssueSink, NAME_ONLY_CHAT_NOTE, NOTE, RunIssue, RunResult,
    discover_files, emit_issue, export_meta, prepare_outputs, project_conversation,
    unreadable_parts_note,
};
pub use process::{
    CancelFlag, Cancelled, LogSink, check_cancel, emit_log, is_cancelled, parallel_for_each,
};
pub use progress::{ProgressEvent, ProgressSink, emit_progress};
pub use run::{finish_run, run_pipeline};
pub use scratch::{
    ATTACHMENT_SPOOL_DIRECTORY, IMESSAGE_READER_DIRECTORY, ScratchDir, WHATSAPP_DIRECTORY,
    sweep_scratch,
};
pub use transforms::ExportTransforms;
