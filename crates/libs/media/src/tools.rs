use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail};

/// Where this process looks for ffmpeg and ffprobe, and which files there
/// answered `-version`.
struct ToolsState {
    /// The Tools Directory, searched after `PATH`. The desktop app sets it
    /// for itself and passes it to the server it starts (`serve
    /// --tools-dir`); a server started by hand has none.
    tools_dir: Option<PathBuf>,
    /// The `PATH` searched instead of the process's own. Only tests set it
    /// (`set_search_path`), so a test decides what is found whether or
    /// not the machine has ffmpeg installed.
    search_path: Option<OsString>,
    generation: u64,
    /// The files that answered `-version`, by path, each beside the file as
    /// it was when it answered. Every lookup searches `PATH` and the Tools
    /// Directory again, which only reads file metadata, and a lookup runs
    /// once per file staged and once per Asset shown; the answer kept here
    /// means a file that answered is not run again until it changes.
    ///
    /// Only an answer is kept, never a failure. A file Gatekeeper blocks, one
    /// missing a shared library, or one that failed to start for a passing
    /// reason is run again on the next lookup, because what fixes it (an
    /// approval in System Settings, an installed library, the passing cause
    /// going away) leaves the file's modified time, size and permissions as
    /// they were, and a kept failure would hide the fix until a restart.
    answered: HashMap<PathBuf, FileStamp>,
}

/// What tells one version of a file from another without reading it: a
/// copy replaced, rewritten or made executable differs in one of these.
#[derive(PartialEq, Eq)]
struct FileStamp {
    /// When the file was last modified, if the platform says.
    modified: Option<SystemTime>,
    /// The file's size in bytes.
    len: u64,
    /// The file's permissions, which `chmod +x` changes.
    permissions: std::fs::Permissions,
    /// On Unix, the status change time, inode and device: a file replaced
    /// by another of the same size and modified time, as a copy that keeps
    /// times does, or moved in from elsewhere, differs in these.
    #[cfg(unix)]
    unix: (i64, i64, u64, u64),
}

impl FileStamp {
    fn of(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
            permissions: metadata.permissions(),
            #[cfg(unix)]
            unix: (
                metadata.ctime(),
                metadata.ctime_nsec(),
                metadata.ino(),
                metadata.dev(),
            ),
        }
    }
}

impl ToolsState {
    /// Forget which files answered, so the next lookup runs them again.
    fn forget(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.answered.clear();
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
            answered: HashMap::new(),
        })
    })
}

/// Name the Tools Directory, searched for ffmpeg and ffprobe after `PATH`,
/// and forget which files answered `-version` before.
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
/// again with `None`, and forget which files answered `-version` before.
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

/// True when ffmpeg and ffprobe are both on `PATH` and answer `-version`.
/// The desktop app downloads neither then, because a person who installed
/// ffmpeg chose it (`docs/adr/0019`).
pub fn ffmpeg_on_path() -> bool {
    let search_path = searched_path();
    ["ffmpeg", "ffprobe"]
        .iter()
        .all(|name| find_on_path(name, search_path.as_deref()).is_some())
}

/// The `PATH` the lookup searches: the one a test put in place
/// (`set_search_path`), else the process's own.
fn searched_path() -> Option<OsString> {
    tools_state()
        .lock()
        .expect("tools state lock")
        .search_path
        .clone()
        .or_else(|| std::env::var_os("PATH"))
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

/// ffmpeg and ffprobe, as the code that decides what to do with a file uses
/// them: the Media Stage's convert and compress passes ([`crate::process`])
/// and the Thumbnail and Preview the server makes ([`crate::versions`]).
///
/// That code only decides which file to re-encode, with which arguments,
/// and what to keep. Running the programs is behind this trait, so a test of
/// a decision passes a fake that answers fixed probes and writes fixed
/// output, and needs neither ffmpeg installed nor the lock over the tool
/// location (`testutil::real_ffmpeg_test_guard`). [`InstalledTools`] is
/// the one implementation outside tests.
///
/// [`MediaTools::probe`] and [`MediaTools::transcode`] run the programs;
/// [`MediaTools::require`] and [`MediaTools::can_probe`] say whether the
/// programs were found where they were looked for and answered `-version`.
pub(crate) trait MediaTools {
    /// Fail, naming what is missing and where it was looked for, when
    /// ffmpeg or ffprobe cannot be used ([`require_ffmpeg`]).
    fn require(&self) -> Result<()>;

    /// True when ffprobe can be used, so [`MediaTools::probe`] can answer.
    fn can_probe(&self) -> bool;

    /// What ffprobe reports about `path` ([`crate::probe_media`]).
    fn probe(&self, path: &Path) -> Result<crate::MediaProbe>;

    /// Run ffmpeg with `args` to write `dest`, which goes after them on
    /// ffmpeg's command line. ffmpeg is killed and the run fails when it
    /// runs longer than what it `reads` allows ([`ffmpeg_time_limit`]).
    /// With `stop`, setting it kills ffmpeg and fails the run
    /// ([`run_ffmpeg_with`]).
    fn transcode(
        &self,
        args: &[String],
        dest: &Path,
        reads: Reads<'_>,
        stop: Option<&AtomicBool>,
    ) -> Result<()>;
}

/// The ffmpeg and ffprobe this process finds on `PATH` or in the Tools
/// Directory ([`ffmpeg_tools`]), looked up again on every call, so a
/// program that arrives, moves or leaves is seen at once.
pub(crate) struct InstalledTools;

impl MediaTools for InstalledTools {
    fn require(&self) -> Result<()> {
        require_ffmpeg()
    }

    fn can_probe(&self) -> bool {
        ffprobe_available()
    }

    fn probe(&self, path: &Path) -> Result<crate::MediaProbe> {
        crate::probe_media(path)
    }

    fn transcode(
        &self,
        args: &[String],
        dest: &Path,
        reads: Reads<'_>,
        stop: Option<&AtomicBool>,
    ) -> Result<()> {
        let played = match reads {
            Reads::OneFrame => None,
            Reads::All(src) => crate::probe::played_for(src),
        };
        run_ffmpeg_with(
            &ffmpeg_command_line(args, dest),
            stop,
            ffmpeg_time_limit(played),
        )
    }
}

/// The arguments ffmpeg is given to write `dest`: `args`, then `dest`.
pub(crate) fn ffmpeg_command_line(args: &[String], dest: &Path) -> Vec<String> {
    let mut line = args.to_vec();
    line.push(dest.to_string_lossy().into_owned());
    line
}

/// How many times a program whose file is busy is started before it is
/// given up on.
///
/// Linux refuses to run a file that any process holds open for writing
/// (`ETXTBSY`, [`io::ErrorKind::ExecutableFileBusy`]). A child this process
/// starts on another thread holds every descriptor the process had open
/// from its fork until its exec, so a program written and run at once, a
/// tool the desktop app has just downloaded or a mock a test has just
/// written, is busy for that instant whenever another child is starting
/// (#2031). The instant ends when that child execs, a few system calls
/// later, so a bounded run of starts, each giving the processor up first,
/// outlasts it: a refused start costs about one spawn, and only a file that
/// stays open pays for all of them. A file still open for writing after
/// them, as one being written is, does not run.
///
/// Twenty is more than three times the most starts the instant has needed.
/// Over fifty runs of this crate's tests with 32 and 64 test threads on 32
/// processors, the most was six. Pinned to two loaded processors, as CI
/// has, it was two.
/// The number has to stay small, because a failure is not kept
/// ([`candidate_runs`]), so every lookup of a file that stays open for
/// writing makes every one of these starts. Twenty refused starts took
/// between a fifth and a half of a second on two loaded processors.
const BUSY_STARTS: usize = 20;

/// True when the program that `start` starts exits successfully, once its
/// file is free. While a start is refused because the file is busy, the
/// program is started again, up to [`BUSY_STARTS`] starts in all. A start
/// refused for any other reason is final.
fn runs_once_the_file_is_free(mut start: impl FnMut() -> io::Result<ExitStatus>) -> bool {
    let mut starts = 0;
    loop {
        starts += 1;
        match start() {
            Ok(status) => return status.success(),
            Err(err) if err.kind() == io::ErrorKind::ExecutableFileBusy && starts < BUSY_STARTS => {
                std::thread::yield_now();
            }
            Err(_) => return false,
        }
    }
}

/// True when running `bin` with `args` exits successfully, trying again
/// while the file is busy ([`runs_once_the_file_is_free`]).
fn command_runs(bin: &Path, args: &[&str]) -> bool {
    runs_once_the_file_is_free(|| {
        Command::new(bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    })
}

/// Where ffmpeg and ffprobe are, by [`find_tools`] over this process's
/// `PATH` and Tools Directory. Every call searches again, so a program that
/// arrives, moves or leaves is seen at once without a restart; only the
/// `-version` answer of a file is kept ([`ToolsState::answered`]).
fn resolve_tools() -> Result<FfmpegTools> {
    let search_path = searched_path();
    find_tools(search_path.as_deref(), tools_dir().as_deref())
}

/// The program `name` in `dir`, `.exe` added on Windows, when it is a file
/// that answers `-version`. The desktop app also asks this of a program in
/// the Tools Directory, to know that the file it keeps there runs.
pub fn tool_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidate = dir.join(executable_name(name));
    let metadata = std::fs::metadata(&candidate).ok()?;
    if !metadata.is_file() {
        return None;
    }
    candidate_runs(&candidate, FileStamp::of(&metadata)).then_some(candidate)
}

/// Whether `path` is a regular file with an executable bit that this
/// account may run. The file is not run. A bit alone is not enough: a file
/// another account owns with only the owner's bit, such as a root-owned
/// `0o744`, has one and can't be run from here.
#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        && rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

/// Whether `path` is a file. Windows runs any `.exe`, so there is no bit to
/// check.
#[cfg(not(unix))]
pub fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// True when `candidate`, as `file` describes it, answers `-version`. An
/// answer is kept in [`ToolsState::answered`], so the same file is run once
/// until it changes or the lookup is forgotten; a failure is not kept, so
/// the file is run again on the next lookup. No lock is held while it runs.
fn candidate_runs(candidate: &Path, file: FileStamp) -> bool {
    let generation = {
        let state = tools_state().lock().expect("tools state lock");
        if state.answered.get(candidate) == Some(&file) {
            return true;
        }
        state.generation
    };
    let runs = command_runs(candidate, &["-version"]);
    if runs {
        let mut state = tools_state().lock().expect("tools state lock");
        if state.generation == generation {
            state.answered.insert(candidate.to_path_buf(), file);
        }
    }
    runs
}

/// Find `name` in each directory of `search_path` in turn.
fn find_on_path(name: &str, search_path: Option<&OsStr>) -> Option<PathBuf> {
    search_path
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| tool_in_dir(&dir, name))
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
    let dir_ffmpeg = tools_dir.and_then(|dir| tool_in_dir(dir, "ffmpeg"));
    let dir_ffprobe = tools_dir.and_then(|dir| tool_in_dir(dir, "ffprobe"));
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

/// Flags after [`QUIET_FFMPEG`] on every ffmpeg run, and in front of every
/// ffprobe run: open local files and pipes, and nothing else. The files
/// come from phone backups, and a playlist or a container inside one can
/// name a network address that ffmpeg would otherwise open.
const LOCAL_FILES_ONLY: [&str; 2] = ["-protocol_whitelist", "file,pipe"];

/// How long ffprobe may run on one file before it is killed. Reading a
/// file's streams takes well under a second; this only ends a probe that
/// hangs, on a damaged file or a stalled disk.
const PROBE_TIME_LIMIT: Duration = Duration::from_secs(2 * 60);

/// The least time one ffmpeg run is given before it is killed, and all a run
/// that reads one frame or a still gets ([`Reads::OneFrame`]).
const SHORTEST_FFMPEG_TIME_LIMIT: Duration = Duration::from_secs(10 * 60);

/// How many seconds one ffmpeg run that reads a whole file is given for
/// each second the file plays ([`Reads::All`]). A conversion runs faster
/// than the file plays on most computers; twenty times leaves room for a
/// slow processor and a slow encoder.
const FFMPEG_SECONDS_PER_SECOND_PLAYED: u32 = 20;

/// The most time one ffmpeg run is given, whatever length a file claims.
/// A damaged or crafted file can claim any length.
const LONGEST_FFMPEG_TIME_LIMIT: Duration = Duration::from_secs(24 * 60 * 60);

/// How much of its source one ffmpeg run reads, which sets how long it may
/// run before it is killed ([`ffmpeg_time_limit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reads<'a> {
    /// A still image, or a video's first frame.
    OneFrame,
    /// Every frame or sample of the file at this path.
    All(&'a Path),
}

/// How long an ffmpeg run that reads a file playing for `played` may take:
/// [`FFMPEG_SECONDS_PER_SECOND_PLAYED`] times its length, at least
/// [`SHORTEST_FFMPEG_TIME_LIMIT`] and at most [`LONGEST_FFMPEG_TIME_LIMIT`],
/// rounded up to whole minutes. The shortest when the length is not known.
pub(crate) fn ffmpeg_time_limit(played: Option<Duration>) -> Duration {
    let limit = played.map_or(SHORTEST_FFMPEG_TIME_LIMIT, |played| {
        played
            .saturating_mul(FFMPEG_SECONDS_PER_SECOND_PLAYED)
            .clamp(SHORTEST_FFMPEG_TIME_LIMIT, LONGEST_FFMPEG_TIME_LIMIT)
    });
    Duration::from_secs(limit.as_secs().div_ceil(60) * 60)
}

/// [`run_ffmpeg_with`] with no stop and the shortest time limit, for a test
/// that makes its fixtures with the real ffmpeg or runs it on purpose.
#[cfg(test)]
pub(crate) fn run_ffmpeg(args: &[String]) -> Result<()> {
    run_ffmpeg_with(args, None, SHORTEST_FFMPEG_TIME_LIMIT)
}

/// [`run_ffmpeg_with`] with `stop`, for a test of stopping the real ffmpeg.
#[cfg(test)]
pub(crate) fn run_ffmpeg_until(args: &[String], stop: &AtomicBool) -> Result<()> {
    run_ffmpeg_with(args, Some(stop), SHORTEST_FFMPEG_TIME_LIMIT)
}

/// How long the wait for a program sleeps between looks, at most. It starts
/// at a millisecond and doubles, so a quick run is not held up and a long
/// one is stopped within this.
const STOP_POLL_MAX: Duration = Duration::from_millis(25);

/// How long a failed stoppable run waits for a stop before it reports the
/// failure. Ctrl-C in a terminal reaches ffmpeg and this process together,
/// and ffmpeg can exit before the handler here has set the stop; the failure
/// is then the stop's, and the caller must not read it as a bad file.
const STOP_GRACE: Duration = Duration::from_millis(200);

/// Whether `stopped` turns true within [`STOP_GRACE`].
fn stop_follows(stopped: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + STOP_GRACE;
    loop {
        if stopped() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Run ffmpeg with [`QUIET_FFMPEG`], `-nostdin`, [`LOCAL_FILES_ONLY`] and
/// `args`, failing with the end of its stderr when it exits non-zero.
///
/// A thread reads stderr while ffmpeg runs. [`QUIET_FFMPEG`] turns the stats
/// line off, but nothing limits how much else ffmpeg writes: a failure can
/// repeat an error for every frame, and an encoder such as libx265 writes to
/// stderr whatever `-loglevel` says. A pipe nobody reads fills at 64 KiB on
/// Linux, after which ffmpeg blocks on the write and never exits (#1178).
///
/// ffmpeg still running after `limit` is killed and waited for, and the run
/// fails with [`RanTooLong`], so a file that ffmpeg never finishes cannot
/// hold up the files after it (#2059). With `stop`, setting it kills ffmpeg,
/// waits for it, and fails the run. A run asked for after `stop` is set never
/// starts ffmpeg. What ffmpeg wrote before it was killed is the caller's to
/// remove.
fn run_ffmpeg_with(args: &[String], stop: Option<&AtomicBool>, limit: Duration) -> Result<()> {
    let started = Instant::now();
    run_ffmpeg_watched(args, stop, limit, &|| started.elapsed() >= limit)
}

/// [`run_ffmpeg_with`], with `expired` saying when `limit` has passed, so a
/// test decides that rather than a clock.
fn run_ffmpeg_watched(
    args: &[String],
    stop: Option<&AtomicBool>,
    limit: Duration,
    expired: &dyn Fn() -> bool,
) -> Result<()> {
    if stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
        bail!("stopped before ffmpeg started");
    }
    let ffmpeg = resolve_tools()?
        .ffmpeg
        .ok_or_else(|| anyhow::anyhow!("ffmpeg not found {}", where_looked()))?;
    let mut command = Command::new(ffmpeg);
    command
        .args(QUIET_FFMPEG)
        .arg("-nostdin")
        .args(LOCAL_FILES_ONLY)
        .args(args)
        .stdout(Stdio::null());
    let ran = run_watched(
        command,
        &Watch {
            program: "ffmpeg",
            stop,
            limit,
            expired,
        },
    )?;
    if ran.status.success() {
        return Ok(());
    }
    Err(FfmpegFailed {
        status: ran.status,
        said: String::from_utf8_lossy(&ran.stderr_tail).trim().to_string(),
    }
    .into())
}

/// Run ffprobe with [`LOCAL_FILES_ONLY`], `args`, then `path`, and answer
/// what it wrote to stdout. ffprobe still running after
/// [`PROBE_TIME_LIMIT`] is killed and the run fails with [`RanTooLong`].
///
/// # Errors
///
/// Returns an error when ffprobe is not found, cannot start, runs too long,
/// or exits non-zero, with the end of its stderr.
pub(crate) fn run_ffprobe(args: &[&str], path: &Path) -> Result<Vec<u8>> {
    let started = Instant::now();
    run_ffprobe_watched(args, path, &|| started.elapsed() >= PROBE_TIME_LIMIT)
}

/// [`run_ffprobe`], with `expired` saying when [`PROBE_TIME_LIMIT`] has
/// passed, so a test decides that rather than a clock.
fn run_ffprobe_watched(args: &[&str], path: &Path, expired: &dyn Fn() -> bool) -> Result<Vec<u8>> {
    let mut command = ffprobe_command()?;
    command
        .args(LOCAL_FILES_ONLY)
        .args(args)
        .arg(path)
        .stdout(Stdio::piped());
    let ran = run_watched(
        command,
        &Watch {
            program: "ffprobe",
            stop: None,
            limit: PROBE_TIME_LIMIT,
            expired,
        },
    )
    .with_context(|| format!("run ffprobe on {}", path.display()))?;
    if !ran.status.success() {
        bail!(
            "ffprobe failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&ran.stderr_tail).trim()
        );
    }
    Ok(ran.stdout.unwrap_or_default())
}

/// What ends a run of ffmpeg or ffprobe early, and what the errors call it.
struct Watch<'a> {
    /// The program's name, as the errors give it.
    program: &'static str,
    /// Setting it kills the program and fails the run.
    stop: Option<&'a AtomicBool>,
    /// How long the program may run, as [`RanTooLong`] gives it.
    limit: Duration,
    /// True once `limit` has passed since the program started.
    expired: &'a dyn Fn() -> bool,
}

/// How a program that exited ended, and what it wrote.
struct Ran {
    status: ExitStatus,
    /// Everything it wrote to stdout, when the command piped stdout.
    stdout: Option<Vec<u8>>,
    /// The last [`STDERR_TAIL_BYTES`] it wrote to stderr.
    stderr_tail: Vec<u8>,
}

/// Run `command` until it exits, reading its stderr, and its stdout when the
/// command pipes it, on threads of their own so a full pipe never blocks
/// it. When `watch` says it is stopped or its time is up, kill it and wait
/// for it, so no process is left behind.
fn run_watched(mut command: Command, watch: &Watch<'_>) -> Result<Ran> {
    let program = watch.program;
    let stopped = || watch.stop.is_some_and(|stop| stop.load(Ordering::Relaxed));
    let mut child = command
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("start {program}"))?;
    let stderr = child
        .stderr
        .take()
        .with_context(|| format!("{program} stderr"))?;
    let stderr_reader = std::thread::spawn(move || read_tail(stderr, STDERR_TAIL_BYTES));
    let stdout_reader = child.stdout.take().map(|mut stdout| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            stdout.read_to_end(&mut out).map(|_| out)
        })
    });
    let mut pause = Duration::from_millis(1);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("wait for {program}"))?
        {
            if !status.success() && watch.stop.is_some() && stop_follows(stopped) {
                bail!("stopped while {program} ran");
            }
            break status;
        }
        let ended: Option<anyhow::Error> = if stopped() {
            Some(anyhow::anyhow!("stopped while {program} ran"))
        } else if (watch.expired)() {
            Some(
                RanTooLong {
                    program,
                    limit: watch.limit,
                }
                .into(),
            )
        } else {
            None
        };
        if let Some(error) = ended {
            // Killed and then waited for, so no process is left behind. The
            // readers end on their own once the pipes close, and are not
            // waited for: what the program said no longer matters.
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(STOP_POLL_MAX);
    };
    let stderr_tail = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("the thread reading {program}'s stderr panicked"))?
        .with_context(|| format!("read {program} stderr"))?;
    let stdout = stdout_reader
        .map(|reader| {
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("the thread reading {program}'s stdout panicked"))?
                .with_context(|| format!("read {program} stdout"))
        })
        .transpose()?;
    Ok(Ran {
        status,
        stdout,
        stderr_tail,
    })
}

/// ffmpeg or ffprobe ran longer than it was given, and was killed. A caller
/// that keeps the reason finds this in the error's chain
/// (`anyhow::Error::downcast_ref`), as it finds [`FfmpegFailed`].
#[derive(Debug)]
pub struct RanTooLong {
    program: &'static str,
    limit: Duration,
}

impl std::fmt::Display for RanTooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} did not finish within {}, so it was stopped",
            self.program,
            spoken(self.limit)
        )
    }
}

impl std::error::Error for RanTooLong {}

#[cfg(any(test, feature = "testutil"))]
impl RanTooLong {
    /// ffmpeg killed after `limit`, for a test of what a caller makes of it.
    pub(crate) fn ffmpeg(limit: Duration) -> Self {
        Self {
            program: "ffmpeg",
            limit,
        }
    }
}

/// `limit` as a sentence gives it: in hours when it is whole hours, else in
/// minutes when it is whole minutes, else in seconds.
fn spoken(limit: Duration) -> String {
    let secs = limit.as_secs();
    let (count, unit) = if secs >= 3600 && secs.is_multiple_of(3600) {
        (secs / 3600, "hour")
    } else if secs >= 60 && secs.is_multiple_of(60) {
        (secs / 60, "minute")
    } else {
        (secs, "second")
    };
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

/// ffmpeg ran and exited with a failure: its exit status and the end of
/// what it wrote to stderr, which says what it found wrong with the file.
/// A caller that keeps the reason finds this in the error's chain
/// (`anyhow::Error::downcast_ref`), apart from the errors this crate writes
/// itself, such as ffmpeg not being found.
#[derive(Debug)]
pub struct FfmpegFailed {
    status: ExitStatus,
    said: String,
}

impl FfmpegFailed {
    /// The last lines ffmpeg wrote to stderr, trimmed, at most
    /// `STDERR_TAIL_BYTES`; empty when it said nothing. It names each file
    /// as it was passed to ffmpeg, so it can carry full paths.
    #[must_use]
    pub fn said(&self) -> &str {
        &self.said
    }
}

impl std::fmt::Display for FfmpegFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ffmpeg failed ({})", self.status)?;
        if !self.said.is_empty() {
            write!(f, ": {}", self.said)?;
        }
        Ok(())
    }
}

impl std::error::Error for FfmpegFailed {}

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

/// Build a `Command` for ffprobe, resolved the same way as every other tool
/// lookup in this module, for [`run_ffprobe`], the one function that runs
/// ffprobe.
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

#[cfg(test)]
mod tests;
