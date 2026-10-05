//! `summarize_staging`, `transcode_staging`, `delete_staging`, and the
//! Import Run record commands.
//!
//! These back the two reviews a staged import stops at:
//! `summarize_staging` recomputes what a staged directory holds so the first
//! review can show it, `transcode_staging` runs the Media stage the
//! exporter deferred (see `extract::exporter_attachment_media`), and
//! `delete_staging` removes the run directory — when a review is closed
//! without approving, when a paused run is discarded, and when the server
//! records the run as finished, since nothing will read the directory again.
//! `read_import_run_record` and `save_import_run_record` keep the record of a
//! paused run's earlier parts in its directory ([`RUN_RECORD_NAME`]).
//!
//! `summarize_staging` and `transcode_staging` take only the directory. They
//! read the run's media settings from it, where `extract` recorded them
//! ([`message_staging::read_media_settings`]), so a summary, the Media stage it
//! forecasts, and the Staging before them all work to the one set of values
//! the Import Run was started with.
//!
//! ## Which directories these commands act on
//!
//! The desktop process owns the Staging Directory and the directories made
//! under it ([`StagingDirectories`]). `set_staging_root` stores the setting,
//! `create_staging_dir` makes a run's directory, and every other command takes
//! only the directory. A directory is acted on when `create_staging_dir` made it
//! and it still holds the `.message-crate-export` sentinel, wherever the
//! Staging Directory points now, so changing the setting never strands a run
//! that started under the earlier one (#1154).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use message_staging::{StagingSummary, TranscodeOptions, TranscodeReport};

use super::events;
use super::events::ExtractProgressEvent;
use super::jobs::{spawn_job, start_job};
use crate::app_directories::RunLog;
use crate::staging_directories::{self, StagingDirectories, StagingRoot};
use crate::state::AppState;

/// The directory `summarize_staging`, `transcode_staging`, `delete_staging`
/// and the Import Run record commands act on.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingArgs {
    /// run directory `create_staging_dir` made.
    pub staging_dir: String,
}

/// The Staging Directory and its default, for Settings.
///
/// # Errors
///
/// Returns an error when the operating system reports no home directory.
#[tauri::command]
pub fn staging_root(
    directories: tauri::State<'_, StagingDirectories>,
) -> Result<StagingRoot, String> {
    directories.describe()
}

/// Store the Staging Directory from Settings. An empty value goes back to
/// the default. The new directory applies to run directories made after this;
/// those made before it keep working.
///
/// # Errors
///
/// Returns an error when the directory is relative or the filesystem root, or
/// the setting cannot be saved.
#[tauri::command]
pub fn set_staging_root(
    directories: tauri::State<'_, StagingDirectories>,
    root: String,
) -> Result<StagingRoot, String> {
    directories.set_root(&root)?;
    directories.describe()
}

/// The log of the Import Run whose directory is `staging_dir`, in the Logs
/// Directory. The log is there once the run's Upload has started, and stays
/// after the run's directory is deleted.
///
/// # Errors
///
/// Returns an error when the operating system names no app-data directory.
#[tauri::command]
pub fn import_run_log(app: tauri::AppHandle, staging_dir: String) -> Result<String, String> {
    let logs = super::paths::logs_dir(&app)?;
    Ok(
        crate::app_directories::import_run_log(&logs, std::path::Path::new(staging_dir.trim()))
            .display()
            .to_string(),
    )
}

/// Make a new run directory under the Staging Directory and return its
/// path. `label` is the Import source.
///
/// # Errors
///
/// Returns an error when the label cannot name a directory, the Staging
/// Directory is unusable, or the directory cannot be made.
#[tauri::command(async)]
pub fn create_staging_dir(
    directories: tauri::State<'_, StagingDirectories>,
    label: String,
) -> Result<String, String> {
    let directory = directories.create(&label, &staging_directories::timestamp_now())?;
    Ok(directory.display().to_string())
}

/// The staged directory, checked by [`StagingDirectories::directory`], and the media
/// settings its Staging recorded there.
///
/// # Errors
///
/// Returns an error when the directory fails the check, or holds no readable
/// media settings because its Staging never finished.
fn staged_directory(
    directories: &StagingDirectories,
    staging_dir: &str,
) -> Result<(PathBuf, TranscodeOptions), String> {
    let staging_dir = directories.directory(staging_dir)?;
    let options =
        message_staging::read_media_settings(&staging_dir).map_err(|error| format!("{error:#}"))?;
    Ok((staging_dir, options))
}

/// Recompute what a staged directory holds, for the first review.
///
/// Reports progress on `extract:progress` with `step: "check"`, so a long
/// summary of a huge directory shows movement on the step the user is already
/// looking at. The read itself (directory walk plus ffprobe calls) runs on a
/// blocking-pool thread via [`tauri::async_runtime::spawn_blocking`], so it
/// cannot stall the async runtime other commands share.
///
/// # Errors
///
/// Returns an error if `staging_dir` is not a run directory this app made,
/// holds no media settings, cannot be read, or the blocking task panicked.
#[tauri::command]
pub async fn summarize_staging(
    app: tauri::AppHandle,
    directories: tauri::State<'_, StagingDirectories>,
    args: StagingArgs,
) -> Result<StagingSummary, String> {
    let (staging_dir, options) = staged_directory(&directories, &args.staging_dir)?;

    let progress_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        message_staging::summarize_staging(&staging_dir, &options, &mut |progress| {
            events::emit(
                &progress_app,
                events::PROGRESS,
                ExtractProgressEvent {
                    step: "check".into(),
                    done: progress.done,
                    total: progress.total,
                    bytes_done: None,
                    bytes_total: None,
                    status: None,
                },
            );
        })
        .map_err(|error| format!("{error:#}"))
    })
    .await
    .map_err(|join_error| format!("summarize_staging did not complete: {join_error}"))?
}

/// `count == 1` ? "" : "s" — the only pluralization these summaries need.
fn plural_s(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// One human-readable sentence describing the Media stage's outcome.
///
/// Used both as the `extract:finished` payload's `summary` field (so a
/// client that falls back to raw JSON still has readable text) and, when
/// either count is nonzero, as an `extract:log` line so the same wording is
/// visible while the Media stage runs, not only after it finishes.
///
/// `too_large` and `failed` get separate clauses on purpose: a `too_large`
/// file WAS converted — it just came out over the limit and will not be
/// uploaded — which is a different fact from `failed`, a file the Media stage could
/// not convert at all. The report has no per-file reasons (those are written
/// into the conversation files' `missing_reason` instead), so this can only
/// speak in counts.
fn transcode_summary(report: &TranscodeReport) -> String {
    let mut clauses = vec![format!(
        "Converted {n} file{s}",
        n = report.converted,
        s = plural_s(report.converted)
    )];
    if report.too_large > 0 {
        clauses.push(format!(
            "{n} will not be uploaded (still too large after conversion)",
            n = report.too_large
        ));
    }
    if report.failed > 0 {
        clauses.push(format!(
            "{n} could not be converted; details are recorded in the staged files",
            n = report.failed
        ));
    }
    format!("{}.", clauses.join("; "))
}

/// Run the Media stage over a staged directory, after the Staging Review
/// approves it.
///
/// Follows `extract`'s job shape: the job starts through [`start_job`], with
/// a cancel flag of its own, the Media stage runs on a background thread, and
/// progress/log/finished go back as `extract:*` events so the UI reuses one
/// progress view. A cancelled Media stage is reported through `extract:error` the
/// same way any other failure is — exactly how a cancelled `extract` run
/// already behaves (`extract` never special-cases its own cancellation
/// either; `spawn_job`'s generic `Err` handling covers both). An earlier
/// version of this command ended a cancelled Media stage quietly instead (an
/// `extract:log` line, `Ok(())`, no `extract:error`); that left
/// `awaitTauriJob`'s promise on the web side permanently unsettled — no
/// `extract:finished`, no `extract:error` — wedging the screen with `running`
/// stuck true and no way back except restarting the app. Do not restore the
/// quiet path.
///
/// Each file the Media stage could not convert goes to the window as an
/// `extract:issue` the moment the Media stage gives up on it, so the window has
/// written it into the run record before an app that closes during the Media stage stops.
/// The report carries counts only, so a nonzero `failed`/`too_large` count
/// is also surfaced as one summarizing `extract:log` line (see
/// [`transcode_summary`]).
///
/// # Errors
///
/// Returns an error if `staging_dir` is not a run directory this app made
/// or holds no media settings, another job is running, or another thread
/// panicked while holding the shared state lock. Failures during the Media stage —
/// including a cancellation and ffmpeg/ffprobe being unavailable — are sent
/// as `extract:error`, verbatim, not returned here.
#[tauri::command(async)]
pub fn transcode_staging(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    directories: tauri::State<'_, StagingDirectories>,
    app: tauri::AppHandle,
    args: StagingArgs,
) -> Result<(), String> {
    let (staging_dir, options) = staged_directory(&directories, &args.staging_dir)?;
    let run_log = RunLog::open(&super::paths::logs_dir(&app)?, &staging_dir);
    let job = start_job(&state, "the Media stage")?;
    let cancel = job.cancel_flag();
    let media_stage_converts = matches!(
        options.mode,
        media::MediaMode::Convert | media::MediaMode::Compress
    );

    let app_handle = app.clone();
    let issues = events::issue_sink(&app);
    spawn_job(app, job, move || {
        if media_stage_converts {
            events::log_to_run(
                &app_handle,
                &run_log,
                "Converting and compressing attachments…".to_string(),
            );
        }

        // Not `move`: the closure only needs `&app_handle` (`emit` takes
        // `&self`), so it borrows the one clone above rather than needing a
        // second — the same handle is still available by reference below,
        // once this borrow ends at the end of the `transcode_staged` call.
        let outcome = message_staging::transcode_staged(
            &staging_dir,
            &options,
            Some(&cancel),
            Some(&issues),
            &mut |progress| {
                events::emit(
                    &app_handle,
                    events::PROGRESS,
                    ExtractProgressEvent {
                        step: "media".into(),
                        done: progress.done,
                        total: progress.total,
                        bytes_done: None,
                        bytes_total: None,
                        status: None,
                    },
                );
            },
        );

        // A cancellation is just another `Err` here — `spawn_job` reports it
        // as `extract:error` with the error chain as `detail`, the same
        // generic path a cancelled `extract` run already goes through. See
        // this function's doc comment for why the earlier quiet-cancel
        // special case was removed.
        let report = outcome.inspect_err(|error| run_log.error(error))?;

        let summary = transcode_summary(&report);
        run_log.line(&summary);
        if report.failed > 0 || report.too_large > 0 {
            events::emit(&app_handle, events::LOG, summary.clone());
        }

        let payload = serde_json::json!({
            "summary": summary,
            "converted": report.converted,
            "skipped": report.skipped,
            "too_large": report.too_large,
            "failed": report.failed,
            "missing": report.missing,
            "repointed": report.repointed,
            "bytes_before": report.bytes_before,
            "bytes_after": report.bytes_after,
        });
        Ok(payload.to_string())
    });

    Ok(())
}

/// Delete a run directory: the decline path's terminal action (Decision
/// 16), and the last action of a successful import, whose staged copy of the
/// messages, Upload log, journal and report the server has no further use for.
///
/// Runs on the async task pool (`#[tauri::command(async)]`) rather than the
/// main thread: `remove_dir_all` over a large run directory would otherwise
/// freeze the window. A directory no longer on disk counts as deleted: the
/// decline path may run after a crash that already removed it.
///
/// # Errors
///
/// Returns an error when `staging_dir` is not a run directory this app made,
/// or the directory cannot be removed. Refuses rather than silently doing
/// nothing, so a path bug here cannot turn into a delete somewhere else on
/// disk.
#[tauri::command(async)]
pub fn delete_staging(
    directories: tauri::State<'_, StagingDirectories>,
    args: StagingArgs,
) -> Result<(), String> {
    directories.delete(&args.staging_dir)
}

/// File in a run directory holding the Import Run's record so far: the
/// Import Errors, timings and counts of the parts that ran before the run
/// paused or stopped at a Review.
///
/// A paused run posts no completion, so the server never sees what its
/// earlier parts recorded. The window writes the record here, beside the
/// Upload's journal, and reads it back when the run resumes, so the completion
/// it finally posts covers the whole run. The leading dot keeps it out of
/// every listing of conversation files, and it is deleted with the directory.
/// It has no `.json` extension, for the reason the media settings file has
/// none (`message_staging::MEDIA_SETTINGS_FILE`): a fresh export into the
/// directory would delete it as an earlier run's output.
pub const RUN_RECORD_NAME: &str = ".message-crate-run";

/// Arguments for [`save_import_run_record`].
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRunRecordArgs {
    /// run directory of the Import Run.
    pub staging_dir: String,
    /// The record as the window builds it. Its shape belongs to the window.
    pub record: serde_json::Value,
}

/// Read the Import Run's record from its run directory.
///
/// Returns `None` when the directory holds no record: a run that has not yet
/// paused or stopped at a Review has not written one.
///
/// # Errors
///
/// Returns an error when the directory is not a run directory this app made,
/// or the record cannot be read or is not JSON.
#[tauri::command(async)]
pub fn read_import_run_record(
    directories: tauri::State<'_, StagingDirectories>,
    args: StagingArgs,
) -> Result<Option<serde_json::Value>, String> {
    let directory = directories.directory(&args.staging_dir)?;
    read_run_record(&directory)
}

/// Write the Import Run's record into its run directory, replacing the one
/// there.
///
/// # Errors
///
/// Returns an error when the directory is not a run directory this app made,
/// or the file cannot be written.
#[tauri::command(async)]
pub fn save_import_run_record(
    directories: tauri::State<'_, StagingDirectories>,
    args: SaveRunRecordArgs,
) -> Result<(), String> {
    let directory = directories.directory(&args.staging_dir)?;
    save_run_record(&directory, &args.record)
}

/// The work of [`read_import_run_record`], in a directory already checked.
fn read_run_record(directory: &Path) -> Result<Option<serde_json::Value>, String> {
    let path = directory.join(RUN_RECORD_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("{} is not readable: {error}", path.display()))
}

/// The work of [`save_import_run_record`], in a directory already checked. It
/// writes a temporary file and renames it over the record, so a crash
/// mid-write leaves the previous record whole.
fn save_run_record(directory: &Path, record: &serde_json::Value) -> Result<(), String> {
    let path = directory.join(RUN_RECORD_NAME);
    let tmp = directory.join(format!("{RUN_RECORD_NAME}.tmp"));
    let body = serde_json::to_vec(record).map_err(|error| error.to_string())?;
    std::fs::write(&tmp, body)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|error| format!("Could not write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staging_directories::Scratch;
    use media::MediaMode;

    /// A record of run directories in its own temporary directories, and one
    /// directory made in it.
    struct Made {
        directories: StagingDirectories,
        run: PathBuf,
        _scratch: Scratch,
    }

    fn made() -> Made {
        let scratch = Scratch::new();
        let run = scratch
            .directories
            .create("imessage-ios", "261002-101500")
            .unwrap();
        Made {
            directories: scratch.reopen(),
            run,
            _scratch: scratch,
        }
    }

    #[test]
    fn a_staged_directory_is_read_with_the_settings_its_staging_recorded() {
        // The summary and the Media stage are given only the directory, so they
        // cannot be handed a mode the run did not start with (#1153).
        let made = made();
        let recorded = TranscodeOptions {
            mode: MediaMode::Compress,
            compress: media::CompressOptions {
                max_fps: 24.0,
                ..Default::default()
            },
            asset_max_bytes: 123_456_789,
        };
        message_staging::write_media_settings(&made.run, &recorded).unwrap();

        let (dir, options) =
            staged_directory(&made.directories, made.run.to_str().unwrap()).unwrap();

        assert_eq!(dir, made.run);
        assert_eq!(options, recorded);
    }

    #[test]
    fn a_paused_run_resumes_and_is_deleted_after_the_staging_directory_changes() {
        // Issue #1154, through what a resume runs: the run record is read,
        // the staged directory is checked for the summary and the Media stage,
        // and the directory is deleted, all after the setting moved.
        let made = made();
        let options = TranscodeOptions {
            mode: MediaMode::Compress,
            compress: media::CompressOptions::default(),
            asset_max_bytes: 1,
        };
        message_staging::write_media_settings(&made.run, &options).unwrap();
        let record = serde_json::json!({ "phase": "staging_review" });
        save_run_record(&made.run, &record).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        made.directories
            .set_root(elsewhere.path().to_str().unwrap())
            .unwrap();
        let run = made.run.to_str().unwrap();

        let directory = made.directories.directory(run).unwrap();
        assert_eq!(read_run_record(&directory).unwrap(), Some(record));
        let (dir, _) = staged_directory(&made.directories, run).unwrap();
        assert_eq!(dir, made.run);
        made.directories.delete(run).unwrap();
        assert!(!made.run.exists());
    }

    #[test]
    fn a_directory_whose_staging_recorded_no_settings_is_refused() {
        let made = made();

        let err = staged_directory(&made.directories, made.run.to_str().unwrap()).unwrap_err();

        assert!(err.contains("media settings"), "{err}");
    }

    #[test]
    fn transcode_summary_gives_too_large_and_failed_separate_clauses() {
        // A too_large file WAS converted (it just won't be uploaded); a
        // failed file was not converted at all. Lumping them into one
        // "could not be converted" count would misstate the too_large ones.
        let report = TranscodeReport {
            converted: 12,
            too_large: 2,
            failed: 1,
            ..Default::default()
        };
        let summary = transcode_summary(&report);
        assert!(summary.contains("Converted 12 files"), "{summary}");
        assert!(summary.contains("2 will not be uploaded"), "{summary}");
        assert!(!summary.contains("2 could not be converted"), "{summary}");
        assert!(summary.contains("1 could not be converted"), "{summary}");
    }

    #[test]
    fn transcode_summary_with_no_issues_is_just_the_converted_count() {
        let report = TranscodeReport {
            converted: 5,
            ..Default::default()
        };
        assert_eq!(transcode_summary(&report), "Converted 5 files.");
    }

    #[test]
    fn a_saved_run_record_reads_back_as_written() {
        let made = made();
        let record = serde_json::json!({ "issues": [{ "kind": "skip" }], "uploadMs": 1200 });

        save_run_record(&made.run, &record).unwrap();

        assert_eq!(read_run_record(&made.run).unwrap(), Some(record));
        assert!(
            !made.run.join(format!("{RUN_RECORD_NAME}.tmp")).exists(),
            "the temporary file is renamed over the record"
        );
    }

    #[test]
    fn a_directory_with_no_run_record_reads_as_none() {
        let made = made();

        assert_eq!(read_run_record(&made.run), Ok(None));
    }
}
