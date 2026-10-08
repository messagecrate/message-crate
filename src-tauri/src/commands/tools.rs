//! What the Media part of Settings → System shows: where ffmpeg, ffprobe and
//! wtsexporter are, or that one is missing.
//!
//! ffmpeg and ffprobe are looked for on `PATH`, then in the Tools Directory;
//! wtsexporter only in the Tools Directory (#1053). The window names no
//! directory: the lookup is this process's, and nothing it is told changes
//! where a program is run from.

use std::path::PathBuf;

/// Where one program is, as Settings shows it.
///
/// Tagged by `state`, so the download (#1053, step 3) adds its own states
/// (in progress, failed and why) beside these two without changing them.
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

/// Look for ffmpeg, ffprobe and wtsexporter where this process runs them from.
#[tauri::command]
pub fn tools_status() -> ToolsStatus {
    ToolsStatus {
        tools_dir: media::tools_dir().map(|dir| dir.display().to_string()),
        ffmpeg: ToolStatus::of(media::ffmpeg_path()),
        ffprobe: ToolStatus::of(media::ffprobe_path()),
        wtsexporter: ToolStatus::of(whatsapp_exporter::wtsexporter_path()),
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

    /// wtsexporter in the Tools Directory is reported where it is, and a
    /// Tools Directory without it reports it missing.
    #[test]
    fn wtsexporter_is_reported_from_the_tools_directory() {
        let _lock = media::testutil::tools_test_lock();
        let previous = media::tools_dir();
        let tools = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "wtsexporter.exe"
        } else {
            "wtsexporter"
        };
        media::set_tools_dir(Some(tools.path().to_path_buf()));
        let missing = tools_status().wtsexporter;
        std::fs::write(tools.path().join(name), "").unwrap();
        let found = tools_status().wtsexporter;
        media::set_tools_dir(previous);

        assert_eq!(missing, ToolStatus::Missing);
        assert_eq!(
            found,
            ToolStatus::Found {
                path: tools.path().join(name).display().to_string()
            }
        );
    }
}
