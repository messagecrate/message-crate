use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail};

mod digest;
pub use digest::file_has_sha256;
#[cfg(any(test, feature = "testutil"))]
pub use digest::times_read;

/// The files that answered `-version`, by path, each beside the file as it
/// was when it answered. Every lookup searches `PATH` and the Tools Directory
/// again, which only reads file metadata, and a lookup runs once per file
/// staged and once per Asset shown; the answer kept here means a file that
/// answered is not run again until it changes. It is a fact about the file,
/// not about where anyone looks, so one map serves every [`InstalledTools`]
/// in the process.
///
/// Only an answer is kept, never a failure. A file Gatekeeper blocks, one
/// missing a shared library, or one that failed to start for a passing
/// reason is run again on the next lookup, because what fixes it (an
/// approval in System Settings, an installed library, the passing cause
/// going away) leaves the file's modified time, size and permissions as
/// they were, and a kept failure would hide the fix until a restart.
fn answered() -> &'static Mutex<HashMap<PathBuf, FileStamp>> {
    static ANSWERED: OnceLock<Mutex<HashMap<PathBuf, FileStamp>>> = OnceLock::new();
    ANSWERED.get_or_init(|| Mutex::new(HashMap::new()))
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

/// ffmpeg and ffprobe, as the code that decides what to do with a file uses
/// them: the Media Stage's convert and compress passes ([`crate::process`]),
/// the size forecast's probe, and the Thumbnail and Preview the server makes
/// ([`crate::versions`]).
///
/// That code only decides which file to re-encode, with which arguments,
/// and what to keep. Running the programs is behind this trait, and the
/// caller passes the implementation in, so a test of a decision, in this
/// crate or another, passes a fake that answers fixed probes and writes
/// fixed output (`testutil::FakeTools`) and needs no ffmpeg installed.
/// [`InstalledTools`] is the one implementation outside tests.
///
/// [`MediaTools::probe`] and [`MediaTools::transcode`] run the programs;
/// [`MediaTools::find`], [`MediaTools::require`] and
/// [`MediaTools::can_probe`] say whether the programs were found where they
/// were looked for and answered `-version`. [`MediaTools::tools_dir`] says
/// where the app's own copies are kept, which is also where the WhatsApp
/// exporter finds `wtsexporter`.
pub trait MediaTools: Send + Sync + std::fmt::Debug {
    /// The Tools Directory these programs are looked for in after `PATH`,
    /// if any. `None` unless an implementation names one.
    fn tools_dir(&self) -> Option<&Path> {
        None
    }

    /// Where ffmpeg and ffprobe are.
    ///
    /// # Errors
    ///
    /// Returns an error when the two are found in different places, which
    /// leaves neither used ([`MediaTools::find`]).
    fn find(&self) -> Result<FfmpegTools>;

    /// Fail, naming what is missing and where it was looked for, when
    /// ffmpeg or ffprobe cannot be used.
    ///
    /// # Errors
    ///
    /// Returns an error when either program is missing, or when the two are
    /// in different places.
    fn require(&self) -> Result<()>;

    /// True when ffmpeg and ffprobe can both be used ([`MediaTools::require`]).
    fn available(&self) -> bool {
        self.require().is_ok()
    }

    /// True when ffprobe can be used, so [`MediaTools::probe`] can answer.
    fn can_probe(&self) -> bool;

    /// What ffprobe reports about `path`'s first video stream.
    ///
    /// # Errors
    ///
    /// Returns an error when ffprobe is missing, exits non-zero, or reports
    /// a line that cannot be read.
    fn probe(&self, path: &Path) -> Result<crate::MediaProbe>;

    /// Run ffmpeg with `args` to write `dest`, which goes after them on
    /// ffmpeg's command line. What it `reads` decides when ffmpeg is killed
    /// and the run fails ([`Killed`]): a run that reads one frame after the
    /// short-step limit of 15 seconds, and one that reads the whole file once it makes
    /// no progress for the no-progress wait
    /// ([`InstalledTools::with_stall_wait`]) or runs past twice the file's
    /// length. With `stop`, setting it kills ffmpeg and fails the run as
    /// stopped.
    ///
    /// # Errors
    ///
    /// Returns an error when ffmpeg is missing, fails, is killed, or is
    /// stopped.
    fn transcode(
        &self,
        args: &[String],
        dest: &Path,
        reads: Reads<'_>,
        stop: Option<&AtomicBool>,
    ) -> Result<()>;
}

/// No ffmpeg or ffprobe at all: what a run is given when its caller passes
/// none, as one that only copies its attachments does. Every lookup fails,
/// saying no tools were given, so a caller that forgets to pass its tools to
/// a run that converts fails at once rather than looking somewhere it was
/// not told to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoTools;

impl NoTools {
    /// The error every lookup fails with.
    fn error() -> anyhow::Error {
        anyhow::anyhow!("no ffmpeg or ffprobe was given to this run")
    }
}

impl MediaTools for NoTools {
    fn find(&self) -> Result<FfmpegTools> {
        Err(Self::error())
    }

    fn require(&self) -> Result<()> {
        Err(Self::error())
    }

    fn can_probe(&self) -> bool {
        false
    }

    fn probe(&self, _path: &Path) -> Result<crate::MediaProbe> {
        Err(Self::error())
    }

    fn transcode(
        &self,
        _args: &[String],
        _dest: &Path,
        _reads: Reads<'_>,
        _stop: Option<&AtomicBool>,
    ) -> Result<()> {
        Err(Self::error())
    }
}

/// The ffmpeg and ffprobe installed on this computer: both on `PATH`, else
/// both in the Tools Directory the caller names, looked up again on every
/// call, so a program that arrives, moves or leaves is seen at once.
///
/// The caller builds it from what it was told: the desktop app from its own
/// Tools Directory, the server from `--tools-dir` and `[server]
/// transcode_stall_secs`. Nothing else in the process says where to look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledTools {
    /// The Tools Directory, searched after `PATH`. The desktop app names its
    /// own and passes it to the server it starts (`serve --tools-dir`); a
    /// server started by hand has none.
    tools_dir: Option<PathBuf>,
    /// The `PATH` searched instead of the process's own. Only tests set it
    /// ([`InstalledTools::searching`]), so a test decides what is found
    /// whether or not the machine has ffmpeg installed.
    search_path: Option<OsString>,
    /// How long a conversion that reads a whole file may go without moving
    /// forward before it is killed.
    stall_wait: Duration,
}

impl Default for InstalledTools {
    /// ffmpeg and ffprobe on `PATH` only, with the default no-progress wait.
    fn default() -> Self {
        Self::new(None)
    }
}

impl InstalledTools {
    /// The programs on `PATH`, else in `tools_dir`, with the default
    /// no-progress wait ([`DEFAULT_STALL_WAIT`]).
    #[must_use]
    pub const fn new(tools_dir: Option<PathBuf>) -> Self {
        Self {
            tools_dir,
            search_path: None,
            stall_wait: DEFAULT_STALL_WAIT,
        }
    }

    /// These programs, with a conversion that reads a whole file killed
    /// once it goes `wait` without moving forward (#2898). The server passes
    /// `[server] transcode_stall_secs`. A wait under a millisecond is taken
    /// as one.
    #[must_use]
    pub fn with_stall_wait(mut self, wait: Duration) -> Self {
        self.stall_wait = wait.max(Duration::from_millis(1));
        self
    }

    /// These programs, searched for on `search_path` instead of the
    /// process's `PATH`, so a test decides what is found.
    #[cfg(any(test, feature = "testutil"))]
    #[must_use]
    pub fn searching(mut self, search_path: impl Into<OsString>) -> Self {
        self.search_path = Some(search_path.into());
        self
    }

    /// The no-progress wait ([`InstalledTools::with_stall_wait`]).
    #[must_use]
    pub fn stall_wait(&self) -> Duration {
        self.stall_wait
    }

    /// Where ffmpeg is, by [`MediaTools::find`]. `None` when it is not
    /// found, does not run, or is in a different place from ffprobe.
    #[must_use]
    pub fn ffmpeg_path(&self) -> Option<PathBuf> {
        self.find().ok().and_then(|tools| tools.ffmpeg)
    }

    /// Where ffprobe is, by [`MediaTools::find`]. `None` when it is not
    /// found, does not run, or is in a different place from ffmpeg.
    #[must_use]
    pub fn ffprobe_path(&self) -> Option<PathBuf> {
        self.find().ok().and_then(|tools| tools.ffprobe)
    }

    /// True when ffmpeg and ffprobe are both on `PATH` and answer
    /// `-version`. The desktop app downloads neither then, because a person
    /// who installed ffmpeg chose it (`docs/adr/0019`).
    #[must_use]
    pub fn on_path(&self) -> bool {
        let search_path = self.searched_path();
        ["ffmpeg", "ffprobe"]
            .iter()
            .all(|name| find_on_path(name, search_path.as_deref()).is_some())
    }

    /// The `PATH` the lookup searches: the one a test put in place
    /// ([`InstalledTools::searching`]), else the process's own.
    fn searched_path(&self) -> Option<OsString> {
        self.search_path
            .clone()
            .or_else(|| std::env::var_os("PATH"))
    }

    /// The places ffmpeg and ffprobe are looked for, as an error names them.
    fn where_looked(&self) -> String {
        match self.tools_dir() {
            Some(dir) => format!("on PATH or in the Tools Directory, {}", dir.display()),
            None => "on PATH".to_string(),
        }
    }

    /// Where ffmpeg is, or an error naming where it was looked for.
    fn ffmpeg(&self) -> Result<PathBuf> {
        self.find()?
            .ffmpeg
            .ok_or_else(|| anyhow::anyhow!("ffmpeg not found {}", self.where_looked()))
    }

    /// Where ffprobe is, or a named "ffprobe not found…" error when it
    /// cannot be resolved anywhere this looks — deliberately, rather than
    /// falling back to a bare `ffprobe` command name: a bare name that then
    /// fails to spawn surfaces as an IO error pointing at the *input file*
    /// ("No such file or directory"), which reads as a problem with the
    /// user's file rather than a missing tool.
    fn ffprobe(&self) -> Result<PathBuf> {
        self.find()?
            .ffprobe
            .ok_or_else(|| anyhow::anyhow!("ffprobe not found {}", self.where_looked()))
    }
}

impl MediaTools for InstalledTools {
    fn tools_dir(&self) -> Option<&Path> {
        self.tools_dir.as_deref()
    }

    /// Both on `PATH`, else both in the Tools Directory ([`find_tools`]). Every call searches again, so a program
    /// that arrives, moves or leaves is seen at once without a restart; only
    /// the `-version` answer of a file is kept ([`answered`]).
    ///
    /// # Errors
    ///
    /// Returns an error naming both paths when one is found only on `PATH`
    /// and the other only in the Tools Directory, because two builds of
    /// different versions would then work on one file.
    fn find(&self) -> Result<FfmpegTools> {
        let search_path = self.searched_path();
        find_tools(search_path.as_deref(), self.tools_dir())
    }

    /// Fail, naming which tool is missing and where it was looked for, when
    /// ffmpeg or ffprobe is not found.
    ///
    /// # Errors
    ///
    /// Returns an error when either tool is missing, or when the two are in
    /// different places ([`MediaTools::find`]).
    fn require(&self) -> Result<()> {
        let missing = self.find()?.missing();
        if missing.is_empty() {
            return Ok(());
        }
        bail!(
            "ffmpeg and ffprobe are required to convert or compress attachments. {} {} not found {}.",
            missing.join(" and "),
            if missing.len() == 1 { "was" } else { "were" },
            self.where_looked()
        )
    }

    fn can_probe(&self) -> bool {
        self.ffprobe_path().is_some()
    }

    fn probe(&self, path: &Path) -> Result<crate::MediaProbe> {
        crate::probe::probe_media(self, path)
    }

    fn transcode(
        &self,
        args: &[String],
        dest: &Path,
        reads: Reads<'_>,
        stop: Option<&AtomicBool>,
    ) -> Result<()> {
        if is_set(stop) {
            return Err(Killed::stopped_before_start(Program::Ffmpeg).into());
        }
        let limit = match reads {
            Reads::OneFrame => Limit::ShortStep,
            Reads::All(src) => Limit::Progress(StallWatch::new(
                self.stall_wait,
                crate::probe::played_for(self, src, stop),
            )),
        };
        let started = Instant::now();
        self.run_ffmpeg_watched(&ffmpeg_command_line(args, dest), stop, limit, &|| {
            started.elapsed()
        })
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

/// What `start` started, once the program's file is free. While a start is
/// refused because the file is busy, the program is started again, up to
/// [`BUSY_STARTS`] starts in all. A start refused for any other reason is
/// final, and answers `None`.
fn started_once_the_file_is_free<T>(mut start: impl FnMut() -> io::Result<T>) -> Option<T> {
    let mut starts = 0;
    loop {
        starts += 1;
        match start() {
            Ok(started) => return Some(started),
            Err(err) if err.kind() == io::ErrorKind::ExecutableFileBusy && starts < BUSY_STARTS => {
                std::thread::yield_now();
            }
            Err(_) => return None,
        }
    }
}

/// True when running `bin` with `args` exits successfully within
/// [`SHORT_STEP_LIMIT`], trying again while the file is busy
/// ([`started_once_the_file_is_free`]). A program still running at the
/// limit is killed and waited for, and does not run (#2883).
fn command_runs(bin: &Path, args: &[&str]) -> bool {
    let started = Instant::now();
    command_runs_watched(bin, args, &|| started.elapsed())
}

/// [`command_runs`], with `now` saying how long the program has run, so a
/// test decides that rather than a clock.
fn command_runs_watched(bin: &Path, args: &[&str], now: &dyn Fn() -> Duration) -> bool {
    let Some(mut child) = started_once_the_file_is_free(|| {
        Command::new(bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }) else {
        return false;
    };
    let waited = wait_or_kill(&mut child, || short_step_over(now()));
    matches!(waited, Ok(Ok(status)) if status.success())
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
/// answer is kept in [`answered`], so the same file is run once until it
/// changes; a failure is not kept, so the file is run again on the next
/// lookup. No lock is held while it runs.
fn candidate_runs(candidate: &Path, file: FileStamp) -> bool {
    if answered().lock().expect("answered lock").get(candidate) == Some(&file) {
        return true;
    }
    let runs = command_runs(candidate, &["-version"]);
    if runs {
        answered()
            .lock()
            .expect("answered lock")
            .insert(candidate.to_path_buf(), file);
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

/// Flags after [`QUIET_FFMPEG`] on every ffmpeg run: write progress, with
/// `out_time`, to stdout, which is free because ffmpeg writes its output to
/// a file. The stats line on stderr stays off.
const PROGRESS_ON_STDOUT: [&str; 2] = ["-progress", "pipe:1"];

/// Flags after [`PROGRESS_ON_STDOUT`] on every ffmpeg run, and in front of every
/// ffprobe run: open local files and pipes, and nothing else. The files
/// come from phone backups, and a playlist or a container inside one can
/// name a network address that ffmpeg would otherwise open.
const LOCAL_FILES_ONLY: [&str; 2] = ["-protocol_whitelist", "file,pipe"];

/// How long a short step may run before it is killed: ffprobe reading a
/// file's details, ffmpeg reading one frame ([`Reads::OneFrame`]), and the
/// `-version` run that finds ffmpeg and ffprobe. Each takes well under a
/// second on a working file; this only ends one that hangs, on a damaged
/// file, a stalled disk or a program that never answers. It is fixed, not a
/// setting (#2898).
pub(crate) const SHORT_STEP_LIMIT: Duration = Duration::from_secs(15);

/// How long a conversion that reads a whole file may go without moving
/// forward before it is killed, when the caller names no other
/// ([`InstalledTools::with_stall_wait`]).
pub const DEFAULT_STALL_WAIT: Duration = Duration::from_secs(120);

/// How much of its source one ffmpeg run reads, which decides what ends it
/// early ([`MediaTools::transcode`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reads<'a> {
    /// A still image, or a video's first frame: a short step.
    OneFrame,
    /// Every frame or sample of the file at this path, watched for progress.
    All(&'a Path),
}

impl<'a> Reads<'a> {
    /// The file a run reads whole; `None` for a run that reads one frame.
    #[must_use]
    pub fn source(self) -> Option<&'a Path> {
        match self {
            Self::OneFrame => None,
            Self::All(src) => Some(src),
        }
    }
}

/// Why a short step that has run for `ran` must end: when it has run
/// [`SHORT_STEP_LIMIT`].
fn short_step_over(ran: Duration) -> Option<KillReason> {
    (ran >= SHORT_STEP_LIMIT).then_some(KillReason::ShortStepTooLong {
        limit: SHORT_STEP_LIMIT,
    })
}

/// What ends one run of ffmpeg or ffprobe early, besides a stop.
pub(crate) enum Limit {
    /// A short step, killed after [`SHORT_STEP_LIMIT`].
    ShortStep,
    /// A conversion that reads a whole file, killed when [`StallWatch`]
    /// says so.
    Progress(StallWatch),
}

/// Whether a conversion that reads a whole file is still moving forward,
/// from the position ffmpeg reports (`out_time` on its `-progress` stream).
///
/// The position is watched, not the size of what ffmpeg writes: a hung
/// ffmpeg can keep writing without moving forward. A conversion that keeps
/// moving runs as long as it needs, so a long video on a slow computer is
/// never cut off for its length (#2898).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StallWatch {
    /// How long the position may stay where it is.
    wait: Duration,
    /// How long the source plays, when ffprobe read it.
    length: Option<Duration>,
    /// The furthest position seen.
    furthest: Duration,
    /// When the position last moved forward, or the run started.
    moved_at: Duration,
    /// When the position reached the end of the source, if it has.
    reached_end_at: Option<Duration>,
}

/// How close to the source's length a position counts as its end. ffmpeg's
/// last `out_time` is the start of the last frame or packet, a little short
/// of the length ffprobe reads.
const END_TOLERANCE: Duration = Duration::from_secs(1);

impl StallWatch {
    /// A watch over a run that has just started, with the no-progress
    /// `wait`, of a source that plays for `length` when known.
    pub(crate) fn new(wait: Duration, length: Option<Duration>) -> Self {
        Self {
            wait,
            length,
            furthest: Duration::ZERO,
            moved_at: Duration::ZERO,
            reached_end_at: None,
        }
    }

    /// Take the latest position ffmpeg reported, if any, at `now`, the time
    /// since the run started, and say why the run must end, if it must:
    /// the position passed twice the source's length, or it has not moved
    /// for the wait.
    ///
    /// Once the position reaches the source's end, ffmpeg is finishing the
    /// file, and with `-movflags +faststart` it rewrites the whole output
    /// then without reporting progress: on a slow disk that takes minutes
    /// for a large video. The wait is then at least twice the time the run
    /// took to reach the end, since the rewrite reads and writes what the
    /// run wrote once.
    pub(crate) fn observe(
        &mut self,
        position: Option<Duration>,
        now: Duration,
    ) -> Option<KillReason> {
        if let Some(position) = position
            && position > self.furthest
        {
            self.furthest = position;
            self.moved_at = now;
        }
        if let Some(length) = self.length {
            if self.furthest > length.saturating_mul(2) {
                return Some(KillReason::RanPastLength { length });
            }
            if self.reached_end_at.is_none()
                && !self.furthest.is_zero()
                && self.furthest + END_TOLERANCE >= length
            {
                self.reached_end_at = Some(self.moved_at);
            }
        }
        let wait = self
            .reached_end_at
            .map_or(self.wait, |at| self.wait.max(at.saturating_mul(2)));
        if now.saturating_sub(self.moved_at) < wait {
            return None;
        }
        Some(if self.furthest.is_zero() {
            KillReason::StalledBeforeOutput { wait }
        } else {
            KillReason::StalledPartway {
                wait,
                at: self.furthest,
            }
        })
    }
}

/// The position on one line of ffmpeg's `-progress` stream, when the line
/// is its `out_time`: `out_time=HH:MM:SS.micro`. `None` for any other line,
/// and for `N/A`. ffmpeg can write a negative time before the first frame,
/// which is the start.
pub(crate) fn out_time(line: &str) -> Option<Duration> {
    let value = line.trim().strip_prefix("out_time=")?;
    if value.starts_with('-') {
        return Some(Duration::ZERO);
    }
    let mut parts = value.splitn(3, ':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let whole = hours
        .checked_mul(3600)?
        .checked_add(minutes.checked_mul(60)?)?;
    Duration::from_secs(whole).checked_add(Duration::try_from_secs_f64(seconds).ok()?)
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

impl InstalledTools {
    /// Run ffmpeg as a short step, with no stop, for a test that makes its
    /// fixtures with the real ffmpeg or runs it on purpose.
    #[cfg(test)]
    pub(crate) fn run_ffmpeg(&self, args: &[String]) -> Result<()> {
        let started = Instant::now();
        self.run_ffmpeg_watched(args, None, Limit::ShortStep, &|| started.elapsed())
    }

    /// [`InstalledTools::run_ffmpeg`] with `stop`, for a test of stopping
    /// the real ffmpeg.
    #[cfg(test)]
    pub(crate) fn run_ffmpeg_until(&self, args: &[String], stop: &AtomicBool) -> Result<()> {
        let started = Instant::now();
        self.run_ffmpeg_watched(args, Some(stop), Limit::ShortStep, &|| started.elapsed())
    }

    /// Run ffmpeg with [`QUIET_FFMPEG`], [`PROGRESS_ON_STDOUT`], `-nostdin`,
    /// [`LOCAL_FILES_ONLY`] and `args`, failing with the end of its stderr
    /// when it exits non-zero ([`FfmpegFailed`]).
    ///
    /// A thread reads stderr while ffmpeg runs. [`QUIET_FFMPEG`] turns the
    /// stats line off, but nothing limits how much else ffmpeg writes: a
    /// failure can repeat an error for every frame, and an encoder such as
    /// libx265 writes to stderr whatever `-loglevel` says. A pipe nobody
    /// reads fills at 64 KiB on Linux, after which ffmpeg blocks on the
    /// write and never exits (#1178). Another thread reads the progress
    /// ffmpeg writes to stdout.
    ///
    /// `limit` decides when ffmpeg is killed early, and `now` says how long
    /// it has run, so a test decides that rather than a clock. ffmpeg killed
    /// is waited for, and the run fails with [`Killed`], so a file ffmpeg
    /// never finishes cannot hold up the files after it (#2059, #2898).
    /// With `stop`, setting it kills ffmpeg, waits for it, and fails the
    /// run. A run asked for after `stop` is set never starts ffmpeg. What
    /// ffmpeg wrote before it was killed is the caller's to remove.
    pub(crate) fn run_ffmpeg_watched(
        &self,
        args: &[String],
        stop: Option<&AtomicBool>,
        limit: Limit,
        now: &dyn Fn() -> Duration,
    ) -> Result<()> {
        let mut command = Command::new(self.ffmpeg()?);
        command
            .args(QUIET_FFMPEG)
            .args(PROGRESS_ON_STDOUT)
            .arg("-nostdin")
            .args(LOCAL_FILES_ONLY)
            .args(args)
            .stdout(Stdio::piped());
        let ran = run_watched(
            command,
            Watch {
                program: Program::Ffmpeg,
                stop,
                limit,
                now,
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

    /// Run ffprobe with [`LOCAL_FILES_ONLY`], `args`, then `path`, and
    /// answer what it wrote to stdout. ffprobe still running after
    /// [`SHORT_STEP_LIMIT`] is killed and the run fails with [`Killed`].
    /// With `stop`, setting it kills ffprobe and fails the run, and a run
    /// asked for after it is set never starts ffprobe.
    ///
    /// # Errors
    ///
    /// Returns an error when ffprobe is not found, cannot start, runs too
    /// long, is stopped, or exits non-zero, with the end of its stderr.
    pub(crate) fn run_ffprobe(
        &self,
        args: &[&str],
        path: &Path,
        stop: Option<&AtomicBool>,
    ) -> Result<Vec<u8>> {
        let started = Instant::now();
        self.run_ffprobe_watched(args, path, stop, &|| started.elapsed())
            .map(|ran| ran.stdout)
    }

    /// [`InstalledTools::run_ffprobe`], answering what ffprobe wrote to
    /// stderr beside what it wrote to stdout, for a run whose warnings
    /// matter.
    ///
    /// # Errors
    ///
    /// As [`InstalledTools::run_ffprobe`].
    pub(crate) fn run_ffprobe_with_stderr(
        &self,
        args: &[&str],
        path: &Path,
        stop: Option<&AtomicBool>,
    ) -> Result<(Vec<u8>, String)> {
        let started = Instant::now();
        let ran = self.run_ffprobe_watched(args, path, stop, &|| started.elapsed())?;
        Ok((
            ran.stdout,
            String::from_utf8_lossy(&ran.stderr_tail).into_owned(),
        ))
    }

    /// [`InstalledTools::run_ffprobe`], with `now` saying how long ffprobe
    /// has run, so a test decides that rather than a clock.
    fn run_ffprobe_watched(
        &self,
        args: &[&str],
        path: &Path,
        stop: Option<&AtomicBool>,
        now: &dyn Fn() -> Duration,
    ) -> Result<Ran> {
        let mut command = Command::new(self.ffprobe()?);
        command
            .stdin(Stdio::null())
            .args(LOCAL_FILES_ONLY)
            .args(args)
            .arg(path)
            .stdout(Stdio::piped());
        let ran = run_watched(
            command,
            Watch {
                program: Program::Ffprobe,
                stop,
                limit: Limit::ShortStep,
                now,
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
        Ok(ran)
    }
}

/// The program a [`Watch`] runs, named as the errors name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Program {
    Ffmpeg,
    Ffprobe,
}

impl std::fmt::Display for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
        })
    }
}

/// Whether `stop` is given and set.
fn is_set(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|stop| stop.load(Ordering::Relaxed))
}

/// What ends a run of ffmpeg or ffprobe early, and what the errors call it.
struct Watch<'a> {
    /// The program, as the errors name it.
    program: Program,
    /// Setting it kills the program and fails the run.
    stop: Option<&'a AtomicBool>,
    /// What else kills it.
    limit: Limit,
    /// How long the program has run.
    now: &'a dyn Fn() -> Duration,
}

/// How a program that exited ended, and what it wrote.
#[derive(Debug)]
struct Ran {
    status: ExitStatus,
    /// Everything it wrote to stdout. Empty for a conversion, whose stdout
    /// is its progress.
    stdout: Vec<u8>,
    /// The last [`STDERR_TAIL_BYTES`] it wrote to stderr.
    stderr_tail: Vec<u8>,
}

/// No position read from ffmpeg's progress yet, in [`run_watched`]'s
/// shared position.
const NO_POSITION: u64 = u64::MAX;

/// Read ffmpeg's `-progress` stream from `source` to its end, putting each
/// `out_time` into `position` in microseconds.
fn read_progress(source: impl Read, position: &AtomicU64) -> io::Result<()> {
    use std::io::BufRead;
    for line in io::BufReader::new(source).lines() {
        if let Some(at) = out_time(&line?) {
            let micros = u64::try_from(at.as_micros()).unwrap_or(NO_POSITION - 1);
            position.store(micros.min(NO_POSITION - 1), Ordering::Release);
        }
    }
    Ok(())
}

/// Run `command` until it exits, reading its stderr, and its stdout when the
/// command pipes it, on threads of their own so a full pipe never blocks
/// it. When `watch` says it is stopped or must end, kill it and wait for
/// it, so no process is left behind.
fn run_watched(mut command: Command, watch: Watch<'_>) -> Result<Ran> {
    let Watch {
        program,
        stop,
        mut limit,
        now,
    } = watch;
    let stopped = || is_set(stop);
    if stopped() {
        return Err(Killed::stopped_before_start(program).into());
    }
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
    let position = Arc::new(AtomicU64::new(NO_POSITION));
    let progress = matches!(limit, Limit::Progress(_));
    let stdout_reader = child.stdout.take().map(|mut stdout| {
        let position = Arc::clone(&position);
        std::thread::spawn(move || {
            if progress {
                read_progress(stdout, &position).map(|()| Vec::new())
            } else {
                let mut out = Vec::new();
                stdout.read_to_end(&mut out).map(|_| out)
            }
        })
    });
    let waited = wait_or_kill(&mut child, || {
        if stopped() {
            return Some(KillReason::Stopped);
        }
        match &mut limit {
            Limit::ShortStep => short_step_over(now()),
            Limit::Progress(watch) => {
                // The time first, then the position: a position stored
                // before the time was read is always seen with it, so a
                // test's clock that starts once the mock has written its
                // progress never meets a position not yet loaded.
                let ran = now();
                let micros = position.load(Ordering::Acquire);
                let at = (micros != NO_POSITION).then(|| Duration::from_micros(micros));
                watch.observe(at, ran)
            }
        }
    })
    .with_context(|| format!("wait for {program}"))?;
    let status = match waited {
        Ok(status) => status,
        // Killed and waited for, so no process is left behind. The readers
        // end on their own once the pipes close, and are not waited for:
        // what the program said no longer matters.
        Err(why) => return Err(Killed { program, why }.into()),
    };
    if !status.success() && stop.is_some() && stop_follows(stopped) {
        return Err(Killed {
            program,
            why: KillReason::Stopped,
        }
        .into());
    }
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
        .transpose()?
        .unwrap_or_default();
    Ok(Ran {
        status,
        stdout,
        stderr_tail,
    })
}

/// Wait for `child` to exit, asking `must_end` between looks. When it gives
/// a reason, kill the child and wait for it, so no process is left behind,
/// and answer the reason.
///
/// # Errors
///
/// Returns an error when the child's state cannot be read.
fn wait_or_kill(
    child: &mut Child,
    mut must_end: impl FnMut() -> Option<KillReason>,
) -> io::Result<std::result::Result<ExitStatus, KillReason>> {
    let mut pause = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Ok(status));
        }
        if let Some(why) = must_end() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Err(why));
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(STOP_POLL_MAX);
    }
}

/// ffmpeg or ffprobe killed before it exited on its own, or, when it was
/// stopped first, never started. A caller that keeps the reason finds this
/// in the error's chain (`anyhow::Error::downcast_ref`), as it finds
/// [`FfmpegFailed`], and reads why with [`Killed::reason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Killed {
    program: Program,
    why: KillReason,
}

/// Why a run of ffmpeg or ffprobe was killed ([`Killed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillReason {
    /// A conversion made no progress for `wait`, and had never moved from
    /// the start: most likely the encoder hung.
    StalledBeforeOutput {
        /// The no-progress wait.
        wait: Duration,
    },
    /// A conversion made no progress for `wait` after reaching `at` in the
    /// file: most likely the file is damaged there.
    StalledPartway {
        /// The no-progress wait.
        wait: Duration,
        /// How far into the file it got.
        at: Duration,
    },
    /// A conversion went past twice the file's `length`: a damaged file
    /// looping.
    RanPastLength {
        /// How long the file plays, as ffprobe read it.
        length: Duration,
    },
    /// The run was stopped while it ran: nothing is wrong with the file.
    Stopped,
    /// The run was stopped before the program started.
    StoppedBeforeStart,
    /// A short step, reading a file's details or one frame, did not finish
    /// within `limit`.
    ShortStepTooLong {
        /// The short step's limit.
        limit: Duration,
    },
}

impl Killed {
    /// Stopped before `program` started.
    fn stopped_before_start(program: Program) -> Self {
        Self {
            program,
            why: KillReason::StoppedBeforeStart,
        }
    }

    /// ffmpeg stopped while it ran.
    pub(crate) fn ffmpeg_stopped() -> Self {
        Self {
            program: Program::Ffmpeg,
            why: KillReason::Stopped,
        }
    }

    /// Why the run was killed.
    #[must_use]
    pub fn reason(&self) -> KillReason {
        self.why
    }
}

#[cfg(any(test, feature = "testutil"))]
impl Killed {
    /// ffmpeg killed for `why`, for a test of what a caller makes of it.
    pub(crate) fn ffmpeg(why: KillReason) -> Self {
        Self {
            program: Program::Ffmpeg,
            why,
        }
    }
}

impl std::fmt::Display for Killed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let program = self.program;
        match self.why {
            KillReason::StalledBeforeOutput { wait } => write!(
                f,
                "{program} made no progress for {} before converting any of the file, so it \
                 was stopped",
                spoken(wait)
            ),
            KillReason::StalledPartway { wait, at } => write!(
                f,
                "{program} made no progress for {} after converting {} of the file, so it was \
                 stopped",
                spoken(wait),
                clock(at)
            ),
            KillReason::RanPastLength { length } => write!(
                f,
                "{program} ran past twice the file's length of {}, so it was stopped as looping \
                 on a damaged file",
                clock(length)
            ),
            KillReason::Stopped => write!(f, "stopped while {program} ran"),
            KillReason::StoppedBeforeStart => write!(f, "stopped before {program} started"),
            KillReason::ShortStepTooLong { limit } => write!(
                f,
                "{program} did not finish within {}, so it was stopped",
                spoken(limit)
            ),
        }
    }
}

impl std::error::Error for Killed {}

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

/// A position in a file, or its length, as a player shows it: `m:ss`, or
/// `h:mm:ss` from an hour, in whole seconds.
fn clock(at: Duration) -> String {
    let secs = at.as_secs();
    let (hours, minutes, seconds) = (secs / 3600, secs / 60 % 60, secs % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
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

#[cfg(test)]
mod tests;
