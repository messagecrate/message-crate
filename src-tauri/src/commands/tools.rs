//! What the Media part of Settings → System shows: where ffmpeg, ffprobe and
//! wtsexporter are, that one is missing, or how its download stands
//! ([`crate::tool_downloads`]).
//!
//! ffmpeg and ffprobe are looked for on `PATH`, then in the Tools Directory;
//! wtsexporter only in the Tools Directory (#1053). The window names no
//! directory: the lookup is this process's, and nothing it is told changes
//! where a program is run from.

use std::path::PathBuf;

use crate::tool_downloads::{DownloadState, Program, ToolDownloads};

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

    /// This status as `download` changes it. A download in progress is
    /// shown whatever was found, because the file is about to change. A
    /// failed one is shown only when the program is not found, because a
    /// program found is still used.
    fn with_download(self, download: Option<DownloadState>) -> Self {
        match (download, self) {
            (Some(DownloadState::Downloading { received, total }), _) => {
                Self::Downloading { received, total }
            }
            (Some(DownloadState::Failed { reason }), Self::Missing | Self::Unusable { .. }) => {
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
    tools_status_with(&downloads)
}

/// [`tools_status`], with the downloads in `downloads`.
fn tools_status_with(downloads: &ToolDownloads) -> ToolsStatus {
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
        ffmpeg: ffmpeg.with_download(downloads.get(Program::Ffmpeg)),
        ffprobe: ffprobe.with_download(downloads.get(Program::Ffprobe)),
        wtsexporter: wtsexporter.with_download(downloads.get(Program::Wtsexporter)),
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
            ffmpeg: ToolStatus::of(Some(PathBuf::from("/usr/bin/ffmpeg"))),
            ffprobe: ToolStatus::of(Some(PathBuf::from("/usr/bin/ffprobe"))),
            wtsexporter: ToolStatus::of(None),
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            serde_json::json!({
                "toolsDir": "/home/sam/message-crate/tools",
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
        let missing = tools_status_with(&downloads).wtsexporter;
        media::testutil::write_with_mode(&tools.path().join(name), 0o755);
        let found = tools_status_with(&downloads).wtsexporter;
        media::set_tools_dir(previous);

        assert_eq!(missing, ToolStatus::Missing);
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
