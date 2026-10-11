//! Fake `imessage-reader` programs, for tests here and in
//! `imessage-ir-exporter`.
//!
//! Each fake is a `/bin/sh` script that reads the request line and prints
//! the events a real, broken, or mismatched program would. They run on Unix
//! only; the real program is covered on every platform by the tests that
//! build it (`tests/helper_process.rs` here and in the exporter).

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use imessage_reader_protocol::Request;

use crate::Helper;
use message_crate_core::{LogSink, ProgressSink};

/// Write `body` as an executable `/bin/sh` script in `dir`. The script
/// reads the request line first, as the real program does.
///
/// A `cp` process writes the script, never this one. Linux refuses to run
/// a file any process holds open for writing (`ETXTBSY`), and a child that
/// another test thread starts holds every descriptor this process had open
/// from its fork until its exec. Written here, the script could be busy
/// when the test starts it; written by `cp`, no descriptor of this process
/// ever had it open for writing, so it starts the first time.
///
/// # Panics
///
/// Panics when the script cannot be written.
pub fn fake_helper(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("imessage-reader");
    let source = dir.join("imessage-reader.source");
    fs::write(&source, format!("#!/bin/sh\nread -r request\n{body}\n")).unwrap();
    let copied = Command::new("cp").arg(&source).arg(&path).status().unwrap();
    assert!(copied.success(), "cp the fake helper: {copied}");
    fs::remove_file(&source).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Start the fake at `path` with `request`.
///
/// # Panics
///
/// Panics when the fake cannot be started.
pub fn spawn_fake(path: &Path, request: &Request) -> Helper {
    Helper::spawn_at(path, request, LogSink::none(), ProgressSink::none())
        .unwrap_or_else(|e| panic!("start the fake helper: {e:#}"))
}

/// The shell line that prints a `source` event with `version`.
#[must_use]
pub fn source_line(version: u32) -> String {
    format!(r#"echo '{{"event":"source","protocol_version":{version},"encrypted":false}}'"#)
}
