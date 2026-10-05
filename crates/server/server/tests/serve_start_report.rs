//! What `serve` tells the program that started it, read the way the desktop
//! app reads it (`src-tauri/src/local_server.rs`): the listening line from its
//! output, and the exit code when another server holds the operation lock. Both
//! values come from `message_crate_serve_protocol`, which the app reads
//! too, so a server that stops giving them fails here (#1416).

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use message_crate_serve_protocol::{LISTENING_LINE, OPERATION_LOCK_HELD_EXIT_CODE};

/// How long a server on an empty database may take to listen or to exit.
const WAIT: Duration = Duration::from_secs(60);

/// A server process, killed when the test ends however it ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn server() -> Command {
    Command::new(env!("CARGO_BIN_EXE_message-crate-server"))
}

/// `serve` as the desktop app starts it, on a free port.
fn serve(data_dir: &Path, static_dir: &Path) -> Command {
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

/// An empty database in `data_dir`, so `serve` listens without first
/// building the Demo Account.
fn create_database(root: &Path, data_dir: &Path) {
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

/// Wait for `child` to exit, or fail the test if it is still running after
/// [`WAIT`].
fn exit_status(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "the server never exited");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn serve_says_it_listens_and_a_second_serve_exits_with_the_lock_held_code() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    let static_dir = root.path().join("static");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&static_dir).unwrap();
    create_database(root.path(), &data_dir);

    let mut first = Running(serve(&data_dir, &static_dir).spawn().unwrap());
    let stderr = first.0.stderr.take().unwrap();
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
            Ok(line) if line.starts_with(LISTENING_LINE) => {
                assert!(
                    line[LISTENING_LINE.len()..].starts_with("http://127.0.0.1:"),
                    "{line}"
                );
                break;
            }
            Ok(line) => seen.push(line),
            Err(_) => panic!(
                "the server never wrote the listening line:\n{}",
                seen.join("\n")
            ),
        }
    }

    // The first server holds the operation lock, so the second cannot start.
    let mut second = Running(serve(&data_dir, &static_dir).spawn().unwrap());
    let status = exit_status(&mut second.0);
    assert_eq!(
        status.code(),
        Some(i32::from(OPERATION_LOCK_HELD_EXIT_CODE)),
        "{status}"
    );
}

#[test]
fn any_other_failure_to_serve_is_not_the_lock_held_code() {
    let root = tempfile::tempdir().unwrap();
    // A file where the Data Directory should be: the start fails for a reason
    // that is not another server.
    let data_dir = root.path().join("data");
    std::fs::write(&data_dir, b"not a directory").unwrap();

    let mut child = Running(serve(&data_dir, root.path()).spawn().unwrap());
    let status = exit_status(&mut child.0);
    assert!(!status.success(), "{status}");
    assert_ne!(
        status.code(),
        Some(i32::from(OPERATION_LOCK_HELD_EXIT_CODE)),
        "{status}"
    );
}
