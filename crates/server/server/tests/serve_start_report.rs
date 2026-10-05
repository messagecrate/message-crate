//! What `serve` tells the program that started it, read the way the desktop
//! app reads it (`src-tauri/src/local_server.rs`): the listening line from its
//! output, and the exit code when another server holds the operation lock. Both
//! values come from `message_crate_serve_protocol`, which the app reads
//! too, so a server that stops giving them fails here (#1416).

mod common;

use std::time::Instant;

use message_crate_serve_protocol::{LISTENING_LINE, OPERATION_LOCK_HELD_EXIT_CODE};

use common::{Running, WAIT, create_database, exit_by, listen, serve};

/// Wait for `child` to exit, or fail the test if it is still running after
/// [`WAIT`].
fn exit_status(child: &mut std::process::Child) -> std::process::ExitStatus {
    exit_by(child, Instant::now() + WAIT).expect("the server never exited")
}

#[test]
fn serve_says_it_listens_and_a_second_serve_exits_with_the_lock_held_code() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    let static_dir = root.path().join("static");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&static_dir).unwrap();
    create_database(root.path(), &data_dir);

    let (_first, address) = listen(&mut serve(&data_dir, &static_dir));
    // The address it bound, not the `:0` it was given.
    let bound: std::net::SocketAddr = address
        .strip_prefix("http://")
        .and_then(|address| address.parse().ok())
        .unwrap_or_else(|| panic!("{LISTENING_LINE}{address}"));
    assert_eq!(bound.ip().to_string(), "127.0.0.1", "{address}");
    assert_ne!(bound.port(), 0, "{address}");

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
