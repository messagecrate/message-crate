use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};

/// Where this process looks for ffmpeg and ffprobe, and where it found them.
struct ToolsState {
    /// The Tools Directory, searched after `PATH`. The desktop app sets it
    /// for itself and passes it to the server it starts (`serve
    /// --tools-dir`); a server started by hand has none.
    tools_dir: Option<PathBuf>,
    /// The `PATH` searched instead of the process's own. Only tests set it
    /// ([`set_search_path`]), so a test decides what is found whether or
    /// not the machine has ffmpeg installed.
    search_path: Option<OsString>,
    generation: u64,
    /// ffmpeg and ffprobe, once both were found in one place.
    found: Option<(PathBuf, PathBuf)>,
}

impl ToolsState {
    /// Forget where the tools were found, so the next lookup searches again.
    fn forget(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.found = None;
    }
}

/// The process-wide tool cache.
fn tools_state() -> &'static Mutex<ToolsState> {
    static STATE: OnceLock<Mutex<ToolsState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(ToolsState {
            tools_dir: None,
            search_path: None,
            generation: 0,
            found: None,
        })
    })
}

/// Name the Tools Directory, searched for ffmpeg and ffprobe after `PATH`,
/// and forget where they were found before.
pub fn set_tools_dir(dir: Option<PathBuf>) {
    let mut state = tools_state().lock().expect("tools state lock");
    state.tools_dir = dir;
    state.forget();
}

/// The Tools Directory [`set_tools_dir`] named, if any.
pub fn tools_dir() -> Option<PathBuf> {
    tools_state()
        .lock()
        .expect("tools state lock")
        .tools_dir
        .clone()
}

/// Search `path` instead of the process's `PATH`, or the process's own
/// again with `None`, and forget where the tools were found before.
#[cfg(any(test, feature = "testutil"))]
pub(crate) fn set_search_path(path: Option<OsString>) {
    let mut state = tools_state().lock().expect("tools state lock");
    state.search_path = path;
    state.forget();
}

/// The `PATH` [`set_search_path`] put in place of the process's, if any.
#[cfg(any(test, feature = "testutil"))]
pub(crate) fn search_path() -> Option<OsString> {
    tools_state()
        .lock()
        .expect("tools state lock")
        .search_path
        .clone()
}

/// Where ffmpeg and ffprobe are. Either is `None` when it was not found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FfmpegTools {
    /// Where ffmpeg is.
    pub ffmpeg: Option<PathBuf>,
    /// Where ffprobe is.
    pub ffprobe: Option<PathBuf>,
}

impl FfmpegTools {
    /// The programs not found, by name.
    pub fn missing(&self) -> Vec<&'static str> {
        [("ffmpeg", &self.ffmpeg), ("ffprobe", &self.ffprobe)]
            .into_iter()
            .filter(|(_, path)| path.is_none())
            .map(|(name, _)| name)
            .collect()
    }
}

/// Where ffmpeg and ffprobe are: both on `PATH`, else both in the Tools
/// Directory.
///
/// # Errors
///
/// Returns an error naming both paths when one is found only on `PATH` and
/// the other only in the Tools Directory, because two builds of different
/// versions would then work on one file.
pub fn ffmpeg_tools() -> Result<FfmpegTools> {
    resolve_tools()
}

/// Where ffmpeg is, by [`ffmpeg_tools`]. `None` when it is not found, does
/// not run, or is in a different place from ffprobe.
pub fn ffmpeg_path() -> Option<PathBuf> {
    resolve_tools().ok().and_then(|tools| tools.ffmpeg)
}

/// Where ffprobe is, by [`ffmpeg_tools`]. `None` when it is not found, does
/// not run, or is in a different place from ffmpeg.
pub fn ffprobe_path() -> Option<PathBuf> {
    resolve_tools().ok().and_then(|tools| tools.ffprobe)
}

/// True when ffmpeg and ffprobe are both found, in one place.
pub fn ffmpeg_available() -> bool {
    resolve_tools().is_ok_and(|tools| tools.missing().is_empty())
}

/// True when ffprobe is found.
pub(crate) fn ffprobe_available() -> bool {
    ffprobe_path().is_some()
}

/// The places ffmpeg and ffprobe are looked for, as an error names them.
fn where_looked() -> String {
    match tools_dir() {
        Some(dir) => format!("on PATH or in the Tools Directory, {}", dir.display()),
        None => "on PATH".to_string(),
    }
}

/// Fail, naming which tool is missing and where it was looked for, when
/// ffmpeg or ffprobe is not found.
///
/// # Errors
///
/// Returns an error when either tool is missing, or when the two are in
/// different places ([`ffmpeg_tools`]).
pub fn require_ffmpeg() -> Result<()> {
    let missing = resolve_tools()?.missing();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "ffmpeg and ffprobe are required to convert or compress attachments. {} {} not found {}.",
        missing.join(" and "),
        if missing.len() == 1 { "was" } else { "were" },
        where_looked()
    )
}

/// True when running `bin` with `args` exits successfully.
fn command_runs(bin: &Path, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Where ffmpeg and ffprobe are, by [`find_tools`] over this process's
/// `PATH` and Tools Directory, remembered once both are found. A tool not
/// found is looked for again next time, so one that arrives later is used.
fn resolve_tools() -> Result<FfmpegTools> {
    loop {
        let (generation, search_path, tools_dir) = {
            let state = tools_state().lock().expect("tools state lock");
            if let Some((ffmpeg, ffprobe)) = &state.found {
                return Ok(FfmpegTools {
                    ffmpeg: Some(ffmpeg.clone()),
                    ffprobe: Some(ffprobe.clone()),
                });
            }
            (
                state.generation,
                state.search_path.clone(),
                state.tools_dir.clone(),
            )
        };
        let search_path = search_path.or_else(|| std::env::var_os("PATH"));

        let resolved = find_tools(search_path.as_deref(), tools_dir.as_deref());

        let mut state = tools_state().lock().expect("tools state lock");
        if state.generation != generation {
            continue;
        }
        if let Ok(FfmpegTools {
            ffmpeg: Some(ffmpeg),
            ffprobe: Some(ffprobe),
        }) = &resolved
        {
            state.found = Some((ffmpeg.clone(), ffprobe.clone()));
        }
        return resolved;
    }
}

/// The tool under `dir` if it is a file and runs.
fn find_tool_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidate = dir.join(executable_name(name));
    if candidate.is_file() && command_runs(&candidate, &["-version"]) {
        Some(candidate)
    } else {
        None
    }
}

/// Find `name` in each directory of `search_path` in turn.
fn find_on_path(name: &str, search_path: Option<&OsStr>) -> Option<PathBuf> {
    search_path
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| find_tool_in_dir(&dir, name))
}

/// Find ffmpeg and ffprobe on `search_path`, then in `tools_dir`, and
/// nowhere else (#1053). A file that does not answer `-version` is passed
/// over, because Convert would start with it and then fail on every file.
///
/// `PATH` comes first because a person who installed ffmpeg chose it, and
/// the Tools Directory holds the copy the desktop app downloads for a
/// computer that has none (`docs/adr/0019`). The two are taken from one
/// place: both from `PATH` when both are there, else both from the Tools
/// Directory. One found only on `PATH` and the other only in the Tools
/// Directory is an error, because they would be two builds, perhaps of
/// different versions.
fn find_tools(search_path: Option<&OsStr>, tools_dir: Option<&Path>) -> Result<FfmpegTools> {
    let path_ffmpeg = find_on_path("ffmpeg", search_path);
    let path_ffprobe = find_on_path("ffprobe", search_path);
    if path_ffmpeg.is_some() && path_ffprobe.is_some() {
        return Ok(FfmpegTools {
            ffmpeg: path_ffmpeg,
            ffprobe: path_ffprobe,
        });
    }
    let dir_ffmpeg = tools_dir.and_then(|dir| find_tool_in_dir(dir, "ffmpeg"));
    let dir_ffprobe = tools_dir.and_then(|dir| find_tool_in_dir(dir, "ffprobe"));
    if dir_ffmpeg.is_some() && dir_ffprobe.is_some() {
        return Ok(FfmpegTools {
            ffmpeg: dir_ffmpeg,
            ffprobe: dir_ffprobe,
        });
    }
    match (&path_ffmpeg, &path_ffprobe, &dir_ffmpeg, &dir_ffprobe) {
        (Some(on_path), None, None, Some(in_dir)) => Err(found_in_two_places_error(
            "ffmpeg", on_path, "ffprobe", in_dir,
        )),
        (None, Some(on_path), Some(in_dir), None) => Err(found_in_two_places_error(
            "ffprobe", on_path, "ffmpeg", in_dir,
        )),
        _ => Ok(FfmpegTools {
            ffmpeg: path_ffmpeg.or(dir_ffmpeg),
            ffprobe: path_ffprobe.or(dir_ffprobe),
        }),
    }
}

/// The error for ffmpeg and ffprobe found in two places.
fn found_in_two_places_error(
    on_path: &str,
    path_copy: &Path,
    in_dir: &str,
    dir_copy: &Path,
) -> anyhow::Error {
    anyhow::anyhow!(
        "{on_path} is on PATH at {} and {in_dir} is in the Tools Directory at {}. \
         Both must be on PATH or both in the Tools Directory.",
        path_copy.display(),
        dir_copy.display()
    )
}

/// The tool's file name, with `.exe` on Windows.
fn executable_name(name: &str) -> String {
    if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// How much of the end of ffmpeg's stderr a failure carries.
const STDERR_TAIL_BYTES: usize = 8 * 1024;

/// Flags in front of every ffmpeg run: no version banner or build
/// configuration, no stats line, and only errors on stderr. A failure then
/// carries the lines that say why it failed, not kilobytes of preamble.
const QUIET_FFMPEG: [&str; 4] = ["-hide_banner", "-nostats", "-loglevel", "error"];

/// Run ffmpeg with [`QUIET_FFMPEG`] and `args`, failing with the end of its
/// stderr when it exits non-zero.
///
/// A thread reads stderr while ffmpeg runs. [`QUIET_FFMPEG`] turns the stats
/// line off, but nothing limits how much else ffmpeg writes: a failure can
/// repeat an error for every frame, and an encoder such as libx265 writes to
/// stderr whatever `-loglevel` says. A pipe nobody reads fills at 64 KiB on
/// Linux, after which ffmpeg blocks on the write and never exits (#1178).
pub(crate) fn run_ffmpeg(args: &[String]) -> Result<()> {
    run_ffmpeg_with(args, None)
}

/// [`run_ffmpeg`], stopped when `stop` is set: ffmpeg is killed and waited
/// for, and the run fails. A run asked for after `stop` is set never starts
/// ffmpeg. What ffmpeg wrote before it was killed is the caller's to remove.
pub(crate) fn run_ffmpeg_until(args: &[String], stop: &AtomicBool) -> Result<()> {
    run_ffmpeg_with(args, Some(stop))
}

/// How long the wait for a stoppable ffmpeg sleeps between looks, at most.
/// It starts at a millisecond and doubles, so a quick conversion is not held
/// up and a long one is stopped within this.
const STOP_POLL_MAX: Duration = Duration::from_millis(25);

/// How long a failed stoppable run waits for a stop before it reports the
/// failure. Ctrl-C in a terminal reaches ffmpeg and this process together,
/// and ffmpeg can exit before the handler here has set the stop; the failure
/// is then the stop's, and the caller must not read it as a bad file.
const STOP_GRACE: Duration = Duration::from_millis(200);

/// Whether `stopped` turns true within [`STOP_GRACE`].
fn stop_follows(stopped: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + STOP_GRACE;
    loop {
        if stopped() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn run_ffmpeg_with(args: &[String], stop: Option<&AtomicBool>) -> Result<()> {
    let stopped = || stop.is_some_and(|stop| stop.load(Ordering::Relaxed));
    if stopped() {
        bail!("stopped before ffmpeg started");
    }
    let ffmpeg = resolve_tools()?
        .ffmpeg
        .ok_or_else(|| anyhow::anyhow!("ffmpeg not found {}", where_looked()))?;
    let mut child = Command::new(ffmpeg)
        .args(QUIET_FFMPEG)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("start ffmpeg")?;
    let stderr = child.stderr.take().context("ffmpeg stderr")?;
    let reader = std::thread::spawn(move || read_tail(stderr, STDERR_TAIL_BYTES));
    let status = if stop.is_some() {
        let mut pause = Duration::from_millis(1);
        loop {
            if let Some(status) = child.try_wait().context("wait for ffmpeg")? {
                if !status.success() && stop_follows(stopped) {
                    drop(reader);
                    bail!("stopped while ffmpeg ran");
                }
                break status;
            }
            if stopped() {
                // Killed and then waited for, so no process is left behind.
                // The reader ends on its own once the pipe closes, and is
                // not waited for: what ffmpeg said no longer matters.
                let _ = child.kill();
                let _ = child.wait();
                drop(reader);
                bail!("stopped while ffmpeg ran");
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(STOP_POLL_MAX);
        }
    } else {
        child.wait().context("wait for ffmpeg")?
    };
    let tail = reader
        .join()
        .map_err(|_| anyhow::anyhow!("the thread reading ffmpeg's stderr panicked"))?
        .context("read ffmpeg stderr")?;
    if status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&tail);
    let said = said.trim();
    if said.is_empty() {
        bail!("ffmpeg failed ({status})")
    }
    bail!("ffmpeg failed ({status}): {said}")
}

/// Read `source` to its end and return its last `limit` bytes, starting at a
/// line when the start was dropped.
fn read_tail(mut source: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut tail = Vec::with_capacity(limit * 2);
    let mut chunk = [0u8; 8 * 1024];
    let mut dropped = false;
    loop {
        let read = match source.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        tail.extend_from_slice(&chunk[..read]);
        if tail.len() > limit * 2 {
            tail.drain(..tail.len() - limit);
            dropped = true;
        }
    }
    if tail.len() > limit {
        tail.drain(..tail.len() - limit);
        dropped = true;
    }
    if dropped && let Some(line_end) = tail.iter().position(|b| matches!(b, b'\n' | b'\r')) {
        tail.drain(..=line_end);
    }
    Ok(tail)
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Probe {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub bitrate: u64,
    /// Frames per second, `None` when ffprobe reported no rate.
    pub fps: Option<f32>,
}

/// Build a `Command` for ffprobe, resolved the same way as every other tool
/// lookup in this module. The one place that decides where ffprobe lives, so
/// [`probe_video`] and the public [`crate::probe_media`] agree with each
/// other and report the same error when the tool is missing.
///
/// # Errors
///
/// Returns a named "ffprobe not found…" error when ffprobe cannot be
/// resolved anywhere this module looks — deliberately, rather than falling
/// back to a bare `ffprobe` command name: a bare name that then fails to
/// spawn surfaces as an IO error pointing at the *input file* ("No such file
/// or directory"), which reads as a problem with the user's file rather than
/// a missing tool.
pub(crate) fn ffprobe_command() -> Result<Command> {
    let ffprobe = resolve_tools()?
        .ffprobe
        .ok_or_else(|| anyhow::anyhow!("ffprobe not found {}", where_looked()))?;
    let mut cmd = Command::new(ffprobe);
    cmd.stdin(Stdio::null());
    Ok(cmd)
}

/// Codec, width, height, frame rate, and bitrate of a video from ffprobe.
pub(crate) fn probe_video(path: &std::path::Path) -> Result<Probe> {
    let mut cmd = ffprobe_command()?;
    cmd.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=codec_name,width,height,avg_frame_rate,bit_rate",
        "-of",
        "csv=p=0",
        path.to_str().unwrap_or(""),
    ]);
    let output = cmd
        .output()
        .with_context(|| format!("run ffprobe on {}", path.display()))?;
    if !output.status.success() {
        bail!("ffprobe failed for {}", path.display());
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = line.trim().split(',').collect();
    let codec = parts.first().copied().unwrap_or("").to_ascii_lowercase();
    let width = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let height = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let fps = parts.get(3).and_then(|s| crate::probe::parse_frame_rate(s));
    let bitrate = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(Probe {
        codec,
        width,
        height,
        bitrate,
        fps,
    })
}

#[cfg(test)]
mod tests;
