//! Library entry: [`ExporterConfig`] to the shared conversation structure, then
//! the chosen output format.
//!
//! Everything a person can get wrong about the source is checked here, in
//! this process, before the `imessage-reader` program is started: the
//! sentences below are the ones the Import screen shows. What only the
//! database can tell (a wrong password, a backup with no Messages in it)
//! comes back from the program as its own error sentence.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use imessage_reader_protocol::{ExportRequest, Platform, Request, Source};
use ios_backup::{Helper, ios_backup_encrypted_flag};
use message_crate_core::{
    AppleConfig, ApplePlatform, CancelFlag, ExportTransforms, ExporterConfig,
    IMESSAGE_READER_DIRECTORY, IssueSink, LogSink, OutputFormat, ProgressEvent, ProgressSink,
    RunIssue, RunResult, ScratchDir, SourceConfig, emit_progress, prepare_outputs,
};
use message_staging::{Disk, check_headroom};

use crate::convert;

/// User-facing copy when a custom attachment directory is missing.
pub(crate) const ATTACHMENT_DIRECTORY_MISSING: &str = "Attachment directory does not exist.";
/// User-facing copy when a supplied Apple Contacts file is missing.
pub(crate) const APPLE_CONTACTS_MISSING: &str = "Apple Contacts file does not exist.";
/// User-facing copy when the macOS Messages database file is missing.
pub(crate) const MESSAGES_DATABASE_MISSING: &str = "Messages database does not exist.";
/// User-facing copy when the directory is not an iPhone backup (or Messages is missing).
pub(crate) const NOT_AN_IPHONE_BACKUP: &str =
    "This directory is not an iPhone backup, or Messages is missing from it.";

/// Where an iPhone backup keeps the Messages database: the SHA-1 of its
/// domain and path, under a directory named by its first two characters.
const MESSAGES_DB_IN_IOS_BACKUP: &str = "3d/3d0d7e5fb2ce288813306e4d4636395e047a3d28";

/// Where an iPhone backup keeps the Contacts database, which the program
/// decrypts beside the Messages database.
const CONTACTS_DB_IN_IOS_BACKUP: &str = "31/31bb7ba8914766d4ba40d6dfb6113c8b614be442";

/// Where the Messages database lives on a Mac.
fn default_macos_db_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Library/Messages/chat.db")
}

/// Whether to resolve attachment bytes for embedding (`.eml` / `.mbox`) or
/// persisting under `attachments/` (CSV / JSON).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentEmbed {
    /// Resolve and embed media bytes (macOS path or iOS decrypt).
    Embed,
    /// Skip media bytes (empty attachment parts still possible via other fields).
    Disabled,
}

/// Map `AppleConfig.copy_method` to attachment handling.
///
/// `clone` copies files, `basic` embeds thumbnails, and `full` embeds
/// originals — all three resolve bytes through the same embed path in this
/// exporter. `disabled` skips media bytes entirely.
fn attachment_embed_from_copy_method(copy_method: &str) -> Result<AttachmentEmbed> {
    match copy_method.trim().to_ascii_lowercase().as_str() {
        "disabled" => Ok(AttachmentEmbed::Disabled),
        "clone" | "basic" | "full" => Ok(AttachmentEmbed::Embed),
        other => bail!("unknown attachment mode {other:?}; use clone, basic, full, or disabled"),
    }
}

/// Everything one export run needs, checked and ready to send.
#[derive(Debug)]
pub(crate) struct ExportOptions {
    /// The Messages data the `imessage-reader` program reads.
    pub source: Source,
    /// [`ExportRequest::attachment_root`].
    pub attachment_root: Option<String>,
    /// [`ExportRequest::contacts_path`].
    pub contacts_path: Option<PathBuf>,
    /// [`ExportRequest::use_caller_id`].
    pub use_caller_id: bool,
    pub export_path: PathBuf,
    /// the Scratch Directory; the program decrypts into a directory under it.
    pub scratch_dir: PathBuf,
    pub attachment_embed: AttachmentEmbed,
    /// Media / obfuscate transforms applied by [`message_ir_format::FormatSink`].
    pub transforms: ExportTransforms,
    /// CSV, EML, MBOX, JSON, or JSON Lines (one JSON object per line).
    pub output_format: OutputFormat,
    /// Human-readable mid-run notes and warnings (desktop sink or stderr).
    pub log: Option<LogSink>,
    /// Typed progress events for the desktop's progress bar.
    pub progress: Option<ProgressSink>,
    /// Rows for the Import Run's record, sent as they are recorded.
    pub issues: Option<IssueSink>,
    /// Cooperative cancel flag, checked between events and before every write.
    pub cancel: Option<CancelFlag>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
}

impl ExportOptions {
    /// The request for the program, which decrypts into `scratch_dir`.
    pub fn export_request(&self, scratch_dir: &Path) -> Request {
        Request::Export(ExportRequest {
            source: self.source.clone(),
            attachment_root: self.attachment_root.clone(),
            contacts_path: self.contacts_path.clone(),
            use_caller_id: self.use_caller_id,
            scratch_dir: scratch_dir.to_path_buf(),
        })
    }

    /// Write one log line when a log sink is configured.
    pub fn emit_log(&self, line: impl AsRef<str>) {
        message_crate_core::emit_log(self.log.as_ref(), line);
    }

    /// Send one typed progress event when a progress sink is configured.
    pub fn emit_progress(&self, event: ProgressEvent) {
        emit_progress(self.progress.as_ref(), event);
    }

    /// Send one row for the Import Run's record when an issue sink is
    /// configured.
    pub fn emit_issue(&self, issue: RunIssue) {
        message_crate_core::emit_issue(self.issues.as_ref(), issue);
    }

    /// The shared cancel check.
    pub fn check_cancel(&self) -> Result<()> {
        message_crate_core::check_cancel(self.cancel.as_ref()).map_err(|e| anyhow!(e))
    }
}

/// Build options from [`ExporterConfig`], start the `imessage-reader`
/// program on the Messages database, and write the export.
///
/// # Errors
///
/// Returns an error when the source is not Apple Messages, the program
/// cannot be found or the database cannot be opened, conversion fails, media
/// processing fails for every candidate file, or the user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    run_with(config, Helper::spawn)
}

/// [`run`], starting the program with `spawn`. Tests pass a fake program.
fn run_with(
    config: &ExporterConfig,
    spawn: impl FnOnce(&Request, Option<LogSink>, Option<ProgressSink>) -> Result<Helper>,
) -> Result<RunResult> {
    let options = options_from_export_config(config)?;
    options.check_cancel()?;
    let output = convert::open_output(&options)?;

    // The program writes decrypted files into a directory of this run's own
    // under the Scratch Directory, beside the identities request's: not
    // the system's temporary directory, which can be a small RAM disk that no
    // check measures (#1134), and not the output directory, which holds only
    // output (#1402). The directory is deleted when the run ends, whichever
    // way it ends; what a killed run left is deleted by the next request
    // and by the sweep when the app starts.
    let scratch_root = options.scratch_dir.join(IMESSAGE_READER_DIRECTORY);
    let report = {
        let scratch = ScratchDir::create(&scratch_root)?;
        check_headroom(
            scratch.path(),
            decrypted_bytes(&options.source),
            Disk::Scratch,
        )?;
        let mut helper = spawn(
            &options.export_request(scratch.path()),
            options.log.clone(),
            options.progress.clone(),
        )?;
        let report = convert::export(&mut helper, &options, output)?;
        helper.finish()?;
        report
    };
    options.check_cancel()?;

    message_crate_core::finish_run(config, &report, config.media.mode.needs_tools())
}

/// Translate the shared exporter config into this exporter's options, rejecting non-Apple sources.
fn options_from_export_config(config: &ExporterConfig) -> Result<ExportOptions> {
    let SourceConfig::Apple(source) = &config.source else {
        bail!("imessage-ir-exporter requires SourceConfig::Apple");
    };

    let db_path = db_path_for(config.primary_input());
    let platform = platform_for(source, &db_path)?;

    if source.backup_password.is_some() && platform != Platform::Ios {
        bail!("backup password is enabled; it can only be used with iOS backups.");
    }
    check_macos_only_path(
        config,
        platform,
        source.attachment_root.as_deref().map(Path::new),
        ATTACHMENT_DIRECTORY_MISSING,
        "An attachment directory was given, but iPhone backups keep attachments inside the backup, so it will be ignored.",
    )?;
    check_macos_only_path(
        config,
        platform,
        source.apple_contacts.as_deref(),
        APPLE_CONTACTS_MISSING,
        "An Apple Contacts file was given, but names for an iPhone backup come from the backup itself, so it will be ignored.",
    )?;
    check_db_path(platform, &db_path)?;

    let attachment_embed = attachment_embed_from_copy_method(&source.copy_method)?;

    // Create the output directory, refused when it is or holds the database,
    // the backup or the attachment directory a Mac export reads, because
    // `convert` cleans it through ExportWriter::open.
    let mut inputs = vec![db_path.clone()];
    if platform == Platform::MacOs
        && let Some(root) = &source.attachment_root
    {
        inputs.push(PathBuf::from(root));
    }
    prepare_outputs(&inputs, &config.output)?;

    Ok(ExportOptions {
        source: Source {
            db_path,
            platform,
            backup_password: source.backup_password.clone(),
        },
        attachment_root: source.attachment_root.clone(),
        contacts_path: source.apple_contacts.clone(),
        use_caller_id: source.use_caller_id,
        export_path: config.output.clone(),
        scratch_dir: config.scratch_dir.clone(),
        attachment_embed,
        transforms: ExportTransforms::from_config(config),
        output_format: config.output_format,
        log: config.log.clone(),
        progress: config.progress.clone(),
        issues: config.issues.clone(),
        cancel: config.cancel.clone(),
        resume: config.resume,
    })
}

/// The bytes the program decrypts into its scratch directory before it reads
/// anything: an encrypted iPhone backup's Messages and Contacts databases,
/// each about the size of its encrypted copy in the backup. Attachments are
/// decrypted one at a time and deleted once read, which the check's slack
/// covers. Nothing is decrypted from a Mac database or a backup that is not
/// encrypted.
fn decrypted_bytes(source: &Source) -> u64 {
    if source.platform != Platform::Ios || ios_backup_encrypted_flag(&source.db_path) != Some(true)
    {
        return 0;
    }
    [MESSAGES_DB_IN_IOS_BACKUP, CONTACTS_DB_IN_IOS_BACKUP]
        .iter()
        .filter_map(|file| std::fs::metadata(source.db_path.join(file)).ok())
        .map(|meta| meta.len())
        .fold(0, u64::saturating_add)
}

/// The input the person chose, or this Mac's own Messages database when
/// they left it empty.
fn db_path_for(input: Option<&Path>) -> PathBuf {
    match input {
        Some(path) if !path.as_os_str().is_empty() => path.to_path_buf(),
        _ => default_macos_db_path(),
    }
}

/// The platform the source names, or the one the backup's layout shows.
fn platform_for(source: &AppleConfig, db_path: &Path) -> Result<Platform> {
    match source.platform {
        Some(ApplePlatform::MacOs) => Ok(Platform::MacOs),
        Some(ApplePlatform::Ios) => Ok(Platform::Ios),
        Some(ApplePlatform::Auto) | None => detect_platform(db_path),
    }
}

/// Tell a backup directory from a database file by layout: a directory holding the
/// Messages database at its hashed path is an iPhone backup, a file is a
/// Mac `chat.db`. Anything else is treated as a Mac path so the missing
/// database is what gets reported.
fn detect_platform(db_path: &Path) -> Result<Platform> {
    if db_path.ends_with(MESSAGES_DB_IN_IOS_BACKUP) {
        bail!(
            "{} is the Messages database inside an iPhone backup; choose the backup directory itself.",
            db_path.display()
        );
    }
    if db_path.join(MESSAGES_DB_IN_IOS_BACKUP).exists() {
        return Ok(Platform::Ios);
    }
    Ok(Platform::MacOs)
}

/// A path option that only a macOS export reads: refused when it points
/// nowhere, and noted in the log as having no effect on an iOS backup.
fn check_macos_only_path(
    config: &ExporterConfig,
    platform: Platform,
    path: Option<&Path>,
    missing: &str,
    ignored_on_ios: &str,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if !path.exists() {
        bail!("{missing}");
    }
    if platform == Platform::Ios {
        config.emit_log(ignored_on_ios);
    }
    Ok(())
}

/// The backup must be laid out as the platform expects: a messages
/// database file on macOS; on iOS a backup directory with its manifest and,
/// when the backup is not encrypted, the database at its hashed path.
fn check_db_path(platform: Platform, db_path: &Path) -> Result<()> {
    match platform {
        Platform::MacOs => {
            if !db_path.is_file() {
                bail!("{MESSAGES_DATABASE_MISSING}");
            }
        }
        Platform::Ios => {
            let manifest = db_path.join("Manifest.plist");
            if !db_path.is_dir() || !manifest.is_file() {
                bail!("{NOT_AN_IPHONE_BACKUP}");
            }
            if ios_backup_encrypted_flag(db_path) == Some(false)
                && !db_path.join(MESSAGES_DB_IN_IOS_BACKUP).is_file()
            {
                bail!("{NOT_AN_IPHONE_BACKUP}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
