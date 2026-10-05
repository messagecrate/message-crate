use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};

struct ToolsState {
    override_dir: Option<PathBuf>,
    generation: u64,
    ffmpeg: Option<PathBuf>,
    ffprobe: Option<PathBuf>,
}

impl ToolsState {
    /// The cached location of `ffmpeg` or `ffprobe`.
    fn cached(&self, name: &str) -> Option<PathBuf> {
        match name {
            "ffmpeg" => self.ffmpeg.clone(),
            "ffprobe" => self.ffprobe.clone(),
            _ => None,
        }
    }

    /// Remember where `ffmpeg` or `ffprobe` was found.
    fn set_cached(&mut self, name: &str, path: Option<PathBuf>) {
        match name {
            "ffmpeg" => self.ffmpeg = path,
            "ffprobe" => self.ffprobe = path,
            _ => {}
        }
    }
}

/// The process-wide tool cache.
fn tools_state() -> &'static Mutex<ToolsState> {
    static STATE: OnceLock<Mutex<ToolsState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(ToolsState {
            override_dir: None,
            generation: 0,
            ffmpeg: None,
            ffprobe: None,
        })
    })
}

/// Store a directory-only override for ffmpeg/ffprobe discovery and clear cached paths.
pub fn set_tools_dir(dir: Option<PathBuf>) {
    let mut state = tools_state().lock().expect("tools state lock");
    state.override_dir = dir;
    state.generation = state.generation.wrapping_add(1);
    state.ffmpeg = None;
    state.ffprobe = None;
}

/// Current tools-directory override, if any (primarily for tests).
pub fn tools_dir() -> Option<PathBuf> {
    tools_state()
        .lock()
        .expect("tools state lock")
        .override_dir
        .clone()
}

/// Result of locating ffmpeg and ffprobe (the GUI's probe result type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegToolsProbe {
    /// Whether both tools were found and pass `-version`.
    pub ok: bool,
    /// Resolved ffmpeg path, if found.
    pub ffmpeg_path: Option<PathBuf>,
    /// Resolved ffprobe path, if found.
    pub ffprobe_path: Option<PathBuf>,
    /// Human-readable list of missing tools when `ok` is false.
    pub error: Option<String>,
}

/// True when both ffmpeg and ffprobe resolve from the search path.
pub fn ffmpeg_available() -> bool {
    resolve_tool("ffmpeg").is_some() && resolve_tool("ffprobe").is_some()
}

/// Fail with an installation hint when ffmpeg and ffprobe are not available.
pub(crate) fn require_ffmpeg() -> Result<()> {
    if ffmpeg_available() {
        Ok(())
    } else {
        bail!(
            "ffmpeg and ffprobe are required to convert or compress attachments. \
             Keep the bundled tools in lib/ next to this program, install ffmpeg on PATH, \
             or set MESSAGE_CRATE_BIN to a directory that contains both."
        )
    }
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

/// Resolve `ffmpeg` / `ffprobe`: tools-dir override, then beside the running
/// executable, `lib/` under its directory, `lib/` under its parent directory,
/// `MESSAGE_CRATE_BIN`, then PATH.
fn resolve_tool(name: &str) -> Option<PathBuf> {
    if !matches!(name, "ffmpeg" | "ffprobe") {
        let override_dir = tools_state()
            .lock()
            .expect("tools state lock")
            .override_dir
            .clone();
        return find_tool_with_override(name, override_dir.as_deref());
    }

    loop {
        let (generation, override_dir) = {
            let state = tools_state().lock().expect("tools state lock");
            if let Some(cached) = state.cached(name) {
                return Some(cached);
            }
            (state.generation, state.override_dir.clone())
        };

        let resolved = find_tool_with_override(name, override_dir.as_deref());

        let mut state = tools_state().lock().expect("tools state lock");
        if state.generation != generation {
            continue;
        }
        if state.cached(name).is_none() {
            state.set_cached(name, resolved.clone());
        }
        return resolved;
    }
}

/// The tool under `dir` if it exists and runs.
fn find_tool_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidate = dir.join(executable_name(name));
    if candidate.is_file() && command_runs(&candidate, &["-version"]) {
        Some(candidate)
    } else {
        None
    }
}

/// Probe both tools in an explicit directory, or fall back to the default
/// resolution path (tools-dir override, beside the executable, `MESSAGE_CRATE_BIN`, PATH).
pub fn probe_ffmpeg_tools(dir: Option<&Path>) -> FfmpegToolsProbe {
    let (ffmpeg, ffprobe) = match dir {
        Some(d) => (
            find_tool_in_dir(d, "ffmpeg"),
            find_tool_in_dir(d, "ffprobe"),
        ),
        None => (resolve_tool("ffmpeg"), resolve_tool("ffprobe")),
    };
    match (ffmpeg, ffprobe) {
        (Some(f), Some(p)) => FfmpegToolsProbe {
            ok: true,
            ffmpeg_path: Some(f),
            ffprobe_path: Some(p),
            error: None,
        },
        (f, p) => {
            let mut parts = Vec::new();
            if f.is_none() {
                parts.push("ffmpeg not found or failed -version");
            }
            if p.is_none() {
                parts.push("ffprobe not found or failed -version");
            }
            FfmpegToolsProbe {
                ok: false,
                ffmpeg_path: f,
                ffprobe_path: p,
                error: Some(parts.join("; ")),
            }
        }
    }
}

/// Locate a tool: in the override directory when set, else beside the program, in `lib/`,
/// in `MESSAGE_CRATE_BIN`, or on PATH.
fn find_tool_with_override(name: &str, override_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = override_dir {
        return find_tool_in_dir(dir, name);
    }

    let executable = executable_name(name);

    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        let candidates = [
            dir.join(&executable),
            dir.join("lib").join(&executable),
            dir.parent()
                .map(|p| p.join("lib").join(&executable))
                .unwrap_or_default(),
        ];
        for candidate in candidates {
            if candidate.as_os_str().is_empty() {
                continue;
            }
            if candidate.is_file() && command_runs(&candidate, &["-version"]) {
                return Some(candidate);
            }
        }
    }

    if let Some(extra) = std::env::var_os("MESSAGE_CRATE_BIN") {
        let candidate = PathBuf::from(extra).join(&executable);
        if candidate.is_file() && command_runs(&candidate, &["-version"]) {
            return Some(candidate);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join(&executable);
            if candidate.is_file() && command_runs(&candidate, &["-version"]) {
                return Some(candidate);
            }
        }
    }

    // Last resort: bare name (PATH lookup by the OS / shell semantics).
    let bare = PathBuf::from(&executable);
    if command_runs(&bare, &["-version"]) {
        return Some(bare);
    }

    None
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
    let ffmpeg = resolve_tool("ffmpeg").ok_or_else(|| {
        anyhow::anyhow!(
            "ffmpeg not found in lib/ (or beside this program), in MESSAGE_CRATE_BIN, or on PATH"
        )
    })?;
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
    let ffprobe = resolve_tool("ffprobe").ok_or_else(|| {
        anyhow::anyhow!(
            "ffprobe not found in lib/ (or beside this program), in MESSAGE_CRATE_BIN, or on PATH"
        )
    })?;
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
