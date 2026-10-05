//! `format` command — convert an existing extract directory to another format.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use message_crate_core::{
    ExporterConfig, FormatConfig, LogSink, MediaConfig, OutputFormat, SourceConfig,
};

use super::events;
use super::jobs::{spawn_job, start_job};
use super::last_log_line_or;
use super::paths::scratch_dir;
use crate::state::{AppState, JobName};

/// Ask this process to rewrite an extract directory in a different file format.
///
/// Returns as soon as the background thread starts. Log lines and the final
/// summary use the same `extract:log` / `extract:finished` / `extract:error`
/// events as Extract, so the UI can reuse one progress view.
///
/// # Errors
///
/// Returns an error if `output_format` is not one of json, jsonl, csv, eml,
/// mbox, xml, or sms-backup-plus, if another job is running, or if another
/// thread panicked while holding the shared state lock. Failures during
/// conversion are sent as `extract:error`.
#[tauri::command(async)]
pub fn format(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    input_dir: String,
    output_dir: String,
    output_format: String,
    started_from: StartedFrom,
    run_started_ms: Option<i64>,
) -> Result<(), String> {
    let fmt = match output_format.as_str() {
        "json" => OutputFormat::Json,
        "jsonl" => OutputFormat::Jsonl,
        "csv" => OutputFormat::Csv,
        "eml" => OutputFormat::Eml,
        "mbox" => OutputFormat::Mbox,
        "xml" => OutputFormat::Xml,
        "sms-backup-plus" => OutputFormat::SmsBackupPlus,
        _ => return Err(format!("unsupported output format '{output_format}'")),
    };
    let run_started = run_started_ms
        .map(|ms| {
            chrono::DateTime::from_timestamp_millis(ms)
                .ok_or_else(|| format!("{ms} is not a time the Export Run could have started"))
        })
        .transpose()?;

    let scratch_dir = scratch_dir(&app)?;
    let job = start_job(&state, started_from.job_name())?;
    let cancel = job.cancel_flag();

    let app_handle = app.clone();
    spawn_job(app, job, move || {
        let log_app = app_handle.clone();
        let config = ExporterConfig {
            inputs: vec![PathBuf::from(&input_dir)],
            output: PathBuf::from(&output_dir),
            scratch_dir,
            timezone: None,
            obfuscate: Default::default(),
            media: MediaConfig::default(),
            cancel: Some(cancel),
            log: Some(LogSink::new(move |line: &str| {
                events::emit(&log_app, events::LOG, line.to_string());
            })),
            // Settings → Convert shows a log, not a progress bar.
            progress: None,
            issues: None,
            output_format: fmt,
            resume: false,
            source: SourceConfig::Format(FormatConfig { run_started }),
        };

        let run_result = message_reexport::run(&config)?;
        let summary = last_log_line_or(&run_result.messages, "Format conversion complete.");
        for line in run_result.messages {
            events::emit(&app_handle, events::LOG, line);
        }
        Ok(summary)
    });

    Ok(())
}

/// The screen that started a `format` run. It runs for Settings → Convert,
/// and as the second step of every Export other than JSON Lines; a refused
/// start names it as that screen does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartedFrom {
    /// The Export screen, for its format step.
    Export,
    /// Settings → Convert.
    Convert,
}

impl StartedFrom {
    fn job_name(self) -> JobName {
        match self {
            Self::Export => JobName::Export,
            Self::Convert => JobName::Convert,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_name(started_from: &str) -> JobName {
        serde_json::from_value::<StartedFrom>(serde_json::json!(started_from))
            .unwrap()
            .job_name()
    }

    #[test]
    fn a_format_run_inside_an_export_is_named_export() {
        assert_eq!(job_name("export"), JobName::Export);
    }

    #[test]
    fn a_format_run_from_settings_is_named_convert() {
        assert_eq!(job_name("convert"), JobName::Convert);
    }
}
