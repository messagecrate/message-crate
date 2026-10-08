//! The releases the app downloads ffmpeg, ffprobe and wtsexporter from, and
//! the SHA-256 of each file, by platform. A newer pinned release is a change
//! to this file alone (`docs/adr/0019`).

use super::{Pinned, Program};

/// The project the ffmpeg and ffprobe files come from.
const FFMPEG_STATIC: &str = "eugeneware/ffmpeg-static";

/// The `ffmpeg-static` release pinned, named `b6.1.1`: its files report
/// ffmpeg 7.0.2, a GPL build with `libx264` and `libx265`.
const FFMPEG_RELEASE: &str = "b6.1.1";

/// The project the wtsexporter files come from: Message Crate's fork, whose
/// JSON says who sent each message and who is in each group.
const WTSEXPORTER_REPO: &str = "messagecrate/WhatsApp-Chat-Exporter";

/// The fork release pinned, the first with `full_key_id` and `reply_key_id`.
const WTSEXPORTER_RELEASE: &str = "0.13.0-mc.2";

/// The `.gz` file of ffmpeg and of ffprobe for one platform, with the
/// SHA-256 of each.
struct FfmpegFiles {
    /// `std::env::consts::OS`.
    os: &'static str,
    /// `std::env::consts::ARCH`.
    arch: &'static str,
    /// ffmpeg's asset and its SHA-256.
    ffmpeg: (&'static str, &'static str),
    /// ffprobe's asset and its SHA-256.
    ffprobe: (&'static str, &'static str),
}

/// The `ffmpeg-static` `b6.1.1` files, by platform.
///
/// The project publishes no checksum file. Each SHA-256 here was computed on
/// 2026-10-08 by downloading the asset with
/// `curl -L https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/<asset>`
/// and running `sha256sum` on it; `gzip -t` passed on each.
///
/// The `.gz` files are downloaded rather than the plain ones because they
/// are a third of the size (19 to 30 MB against 45 to 83 MB), and the check
/// is on the file as it arrives, before it is unpacked.
///
/// Windows on ARM gets the x64 files, which Windows runs under emulation.
/// Whether they run there is not checked yet (#1053).
const FFMPEG_FILES: &[FfmpegFiles] = &[
    FfmpegFiles {
        os: "linux",
        arch: "x86_64",
        ffmpeg: (
            "ffmpeg-linux-x64.gz",
            "bfe8a8fc511530457b528c48d77b5737527b504a3797a9bc4866aeca69c2dffa",
        ),
        ffprobe: (
            "ffprobe-linux-x64.gz",
            "25d9b6ccb05e3d9de9e04e31e2506d8dd7f9f0418981965ac6df12e8d3afd067",
        ),
    },
    FfmpegFiles {
        os: "linux",
        arch: "aarch64",
        ffmpeg: (
            "ffmpeg-linux-arm64.gz",
            "754a678672298bc68156adff58aa7385a592c2b30b1d0ae8750c45c915c4bac0",
        ),
        ffprobe: (
            "ffprobe-linux-arm64.gz",
            "2ab6aba60ee84412dff9188720703376cb4e7aaf7e0b5e43aa8249f2acae5bf8",
        ),
    },
    FfmpegFiles {
        os: "macos",
        arch: "x86_64",
        ffmpeg: (
            "ffmpeg-darwin-x64.gz",
            "929b375c1182d956c51f7ac25e0b2b0411fb01f6f407aa15c9758efeb4242106",
        ),
        ffprobe: (
            "ffprobe-darwin-x64.gz",
            "d4da574d6e2e197bd259b47d69cf262df9e312af24ad960444f6d806d3d4c186",
        ),
    },
    FfmpegFiles {
        os: "macos",
        arch: "aarch64",
        ffmpeg: (
            "ffmpeg-darwin-arm64.gz",
            "8923876afa8db5585022d7860ec7e589af192f441c56793971276d450ed3bbfa",
        ),
        ffprobe: (
            "ffprobe-darwin-arm64.gz",
            "d986a8ec7b030899fe66a8a288ed809a3543338705a3ce178cfb85869c5d80be",
        ),
    },
    FfmpegFiles {
        os: "windows",
        arch: "x86_64",
        ffmpeg: (
            "ffmpeg-win32-x64.gz",
            "8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77",
        ),
        ffprobe: (
            "ffprobe-win32-x64.gz",
            "f309e6223ad89d2fe54bccd420a7709b66fd27540674e92309578ed491a43c8d",
        ),
    },
    FfmpegFiles {
        os: "windows",
        arch: "aarch64",
        ffmpeg: (
            "ffmpeg-win32-x64.gz",
            "8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77",
        ),
        ffprobe: (
            "ffprobe-win32-x64.gz",
            "f309e6223ad89d2fe54bccd420a7709b66fd27540674e92309578ed491a43c8d",
        ),
    },
];

/// The wtsexporter file for each platform the fork builds for, as
/// `(os, arch, asset, sha256)`. The checksums are copied from the release's
/// `SHA256SUMS` asset, read on 2026-10-08; the app never fetches that file.
/// Linux on ARM has no file.
const WTSEXPORTER_FILES: &[(&str, &str, &str, &str)] = &[
    (
        "linux",
        "x86_64",
        "wtsexporter_linux_x64",
        "2756dfbcc7e320390e34244a8fc7160770d99d6fef17dea6a2d6ce269b62f71b",
    ),
    (
        "macos",
        "x86_64",
        "wtsexporter_macos_x64",
        "b0e885474108d810fa7ba31360275494fc93ebadae117859ba7e90fa705f45e3",
    ),
    (
        "macos",
        "aarch64",
        "wtsexporter_macos_arm64",
        "03f90f32e956a5df1cbfbc1a1a87524ab05450f645703df0c16d630671eda9bf",
    ),
    (
        "windows",
        "x86_64",
        "wtsexporter_win_x64.exe",
        "4f4c91ab7829fa4a78673b5124c354b7be0c6d7446689ac58c56461f7988707f",
    ),
    (
        "windows",
        "aarch64",
        "wtsexporter_win_arm64.exe",
        "aa154042bec5b96953dda8d3c6daabac4fbbd9bd51c28fd350ba416317d9ec10",
    ),
];

/// The pinned files for a computer whose `std::env::consts` are `os` and
/// `arch`: ffmpeg and ffprobe, then wtsexporter. A platform with no file
/// for a program gets nothing for it.
pub fn pinned_for(os: &str, arch: &str) -> Vec<Pinned> {
    let mut pinned = Vec::new();
    if let Some(files) = FFMPEG_FILES.iter().find(|f| f.os == os && f.arch == arch) {
        for (program, (asset, sha256)) in [
            (Program::Ffmpeg, files.ffmpeg),
            (Program::Ffprobe, files.ffprobe),
        ] {
            pinned.push(Pinned {
                program,
                repo: FFMPEG_STATIC,
                release: FFMPEG_RELEASE,
                asset,
                sha256,
                gzip: true,
            });
        }
    }
    if let Some(&(_, _, asset, sha256)) = WTSEXPORTER_FILES
        .iter()
        .find(|(file_os, file_arch, _, _)| *file_os == os && *file_arch == arch)
    {
        pinned.push(Pinned {
            program: Program::Wtsexporter,
            repo: WTSEXPORTER_REPO,
            release: WTSEXPORTER_RELEASE,
            asset,
            sha256,
            gzip: false,
        });
    }
    pinned
}
