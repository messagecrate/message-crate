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

use super::events::{self, ImportProgressEvent, Step};
use crate::tool_downloads::{self, DownloadState, Pinned, Program, ToolDownloads, WaitError};

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
    /// found. It is tried again the next time the app starts, or with Try
    /// again on the Import form.
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
    /// as missing, so the window keeps asking while this is true. Another
    /// app's check on the same Tools Directory is not counted: Try again
    /// reports it as [`RetryResult::AlreadyRunning`].
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
    tools_status_with(&downloads, &pinned_here())
}

/// The downloads the app has for this computer.
fn pinned_here() -> Vec<Pinned> {
    tool_downloads::pinned_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// What Try again did, as [`retry_tool_downloads`] tells the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RetryResult {
    /// A check started in this process, and `checking` is true until it ends.
    Started,
    /// Another check holds the lock on the Tools Directory: the start-up
    /// check, an earlier Try again, or another app's check. Nothing started.
    /// `checking` is true only when that check runs in this process, so the
    /// window can't tell from the status when another app's check ends.
    AlreadyRunning,
    /// The app has no Tools Directory (no home directory), which the status
    /// shows as `toolsDir: null`.
    NoToolsDirectory,
    /// The check's thread could not be started, and each program it would
    /// have downloaded shows as a failed download with that reason.
    CouldNotStart,
}

/// Check the Tools Directory again and download what is missing, as **Try
/// again** on the Import form asks (#1053). Returns at once: the check runs
/// on a thread of its own, and the window follows it through
/// [`tools_status`].
#[tauri::command]
pub fn retry_tool_downloads(downloads: tauri::State<'_, ToolDownloads>) -> RetryResult {
    let Some(dir) = media::tools_dir() else {
        return RetryResult::NoToolsDirectory;
    };
    let pinned = pinned_here();
    let status = tools_status_with(&downloads, &pinned);
    let not_found: Vec<Program> = [
        (Program::Ffmpeg, &status.ffmpeg),
        (Program::Ffprobe, &status.ffprobe),
        (Program::Wtsexporter, &status.wtsexporter),
    ]
    .into_iter()
    .filter(|(_, status)| !matches!(status, ToolStatus::Found { .. }))
    .map(|(program, _)| program)
    .collect();
    match tool_downloads::retry(
        dir,
        tool_downloads::GITHUB.to_string(),
        pinned,
        &downloads,
        &not_found,
    ) {
        // Nothing joins the check's thread: the window follows the check
        // through `tools_status`.
        tool_downloads::Retry::Started(_thread) => RetryResult::Started,
        tool_downloads::Retry::AlreadyRunning => RetryResult::AlreadyRunning,
        tool_downloads::Retry::CouldNotStart => RetryResult::CouldNotStart,
    }
}

/// How far a download waited for moves before the window hears of it again,
/// so a download heard of at every 64 KiB read doesn't send an event each.
const WAITING_STEP_BYTES: u64 = 256 * 1024;

/// Wait while any of `programs` is downloading, before an import runs it
/// (#1053). The wait goes to the window as `desktop-job:progress` events on
/// `step` ([`ImportProgressEvent::waiting`]): one when it starts, and one
/// each time the program waited for changes or its download moves by
/// [`WAITING_STEP_BYTES`]. The window writes the progress line from them,
/// in the byte format the Import form uses.
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
    step: Step,
) -> anyhow::Result<()> {
    let mut last: Option<(Program, u64, Option<u64>)> = None;
    downloads
        .wait_for(
            programs,
            &|| cancel.load(Ordering::Relaxed),
            &mut |program, received, total| {
                let now = (program, received, total);
                if !worth_sending(last, now) {
                    return;
                }
                events::emit(
                    app,
                    events::PROGRESS,
                    ImportProgressEvent::waiting(step, program, received, total),
                );
                last = Some(now);
            },
        )
        .map_err(|err| match err {
            WaitError::Cancelled => anyhow::Error::new(Cancelled),
            failed @ WaitError::Failed { .. } => anyhow::Error::new(failed),
        })
}

/// Whether the wait at `now` is news to the window, which last heard `last`.
fn worth_sending(
    last: Option<(Program, u64, Option<u64>)>,
    now: (Program, u64, Option<u64>),
) -> bool {
    let Some((program, received, total)) = last else {
        return true;
    };
    program != now.0 || total != now.2 || now.1.abs_diff(received) >= WAITING_STEP_BYTES
}

/// [`tools_status`], with the downloads in `downloads` and `pinned` as the
/// downloads for this computer. A wtsexporter counts only when it is the
/// pinned one, where there is one (#2692).
fn tools_status_with(downloads: &ToolDownloads, pinned: &[Pinned]) -> ToolsStatus {
    let pin = |program: Program| pinned.iter().find(|pin| pin.program == program);
    let status = |program: Program, found: ToolStatus| {
        found
            .with_pin(pin(program).is_some())
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
    let wtsexporter = match whatsapp_exporter::wtsexporter_in_dir(
        media::tools_dir().as_deref(),
        pin(Program::Wtsexporter).map(|pin| pin.program_sha256),
    ) {
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

    /// The window hears of a wait when it starts, when the program or the
    /// size changes, and when the download moves by a step, not at every
    /// read.
    #[test]
    fn a_wait_is_sent_when_it_moves_by_a_step() {
        let first = (Program::Ffmpeg, 0, Some(10_000_000));
        assert!(worth_sending(None, first));
        assert!(!worth_sending(
            Some(first),
            (Program::Ffmpeg, 64 * 1024, Some(10_000_000))
        ));
        assert!(worth_sending(
            Some(first),
            (Program::Ffmpeg, WAITING_STEP_BYTES, Some(10_000_000))
        ));
        assert!(worth_sending(
            Some(first),
            (Program::Ffprobe, 0, Some(10_000_000))
        ));
        assert!(worth_sending(Some(first), (Program::Ffmpeg, 0, None)));
    }

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

    /// wtsexporter pinned to `bytes`, as the download for this computer.
    fn wtsexporter_pinned_to(bytes: &[u8]) -> Pinned {
        Pinned {
            program: Program::Wtsexporter,
            repo: "owner/project",
            release: "r1",
            asset: "wtsexporter-test",
            sha256: Box::leak(file_io::sha256_hex(bytes).into_boxed_str()),
            program_sha256: Box::leak(file_io::sha256_hex(bytes).into_boxed_str()),
            gzipped: false,
        }
    }

    /// A Tools Directory without wtsexporter reports it missing. A file
    /// there that is not the pinned file, such as a copy still being
    /// written, is reported missing too, and the pinned file is found
    /// (#2692). Where the app has no download, a file there that may run is
    /// found.
    #[test]
    fn wtsexporter_is_reported_from_the_tools_directory() {
        let _lock = media::testutil::tools_test_lock();
        let previous = media::tools_dir();
        let tools = tempfile::tempdir().unwrap();
        let name = whatsapp_exporter::wtsexporter_file_name();
        let program = tools.path().join(name);
        media::set_tools_dir(Some(tools.path().to_path_buf()));
        let downloads = ToolDownloads::default();
        let pinned = [wtsexporter_pinned_to(b"the pinned file")];
        let missing = tools_status_with(&downloads, &pinned).wtsexporter;
        let unavailable = tools_status_with(&downloads, &[]).wtsexporter;
        media::testutil::write_with_mode(&program, 0o755);
        std::fs::write(&program, b"the pinned").unwrap();
        let truncated = tools_status_with(&downloads, &pinned).wtsexporter;
        let found_by_hand = tools_status_with(&downloads, &[]).wtsexporter;
        std::fs::write(&program, b"the pinned file").unwrap();
        let found = tools_status_with(&downloads, &pinned).wtsexporter;
        media::set_tools_dir(previous);

        assert_eq!(missing, ToolStatus::Missing);
        // No download for this computer: told apart, so the form offers no
        // Try again.
        assert_eq!(unavailable, ToolStatus::Unavailable);
        assert_eq!(
            serde_json::to_value(&unavailable).unwrap(),
            serde_json::json!({ "state": "unavailable" })
        );
        assert_eq!(truncated, ToolStatus::Missing);
        let at = ToolStatus::Found {
            path: program.display().to_string(),
        };
        assert_eq!(found_by_hand, at);
        assert_eq!(found, at);
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
