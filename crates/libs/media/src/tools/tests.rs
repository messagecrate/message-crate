use super::*;
use crate::testutil::tools_test_lock;
use std::fs;
use std::os::unix::fs::PermissionsExt;

struct RestoreToolsDir(Option<PathBuf>);

impl RestoreToolsDir {
    fn capture() -> Self {
        Self(tools_dir())
    }
}

impl Drop for RestoreToolsDir {
    fn drop(&mut self) {
        set_tools_dir(self.0.clone());
    }
}

fn write_mock_tool(path: &Path) {
    fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// Point the tool location at a folder holding an `ffmpeg` that runs
/// `body` and an `ffprobe` that does nothing. Both answer `-version`, so
/// the lookup accepts them.
fn mock_ffmpeg_dir(body: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_mock_tool(&dir.path().join("ffprobe"));
    let ffmpeg = dir.path().join("ffmpeg");
    fs::write(
        &ffmpeg,
        format!("#!/bin/sh\n[ \"$1\" = -version ] && exit 0\n{body}\n"),
    )
    .unwrap();
    let mut perms = fs::metadata(&ffmpeg).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&ffmpeg, perms).unwrap();
    set_tools_dir(Some(dir.path().to_path_buf()));
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

#[cfg(unix)]
#[test]
fn probe_folder_requires_both_tools() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let dir = tempfile::tempdir().unwrap();
    write_mock_tool(&dir.path().join("ffmpeg"));

    let probe = probe_ffmpeg_tools(Some(dir.path()));
    assert!(!probe.ok);
    assert!(probe.ffmpeg_path.is_some());
    assert!(probe.ffprobe_path.is_none());
}

/// A file with the right name that cannot run is not a tool: Convert
/// would start and then fail on every file.
#[cfg(unix)]
#[test]
fn probe_folder_refuses_tools_that_cannot_run() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let dir = tempfile::tempdir().unwrap();
    for name in ["ffmpeg", "ffprobe"] {
        fs::write(dir.path().join(name), "not a program").unwrap();
    }

    let probe = probe_ffmpeg_tools(Some(dir.path()));
    assert!(!probe.ok);
    assert_eq!(probe.ffmpeg_path, None);
    assert_eq!(probe.ffprobe_path, None);
}

#[cfg(unix)]
#[test]
fn set_tools_dir_overrides_and_clears_cache() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let dir = tempfile::tempdir().unwrap();
    for name in ["ffmpeg", "ffprobe"] {
        write_mock_tool(&dir.path().join(name));
    }
    set_tools_dir(Some(dir.path().to_path_buf()));
    assert_eq!(tools_dir(), Some(dir.path().to_path_buf()));
    assert!(ffmpeg_available());
    set_tools_dir(None);
    assert_eq!(tools_dir(), None);
}

#[cfg(unix)]
#[test]
fn missing_ffprobe_names_the_tool_not_the_input_file() {
    // A `Command` built from a bare, unresolved "ffprobe" would fail to
    // spawn with an IO error naming whatever path was passed as the
    // *input* — "No such file or directory" on `/path/to/IMG_0001.HEIC"
    // — which reads as a problem with the user's file, not the missing
    // tool. `ffprobe_command` must fail before that, with a message that
    // names ffprobe.
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let empty = tempfile::tempdir().unwrap();
    set_tools_dir(Some(empty.path().to_path_buf()));

    let err = ffprobe_command().expect_err("no ffprobe in an empty override dir");
    let message = err.to_string();
    assert!(
        message.contains("ffprobe not found"),
        "message was {message:?}"
    );
}

#[cfg(unix)]
#[test]
fn probe_candidate_folder_does_not_change_override() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    let live = tempfile::tempdir().unwrap();
    for name in ["ffmpeg", "ffprobe"] {
        write_mock_tool(&live.path().join(name));
    }
    set_tools_dir(Some(live.path().to_path_buf()));

    let candidate = tempfile::tempdir().unwrap();
    write_mock_tool(&candidate.path().join("ffmpeg"));

    let _probe = probe_ffmpeg_tools(Some(candidate.path()));
    assert_eq!(tools_dir(), Some(live.path().to_path_buf()));
}

#[cfg(unix)]
#[test]
fn find_tool_prefers_message_crate_bin() {
    let _guard = tools_test_lock();
    let _restore = RestoreToolsDir::capture();
    set_tools_dir(None);
    let dir = tempfile::tempdir().unwrap();
    write_mock_tool(&dir.path().join("ffmpeg"));

    // SAFETY: test-only env mutation; this test holds tools_test_lock so no
    // concurrent resolve_tool calls run. In production, set_tools_dir override
    // is checked before MESSAGE_CRATE_BIN; job threads share the same override.
    unsafe {
        std::env::set_var("MESSAGE_CRATE_BIN", dir.path());
    }
    let found = resolve_tool("ffmpeg").expect("ffmpeg from MESSAGE_CRATE_BIN");
    assert_eq!(found, dir.path().join("ffmpeg"));
    unsafe {
        std::env::remove_var("MESSAGE_CRATE_BIN");
    }
}
