//! A server the desktop app started stops itself once the app is gone, and a
//! server started any other way never does (#1934).
//!
//! Each test starts `serve` from a shell that stands in for the app. Like the
//! app, the shell holds the only read end of the server's standard error, so
//! the pipe breaks when the shell ends: the shell starts the server writing
//! into a named pipe, prints the server's process id, and copies what the
//! server writes to its own output from a background loop. Closing the
//! shell's standard input ends the loop and the shell the way a crash ends
//! the app, without asking the server anything.
//!
//! Unix only: on Windows the server waits on the app's process handle
//! instead, and CI runs these tests on Linux.

#![cfg(unix)]

// This test starts the server from a shell, so it uses only some of the
// shared helpers.
#[allow(dead_code)]
mod common;

use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use message_crate_serve_protocol::{EXIT_WITH_PARENT_ARG, PARENT_CHECK_INTERVAL};

use common::{WAIT, empty_message_crate, listening_address};

/// How long after a check the server may take to log that the stop begins.
/// It is a quarter of [`PARENT_CHECK_INTERVAL`], so a server that missed a
/// check fails the test.
const NOTICE_SLACK: Duration = Duration::from_millis(500);

/// How long a server on an empty database may take to exit once its stop
/// began: no request is in flight and no media pass runs, so this is the
/// time to drain nothing and exit, with room for a loaded CI runner.
const STOP_ALLOWANCE: Duration = Duration::from_secs(10);

/// The line the server writes to its log as a stop it was asked for begins,
/// before it tells the media pass to stop ffmpeg.
const SHUTTING_DOWN_LINE: &str = "The server is shutting down";

/// The line the server writes to its log as the last thing `serve` does
/// after a stop it was asked for, once the requests in flight and ffmpeg
/// have stopped.
const STOPPED_LINE: &str = "The server has stopped";

/// The shell standing in for the app, and the server it started.
struct Started {
    /// The shell. It exits once its standard input closes.
    parent: Child,
    /// The shell's standard input. Dropping it ends the shell.
    parent_input: Option<ChildStdin>,
    /// The server's process id, killed when the test ends however it ends.
    server_pid: libc::pid_t,
    /// What the server writes to standard error, a line at a time, while the
    /// shell runs.
    lines: Receiver<String>,
    /// The server's Data Directory, which holds its log.
    data_dir: PathBuf,
}

impl Drop for Started {
    fn drop(&mut self) {
        // SAFETY: `kill` takes no pointers. The id was the server's, and a
        // test ends long before the system could hand it to another process.
        unsafe {
            libc::kill(self.server_pid, libc::SIGKILL);
        }
        let _ = self.parent.kill();
        let _ = self.parent.wait();
    }
}

impl Started {
    /// Start the shell, which starts `serve` on the empty Message Crate in
    /// `root`, with `--exit-with-parent $$`, naming the shell, when
    /// `exit_with_parent`. Returns once the server listens, with its address.
    fn serve(root: &Path, exit_with_parent: bool) -> (Self, SocketAddr) {
        let (data_dir, static_dir) = empty_message_crate(root);
        let flag = if exit_with_parent {
            format!("--{EXIT_WITH_PARENT_ARG} $$")
        } else {
            String::new()
        };
        // `$0` is the server program, `$1` the Data Directory, `$2` the
        // website directory, `$3` the named pipe. Opening the pipe waits for
        // the other end, so the server writes nothing before the loop reads.
        // `read` keeps the shell alive until its standard input closes, when
        // it fails, so `|| true` makes the shell's own exit a success.
        let script = format!(
            r#"mkfifo "$3"
"$0" serve --data-dir "$1" --bind 127.0.0.1:0 --static-dir "$2" {flag} 2>"$3" >/dev/null </dev/null &
echo $!
while IFS= read -r line; do echo "$line"; done <"$3" &
copier=$!
read _ || true
kill $copier"#
        );
        let mut parent = Command::new("sh")
            .arg("-c")
            .arg(script)
            .arg(env!("CARGO_BIN_EXE_message-crate-server"))
            .arg(&data_dir)
            .arg(&static_dir)
            .arg(root.join("server-stderr"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(parent.stdout.take().unwrap());
        let mut pid = String::new();
        output.read_line(&mut pid).unwrap();
        let server_pid = pid.trim().parse().unwrap();
        let (sender, lines) = mpsc::channel();
        // Reads until the shell's output closes, so the pipe never fills.
        thread::spawn(move || {
            for line in output.lines().map_while(Result::ok) {
                let _ = sender.send(line);
            }
        });
        let parent_input = parent.stdin.take();
        let started = Self {
            parent,
            parent_input,
            server_pid,
            lines,
            data_dir,
        };
        let address = started.listening();
        (started, address)
    }

    /// Wait for the server's listening line and return the address it names.
    fn listening(&self) -> SocketAddr {
        let deadline = Instant::now() + WAIT;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(left).unwrap_or_else(|error| {
                panic!(
                    "the server wrote no listening line ({error}):\n{}",
                    seen.join("\n")
                )
            });
            if let Some(address) = listening_address(&line) {
                return address;
            }
            seen.push(line);
        }
    }

    /// End the shell without telling the server, as a crash ends the app.
    /// Returns when the shell has exited.
    fn end_parent(&mut self) -> Instant {
        drop(self.parent_input.take());
        let status = self.parent.wait().unwrap();
        assert!(status.success(), "{status}");
        Instant::now()
    }

    /// Whether the server is still a running process.
    fn server_runs(&self) -> bool {
        // SAFETY: `kill` with signal 0 takes no pointers and sends nothing;
        // it only reports whether the process exists.
        let exists = unsafe { libc::kill(self.server_pid, 0) } == 0;
        // An exited server that its new parent has not yet reaped still
        // exists, as a zombie, state `Z` in `/proc` on Linux.
        let zombie =
            std::fs::read_to_string(format!("/proc/{}/stat", self.server_pid)).is_ok_and(|stat| {
                stat.rsplit_once(')')
                    .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
            });
        exists && !zombie
    }

    /// Whether the server has exited by `deadline`.
    fn exited_by(&self, deadline: Instant) -> bool {
        loop {
            if !self.server_runs() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Everything in the server's log.
    fn log(&self) -> String {
        let mut log = String::new();
        for entry in std::fs::read_dir(self.data_dir.join("logs")).unwrap() {
            log.push_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap());
        }
        log
    }

    /// Whether the server's log holds `line` by `deadline`.
    fn logged_by(&self, line: &str, deadline: Instant) -> bool {
        loop {
            if self.log().contains(line) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
fn a_server_told_to_exit_with_its_parent_stops_once_the_parent_is_gone() {
    let root = tempfile::tempdir().unwrap();
    let (mut started, _address) = Started::serve(root.path(), true);

    let parent_ended = started.end_parent();

    // The stop begins at the first check after the parent ended.
    assert!(
        started.logged_by(
            SHUTTING_DOWN_LINE,
            parent_ended + PARENT_CHECK_INTERVAL + NOTICE_SLACK
        ),
        "the server had not begun to stop {:?} after its parent was gone:\n{}",
        PARENT_CHECK_INTERVAL + NOTICE_SLACK,
        started.log()
    );
    assert!(
        started.exited_by(Instant::now() + STOP_ALLOWANCE),
        "the server kept running {STOP_ALLOWANCE:?} after its stop began"
    );
    // It stopped the way Ctrl-C stops it, to the end, not by a panic on the
    // standard error the parent took with it.
    let log = started.log();
    assert!(log.contains(STOPPED_LINE), "the server's log:\n{log}");
}

#[test]
fn a_server_started_without_the_flag_keeps_running_when_its_parent_is_gone() {
    let root = tempfile::tempdir().unwrap();
    let (mut started, address) = Started::serve(root.path(), false);

    let parent_ended = started.end_parent();

    // Twice the interval a watching server checks at: one that watched would
    // have begun to stop by now.
    assert!(
        !started.exited_by(parent_ended + 2 * PARENT_CHECK_INTERVAL + NOTICE_SLACK),
        "the server stopped on its own"
    );
    let log = started.log();
    assert!(
        !log.contains(SHUTTING_DOWN_LINE),
        "the server's log:\n{log}"
    );
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_secs(2)).is_ok(),
        "the server no longer listens on {address}"
    );
}
