//! The wtsexporter release the desktop app downloads, and the SHA-256 of
//! each platform's file. Only that file counts as wtsexporter in the Tools
//! Directory (#2692): the app needs the fork's first build with
//! `full_key_id` and `reply_key_id`, so another build is not a working copy,
//! and a copy still being written is not the file yet.
//!
//! The desktop app's download (`src-tauri/src/tool_downloads/pins.rs`)
//! reads its wtsexporter pin from here, so the file the lookup accepts and
//! the file the download fetches are one. A newer release is a change to
//! this file, to [`super::RELEASE_FILE_HINT`], to the user guide's
//! troubleshooting page,
//! `docs/src/content/docs/docs/user/features/owner/troubleshooting.md`, and
//! to the developer install in `AGENTS.md` and
//! `docs/src/content/docs/docs/developer/contributing.md`, which name it.

/// The project the wtsexporter files come from: Message Crate's fork, whose
/// JSON says who sent each message and who is in each group.
pub const WTSEXPORTER_REPO: &str = "messagecrate/WhatsApp-Chat-Exporter";

/// The fork release pinned, the first with `full_key_id` and `reply_key_id`.
pub const WTSEXPORTER_RELEASE: &str = "0.13.0-mc.2";

/// One platform's wtsexporter file in the pinned release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WtsexporterFile {
    /// The release asset's name.
    pub asset: &'static str,
    /// The asset's SHA-256, which is the program's: the file is not packed.
    pub sha256: &'static str,
}

/// The wtsexporter file for each platform the fork builds for, as
/// `(os, arch, file)`. The checksums are copied from the release's
/// `SHA256SUMS` asset, read on 2026-10-08; the app never fetches that file.
/// Linux on ARM has no file.
const WTSEXPORTER_FILES: &[(&str, &str, WtsexporterFile)] = &[
    (
        "linux",
        "x86_64",
        WtsexporterFile {
            asset: "wtsexporter_linux_x64",
            sha256: "2756dfbcc7e320390e34244a8fc7160770d99d6fef17dea6a2d6ce269b62f71b",
        },
    ),
    (
        "macos",
        "x86_64",
        WtsexporterFile {
            asset: "wtsexporter_macos_x64",
            sha256: "b0e885474108d810fa7ba31360275494fc93ebadae117859ba7e90fa705f45e3",
        },
    ),
    (
        "macos",
        "aarch64",
        WtsexporterFile {
            asset: "wtsexporter_macos_arm64",
            sha256: "03f90f32e956a5df1cbfbc1a1a87524ab05450f645703df0c16d630671eda9bf",
        },
    ),
    (
        "windows",
        "x86_64",
        WtsexporterFile {
            asset: "wtsexporter_win_x64.exe",
            sha256: "4f4c91ab7829fa4a78673b5124c354b7be0c6d7446689ac58c56461f7988707f",
        },
    ),
    (
        "windows",
        "aarch64",
        WtsexporterFile {
            asset: "wtsexporter_win_arm64.exe",
            sha256: "aa154042bec5b96953dda8d3c6daabac4fbbd9bd51c28fd350ba416317d9ec10",
        },
    ),
];

/// The pinned wtsexporter file for a computer whose `std::env::consts` are
/// `os` and `arch`. `None` where the fork publishes none, as on Linux on
/// ARM.
pub fn pinned_wtsexporter(os: &str, arch: &str) -> Option<WtsexporterFile> {
    WTSEXPORTER_FILES
        .iter()
        .find(|(file_os, file_arch, _)| *file_os == os && *file_arch == arch)
        .map(|&(_, _, file)| file)
}

/// The pinned file's SHA-256 for this computer, `None` where there is none.
pub(super) fn pinned_sha256_here() -> Option<&'static str> {
    pinned_wtsexporter(std::env::consts::OS, std::env::consts::ARCH).map(|file| file.sha256)
}
