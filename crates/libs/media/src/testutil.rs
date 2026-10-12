//! Test helpers for code that runs ffmpeg and ffprobe.
//!
//! Compiled for this crate's own tests and, through the `testutil` feature,
//! for the tests of every crate that lists `media` with that feature as a
//! dev-dependency. Every test in the workspace that needs the real tools
//! gets them from [`real_ffmpeg`], so the rule for a missing ffmpeg lives in
//! one place.
//!
//! Where to look for the tools is part of the [`InstalledTools`] a test
//! builds and passes in, never process-wide, so a test that points the
//! lookup at a mock or empty directory and a test that runs the real ffmpeg
//! never see each other's choice, and need no lock between them.
//!
//! A test of what the Media Stage, the size forecast, or a Thumbnail or
//! Preview decides, rather than of what ffmpeg makes of it, passes
//! [`FakeTools`] instead and needs no ffmpeg installed.

use std::io::Write;
use std::path::Path;

use crate::tools::InstalledTools;

pub use crate::fake_tools::{FakeTools, video_probe};

/// How many times [`crate::file_has_sha256`] read the file at a path, for a
/// test that a file is not read again while it is unchanged.
pub use crate::tools::times_read;

/// A 1x1 PNG, which the convert pass turns into a JPEG in both Convert and
/// Compress.
///
/// Plain RGB (PNG color type 2), not RGBA, because ffmpeg's PNG decoder
/// fails on a 1x1 RGBA image and reads this one cleanly.
#[rustfmt::skip]
pub const PNG_1X1_RGB: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// [`PNG_1X1_RGB`] in standard base64, as an SMS Backup & Restore backup
/// holds a part. The workspace has no base64 encoder, so the encoding is
/// written out.
pub const PNG_1X1_RGB_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

/// The error an ffmpeg run fails with when it is killed for `why`
/// ([`crate::Killed`]), for a test of what a caller makes of it.
pub fn ffmpeg_killed(why: crate::KillReason) -> anyhow::Error {
    crate::Killed::ffmpeg(why).into()
}

/// Whether CI is running this process. GitHub Actions sets `CI=true`.
fn running_in_ci() -> bool {
    std::env::var("CI").is_ok_and(|value| !value.is_empty() && value != "false" && value != "0")
}

/// ffmpeg and ffprobe as this machine has them on `PATH`, for a test that
/// needs the real programs. `None` means they are not there, and the test
/// should return.
///
/// Under CI a missing ffmpeg panics instead. CI installs ffmpeg for every job
/// that runs these tests, so a missing one there is a broken job, and a test
/// that returned early would report a pass it never earned. On a developer
/// machine the test skips, and says so on stderr.
///
/// # Panics
///
/// When `CI` is set and ffmpeg or ffprobe cannot be found.
#[must_use]
pub fn real_ffmpeg() -> Option<InstalledTools> {
    let tools = InstalledTools::default();
    if tools.require().is_ok() {
        return Some(tools);
    }
    let current = std::thread::current();
    let test = current.name().unwrap_or("an unnamed test");
    assert!(
        !running_in_ci(),
        "{test} needs ffmpeg and ffprobe, and CI is set but they were not found. \
         Install ffmpeg on PATH in this CI job."
    );
    // Written to the stderr handle rather than through `eprintln!`, which the
    // test harness captures and throws away for a test that passes.
    let _ = writeln!(
        std::io::stderr(),
        "skipped {test}: ffmpeg or ffprobe was not found"
    );
    None
}

/// ffmpeg and ffprobe looked for on an empty `PATH` and in a Tools
/// Directory that does not exist. Those are the only places they are looked
/// for, so they are missing whether or not the machine has ffmpeg installed.
#[must_use]
pub fn no_ffmpeg() -> InstalledTools {
    let nowhere = std::env::temp_dir()
        .join(format!("message-crate-no-tools-{}", std::process::id()))
        .join("does-not-exist");
    InstalledTools::new(Some(nowhere)).searching("")
}

/// Whether this account may read a file whose mode allows no one to, tried
/// on a file written in `dir`. Root, or an account with the capability to
/// skip permission checks, may, and may also run any file with an
/// executable bit, so a test of a file this account may not run skips when
/// this is true.
#[cfg(unix)]
pub fn may_read_any_file(dir: &Path) -> bool {
    let probe = dir.join("may-read-any-file");
    write_with_mode(&probe, 0o000);
    let read = std::fs::read(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    read
}

/// Write an empty file at `path` with the Unix permission bits `mode`, as a
/// program put in the Tools Directory for a test: `0o755` for one that may
/// run, `0o644` for one copied in without `chmod +x`. Off Unix the mode is
/// not set.
pub fn write_with_mode(path: &Path, mode: u32) {
    std::fs::write(path, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = mode;
}

/// A clock for a watched run that reads zero until `started` exists, and
/// then an hour more on every read, so every limit and wait passes once the
/// mock has started, and none before. It waits on no real clock.
#[cfg(test)]
pub(crate) fn clock_from(started: &Path) -> impl Fn() -> std::time::Duration + '_ {
    let hours = std::cell::Cell::new(0u64);
    move || {
        if started.is_file() {
            hours.set(hours.get() + 1);
        }
        std::time::Duration::from_secs(hours.get() * 3600)
    }
}

/// A mock ffmpeg body that writes its pid, then `progress` on its progress
/// stream, then marks itself `started` and never exits. Between the two it
/// writes 256 KiB of progress lines that carry no position: a pipe holds 64
/// KiB, so the mock gets past them only once the reader has read, and so
/// recorded, the position before them. The test's clock starts only then,
/// with no race between the reader and the watch.
#[cfg(test)]
pub(crate) fn progress_then_hang(progress: &str) -> String {
    format!(
        "echo $$ > \"$(dirname \"$0\")/pid\"\n\
         printf '{progress}'\n\
         yes frame=1 | head -c 262144\n\
         touch \"$(dirname \"$0\")/started\"\nexec sleep 600"
    )
}
