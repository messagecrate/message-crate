//! What `serve` tells the program that started it, read the way the desktop
//! app reads it (`src-tauri/src/local_server.rs`): the listening line from its
//! output, and the exit code when another server holds the operation lock. Both
//! values come from `message_crate_serve_protocol`, which the app reads
//! too, so a server that stops giving them fails here (#1416).

mod common;

use std::time::Instant;

use message_crate_serve_protocol::OPERATION_LOCK_HELD_EXIT_CODE;

use common::log::{listen_then_close_stderr, newest_log};
use common::{Running, WAIT, empty_message_crate, listen, serve};

#[test]
fn serve_says_it_listens_and_a_second_serve_exits_with_the_lock_held_code() {
    let root = tempfile::tempdir().unwrap();
    let (data_dir, static_dir) = empty_message_crate(root.path());

    let (_first, address) = listen(&mut serve(&data_dir, &static_dir));
    // The address it bound, not the `:0` it was given.
    assert_eq!(address.ip().to_string(), "127.0.0.1", "{address}");
    assert_ne!(address.port(), 0, "{address}");

    // The first server holds the operation lock, so the second cannot start.
    let mut second = Running::spawn(&mut serve(&data_dir, &static_dir));
    let status = second
        .exit_by(Instant::now() + WAIT)
        .expect("the server never exited");
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

    let mut child = Running::spawn(&mut serve(&data_dir, root.path()));
    let status = child
        .exit_by(Instant::now() + WAIT)
        .expect("the server never exited");
    assert!(!status.success(), "{status}");
    assert_ne!(
        status.code(),
        Some(i32::from(OPERATION_LOCK_HELD_EXIT_CODE)),
        "{status}"
    );
}

/// The line of `log` that holds `text`, failing the test with the whole log
/// when there is none.
fn line_holding<'a>(log: &'a str, text: &str) -> &'a str {
    log.lines()
        .find(|line| line.contains(text))
        .unwrap_or_else(|| panic!("no line holds {text:?}:\n{log}"))
}

/// `serve`'s start-up lines reach the server's log, not stderr alone, so
/// the owner of a Message Crate in Docker or in the desktop app can read
/// them (#2079).
#[test]
fn serve_writes_its_start_up_lines_in_the_server_log() {
    let root = tempfile::tempdir().unwrap();
    let (data_dir, static_dir) = empty_message_crate(root.path());

    let (_server, _address) = listen(serve(&data_dir, &static_dir).env("RUST_LOG", "info"));

    let log = newest_log(&data_dir);
    assert!(line_holding(&log, "is open, in journal mode").contains(" INFO "));
    assert!(line_holding(&log, "An attachment may be up to").contains(" INFO "));
}

/// A database that cannot be opened stops `serve` after it has opened the
/// server's log, and the reason is the last line of the newest file, at
/// `ERROR`, so the owner can read why the Message Crate would not start
/// (#2079).
#[test]
fn a_database_that_cannot_be_opened_is_named_in_the_server_log() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    let static_dir = root.path().join("static");
    std::fs::create_dir_all(&static_dir).unwrap();
    // A directory where the database file should be.
    std::fs::create_dir_all(data_dir.join("messagecrate.db")).unwrap();

    let mut server = Running::spawn(serve(&data_dir, &static_dir).env("RUST_LOG", "info"));
    let status = server
        .exit_by(Instant::now() + WAIT)
        .expect("the server never exited");
    assert!(!status.success(), "{status}");

    let log = newest_log(&data_dir);
    let last = log.lines().last().unwrap_or_default();
    assert!(last.contains(" ERROR "), "{log}");
    assert!(last.contains("The server stopped with an error"), "{log}");
    assert!(last.contains("messagecrate.db"), "{log}");
}

/// A server whose stderr has no reader any more, as when the desktop app
/// that started it crashed, still answers requests and writes their lines
/// to its log. A line that cannot be written to stderr is dropped, never
/// reported with a panic (#1934).
#[test]
fn a_server_whose_stderr_is_gone_still_answers_and_logs() {
    use std::io::{Read, Write};

    let root = tempfile::tempdir().unwrap();
    let (data_dir, static_dir) = empty_message_crate(root.path());
    let (_server, address) =
        listen_then_close_stderr(serve(&data_dir, &static_dir).env("RUST_LOG", "info"));

    // Twice: the first answer's line is the first write to the closed pipe.
    for _ in 0..2 {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .write_all(b"GET /v1/server HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    }
    assert!(line_holding(&newest_log(&data_dir), "/v1/server").contains(" INFO "));
}
