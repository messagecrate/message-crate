//! What the Media part of Settings → System shows: where ffmpeg, ffprobe and
//! wtsexporter are, that one is missing, or how its download stands
//! ([`crate::tool_downloads`]).
//!
//! ffmpeg and ffprobe are looked for on `PATH`, then in the Tools Directory;
//! wtsexporter only in the Tools Directory (#1053). The window names no
//! directory: the lookup is this process's, and nothing it is told changes
//! where a program is run from.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use message_crate_core::{CancelFlag, Cancelled};
use tauri::AppHandle;

use super::events::{self, ExtractProgressEvent};
use crate::tool_downloads::{self, DownloadState, Program, ToolDownloads, WaitError};

/// Where one program is, as Settings shows it.
///
/// Tagged by `state`, one tag per state, so a state can be added beside the
/// others without changing them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ToolStatus {
    /// The program was found, at `path`.
    Found {
        /// Where it is.
        path: String,
    },
    /// The program is on neither place it is looked for.
    Missing,
    /// The program is on neither place it is looked for, and the app has no
    /// download of it for this computer (wtsexporter on Linux on ARM), so
    /// Try again can't bring it. A copy put in the Tools Directory by hand
    /// is used.
    Unavailable,
    /// The program was found but is not used, for `reason`.
    Unusable {
        /// Why it is not used.
        reason: String,
    },
    /// The program is downloading into the Tools Directory.
    Downloading {
        /// Bytes received so far.
        received: u64,
        /// The file's size, `None` when the server did not say.
        total: Option<u64>,
    },
    /// The program's download failed, for `reason`, and the program is not
    /// found. It is tried again the next time the app starts.
    DownloadFailed {
        /// Why the download failed.
        reason: String,
    },
}

impl ToolStatus {
    /// The status of a program found at `path`, or missing.
    fn of(path: Option<PathBuf>) -> Self {
        match path {
            Some(path) => Self::Found {
                path: path.display().to_string(),
            },
            None => Self::Missing,
        }
    }

    /// This status, with a missing program the app has no download for
    /// (`pinned` is false) told apart as unavailable.
    fn with_pin(self, pinned: bool) -> Self {
        match self {
            Self::Missing if !pinned => Self::Unavailable,
            status => status,
        }
    }

    /// This status as `download` changes it. A download in progress is
    /// shown whatever was found, because the file is about to change. A
    /// failed one is shown only when the program is missing: a program found
    /// is still used, and a program not used says why, which is the cause a
    /// person must fix (two places, or no permission to run).
    fn with_download(self, download: Option<DownloadState>) -> Self {
        match (download, self) {
            (Some(DownloadState::Downloading { received, total }), _) => {
                Self::Downloading { received, total }
            }
            (Some(DownloadState::Failed { reason }), Self::Missing) => {
                Self::DownloadFailed { reason }
            }
            (_, status) => status,
        }
    }
}

/// Where each program the app runs is, and the Tools Directory it keeps them in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsStatus {
    /// The Tools Directory, `None` when the app has none (no home directory).
    pub tools_dir: Option<String>,
    /// Whether a check of the Tools Directory runs in this process now, the
    /// start-up check or Try again. A program it has not looked at yet shows
    /// as missing, so the window keeps asking while this is true.
    pub checking: bool,
    /// ffmpeg, from `PATH` or the Tools Directory.
    pub ffmpeg: ToolStatus,
    /// ffprobe, from `PATH` or the Tools Directory.
    pub ffprobe: ToolStatus,
    /// wtsexporter, from the Tools Directory only.
    pub wtsexporter: ToolStatus,
}

/// Look for ffmpeg, ffprobe and wtsexporter where this process runs them from,
/// and say how each one's download stands.
#[tauri::command]
pub fn tools_status(downloads: tauri::State<'_, ToolDownloads>) -> ToolsStatus {
    tools_status_with(&downloads, &pinned_programs())
}

/// The programs the app has a download of for this computer.
fn pinned_programs() -> Vec<Program> {
    tool_downloads::pinned_for(std::env::consts::OS, std::env::consts::ARCH)
        .into_iter()
        .map(|pin| pin.program)
        .collect()
}

/// Check the Tools Directory again and download what is missing, as **Try
/// again** on the Import form asks (#1053). Returns at once: the check runs
/// on a thread of its own, and the window follows it through
/// [`tools_status`].
///
/// Returns whether a check started. `false` in three cases:
///
/// - a check was already running (the start-up check, an earlier Try again,
///   or another app's), and the window follows it through `checking`;
/// - the app has no Tools Directory (no home directory), which the status
///   shows as `toolsDir: null`;
/// - the check's thread could not be started, and each program it would
///   have downloaded shows as a failed download with that reason.
#[tauri::command]
pub fn retry_tool_downloads(downloads: tauri::State<'_, ToolDownloads>) -> bool {
    let Some(dir) = media::tools_dir() else {
        return false;
    };
    let status = tools_status_with(&downloads, &pinned_programs());
    let not_found: Vec<Program> = [
        (Program::Ffmpeg, &status.ffmpeg),
        (Program::Ffprobe, &status.ffprobe),
        (Program::Wtsexporter, &status.wtsexporter),
    ]
    .into_iter()
    .filter(|(_, status)| !matches!(status, ToolStatus::Found { .. }))
    .map(|(program, _)| program)
    .collect();
    let pinned = tool_downloads::pinned_for(std::env::consts::OS, std::env::consts::ARCH);
    tool_downloads::retry(
        dir,
        tool_downloads::GITHUB.to_string(),
        pinned,
        &downloads,
        &not_found,
    )
    .is_some()
}

/// Wait while any of `programs` is downloading, before an import runs it
/// (#1053). Each change of the wait goes to the window as an
/// `extract:progress` event on `step`, whose status is the progress line:
/// "Waiting for the wtsexporter download (12.0 MB of 30.0 MB)".
///
/// # Errors
///
/// Returns [`Cancelled`] when the run is cancelled while it waits, and the
/// failed download's reason when a download waited for fails.
pub(crate) fn wait_for_downloads(
    app: &AppHandle,
    downloads: &ToolDownloads,
    programs: &[Program],
    cancel: &CancelFlag,
    step: &str,
) -> anyhow::Result<()> {
    let mut last: Option<String> = None;
    downloads
        .wait_for(
            programs,
            &|| cancel.load(Ordering::Relaxed),
            &mut |program, received, total| {
                let line = tool_downloads::waiting_line(program, received, total);
                if last.as_deref() == Some(line.as_str()) {
                    return;
                }
                events::emit(app, events::PROGRESS, waiting_event(step, &line));
                last = Some(line);
            },
        )
        .map_err(|err| match err {
            WaitError::Cancelled => anyhow::Error::new(Cancelled),
            failed @ WaitError::Failed { .. } => anyhow::Error::new(failed),
        })
}

/// The `extract:progress` event of a run waiting for a download: no counts,
/// and the progress line as its status, which the window shows as it is.
fn waiting_event(step: &str, line: &str) -> ExtractProgressEvent {
    ExtractProgressEvent {
        step: step.to_string(),
        done: 0,
        total: 0,
        bytes_done: None,
        bytes_total: None,
        status: Some(line.to_string()),
    }
}

/// [`tools_status`], with the downloads in `downloads` and a download for
/// this computer of each of `pinned`.
fn tools_status_with(downloads: &ToolDownloads, pinned: &[Program]) -> ToolsStatus {
    let status = |program: Program, found: ToolStatus| {
        found
            .with_pin(pinned.contains(&program))
            .with_download(downloads.get(program))
    };
    let (ffmpeg, ffprobe) = match media::ffmpeg_tools() {
        Ok(tools) => (ToolStatus::of(tools.ffmpeg), ToolStatus::of(tools.ffprobe)),
        // Found in two places: neither is used, and both say why.
        Err(err) => {
            let unusable = ToolStatus::Unusable {
                reason: err.to_string(),
            };
            (unusable.clone(), unusable)
        }
    };
    let wtsexporter = match whatsapp_exporter::wtsexporter_path() {
        Ok(path) => ToolStatus::of(path),
        Err(err) => ToolStatus::Unusable {
            reason: err.to_string(),
        },
    };
    ToolsStatus {
        tools_dir: media::tools_dir().map(|dir| dir.display().to_string()),
        checking: downloads.checking(),
        ffmpeg: status(Program::Ffmpeg, ffmpeg),
        ffprobe: status(Program::Ffprobe, ffprobe),
        wtsexporter: status(Program::Wtsexporter, wtsexporter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window reads `state` and, for a found program, `path`.
    #[test]
    fn a_status_is_sent_as_its_state_and_path() {
        let status = ToolsStatus {
            tools_dir: Some("/home/sam/message-crate/tools".into()),
            checking: true,
            ffmpeg: ToolStatus::of(Some(PathBuf::from("/usr/bin/ffmpeg"))),
            ffprobe: ToolStatus::of(Some(PathBuf::from("/usr/bin/ffprobe"))),
            wtsexporter: ToolStatus::of(None),
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            serde_json::json!({
                "toolsDir": "/home/sam/message-crate/tools",
                "checking": true,
                "ffmpeg": { "state": "found", "path": "/usr/bin/ffmpeg" },
                "ffprobe": { "state": "found", "path": "/usr/bin/ffprobe" },
                "wtsexporter": { "state": "missing" },
            })
        );
    }

    /// A program found but not used carries its reason, which the window shows.
    #[test]
    fn an_unusable_program_is_sent_with_its_reason() {
        let status = ToolStatus::Unusable {
            reason: "ffmpeg is on PATH at /usr/bin/ffmpeg and ffprobe is elsewhere.".into(),
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            serde_json::json!({
                "state": "unusable",
                "reason": "ffmpeg is on PATH at /usr/bin/ffmpeg and ffprobe is elsewhere.",
            })
        );
    }

    /// wtsexporter in the Tools Directory is reported where it is, and a
    /// Tools Directory without it reports it missing.
    #[test]
    fn wtsexporter_is_reported_from_the_tools_directory() {
        let _lock = media::testutil::tools_test_lock();
        let previous = media::tools_dir();
        let tools = tempfile::tempdir().unwrap();
        let name = whatsapp_exporter::wtsexporter_file_name();
        media::set_tools_dir(Some(tools.path().to_path_buf()));
        let downloads = ToolDownloads::default();
        let pinned = [Program::Wtsexporter];
        let missing = tools_status_with(&downloads, &pinned).wtsexporter;
        let unavailable = tools_status_with(&downloads, &[]).wtsexporter;
        media::testutil::write_with_mode(&tools.path().join(name), 0o755);
        let found = tools_status_with(&downloads, &pinned).wtsexporter;
        let found_by_hand = tools_status_with(&downloads, &[]).wtsexporter;
        media::set_tools_dir(previous);

        assert_eq!(missing, ToolStatus::Missing);
        // No download for this computer: told apart, so the form offers no
        // Try again; a copy put there by hand is found all the same.
        assert_eq!(unavailable, ToolStatus::Unavailable);
        assert_eq!(
            serde_json::to_value(&unavailable).unwrap(),
            serde_json::json!({ "state": "unavailable" })
        );
        assert_eq!(found_by_hand, found);
        assert_eq!(
            found,
            ToolStatus::Found {
                path: tools.path().join(name).display().to_string()
            }
        );
    }

    /// A download in progress is sent with its bytes so far and its total,
    /// and a failed one with its reason, in the shape the window reads
    /// (`ToolStatus` in `web/src/lib/tauri.ts`).
    #[test]
    fn a_download_is_sent_as_its_progress_or_its_failure() {
        let downloading = ToolStatus::Missing.with_download(Some(DownloadState::Downloading {
            received: 1024,
            total: Some(4096),
        }));
        assert_eq!(
            serde_json::to_value(&downloading).unwrap(),
            serde_json::json!({ "state": "downloading", "received": 1024, "total": 4096 })
        );
        let unknown_total = ToolStatus::Missing.with_download(Some(DownloadState::Downloading {
            received: 10,
            total: None,
        }));
        assert_eq!(
            serde_json::to_value(&unknown_total).unwrap(),
            serde_json::json!({ "state": "downloading", "received": 10, "total": null })
        );
        let failed = ToolStatus::Missing.with_download(Some(DownloadState::Failed {
            reason: "The download's server answered 404 Not Found.".into(),
        }));
        assert_eq!(
            serde_json::to_value(&failed).unwrap(),
            serde_json::json!({
                "state": "downloadFailed",
                "reason": "The download's server answered 404 Not Found.",
            })
        );
    }

    /// A program not used keeps its reason when its download failed, because
    /// that reason is what a person must fix.
    #[test]
    fn an_unusable_program_keeps_its_reason_over_a_failed_download() {
        let unusable = ToolStatus::Unusable {
            reason: "ffmpeg is on PATH at /usr/bin/ffmpeg and ffprobe is elsewhere.".into(),
        };
        assert_eq!(
            unusable.clone().with_download(Some(DownloadState::Failed {
                reason: "no network".into()
            })),
            unusable
        );
    }

    /// A program found stays found when its download failed, because it is
    /// still the one used; a download in progress is shown over it.
    #[test]
    fn a_found_program_hides_a_failed_download_but_not_one_in_progress() {
        let found = || ToolStatus::Found {
            path: "/home/sam/message-crate/tools/wtsexporter".into(),
        };
        assert_eq!(
            found().with_download(Some(DownloadState::Failed {
                reason: "no network".into()
            })),
            found()
        );
        assert_eq!(
            found().with_download(Some(DownloadState::Downloading {
                received: 1,
                total: None
            })),
            ToolStatus::Downloading {
                received: 1,
                total: None
            }
        );
    }
}
