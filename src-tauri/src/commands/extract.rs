//! `extract` and `cancel` commands.
//!
//! `extract` starts the selected exporter on a background thread and returns
//! immediately. Progress is sent back as Tauri events:
//! `desktop-job:log` (one human-readable log line), `desktop-job:progress` (one
//! typed [`ImportProgressEvent`](events::ImportProgressEvent), mapped from
//! the exporter's `ProgressEvent`), `desktop-job:file-written` (one
//! [`ImportFileWrittenEvent`](events::ImportFileWrittenEvent), a conversation
//! file the write queue finished, from the same `ProgressEvent`s),
//! `desktop-job:issue` (one [`ImportIssueEvent`](events::ImportIssueEvent),
//! a row for the Import Run's record, sent the moment the exporter records
//! it), `desktop-job:finished` (a summary string or JSON object), and
//! `desktop-job:error` ([`DesktopJobErrorEvent`](events::DesktopJobErrorEvent)).
//!
//! `cancel` sets the cancel flag of the job that is running (see
//! `commands::jobs`). The exporter checks it between steps through
//! `RunContext.cancel`.
//!
//! Which exporter a backup runs, and how its form is filled, is looked up in
//! [`exporters`].

use std::path::Path;
use std::sync::{Arc, Mutex};

use media::{CompressOptions, InstalledTools, MaxResolution, MediaMode};
use message_crate_core::{
    AttachmentFiles, ExporterConfig, Form, FormContext, OutputFormat, RunResult,
};
use message_staging::TranscodeOptions;

use super::events;
use super::jobs::{cancel_running_job, spawn_job, start_job};
use super::last_log_line_or;
use super::paths::{logs_dir, scratch_dir};
use crate::app_directories::RunLog;
use crate::run_directories::RunDirectories;
use crate::state::{AppState, JobName};
use crate::tool_downloads::ToolDownloads;

mod exporters;

/// Ask this process to stop the job that is running. Does nothing when no
/// job runs.
///
/// Sets that job's cancel flag. The job checks the flag between steps and
/// exits on its own. There is no hard kill.
///
/// # Errors
///
/// Returns an error if another thread panicked while holding the shared
/// state lock.
#[tauri::command(async)]
pub fn cancel(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    cancel_running_job(&state)
}

/// The `desktop-job:finished` payload: the run's last log line as the summary,
/// and the conversation and message counts the exporter reported.
///
/// The counts come from the exporter rather than from reading the output
/// directory again, because that directory also holds the staged attachments, and
/// an attachment can be named `*.jsonl` and hold any bytes at all.
fn finished_payload(run_result: &RunResult) -> String {
    serde_json::json!({
        "summary": last_log_line_or(&run_result.messages, "Export complete."),
        "files_parsed": run_result.conversations,
        "messages_parsed": run_result.message_count,
    })
    .to_string()
}

/// User-facing parameters for the `extract` command (before defaults/parsing).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractArgs {
    /// Backup source key, for example `imessage-ios` or `whatsapp-android`.
    pub source: String,
    /// Path to the phone backup (a directory, database file, or XML file).
    pub path: String,
    /// run directory `create_run_dir` made, which the exporter writes
    /// conversation files into.
    pub output_dir: String,
    /// Password for encrypted backups, when the source needs one.
    pub backup_password: Option<String>,
    /// Attachment handling choice: `copy`, `convert`, `compress`, or `skip`.
    pub attachment_media: Option<String>,
    /// Long-edge cap for compressed video: `720p`, `1080p`, or `4k`.
    pub media_max_resolution: Option<String>,
    /// Frame-rate cap for compressed video, for example `30`.
    pub media_max_fps: Option<String>,
    /// Size below which a video is not compressed, as a whole number of
    /// megabytes, for example `20`.
    pub media_min_size: Option<String>,
    /// When true, replace names and phone numbers with fake ones.
    pub obfuscate: Option<bool>,
    /// Zone iMazing dates are read in: an IANA name (`America/New_York`) or a
    /// fixed offset (`UTC-05:00`). The screen sends the account's zone unless
    /// the person picked another in the advanced section.
    pub timezone: Option<String>,
    /// The country of the phone the backup came from, as an ISO 3166-1
    /// alpha-2 code (`GB`), or none. SMS Backup+ keys numbers written
    /// without their `+` code in it; the server applies it to every file of
    /// the Import Run, which the web app creates with it (#1676).
    pub phone_country: Option<String>,
    /// Owner phone numbers for the Android SMS sources (SMS Backup & Restore,
    /// GO SMS Pro, SMS Backup+) and both WhatsApp platforms.
    pub owner_phones: Option<Vec<String>>,
    /// Owner email addresses for SMS Backup+, whose archive is Gmail-backed
    /// and needs them to tell sent mail from received.
    pub owner_emails: Option<Vec<String>>,
    /// Alternate directory for Attachments and StickerCache (Mac and jailbreak).
    pub attachment_root: Option<String>,
    /// Path to an Apple AddressBook file (Mac and jailbreak).
    pub apple_contacts: Option<String>,
    /// WhatsApp decryption key or key-file path (Android crypt backups).
    pub whatsapp_key: Option<String>,
    /// Optional WhatsApp contacts database (`wa.db` / `ContactsV2.sqlite`).
    pub whatsapp_wa: Option<String>,
    /// Optional WhatsApp media directory.
    pub whatsapp_media: Option<String>,
    /// Optional explicit `msgstore.db` path.
    pub whatsapp_db: Option<String>,
    /// WhatsApp Business backup (iPhone only; Android stays false).
    pub is_business_app: Option<bool>,
    /// Continue an interrupted export in the same output directory: previous
    /// output is kept and conversations already written are skipped.
    pub resume: Option<bool>,
    /// The server's attachment size limit, in bytes, as the app read it from
    /// `GET /v1/server` when the Import Run was created. Staging records it
    /// with the run's media settings, where the Staging Review's forecast
    /// and the Media stage read it.
    pub asset_max_bytes: u64,
}

/// Ask this process to parse a phone backup and write conversation files.
///
/// Returns as soon as the background thread starts. Log lines, progress,
/// issues, and the final summary are sent as `desktop-job:log`,
/// `desktop-job:progress`, `desktop-job:issue`, `desktop-job:finished`, and
/// `desktop-job:error`. Each issue goes out the moment the exporter records it,
/// so the window has written it into the run record before an app that
/// closes mid-Staging stops. Output is JSON Lines (one JSON object per line)
/// so the Import screen's Upload can read it later.
///
/// This is where an Import Run's media settings are decided: the mode the
/// person chose and the compress fields are parsed here, before anything is
/// staged, and recorded in the run directory once Staging finishes (see
/// [`run_staging`]). `summarize_staging` and `transcode_staging` read them
/// from there and are never given them again.
///
/// # Errors
///
/// Returns an error if `output_dir` is not a run directory this app made, a
/// form field is invalid, the source is unknown, another job is running, or
/// another thread panicked while holding the shared state lock. Failures
/// during the export itself are sent as `desktop-job:error`, not returned here.
#[tauri::command(async)]
pub fn extract(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    directories: tauri::State<'_, RunDirectories>,
    downloads: tauri::State<'_, ToolDownloads>,
    installed: tauri::State<'_, InstalledTools>,
    app: tauri::AppHandle,
    args: ExtractArgs,
) -> Result<(), String> {
    // The exporter cleans the directory it writes into, so it writes only into
    // one this app made for staging.
    let output_dir = directories.directory(&args.output_dir)?;
    let options = ExtractOptions {
        backup_password: args.backup_password.unwrap_or_default(),
        attachment_media: parse_attachment_media(args.attachment_media.as_deref())?,
        media_max_resolution: parse_max_resolution(args.media_max_resolution.as_deref())?,
        media_max_fps: args.media_max_fps.unwrap_or_else(|| "30".into()),
        media_min_size: args.media_min_size.unwrap_or_else(|| "20".into()),
        obfuscate: args.obfuscate.unwrap_or(false),
        // `Form` trims and drops empty values itself, so the raw strings can
        // pass through unchanged.
        timezone: args.timezone.unwrap_or_default(),
        phone_country: args.phone_country.unwrap_or_default(),
        owner_phones: args.owner_phones.unwrap_or_default(),
        owner_emails: args.owner_emails.unwrap_or_default(),
        attachment_root: args.attachment_root.unwrap_or_default(),
        apple_contacts: args.apple_contacts.unwrap_or_default(),
        whatsapp_key: args.whatsapp_key.unwrap_or_default(),
        whatsapp_wa: args.whatsapp_wa.unwrap_or_default(),
        whatsapp_media: args.whatsapp_media.unwrap_or_default(),
        whatsapp_db: args.whatsapp_db.unwrap_or_default(),
        is_business_app: args.is_business_app.unwrap_or(false),
    };

    let media_settings = media_settings_for(&options, args.asset_max_bytes)?;
    let mut config = build_exporter_config(
        &scratch_dir(&app)?,
        &installed,
        &args.source,
        &args.path,
        &output_dir.display().to_string(),
        &options,
    )?;
    config.output.resume = args.resume.unwrap_or(false);

    let job = start_job(&state, JobName::Staging)?;

    let app_handle = app.clone();
    let cancel = job.cancel_flag();
    config.context.cancel = cancel.clone();
    let downloads = downloads.inner().clone();
    // Two channels, two jobs: log lines are for the person reading the log
    // panel, progress events are for the bar. Nothing reads counts out of
    // the prose.
    // The same lines go into the run's log in the Logs Directory, which
    // outlives the run's directory.
    let run_log = RunLog::open(&logs_dir(&app)?, &output_dir);
    config.context.log = events::run_log_sink(&app_handle, &run_log);
    config.context.progress = events::progress_sink(&app_handle);
    config.context.issues = events::run_issue_sink(&app_handle, &run_log);

    spawn_job(app, job, Some(run_log.clone()), move || {
        // A program still downloading is waited for, and the wait is the
        // Staging row's progress line (#1053).
        super::tools::wait_for_downloads(
            &app_handle,
            &downloads,
            exporters::exporter(&config.source).programs,
            &cancel,
            events::Step::Setup,
        )
        .inspect_err(|error| run_log.error(error))?;
        let run_result = run_staging(&config, &output_dir, &media_settings)
            .inspect_err(|error| run_log.error(error))?;
        let payload = finished_payload(&run_result);
        for line in run_result.messages {
            events::log_to_run(&app_handle, &run_log, line);
        }
        Ok(payload)
    });

    Ok(())
}

/// Form fields from the Import screen after defaults are filled in.
struct ExtractOptions {
    backup_password: String,
    attachment_media: MediaMode,
    media_max_resolution: MaxResolution,
    media_max_fps: String,
    media_min_size: String,
    obfuscate: bool,
    timezone: String,
    phone_country: String,
    owner_phones: Vec<String>,
    owner_emails: Vec<String>,
    attachment_root: String,
    apple_contacts: String,
    whatsapp_key: String,
    whatsapp_wa: String,
    whatsapp_media: String,
    whatsapp_db: String,
    is_business_app: bool,
}

/// Parse the attachment handling choice from the Import form. The UI says
/// "copy" and "skip", which [`MediaMode::parse`] reads as Clone and
/// Disabled.
///
/// # Errors
///
/// Returns an error if the string is not copy, convert, compress, or skip.
fn parse_attachment_media(raw: Option<&str>) -> Result<MediaMode, String> {
    let Some(raw) = raw.and_then(message_ir::trimmed) else {
        return Ok(MediaMode::default());
    };
    MediaMode::parse(raw).ok_or_else(|| {
        format!("invalid attachment_media '{raw}' (expected copy, convert, compress, or skip)")
    })
}

/// Parse the compressed-video resolution cap from the Import form.
///
/// # Errors
///
/// Returns an error if the string is not 720p, 1080p, or 4k.
fn parse_max_resolution(raw: Option<&str>) -> Result<MaxResolution, String> {
    let Some(raw) = raw.and_then(message_ir::trimmed) else {
        return Ok(MaxResolution::default());
    };
    MaxResolution::parse(raw).ok_or_else(|| {
        format!("invalid media_max_resolution '{raw}' (expected 720p, 1080p, or 4k)")
    })
}

/// Build the `CompressOptions` the Media stage will use, from the
/// max-resolution/fps/min-size fields the Import form sends.
///
/// `CompressOptions` only takes effect under [`media::MediaMode::Compress`],
/// so the real options are built only when `Compress` was chosen and
/// `CompressOptions::default()` is returned otherwise. The errors are
/// [`media::compress_options_from_form`]'s, which name the form's fields,
/// because the Import Run shows them to the person as they are.
///
/// # Errors
///
/// Returns an error if `max_fps` is not a positive number or `min_size` is
/// not a whole number of megabytes.
fn parse_compress_options(
    chosen: MediaMode,
    max_resolution: MaxResolution,
    max_fps: &str,
    min_size: &str,
) -> Result<CompressOptions, String> {
    if chosen != MediaMode::Compress {
        return Ok(CompressOptions::default());
    }
    media::compress_options_from_form(max_resolution, max_fps, min_size, true)
        .map_err(|e| format!("{e:#}"))
}

/// The media settings an Import Run works to, decided once when its Staging
/// starts: the mode the person chose, the compress options parsed from the
/// form's fields, and the server's attachment size limit.
///
/// A bad compress field fails here, before anything is staged, rather than
/// at the summary after hours of Staging.
///
/// # Errors
///
/// Returns an error if a compress field is invalid (see
/// [`parse_compress_options`]).
fn media_settings_for(
    options: &ExtractOptions,
    asset_max_bytes: u64,
) -> Result<TranscodeOptions, String> {
    Ok(TranscodeOptions {
        mode: options.attachment_media,
        compress: parse_compress_options(
            options.attachment_media,
            options.media_max_resolution,
            &options.media_max_fps,
            &options.media_min_size,
        )?,
        asset_max_bytes,
    })
}

/// Run the exporter into `output_dir`, then record the run's media settings
/// beside the export sentinel it wrote.
///
/// The record is written after the exporter and not before, because a fresh
/// export refuses a directory that already holds files it does not recognise.
/// A Staging that is interrupted leaves no record. Nothing reads one until
/// Staging finishes, and the resumed Staging writes it.
///
/// # Errors
///
/// Returns an error if the exporter fails or the record cannot be written.
fn run_staging(
    config: &ExporterConfig,
    output_dir: &Path,
    media_settings: &TranscodeOptions,
) -> anyhow::Result<RunResult> {
    let run_result = exporters::run(config)?;
    message_staging::write_media_settings(output_dir, media_settings)?;
    Ok(run_result)
}

/// Build the exporter config the background thread will run, with its
/// scratch data under `scratch_dir`, the Scratch Directory.
///
/// The Import screen's `source` key is looked up in
/// [`exporters::IMPORT_SOURCES`], which names its backup type and fills the
/// shared [`Form`], and the form goes through `Form::to_config`, so the Form
/// builders in message-crate-core are the single source of truth for field
/// mapping and validation. Every path writes JSON Lines (one JSON object per
/// line).
///
/// # Errors
///
/// Returns an error if the source is unknown or `Form::to_config` rejects
/// the form (missing input path, missing owner phones, …). Multiple
/// validation problems are joined with `; `. Staging stages the original
/// attachment files or none ([`AttachmentFiles`]), so the compress fields
/// are not checked here: [`media_settings_for`] checks them, and the Media
/// stage checks ffmpeg against the person's choice. `installed` is passed
/// on, so the WhatsApp exporter finds `wtsexporter` in its Tools Directory.
fn build_exporter_config(
    scratch_dir: &Path,
    installed: &InstalledTools,
    source: &str,
    path: &str,
    output_dir: &str,
    options: &ExtractOptions,
) -> Result<ExporterConfig, String> {
    let mut form = Form {
        output: output_dir.to_string(),
        // Staging copies the originals under Copy, Convert and Compress, and
        // the desktop runs the Media Stage itself after the Staging Review,
        // so converting waits until the person has approved it.
        attachments: if options.attachment_media == MediaMode::Disabled {
            AttachmentFiles::Skip
        } else {
            AttachmentFiles::Originals
        },
        obfuscate: options.obfuscate,
        phone_country: options.phone_country.clone(),
        // Import and Upload read conversation files as JSON Lines (one JSON
        // object per line).
        output_format: OutputFormat::Jsonl,
        ..Default::default()
    };

    let import_source =
        exporters::import_source(source).ok_or_else(|| format!("unsupported source '{source}'"))?;
    (import_source.fill)(&mut form, path, options);

    let ctx = FormContext {
        scratch_dir,
        tools: Arc::new(installed.clone()),
    };
    form.to_config(import_source.exporter, ctx)
        .map_err(|errors| errors.join("; "))
}

#[cfg(test)]
mod tests;
