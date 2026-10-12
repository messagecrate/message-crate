use super::*;
use crate::testutil::tools_test_lock;
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// Where the tools were looked for when a test started, put back when it ends.
struct RestoreToolsDir(Option<PathBuf>, Option<OsString>);

impl RestoreToolsDir {
    fn capture() -> Self {
        Self(tools_dir(), search_path())
    }
}

impl Drop for RestoreToolsDir {
    fn drop(&mut self) {
        set_search_path(self.1.clone());
        set_tools_dir(self.0.clone());
    }
}

fn write_mock_tool(path: &Path) {
    fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// Point the tool location at a Tools Directory holding an `ffmpeg` that
/// runs `body` and an `ffprobe` that does nothing, with an empty `PATH`
/// before it. Both answer `-version`, so the lookup accepts them.
fn mock_ffmpeg_dir(body: &str) -> tempfile::TempDir {
    mock_tools_dir(body, "exit 0")
}

/// Point the tool location at a Tools Directory holding an `ffmpeg` that
/// runs `ffmpeg_body` and an `ffprobe` that runs `ffprobe_body`, with an
/// empty `PATH` before it. Both answer `-version`, so the lookup accepts
/// them.
fn mock_tools_dir(ffmpeg_body: &str, ffprobe_body: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [("ffmpeg", ffmpeg_body), ("ffprobe", ffprobe_body)] {
        let program = dir.path().join(name);
        fs::write(
            &program,
            format!("#!/bin/sh\n[ \"$1\" = -version ] && exit 0\n{body}\n"),
        )
        .unwrap();
        let mut perms = fs::metadata(&program).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&program, perms).unwrap();
    }
    set_search_path(Some(OsString::new()));
    set_tools_dir(Some(dir.path().to_path_buf()));
    dir
}

/// A mock program body that writes its process id to `pid` in its own
/// directory and then never exits. `exec` makes the `sleep` the process
/// that was started, so killing it leaves nothing running.
const WRITE_PID_AND_HANG: &str = "echo $$ > \"$(dirname \"$0\")/pid.tmp\"\n\
     mv \"$(dirname \"$0\")/pid.tmp\" \"$(dirname \"$0\")/pid\"\nexec sleep 600";

/// Whether the process `pid` still exists.
fn still_runs(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// A clock for a watched run that reads zero until `started` exists, and
/// then an hour more on every read, so every limit and wait passes once the
/// mock has started, and none before. It waits on no real clock.
fn clock_from(started: &Path) -> impl Fn() -> Duration + '_ {
    let hours = std::cell::Cell::new(0u64);
    move || {
        if started.is_file() {
            hours.set(hours.get() + 1);
        }
        Duration::from_secs(hours.get() * 3600)
    }
}

/// An ffmpeg that never exits on one frame is killed once the short step's
/// 15 seconds are up, and the run fails saying so, so one file ffmpeg hangs
/// on cannot hold up the media pass for good (#2059, #2898).
#[cfg(unix)]
#[test]
fn ffmpeg_that_never_finishes_one_frame_is_killed_after_the_short_step_limit() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let dir = mock_ffmpeg_dir(WRITE_PID_AND_HANG);
    let pid_file = dir.path().join("pid");

    let err = run_ffmpeg_watched(
        &["out.jpg".to_string()],
        Some(&AtomicBool::new(false)),
        Limit::ShortStep,
        &clock_from(&pid_file),
    )
    .expect_err("a run past its time fails");

    assert_eq!(
        err.downcast_ref::<Killed>().map(Killed::reason),
        Some(KillReason::ShortStepTooLong {
            limit: SHORT_STEP_LIMIT
        }),
        "{err:#}"
    );
    assert_eq!(
        format!("{err:#}"),
        "ffmpeg did not finish within 15 seconds, so it was stopped"
    );
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    assert!(!still_runs(&pid), "ffmpeg {pid} still runs");
}

/// An ffprobe that never exits is killed once the short step's 15 seconds
/// are up too, and the probe fails (#2059, #2898).
#[cfg(unix)]
#[test]
fn ffprobe_that_never_finishes_is_killed_after_the_short_step_limit() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let dir = mock_tools_dir("exit 0", WRITE_PID_AND_HANG);
    let pid_file = dir.path().join("pid");

    let err = run_ffprobe_watched(
        &["-v", "error"],
        Path::new("clip.mp4"),
        None,
        &clock_from(&pid_file),
    )
    .expect_err("a probe past its time fails");

    assert!(err.downcast_ref::<Killed>().is_some(), "{err:#}");
    assert!(
        format!("{err:#}").contains("ffprobe did not finish within 15 seconds"),
        "{err:#}"
    );
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    assert!(!still_runs(&pid), "ffprobe {pid} still runs");
}

/// A conversion whose ffmpeg writes no progress and never exits is killed
/// once the no-progress wait has passed, as stalled before any output, and
/// the process is reaped (#2898).
#[cfg(unix)]
#[test]
fn a_conversion_that_never_moves_is_killed_as_stalled_before_any_output() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let dir = mock_ffmpeg_dir(WRITE_PID_AND_HANG);
    let pid_file = dir.path().join("pid");
    let wait = Duration::from_secs(120);

    let err = run_ffmpeg_watched(
        &["out.mp4".to_string()],
        None,
        Limit::Progress(StallWatch::new(wait, Some(Duration::from_secs(60)))),
        &clock_from(&pid_file),
    )
    .expect_err("a stalled run fails");

    assert_eq!(
        err.downcast_ref::<Killed>().map(Killed::reason),
        Some(KillReason::StalledBeforeOutput { wait }),
        "{err:#}"
    );
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    assert!(!still_runs(&pid), "ffmpeg {pid} still runs");
}

/// A conversion whose `out_time` passes twice the file's length is killed
/// as looping on a damaged file, and the process is reaped. The clock never
/// moves, so only the position can end the run (#2898).
#[cfg(unix)]
#[test]
fn a_conversion_past_twice_its_length_is_killed() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    // The pid is written before the progress, so it is there once the run
    // is killed.
    let dir = mock_ffmpeg_dir(
        "echo $$ > \"$(dirname \"$0\")/pid\"\n\
         printf 'out_time=-00:00:00.023220\\nprogress=continue\\n\
         out_time=00:00:03.000000\\nprogress=continue\\n'\nexec sleep 600",
    );
    let pid_file = dir.path().join("pid");

    let err = run_ffmpeg_watched(
        &["out.mp4".to_string()],
        None,
        Limit::Progress(StallWatch::new(
            Duration::from_secs(120),
            Some(Duration::from_secs(1)),
        )),
        &|| Duration::ZERO,
    )
    .expect_err("a run past twice its length fails");

    assert_eq!(
        err.downcast_ref::<Killed>().map(Killed::reason),
        Some(KillReason::RanPastLength {
            length: Duration::from_secs(1)
        }),
        "{err:#}"
    );
    assert_eq!(
        format!("{err:#}"),
        "ffmpeg ran past twice the file's length of 0:01, so it was stopped as looping on a \
         damaged file"
    );
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    assert!(!still_runs(&pid), "ffmpeg {pid} still runs");
}

/// A conversion that writes progress and exits succeeds, whatever its
/// progress said.
#[cfg(unix)]
#[test]
fn a_conversion_that_exits_after_its_progress_succeeds() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let _dir = mock_ffmpeg_dir(
        "printf 'out_time=00:00:01.000000\\nprogress=continue\\n\
         out_time=00:00:02.000000\\nprogress=end\\n'\nexit 0",
    );

    run_ffmpeg_watched(
        &["out.mp4".to_string()],
        None,
        Limit::Progress(StallWatch::new(
            Duration::from_secs(120),
            Some(Duration::from_secs(2)),
        )),
        &|| Duration::ZERO,
    )
    .expect("a run that exits 0 succeeds");
}

/// A program that never exits on `-version` is killed once the short
/// step's 15 seconds are up and answers "not found", and leaves no process,
/// so a broken ffmpeg on `PATH` cannot hold every lookup for good (#2883).
#[cfg(unix)]
#[test]
fn a_program_that_never_answers_version_does_not_run_and_is_reaped() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("ffmpeg");
    fs::write(&program, format!("#!/bin/sh\n{WRITE_PID_AND_HANG}\n")).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let pid_file = dir.path().join("pid");

    assert!(!command_runs_watched(
        &program,
        &["-version"],
        &clock_from(&pid_file)
    ));

    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    assert!(!still_runs(&pid), "{pid} still runs");
}

/// A program that answers `-version` within the short step runs.
#[cfg(unix)]
#[test]
fn a_program_that_answers_version_runs() {
    let tools = mock_tools();
    assert!(command_runs_watched(
        &tools.path().join("ffmpeg"),
        &["-version"],
        &|| Duration::ZERO
    ));
}

/// ffprobe's answer on stdout comes back, after the flags that keep it to
/// local files.
#[cfg(unix)]
#[test]
fn ffprobe_answers_what_it_wrote_and_opens_only_local_files() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let _dir = mock_tools_dir("exit 0", "echo \"$@\"");

    let out = run_ffprobe(&["-v", "error"], Path::new("clip.mp4"), None).unwrap();

    assert_eq!(
        String::from_utf8(out).unwrap(),
        "-protocol_whitelist file,pipe -v error clip.mp4\n"
    );
}

/// Every ffmpeg run writes its progress to stdout, reads nothing from stdin
/// and opens only local files and pipes, after the quiet flags (#2059,
/// #2898).
#[cfg(unix)]
#[test]
fn ffmpeg_reads_no_stdin_and_opens_only_local_files() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let dir = mock_ffmpeg_dir("echo \"$@\" > \"$(dirname \"$0\")/args\"");

    run_ffmpeg(&[
        "-i".to_string(),
        "in.mov".to_string(),
        "out.mp4".to_string(),
    ])
    .unwrap();

    assert_eq!(
        fs::read_to_string(dir.path().join("args")).unwrap(),
        "-hide_banner -nostats -loglevel error -progress pipe:1 -nostdin \
         -protocol_whitelist file,pipe -i in.mov out.mp4\n"
    );
}

/// A minute, for the scripted progress streams below.
const MINUTE: Duration = Duration::from_secs(60);

/// A conversion whose position keeps moving is never stopped, however long
/// it runs: a thousand hours of a long video on a slow computer, a second
/// of the file every minute (#2898).
#[test]
fn a_conversion_that_keeps_moving_is_never_stopped() {
    let mut watch = StallWatch::new(2 * MINUTE, None);
    for minute in 0..60_000u64 {
        let now = MINUTE * u32::try_from(minute).unwrap();
        assert_eq!(
            watch.observe(Some(Duration::from_secs(minute)), now),
            None,
            "stopped at minute {minute}"
        );
    }
}

/// A slow conversion of a file with a known length that keeps moving is
/// never stopped either, until it passes twice that length.
#[test]
fn a_slow_conversion_within_its_length_is_never_stopped() {
    let length = Duration::from_secs(3600);
    let mut watch = StallWatch::new(2 * MINUTE, Some(length));
    for second in 0..=7200u64 {
        let now = MINUTE * u32::try_from(second).unwrap();
        assert_eq!(watch.observe(Some(Duration::from_secs(second)), now), None);
    }
}

/// A position that stops moving is stopped once the wait has passed since
/// it last moved, as stalled partway, and not before.
#[test]
fn a_conversion_that_stops_moving_is_stopped_after_the_wait() {
    let wait = 2 * MINUTE;
    let mut watch = StallWatch::new(wait, Some(10 * MINUTE));
    let at = Duration::from_secs(30);
    assert_eq!(watch.observe(Some(at), MINUTE), None);
    assert_eq!(watch.observe(Some(at), 2 * MINUTE), None);
    assert_eq!(
        watch.observe(Some(at), 3 * MINUTE - Duration::from_millis(1)),
        None
    );
    assert_eq!(
        watch.observe(Some(at), 3 * MINUTE),
        Some(KillReason::StalledPartway { wait, at })
    );
}

/// A position that goes back is not progress: only a position past the
/// furthest seen moves the run forward.
#[test]
fn a_position_that_goes_back_is_not_progress() {
    let wait = 2 * MINUTE;
    let mut watch = StallWatch::new(wait, None);
    let furthest = Duration::from_secs(40);
    assert_eq!(watch.observe(Some(furthest), Duration::ZERO), None);
    assert_eq!(watch.observe(Some(Duration::from_secs(10)), MINUTE), None);
    assert_eq!(
        watch.observe(Some(Duration::from_secs(39)), 2 * MINUTE),
        Some(KillReason::StalledPartway { wait, at: furthest })
    );
}

/// A run that never reports a position past the start is stopped as
/// stalled before any output once the wait has passed since it started.
#[test]
fn a_conversion_that_never_moves_is_stalled_before_any_output() {
    let wait = 2 * MINUTE;
    let mut watch = StallWatch::new(wait, None);
    assert_eq!(watch.observe(None, MINUTE), None);
    assert_eq!(watch.observe(Some(Duration::ZERO), MINUTE), None);
    assert_eq!(
        watch.observe(Some(Duration::ZERO), wait),
        Some(KillReason::StalledBeforeOutput { wait })
    );
}

/// A position past twice the file's length is stopped at once, while it is
/// still moving; at twice the length it is not yet.
#[test]
fn a_conversion_past_twice_its_length_is_stopped() {
    let length = Duration::from_secs(5);
    let mut watch = StallWatch::new(2 * MINUTE, Some(length));
    assert_eq!(
        watch.observe(Some(Duration::from_secs(10)), Duration::ZERO),
        None
    );
    assert_eq!(
        watch.observe(Some(Duration::from_millis(10_001)), Duration::ZERO),
        Some(KillReason::RanPastLength { length })
    );
}

/// ffmpeg's `out_time` lines are read as positions; other lines, `N/A` and
/// a line that is not a time are not, and a negative time is the start.
#[test]
fn out_time_reads_ffmpegs_progress_lines() {
    assert_eq!(
        out_time("out_time=01:02:03.500000"),
        Some(Duration::from_millis(3_723_500))
    );
    assert_eq!(out_time("out_time=00:00:00.000000\n"), Some(Duration::ZERO));
    assert_eq!(out_time("out_time=-00:00:00.023220"), Some(Duration::ZERO));
    assert_eq!(out_time("out_time=N/A"), None);
    assert_eq!(out_time("out_time_us=3723500000"), None);
    assert_eq!(out_time("progress=continue"), None);
    assert_eq!(out_time("out_time=nonsense"), None);
}

/// A position or length is said as a player shows it.
#[test]
fn a_position_is_said_as_a_player_shows_it() {
    assert_eq!(clock(Duration::from_millis(5_900)), "0:05");
    assert_eq!(clock(Duration::from_secs(65)), "1:05");
    assert_eq!(clock(Duration::from_secs(3 * 3600 + 7)), "3:00:07");
}

/// The no-progress wait is the default until it is set, and a wait under a
/// millisecond is taken as one.
#[test]
fn the_no_progress_wait_is_the_default_until_set() {
    let _guard = tools_test_lock();
    let _restore = RestoreStallWait;
    assert_eq!(stall_wait(), DEFAULT_STALL_WAIT);
    set_stall_wait(Duration::from_secs(30));
    assert_eq!(stall_wait(), Duration::from_secs(30));
    set_stall_wait(Duration::ZERO);
    assert_eq!(stall_wait(), Duration::from_millis(1));
}

/// Puts the default no-progress wait back when dropped, so a test that
/// fails part way does not leave every later conversion in this process
/// stopped as stalled.
struct RestoreStallWait;

impl Drop for RestoreStallWait {
    fn drop(&mut self) {
        set_stall_wait(DEFAULT_STALL_WAIT);
    }
}

/// A conversion that reaches the end of its file is finishing it: with
/// `+faststart` ffmpeg rewrites the whole output without reporting
/// progress, which on a slow disk takes longer than the wait. It is given
/// twice the time it took to reach the end, and stopped after that.
#[test]
fn a_conversion_finishing_its_file_gets_twice_the_time_it_took() {
    let wait = 2 * MINUTE;
    let length = Duration::from_secs(600);
    let mut watch = StallWatch::new(wait, Some(length));
    // Reaches the end, a frame short of it, after 10 minutes.
    let end = length - Duration::from_millis(40);
    assert_eq!(watch.observe(Some(end), 10 * MINUTE), None);
    // Silent for 19 minutes while it rewrites the file: not stopped.
    assert_eq!(watch.observe(Some(end), 29 * MINUTE), None);
    assert_eq!(
        watch.observe(Some(end), 30 * MINUTE),
        Some(KillReason::StalledPartway {
            wait: 20 * MINUTE,
            at: end
        })
    );
}

/// A conversion that stops short of the end gets only the wait, however
/// long it took to get there.
#[test]
fn a_conversion_short_of_its_end_gets_only_the_wait() {
    let wait = 2 * MINUTE;
    let mut watch = StallWatch::new(wait, Some(Duration::from_secs(600)));
    let at = Duration::from_secs(500);
    assert_eq!(watch.observe(Some(at), 60 * MINUTE), None);
    assert_eq!(
        watch.observe(Some(at), 62 * MINUTE),
        Some(KillReason::StalledPartway { wait, at })
    );
}

/// A time limit is said in the largest whole unit.
#[test]
fn a_time_limit_is_said_in_its_largest_whole_unit() {
    assert_eq!(spoken(Duration::from_secs(2 * 60)), "2 minutes");
    assert_eq!(spoken(Duration::from_secs(60)), "1 minute");
    assert_eq!(spoken(Duration::from_secs(20 * 3600)), "20 hours");
    assert_eq!(spoken(Duration::from_secs(90 * 60)), "90 minutes");
    assert_eq!(spoken(Duration::from_secs(45)), "45 seconds");
}

/// A directory holding an `ffmpeg` and an `ffprobe` that both run.
fn mock_tools() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["ffmpeg", "ffprobe"] {
        write_mock_tool(&dir.path().join(name));
    }
    dir
}

/// `run_ffmpeg` on another thread, failing the test when it has not
/// returned within a minute rather than hanging the test run.
fn run_ffmpeg_within_a_minute(args: &[&str]) -> Result<()> {
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(run_ffmpeg(&args));
    });
    receiver
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("run_ffmpeg did not return within a minute")
}

/// ffmpeg can write more to stderr than a pipe holds (64 KiB on Linux),
/// with repeated errors or an encoder that ignores `-loglevel`. A pipe
/// nobody reads blocks ffmpeg on the write, and the wait for it never
/// returns (#1178).
#[cfg(unix)]
#[test]
fn run_ffmpeg_returns_when_ffmpeg_writes_more_than_a_pipe_holds() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let _dir =
        mock_ffmpeg_dir("dd if=/dev/zero bs=1024 count=1024 2>/dev/null | tr '\\0' x >&2\nexit 0");

    run_ffmpeg_within_a_minute(&["-i", "in.mov", "out.mp4"]).expect("ffmpeg exits 0");
}

/// Setting the stop kills ffmpeg and waits for it, so a stopped server
/// leaves no conversion running (#1729), and a run asked for once it is
/// set never starts one.
#[cfg(unix)]
#[test]
fn run_ffmpeg_until_kills_ffmpeg_when_stopped() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let dir = mock_ffmpeg_dir("echo $$ > \"$(dirname \"$0\")/pid\"\nexec sleep 600");
    let pid_file = dir.path().join("pid");
    let stop = std::sync::Arc::new(AtomicBool::new(false));

    let running = {
        let stop = std::sync::Arc::clone(&stop);
        std::thread::spawn(move || run_ffmpeg_until(&["out.mp4".to_string()], &stop))
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !pid_file.is_file() {
        assert!(std::time::Instant::now() < deadline, "ffmpeg did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    stop.store(true, Ordering::Relaxed);
    let err = running.join().unwrap().expect_err("a stopped run fails");

    assert!(format!("{err:#}").contains("stopped"), "{err:#}");
    let alive = Command::new("kill")
        .args(["-0", &pid])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!alive.success(), "ffmpeg {pid} still runs");
    fs::remove_file(&pid_file).unwrap();
    let err = run_ffmpeg_until(&["out.mp4".to_string()], &stop).expect_err("stopped");
    assert!(format!("{err:#}").contains("stopped"), "{err:#}");
    assert!(!pid_file.exists(), "no ffmpeg starts once the stop is set");
}

/// Ctrl-C in a terminal reaches ffmpeg and the server together, and ffmpeg
/// can exit before the server's handler sets the stop. A failure the stop
/// follows within the grace is the stop's, so the server keeps the Asset
/// queued rather than reading it as a bad file (#1729).
#[cfg(unix)]
#[test]
fn a_failure_the_stop_follows_is_a_stop() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let _dir = mock_ffmpeg_dir("exit 255");
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let setter = {
        let stop = std::sync::Arc::clone(&stop);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            stop.store(true, Ordering::Relaxed);
        })
    };

    let err = run_ffmpeg_until(&["out.mp4".to_string()], &stop).expect_err("ffmpeg exits 255");
    setter.join().unwrap();

    assert!(format!("{err:#}").contains("stopped"), "{err:#}");
}

#[cfg(unix)]
#[test]
fn run_ffmpeg_failure_carries_what_ffmpeg_said() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let _dir =
        mock_ffmpeg_dir("echo 'in.mov: Invalid data found when processing input' >&2\nexit 1");

    let err = run_ffmpeg_within_a_minute(&["-i", "in.mov", "out.mp4"]).expect_err("ffmpeg exits 1");
    let message = format!("{err:#}");
    assert!(
        message.contains("in.mov: Invalid data found when processing input"),
        "message was {message:?}"
    );
}

/// ffmpeg writes its banner, build configuration, stats lines and
/// warnings unless told not to, and a failure carried them in front of its
/// cause. The mock writes them all unless its first four arguments are the
/// quiet flags, which ffmpeg reads as global options only before the
/// first input (#1412).
#[cfg(unix)]
#[test]
fn run_ffmpeg_failure_carries_the_cause_without_the_banner_or_stats() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let _dir = mock_ffmpeg_dir(concat!(
        "[ \"$1 $2 $3 $4\" = '-hide_banner -nostats -loglevel error' ] || { ",
        "echo 'ffmpeg version 6.1.1 Copyright (c) 2000-2023 the FFmpeg developers' >&2; ",
        "echo '  configuration: --enable-gpl --enable-libx265' >&2; ",
        "echo 'frame=  12 fps=0.0 q=0.0 size=       0kB time=00:00:00.40' >&2; ",
        "echo 'Guessed Channel Layout for Input Stream #0.1 : mono' >&2; }\n",
        "echo 'in.mov: Invalid data found when processing input' >&2\nexit 1",
    ));

    let err = run_ffmpeg_within_a_minute(&["-i", "in.mov", "out.mp4"]).expect_err("ffmpeg exits 1");

    let message = format!("{err:#}");
    assert!(
        message.contains("in.mov: Invalid data found when processing input"),
        "the cause is kept: {message:?}"
    );
    for preamble in [
        "ffmpeg version",
        "configuration:",
        "frame=",
        "Guessed Channel Layout",
    ] {
        assert!(!message.contains(preamble), "{preamble} kept: {message:?}");
    }
}

/// A failure after megabytes of warnings keeps the end, where ffmpeg
/// writes the reason it stopped, and drops the rest.
#[cfg(unix)]
#[test]
fn run_ffmpeg_failure_keeps_only_the_end_of_a_long_error_output() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let _dir = mock_ffmpeg_dir(
        "i=0\nwhile [ $i -lt 20000 ]; do echo \"warning for frame $i\" >&2; i=$((i+1)); done\n\
         echo 'Conversion failed!' >&2\nexit 1",
    );

    let err = run_ffmpeg_within_a_minute(&["-i", "in.mov", "out.mp4"]).expect_err("ffmpeg exits 1");
    let message = format!("{err:#}");
    assert!(
        message.ends_with("Conversion failed!"),
        "message ends {:?}",
        &message[message.len().saturating_sub(200)..]
    );
    assert!(
        !message.contains("warning for frame 0\n"),
        "the start was kept"
    );
    assert!(
        message.len() <= STDERR_TAIL_BYTES + 100,
        "message is {} bytes",
        message.len()
    );
}

/// The real ffmpeg, given an input that does not exist, says so, and the
/// error says what it said.
#[test]
fn real_ffmpeg_failure_names_the_missing_input() {
    let Some(_guard) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.mov");
    let output = dir.path().join("out.mp4");

    let err = run_ffmpeg(&[
        "-y".to_string(),
        "-i".to_string(),
        missing.display().to_string(),
        output.display().to_string(),
    ])
    .expect_err("the input does not exist");
    let message = format!("{err:#}");
    assert!(
        message.contains("No such file or directory"),
        "message was {message:?}"
    );
}

/// Both programs from `dir`.
fn both_in(dir: &Path) -> FfmpegTools {
    FfmpegTools {
        ffmpeg: Some(dir.join("ffmpeg")),
        ffprobe: Some(dir.join("ffprobe")),
    }
}

/// A directory holding only `name`, which runs.
fn only(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_mock_tool(&dir.path().join(name));
    dir
}

/// A person who installed ffmpeg chose it, so `PATH` wins over the copy in
/// the Tools Directory (#1053).
#[cfg(unix)]
#[test]
fn path_is_searched_before_the_tools_directory() {
    let on_path = mock_tools();
    let tools = mock_tools();
    let empty = tempfile::tempdir().unwrap();
    let search = std::env::join_paths([empty.path(), on_path.path()]).unwrap();

    assert_eq!(
        find_tools(Some(&search), Some(tools.path())).unwrap(),
        both_in(on_path.path())
    );
}

/// With nothing on `PATH`, the Tools Directory's copies are used.
#[cfg(unix)]
#[test]
fn the_tools_directory_is_searched_when_path_has_no_ffmpeg() {
    let tools = mock_tools();
    let empty = tempfile::tempdir().unwrap();

    assert_eq!(
        find_tools(Some(empty.path().as_os_str()), Some(tools.path())).unwrap(),
        both_in(tools.path())
    );
    assert_eq!(
        find_tools(Some(empty.path().as_os_str()), None).unwrap(),
        FfmpegTools::default()
    );
}

/// A file with the right name that cannot run is not a tool: Convert
/// would start and then fail on every file. The lookup goes on to the
/// Tools Directory.
#[cfg(unix)]
#[test]
fn a_tool_that_cannot_run_is_passed_over() {
    let broken = tempfile::tempdir().unwrap();
    fs::write(broken.path().join("ffmpeg"), "not a program").unwrap();
    fs::write(broken.path().join("ffprobe"), "not a program").unwrap();
    let tools = mock_tools();

    assert_eq!(
        find_tools(Some(broken.path().as_os_str()), Some(tools.path())).unwrap(),
        both_in(tools.path())
    );
    assert_eq!(
        find_tools(Some(broken.path().as_os_str()), None).unwrap(),
        FfmpegTools::default()
    );
}

/// ffmpeg on `PATH` without ffprobe beside it is passed over for the pair
/// in the Tools Directory, so the two are one build.
#[cfg(unix)]
#[test]
fn ffmpeg_on_path_without_ffprobe_gives_way_to_the_tools_directory() {
    let on_path = only("ffmpeg");
    let tools = mock_tools();

    assert_eq!(
        find_tools(Some(on_path.path().as_os_str()), Some(tools.path())).unwrap(),
        both_in(tools.path())
    );
}

/// ffmpeg only on `PATH` and ffprobe only in the Tools Directory would be
/// two builds working on one file, so the lookup fails and names both.
#[cfg(unix)]
#[test]
fn ffmpeg_and_ffprobe_in_two_places_is_an_error_naming_both() {
    let on_path = only("ffmpeg");
    let tools = only("ffprobe");

    let err =
        find_tools(Some(on_path.path().as_os_str()), Some(tools.path())).expect_err("two places");
    let message = err.to_string();
    for path in [on_path.path().join("ffmpeg"), tools.path().join("ffprobe")] {
        assert!(
            message.contains(&path.display().to_string()),
            "message was {message:?}"
        );
    }
}

/// Start `program` with `-version`, as the lookup does.
fn start_version(program: &Path) -> io::Result<ExitStatus> {
    Command::new(program)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

/// Linux refuses to run a file some process holds open for writing, and a
/// child started on another thread holds every open descriptor between
/// its fork and its exec. A mock written and run at once met that in CI and
/// was read as broken (#2031). The lookup starts the program again, and
/// finds it once the writer closes. Here the test is the writer, and
/// closes the file after the first refused start, so the retry is proven
/// with the kernel's own refusal and no race. How many starts it takes is
/// not fixed: children the other tests start at the same time hold their
/// inherited copy of the descriptor until they exec, which is the race
/// itself, so only the first refusal and the eventual answer are asserted.
#[cfg(target_os = "linux")]
#[test]
fn a_program_held_open_for_writing_is_found_once_the_writer_closes() {
    let tools = mock_tools();
    let ffmpeg = tools.path().join("ffmpeg");
    let mut writer = Some(fs::File::options().write(true).open(&ffmpeg).unwrap());
    let mut starts = 0;

    let runs = started_once_the_file_is_free(|| {
        starts += 1;
        let started = start_version(&ffmpeg);
        if starts == 1 {
            assert_eq!(
                started.as_ref().err().map(io::Error::kind),
                Some(io::ErrorKind::ExecutableFileBusy),
                "the first start, with the file open for writing, is refused as busy"
            );
            writer.take();
        }
        started
    })
    .is_some_and(|status| status.success());

    assert!(runs, "found once the writer closed");
    assert!(starts > 1, "started again after the refusal");
}

/// A file open for writing for the whole lookup, as one still being
/// written is, does not run, and the lookup gives up on it rather than
/// starting it for ever.
#[cfg(target_os = "linux")]
#[test]
fn a_program_held_open_for_writing_throughout_is_not_found() {
    let tools = mock_tools();
    let _writer = fs::File::options()
        .write(true)
        .open(tools.path().join("ffmpeg"))
        .unwrap();
    assert_eq!(tool_in_dir(tools.path(), "ffmpeg"), None);
    assert_eq!(
        tool_in_dir(tools.path(), "ffprobe"),
        Some(tools.path().join("ffprobe"))
    );
}

/// Only a busy file is started again. A start refused for another reason,
/// as one a missing shared library gives, is refused once. Making all
/// [`BUSY_STARTS`] starts on every lookup would make every lookup of a
/// broken program slow.
#[test]
fn a_start_refused_for_another_reason_is_not_made_again() {
    let mut starts = 0;

    let started = started_once_the_file_is_free(|| -> io::Result<ExitStatus> {
        starts += 1;
        Err(io::Error::from(io::ErrorKind::NotFound))
    });

    assert!(started.is_none());
    assert_eq!(starts, 1);
}

/// A program that appends a line to `log` each time it runs.
fn counting_tool(path: &Path, log: &Path) {
    fs::write(
        path,
        format!("#!/bin/sh\necho run >> '{}'\nexit 0\n", log.display()),
    )
    .unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// How many times the programs writing to `log` ran.
fn runs_in(log: &Path) -> usize {
    fs::read_to_string(log).map_or(0, |text| text.lines().count())
}

/// With ffmpeg and ffprobe in two places, every lookup fails and so
/// searches again, once per file staged. Each program found was run once
/// to see that it answers, and is not run again.
#[cfg(unix)]
#[test]
fn a_second_lookup_in_two_places_runs_nothing() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let on_path = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let log = tools.path().join("runs.log");
    counting_tool(&on_path.path().join("ffmpeg"), &log);
    counting_tool(&tools.path().join("ffprobe"), &log);
    set_search_path(Some(on_path.path().as_os_str().to_owned()));
    set_tools_dir(Some(tools.path().to_path_buf()));

    assert!(ffmpeg_tools().is_err());
    let first = runs_in(&log);
    assert_eq!(first, 2, "each program runs once to answer -version");
    assert!(ffmpeg_tools().is_err());
    assert!(ffprobe_path().is_none());
    assert_eq!(runs_in(&log), first);
}

/// A file that could not run is run again on the next lookup, because a
/// failure to answer `-version` is never kept: once it is made executable
/// it is found.
#[cfg(unix)]
#[test]
fn a_tool_made_executable_later_is_found() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let tools = mock_tools();
    set_search_path(Some(OsString::new()));
    set_tools_dir(Some(tools.path().to_path_buf()));
    let ffprobe = tools.path().join("ffprobe");
    let mut perms = fs::metadata(&ffprobe).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&ffprobe, perms.clone()).unwrap();
    assert!(!ffmpeg_available());

    perms.set_mode(0o755);
    fs::set_permissions(&ffprobe, perms).unwrap();
    assert!(ffmpeg_available());
}

/// A pair found in one place and then moved to another, under the same
/// names, is found at the new place on the next lookup: the place is never
/// kept, only each file's answer.
#[cfg(unix)]
#[test]
fn a_pair_moved_to_another_directory_is_found_there() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let on_path = mock_tools();
    let tools = tempfile::tempdir().unwrap();
    set_search_path(Some(on_path.path().as_os_str().to_owned()));
    set_tools_dir(Some(tools.path().to_path_buf()));
    assert_eq!(ffmpeg_tools().unwrap(), both_in(on_path.path()));

    for name in ["ffmpeg", "ffprobe"] {
        fs::rename(on_path.path().join(name), tools.path().join(name)).unwrap();
    }
    assert_eq!(ffmpeg_tools().unwrap(), both_in(tools.path()));
}

/// A program whose `-version` fails, as one Gatekeeper blocks or one
/// missing a shared library does, is run again on the next lookup and found
/// once it answers. Fixing either leaves the file as it was, so a kept
/// failure would hide the fix until a restart.
#[cfg(unix)]
#[test]
fn a_program_that_failed_once_is_found_when_it_answers() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let tools = mock_tools();
    let marker = tools.path().join("blocked");
    fs::write(&marker, "").unwrap();
    let ffprobe = tools.path().join("ffprobe");
    fs::write(
        &ffprobe,
        format!(
            "#!/bin/sh
[ -e '{}' ] && exit 1
exit 0
",
            marker.display()
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&ffprobe).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&ffprobe, perms).unwrap();
    set_search_path(Some(OsString::new()));
    set_tools_dir(Some(tools.path().to_path_buf()));
    assert!(!ffmpeg_available());

    fs::remove_file(&marker).unwrap();
    assert!(ffmpeg_available());
}

/// A program replaced in place by another file, with the same size,
/// content and modified time, is a new file (a new inode), so it is run
/// again rather than trusted on the old file's answer.
#[cfg(unix)]
#[test]
fn a_program_replaced_in_place_is_run_again() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let tools = tempfile::tempdir().unwrap();
    let log = tools.path().join("runs.log");
    for name in ["ffmpeg", "ffprobe"] {
        counting_tool(&tools.path().join(name), &log);
    }
    set_search_path(Some(OsString::new()));
    set_tools_dir(Some(tools.path().to_path_buf()));
    assert!(ffmpeg_available());
    assert_eq!(runs_in(&log), 2);
    assert!(ffmpeg_available());
    assert_eq!(runs_in(&log), 2, "an unchanged file is not run again");

    let ffmpeg = tools.path().join("ffmpeg");
    let modified = fs::metadata(&ffmpeg).unwrap().modified().unwrap();
    let replacement = tools.path().join("ffmpeg.new");
    counting_tool(&replacement, &log);
    fs::File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    fs::rename(&replacement, &ffmpeg).unwrap();

    assert!(ffmpeg_available());
    assert_eq!(runs_in(&log), 3, "the replaced ffmpeg runs once more");
}

/// With one program in neither place, the other is reported where it is
/// and the missing one is named.
#[cfg(unix)]
#[test]
fn a_lone_program_is_found_and_the_other_named_missing() {
    let on_path = only("ffmpeg");
    let empty = tempfile::tempdir().unwrap();

    let tools = find_tools(Some(on_path.path().as_os_str()), Some(empty.path())).unwrap();
    assert_eq!(tools.ffmpeg, Some(on_path.path().join("ffmpeg")));
    assert_eq!(tools.missing(), vec!["ffprobe"]);
}

/// `PATH` and the Tools Directory are the only places looked in:
/// `MESSAGE_CRATE_BIN` is not read any more (#1053).
#[cfg(unix)]
#[test]
fn message_crate_bin_is_not_searched() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let elsewhere = mock_tools();
    set_search_path(Some(OsString::new()));
    set_tools_dir(None);

    // SAFETY: test-only env mutation; this test holds tools_test_lock so no
    // concurrent lookup runs.
    unsafe {
        std::env::set_var("MESSAGE_CRATE_BIN", elsewhere.path());
    }
    let found = ffmpeg_path();
    unsafe {
        std::env::remove_var("MESSAGE_CRATE_BIN");
    }
    assert_eq!(found, None);
}

/// A new Tools Directory is searched at once: a tool found before is
/// forgotten.
#[cfg(unix)]
#[test]
fn set_tools_dir_clears_what_was_found() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let first = mock_tools();
    let second = mock_tools();
    set_search_path(Some(OsString::new()));

    set_tools_dir(Some(first.path().to_path_buf()));
    assert_eq!(ffmpeg_path(), Some(first.path().join("ffmpeg")));
    set_tools_dir(Some(second.path().to_path_buf()));
    assert_eq!(tools_dir(), Some(second.path().to_path_buf()));
    assert_eq!(ffmpeg_path(), Some(second.path().join("ffmpeg")));
}

/// A tool not found is looked for again, so a copy that arrives in the
/// Tools Directory after the first lookup is used without a restart.
#[cfg(unix)]
#[test]
fn a_tool_that_arrives_later_is_found() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let tools = tempfile::tempdir().unwrap();
    set_search_path(Some(OsString::new()));
    set_tools_dir(Some(tools.path().to_path_buf()));
    assert!(!ffmpeg_available());

    for name in ["ffmpeg", "ffprobe"] {
        write_mock_tool(&tools.path().join(name));
    }
    assert!(ffmpeg_available());
}

#[cfg(unix)]
#[test]
fn missing_ffprobe_names_the_tool_not_the_input_file() {
    // A `Command` built from a bare, unresolved "ffprobe" would fail to
    // spawn with an IO error naming whatever path was passed as the
    // *input* — "No such file or directory" on `/path/to/IMG_0001.HEIC"
    // — which reads as a problem with the user's file, not the missing
    // tool. `ffprobe_command` must fail before that, with a message that
    // names ffprobe and where it was looked for.
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let empty = tempfile::tempdir().unwrap();
    set_search_path(Some(OsString::new()));
    set_tools_dir(Some(empty.path().to_path_buf()));

    let err = ffprobe_command().expect_err("no ffprobe on PATH or in the Tools Directory");
    let message = err.to_string();
    assert!(
        message.contains("ffprobe not found on PATH or in the Tools Directory"),
        "message was {message:?}"
    );
    assert!(
        message.contains(&empty.path().display().to_string()),
        "message was {message:?}"
    );
}

/// A file with an executable bit is executable, and the same file without
/// one is not: the WhatsApp exporter refuses a `wtsexporter` without the
/// bit, and the desktop app downloads it again.
#[cfg(unix)]
#[test]
fn a_file_is_executable_only_with_an_executable_bit() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("wtsexporter");
    fs::write(&program, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!is_executable(&program));

    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(is_executable(&program));
}

/// A file with an executable bit this account may not run is not
/// executable: a `wtsexporter` with only another class's bits, as a
/// root-owned `0o744` file is to any other account, must not pass for one
/// that runs. It skips where [`crate::testutil::may_read_any_file`] says so.
#[cfg(unix)]
#[test]
fn a_file_this_account_may_not_run_is_not_executable() {
    let dir = tempfile::tempdir().unwrap();
    if crate::testutil::may_read_any_file(dir.path()) {
        return;
    }
    let program = dir.path().join("wtsexporter");
    fs::write(&program, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o611)).unwrap();
    assert!(!is_executable(&program));
}

/// A directory has an executable bit too, and is not a program: one left
/// where `wtsexporter` belongs must not pass for it.
#[cfg(unix)]
#[test]
fn a_directory_is_not_executable() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_program = dir.path().join("wtsexporter");
    fs::create_dir(&not_a_program).unwrap();
    fs::set_permissions(&not_a_program, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!is_executable(&not_a_program));
}

/// Nothing at the path is not executable.
#[test]
fn a_missing_file_is_not_executable() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_executable(&dir.path().join("wtsexporter")));
}

/// A conversion asked for once the server is stopping starts neither the
/// ffprobe that reads the file's length nor ffmpeg, so a stalled disk
/// cannot hold the server's stop up (#2059).
#[cfg(unix)]
#[test]
fn a_stopped_transcode_runs_neither_program() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let ran = "touch \"$(dirname \"$0\")/ran\"";
    let dir = mock_tools_dir(ran, ran);

    let err = InstalledTools
        .transcode(
            &["-i".to_string(), "in.mov".to_string()],
            Path::new("out.mp4"),
            Reads::All(Path::new("in.mov")),
            Some(&AtomicBool::new(true)),
        )
        .expect_err("a stopped run fails");

    assert!(
        err.downcast_ref::<Killed>().is_some_and(Killed::is_stop),
        "{err:#}"
    );
    assert!(!dir.path().join("ran").exists(), "a program ran");
}
