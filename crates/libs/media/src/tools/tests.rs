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
    set_search_path(Some(OsString::new()));
    set_tools_dir(Some(dir.path().to_path_buf()));
    dir
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

    let runs = runs_once_the_file_is_free(|| {
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
    });

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

    let runs = runs_once_the_file_is_free(|| {
        starts += 1;
        Err(io::Error::from(io::ErrorKind::NotFound))
    });

    assert!(!runs);
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
