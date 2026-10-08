//! Test helpers for code that runs ffmpeg and ffprobe.
//!
//! Compiled for this crate's own tests and, through the `testutil` feature,
//! for the tests of every crate that lists `media` with that feature as a
//! dev-dependency. Every test in the workspace that needs the real tools
//! gates on [`real_ffmpeg_test_guard`], so the rule for a missing ffmpeg
//! lives in one place.
//!
//! The tool location is process-wide ([`crate::set_tools_dir`] and the
//! `PATH` tests put in place of the process's), so a test
//! that points it somewhere else and a test that runs the real ffmpeg must
//! not overlap. One lock serializes them: tests that only read the location
//! share it, and a test that changes the location holds it alone.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use std::ffi::OsString;

use crate::tools::{ffmpeg_available, search_path, set_search_path, set_tools_dir, tools_dir};

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

/// The lock over the process-wide tool location.
fn tools_lock() -> &'static RwLock<()> {
    static LOCK: OnceLock<RwLock<()>> = OnceLock::new();
    LOCK.get_or_init(|| RwLock::new(()))
}

/// Hold the tool location still, for a test that changes it.
///
/// A test that changes where the tools are looked for must hold this for as long as
/// the location differs from the one it found, and put that one back before
/// releasing it. Otherwise a test that runs the real ffmpeg can resolve the
/// tools in the instant another test has them pointed at an empty or mock
/// directory.
pub fn tools_test_lock() -> RwLockWriteGuard<'static, ()> {
    tools_lock().write().unwrap_or_else(PoisonError::into_inner)
}

/// Whether CI is running this process. GitHub Actions sets `CI=true`.
fn running_in_ci() -> bool {
    std::env::var("CI").is_ok_and(|value| !value.is_empty() && value != "false" && value != "0")
}

/// Hold the tool location still for a test that needs the real ffmpeg, and
/// say whether ffmpeg and ffprobe are there. `None` means they are not, and
/// the test should return.
///
/// Under CI a missing ffmpeg panics instead. CI installs ffmpeg for every job
/// that runs these tests, so a missing one there is a broken job, and a test
/// that returned early would report a pass it never earned. On a developer
/// machine the test skips, and says so on stderr.
///
/// Taking the lock and asking whether ffmpeg is available are one call
/// because doing either without the other lets a test be answered by an
/// empty or mock directory another test has the location pointed at for
/// that instant (#308).
///
/// # Panics
///
/// When `CI` is set and ffmpeg or ffprobe cannot be found.
#[must_use]
pub fn real_ffmpeg_test_guard() -> Option<RwLockReadGuard<'static, ()>> {
    let guard = tools_lock().read().unwrap_or_else(PoisonError::into_inner);
    if ffmpeg_available() {
        return Some(guard);
    }
    drop(guard);
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

/// The tool location made empty for as long as this lives.
///
/// Holds [`tools_test_lock`], searches an empty `PATH` and a Tools
/// Directory that does not exist, and puts both back when dropped. Those are
/// the only places the tools are looked for, so they are missing whether or
/// not the machine has ffmpeg installed.
pub struct ToolsHidden {
    previous_tools_dir: Option<PathBuf>,
    previous_search_path: Option<OsString>,
    _lock: RwLockWriteGuard<'static, ()>,
}

/// Make ffmpeg and ffprobe unavailable to this process until the returned
/// guard is dropped.
#[must_use]
pub fn hide_ffmpeg() -> ToolsHidden {
    let lock = tools_test_lock();
    let previous_tools_dir = tools_dir();
    let previous_search_path = search_path();
    let nowhere = std::env::temp_dir()
        .join(format!("message-crate-no-tools-{}", std::process::id()))
        .join("does-not-exist");
    set_search_path(Some(OsString::new()));
    set_tools_dir(Some(nowhere));
    ToolsHidden {
        previous_tools_dir,
        previous_search_path,
        _lock: lock,
    }
}

/// Search `search_path` instead of the process's `PATH`, and `tools_dir`
/// as the Tools Directory, until the returned guard is dropped. Holds
/// [`tools_test_lock`], as [`hide_ffmpeg`] does.
#[must_use]
pub fn locate_tools(search_path: OsString, tools_dir: PathBuf) -> ToolsHidden {
    let hidden = hide_ffmpeg();
    set_search_path(Some(search_path));
    set_tools_dir(Some(tools_dir));
    hidden
}

impl Drop for ToolsHidden {
    fn drop(&mut self) {
        set_search_path(self.previous_search_path.take());
        set_tools_dir(self.previous_tools_dir.take());
    }
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
