//! A panic on any thread of the server reaches the server's log, not stderr
//! alone (#2086). It runs in a test binary of its own because it installs
//! the process-wide subscriber and panic hook `serve` installs
//! (`logging::init`), which a unit test never does.

// Only `newest_log` is used here.
#[allow(dead_code)]
mod common;

use message_crate_server::logging;

#[test]
fn a_panic_on_a_server_thread_leaves_an_error_line_in_the_server_log() {
    let data_dir = tempfile::tempdir().unwrap();
    logging::init();
    let _files = logging::write_files_in(data_dir.path()).unwrap();

    let panicked = std::thread::spawn(|| panic!("the panic this test makes")).join();
    assert!(panicked.is_err(), "the thread panicked");

    let log = common::log::newest_log(data_dir.path());
    let line = log
        .lines()
        .find(|line| line.contains("the panic this test makes"))
        .unwrap_or_else(|| panic!("no line names the panic:\n{log}"));
    assert!(line.contains("ERROR"), "{line}");
    // The place it happened, which the panic's own text carries.
    assert!(line.contains("panic_log.rs"), "{line}");
}
