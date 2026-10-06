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
/// address, followed by the address it bound as `http://<address>`: with
/// `--bind 127.0.0.1:0`, the port the operating system chose. Only the process
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

/// The long name of the `serve` flag that names, by process id, the process
/// that started the server: `--exit-with-parent <pid>`.
///
/// The desktop app passes its own id. A server given the flag stops itself
/// once that process is gone, the same way it stops on Ctrl-C or SIGTERM, so
/// a crashed app leaves no server behind (#1934). A server started without
/// the flag, by Docker or `./scripts/run-dev.sh`, watches nothing.
pub const EXIT_WITH_PARENT_ARG: &str = "exit-with-parent";

/// How often a server started with [`EXIT_WITH_PARENT_ARG`] checks on Unix
/// whether its parent is still the process it was given. On Unix a process
/// whose parent ends gets a new parent, and nothing tells it so, which is why
/// the server asks. On Windows the server waits on the parent's process
/// handle, which is signalled the moment the parent ends.
///
/// The server reads it, and its tests hold it to it. It is part of the
/// interface because it is how long the server of a crashed app may still
/// answer: an app started again within it finds that server, uses it, and
/// starts its own once its next check of the address finds it gone.
pub const PARENT_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
