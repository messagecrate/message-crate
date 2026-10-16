//! The releases the app downloads ffmpeg, ffprobe and wtsexporter from, and
//! the SHA-256 of each file, by platform. A newer pinned ffmpeg release is a
//! change to this file and to the user guide's troubleshooting page,
//! `docs/src/content/docs/docs/user/features/owner/troubleshooting.md`,
//! which names the release and its files for a copy put in the Tools
//! Directory by hand (`docs/adr/0019`). The wtsexporter release and its
//! checksums are the WhatsApp exporter's
//! (`crates/exporters/whatsapp-exporter/src/wtsexporter/pin.rs`), because
//! its lookup counts only the pinned file as wtsexporter (#2692); that file
//! says what a newer one changes.
//! A renamed heading on that page is a change to
//! `tool_downloads::troubleshooting` for ffmpeg, and for wtsexporter to
//! `WTSEXPORTER_TROUBLESHOOTING` in the exporter's `wtsexporter.rs`, which
//! `troubleshooting` gives as it is.

use super::{Pinned, Program};

/// The project the ffmpeg and ffprobe files come from.
const FFMPEG_STATIC: &str = "eugeneware/ffmpeg-static";

/// The `ffmpeg-static` release pinned. Its files are GPL builds with
/// `libx264` and `libx265`, but not of one ffmpeg version: each platform's
/// ffmpeg and ffprobe report the version below, as `-version` prints it on
/// Linux x64 and as the version string built into the other files reads
/// (checked 2026-10-16):
///
/// - Linux x64 and arm64: `7.0.2-static https://johnvansickle.com/ffmpeg/`.
/// - macOS x64: `6.1.1-tessus`.
/// - macOS arm64: `6.0`.
/// - Windows x64, which Windows on ARM gets too:
///   `6.1.1-essentials_build-www.gyan.dev`.
const FFMPEG_RELEASE: &str = "b6.1.1";

/// The `.gz` file of ffmpeg and of ffprobe for one platform, with the
/// SHA-256 of each `.gz` and of the program it unpacks to.
struct FfmpegFiles {
    /// `std::env::consts::OS`.
    os: &'static str,
    /// `std::env::consts::ARCH`.
    arch: &'static str,
    /// ffmpeg's asset, its SHA-256, and the unpacked program's SHA-256.
    ffmpeg: (&'static str, &'static str, &'static str),
    /// ffprobe's asset, its SHA-256, and the unpacked program's SHA-256.
    ffprobe: (&'static str, &'static str, &'static str),
}

/// The files of the pinned `ffmpeg-static` release, by platform.
///
/// The project publishes no checksum file. Each SHA-256 here was computed on
/// 2026-10-08 by downloading the asset with
/// `curl -L https://github.com/eugeneware/ffmpeg-static/releases/download/<release>/<asset>`
/// and running `sha256sum` on it; `gzip -t` passed on each. The program's
/// SHA-256 was computed the same day by `gunzip -c <asset> | sha256sum`, so
/// the app knows a copy put in the Tools Directory by hand for the pinned one.
///
/// The `.gz` files are downloaded rather than the plain ones because they
/// are a third of the size (19 to 30 MB against 45 to 83 MB). The file is
/// checked as it arrives, before it is unpacked, and the program it unpacks
/// to is checked again.
const FFMPEG_FILES: &[FfmpegFiles] = &[
    FfmpegFiles {
        os: "linux",
        arch: "x86_64",
        ffmpeg: (
            "ffmpeg-linux-x64.gz",
            "bfe8a8fc511530457b528c48d77b5737527b504a3797a9bc4866aeca69c2dffa",
            "e7e7fb30477f717e6f55f9180a70386c62677ef8a4d4d1a5d948f4098aa3eb99",
        ),
        ffprobe: (
            "ffprobe-linux-x64.gz",
            "25d9b6ccb05e3d9de9e04e31e2506d8dd7f9f0418981965ac6df12e8d3afd067",
            "4f231a1960d83e403d08f7971e271707bec278a9ae18e21b8b5b03186668450d",
        ),
    },
    FfmpegFiles {
        os: "linux",
        arch: "aarch64",
        ffmpeg: (
            "ffmpeg-linux-arm64.gz",
            "754a678672298bc68156adff58aa7385a592c2b30b1d0ae8750c45c915c4bac0",
            "6bb182d0d75d23028db82e9e4f723ca69b853d055698486e6984ddb2c06fb8ce",
        ),
        ffprobe: (
            "ffprobe-linux-arm64.gz",
            "2ab6aba60ee84412dff9188720703376cb4e7aaf7e0b5e43aa8249f2acae5bf8",
            "d17ae9b4c297d48e2521ba14e417bb0537c6ff77c584cdbcd6bb0d8d0307a2e8",
        ),
    },
    FfmpegFiles {
        os: "macos",
        arch: "x86_64",
        ffmpeg: (
            "ffmpeg-darwin-x64.gz",
            "929b375c1182d956c51f7ac25e0b2b0411fb01f6f407aa15c9758efeb4242106",
            "ebdddc936f61e14049a2d4b549a412b8a40deeff6540e58a9f2a2da9e6b18894",
        ),
        ffprobe: (
            "ffprobe-darwin-x64.gz",
            "d4da574d6e2e197bd259b47d69cf262df9e312af24ad960444f6d806d3d4c186",
            "fa3add0ce901f7241abe0dfc0155d958fc834aca3f8ce61f87cc712ae669c1e0",
        ),
    },
    FfmpegFiles {
        os: "macos",
        arch: "aarch64",
        ffmpeg: (
            "ffmpeg-darwin-arm64.gz",
            "8923876afa8db5585022d7860ec7e589af192f441c56793971276d450ed3bbfa",
            "a90e3db6a3fd35f6074b013f948b1aa45b31c6375489d39e572bea3f18336584",
        ),
        ffprobe: (
            "ffprobe-darwin-arm64.gz",
            "d986a8ec7b030899fe66a8a288ed809a3543338705a3ce178cfb85869c5d80be",
            "bb2db6f5d8cef919da12fbf592119a987202a8c060a886f3cab091f9cab90b64",
        ),
    },
    WINDOWS_X64,
    // Windows on ARM gets the x64 files, which Windows runs under
    // emulation.
    FfmpegFiles {
        arch: "aarch64",
        ..WINDOWS_X64
    },
];

/// The Windows x64 entry of [`FFMPEG_FILES`]. The doc comment on
/// [`FFMPEG_FILES`] says how these SHA-256 values were computed.
const WINDOWS_X64: FfmpegFiles = FfmpegFiles {
    os: "windows",
    arch: "x86_64",
    ffmpeg: (
        "ffmpeg-win32-x64.gz",
        "8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77",
        "04e1307997530f9cf2fe35cba2ca7e8875ca91da02f89d6c7243df819c94ad00",
    ),
    ffprobe: (
        "ffprobe-win32-x64.gz",
        "f309e6223ad89d2fe54bccd420a7709b66fd27540674e92309578ed491a43c8d",
        "3a7e2dc003dc2cd1472827e4c7c4f056ae1ae0ae7c5bbc580c99b49827351ba4",
    ),
};

/// The pinned files for a computer whose `std::env::consts` are `os` and
/// `arch`: ffmpeg and ffprobe, then wtsexporter. A platform with no file
/// for a program gets nothing for it.
pub fn pinned_for(os: &str, arch: &str) -> Vec<Pinned> {
    let mut pinned = Vec::new();
    if let Some(files) = FFMPEG_FILES.iter().find(|f| f.os == os && f.arch == arch) {
        for (program, (asset, sha256, program_sha256)) in [
            (Program::Ffmpeg, files.ffmpeg),
            (Program::Ffprobe, files.ffprobe),
        ] {
            pinned.push(Pinned {
                program,
                repo: FFMPEG_STATIC,
                release: FFMPEG_RELEASE,
                asset,
                sha256,
                program_sha256,
                gzipped: true,
            });
        }
    }
    if let Some(file) = whatsapp_exporter::pinned_wtsexporter(os, arch) {
        pinned.push(Pinned {
            program: Program::Wtsexporter,
            repo: whatsapp_exporter::WTSEXPORTER_REPO,
            release: whatsapp_exporter::WTSEXPORTER_RELEASE,
            asset: file.asset,
            sha256: file.sha256,
            program_sha256: file.sha256,
            gzipped: false,
        });
    }
    pinned
}
