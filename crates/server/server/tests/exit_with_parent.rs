//! A server the desktop app started stops itself once the app is gone, and a
//! server started any other way never does (#1934).
//!
//! Each test starts `serve` from a shell that stands in for the app: the shell
//! starts the server in the background, prints its process id, and waits on
//! its standard input. Closing that input ends the shell the way a crash ends
//! the app, without asking the server anything. The server writes to the
//! shell's standard error, which the test reads, so the pipe closes only once
//! the shell and the server have both exited.
//!
//! Unix only: on Windows the server waits on the app's process handle
//! instead, and CI runs these tests on Linux.

#![cfg(unix)]

// This test starts the server from a shell, so it uses only
// `create_database` and `WAIT` of the shared helpers.
#[allow(dead_code)]
mod common;

use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use message_crate_serve_protocol::{EXIT_WITH_PARENT_ARG, LISTENING_LINE, PARENT_CHECK_INTERVAL};

use common::{WAIT, create_database};

/// How long a server on an empty database may take to stop once it has
/// noticed its parent is gone: no request is in flight and no media pass
/// runs, so this is the time to drain nothing and exit.
const STOP_ALLOWANCE: Duration = Duration::from_secs(5);

/// The shell standing in for the app, and the server it started.
struct Started {
    /// The shell. It exits once its standard input closes.
    parent: Child,
    /// The shell's standard input. Dropping it ends the shell.
    parent_input: Option<ChildStdin>,
    /// The server's process id, killed when the test ends however it ends.
    server_pid: libc::pid_t,
    /// The server's standard error, a line at a time. Disconnected once the
    /// shell and the server have both exited.
    lines: Receiver<String>,
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
    /// Start the shell, which starts `serve` on an empty database with
    /// `exit_with_parent` among its arguments: `--exit-with-parent $$` names
    /// the shell. Returns once the server listens, with its address.
    fn serve(data_dir: &Path, static_dir: &Path, exit_with_parent: bool) -> (Self, SocketAddr) {
        let flag = if exit_with_parent {
            format!("--{EXIT_WITH_PARENT_ARG} $$")
        } else {
            String::new()
        };
        // `$0` is the server program, `$1` the Data Directory, `$2` the
        // website directory. `$!` is the background server's id, and `read`
        // keeps the shell alive until its standard input closes. `read`
        // fails there, so `|| true` makes the shell's own exit a success.
        let script = format!(
            r#""$0" serve --data-dir "$1" --bind 127.0.0.1:0 --static-dir "$2" {flag} >/dev/null </dev/null & echo $!; read _ || true"#
        );
        let mut parent = Command::new("sh")
            .arg("-c")
            .arg(script)
            .arg(env!("CARGO_BIN_EXE_message-crate-server"))
            .arg(data_dir)
            .arg(static_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut pid = String::new();
        BufReader::new(parent.stdout.take().unwrap())
            .read_line(&mut pid)
            .unwrap();
        let server_pid = pid.trim().parse().unwrap();
        let stderr = parent.stderr.take().unwrap();
        let (sender, lines) = mpsc::channel();
        // Reads until the output closes, so the pipe never fills.
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = sender.send(line);
            }
        });
        let parent_input = parent.stdin.take();
        let started = Self {
            parent,
            parent_input,
            server_pid,
            lines,
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
            if let Some(url) = line.strip_prefix(LISTENING_LINE) {
                return url
                    .strip_prefix("http://")
                    .and_then(|address| address.parse().ok())
                    .unwrap_or_else(|| panic!("the listening line names no address: {line}"));
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

    /// Whether the server's output closed, which it does when the server
    /// exits, before `deadline`.
    fn exited_by(&self, deadline: Instant) -> bool {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(_) => {}
                Err(RecvTimeoutError::Disconnected) => return true,
                Err(RecvTimeoutError::Timeout) => return false,
            }
        }
    }
}

/// A Data Directory with an empty database and an empty website directory,
/// so `serve` listens without first building the Demo Account.
fn empty_message_crate(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let data_dir = root.join("data");
    let static_dir = root.join("static");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&static_dir).unwrap();
    create_database(root, &data_dir);
    (data_dir, static_dir)
}

#[test]
fn a_server_told_to_exit_with_its_parent_stops_once_the_parent_is_gone() {
    let root = tempfile::tempdir().unwrap();
    let (data_dir, static_dir) = empty_message_crate(root.path());
    let (mut started, _address) = Started::serve(&data_dir, &static_dir, true);

    let parent_ended = started.end_parent();

    assert!(
        started.exited_by(parent_ended + PARENT_CHECK_INTERVAL + STOP_ALLOWANCE),
        "the server kept running {:?} after its parent was gone",
        PARENT_CHECK_INTERVAL + STOP_ALLOWANCE
    );
}

#[test]
fn a_server_started_without_the_flag_keeps_running_when_its_parent_is_gone() {
    let root = tempfile::tempdir().unwrap();
    let (data_dir, static_dir) = empty_message_crate(root.path());
    let (mut started, address) = Started::serve(&data_dir, &static_dir, false);

    let parent_ended = started.end_parent();

    // Twice the interval a watching server checks at, and the time it takes
    // to stop: a server that watched would be gone by now.
    assert!(
        !started.exited_by(parent_ended + 2 * PARENT_CHECK_INTERVAL + STOP_ALLOWANCE),
        "the server stopped on its own"
    );
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_secs(2)).is_ok(),
        "the server no longer listens on {address}"
    );
}
