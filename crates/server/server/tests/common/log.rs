//! The server's log as a test reads it, and a server whose stderr is gone.

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;

use super::{Running, listening_address};

/// The newest file of the server's log in `data_dir`, the one every line
/// goes to: the files are numbered, and the highest number is the newest.
pub fn newest_log(data_dir: &Path) -> String {
    let newest = std::fs::read_dir(data_dir.join("logs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .max()
        .expect("the server's log has a file");
    std::fs::read_to_string(newest).unwrap()
}

/// `listen`, then close the read end of the server's stderr, as a desktop
/// app that crashed leaves it: every later write to stderr fails (#1934).
pub fn listen_then_close_stderr(command: &mut Command) -> (Running, SocketAddr) {
    let mut child = Running::spawn(command);
    let mut stderr = BufReader::new(child.0.stderr.take().unwrap());
    let mut seen = Vec::new();
    loop {
        let mut line = String::new();
        let read = stderr.read_line(&mut line).unwrap();
        assert_ne!(
            read,
            0,
            "the server closed its output before the listening line:\n{}",
            seen.join("")
        );
        if let Some(address) = listening_address(line.trim_end()) {
            return (child, address);
        }
        seen.push(line);
    }
}
