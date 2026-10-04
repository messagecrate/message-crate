//! How `message-crate-server serve` tells the program that started it how
//! the start went, defined once for the server and the desktop app, which
//! starts it (`src-tauri/src/local_server.rs`). It is the process interface
//! between the two, as `imessage-reader-protocol` is between the app and the
//! Apple Messages Reader, and is kept apart from the HTTP API's types.
//!
//! The app used to find both by matching sentences it had copied from the
//! server's output, so rewording either sentence broke the app with no
//! compile error and no failing test (#1416). The server now writes and
//! returns these values, and the app reads them, from here.

/// The start of the line `serve` writes to standard error once it holds its
/// address, followed by the address as `http://<bind>`. Only the process
/// that bound the port writes it, so it tells the app's own server from
/// another Message Crate answering at the same address.
pub const LISTENING_LINE: &str = "message-crate-server serve listening on ";

/// The exit code of `serve` when another server, or `reset-demo`, holds the
/// operation lock of its database. Every other failure exits with 1.
///
/// The desktop app reads it as the server of a second window started at the
/// same moment, and waits for that server to answer rather than reporting a
/// failed start. 75 is `EX_TEMPFAIL` in BSD's `sysexits.h`: try again later.
pub const OPERATION_LOCK_HELD_EXIT_CODE: u8 = 75;
