//! Starting `message-crate-server serve` as the desktop app does, shared by
//! the tests that run the binary.

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use message_crate_serve_protocol::LISTENING_LINE;

/// How long a server on an empty database may take to listen or to exit.
pub const WAIT: Duration = Duration::from_secs(60);

/// A server process, killed when the test ends however it ends.
pub struct Running(Child);

impl Running {
    /// Start `command`.
    pub fn spawn(command: &mut Command) -> Self {
        Self(command.spawn().unwrap())
    }

    /// How the server exited, or `None` while it is still running at
    /// `deadline`.
    pub fn exit_by(&mut self, deadline: Instant) -> Option<ExitStatus> {
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn server() -> Command {
    Command::new(env!("CARGO_BIN_EXE_message-crate-server"))
}

/// An empty database in `data_dir`, so `serve` listens without first
/// building the Demo Account.
pub fn create_database(root: &Path, data_dir: &Path) {
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[paths]\ndb = {:?}\ndata_dir = {:?}\n",
            data_dir.join("messagecrate.db"),
            data_dir
        ),
    )
    .unwrap();
    let output = server()
        .arg("create-database")
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

/// `serve` as the desktop app starts it, on port 0. The server binds the
/// port the operating system chooses and names it on its listening line, so
/// no other process can take the port between the choice and the bind.
pub fn serve(data_dir: &Path, static_dir: &Path) -> Command {
    let mut command = server();
    command
        .arg("serve")
        .arg("--data-dir")
        .arg(data_dir)
        .arg("--bind")
        .arg("127.0.0.1:0")
        .arg("--static-dir")
        .arg(static_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

/// Start `command` and wait for its listening line: the running server and
/// the address the line names. A server that closes its output first fails
/// the test with its exit status and every line it wrote.
pub fn listen(command: &mut Command) -> (Running, SocketAddr) {
    let mut child = Running::spawn(command);
    let stderr = child.0.stderr.take().unwrap();
    let (lines, received) = mpsc::channel();
    // Reads until the server's output closes, so the pipe never fills.
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = lines.send(line);
        }
    });
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match received.recv_timeout(left) {
            Ok(line) => match line.strip_prefix(LISTENING_LINE) {
                Some(url) => {
                    let address = url
                        .strip_prefix("http://")
                        .and_then(|address| address.parse().ok())
                        .unwrap_or_else(|| panic!("the listening line names no address: {line}"));
                    return (child, address);
                }
                None => seen.push(line),
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!(
                "the server closed its output before the listening line ({}):\n{}",
                child
                    .exit_by(deadline)
                    .map_or_else(|| "still running".to_string(), |status| status.to_string()),
                seen.join("\n")
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => panic!(
                "the server wrote no listening line in {WAIT:?}:\n{}",
                seen.join("\n")
            ),
        }
    }
}
