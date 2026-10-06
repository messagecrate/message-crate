//! `serve --exit-with-parent <pid>`: the server stops once the process that
//! started it is gone (#1934).
//!
//! The desktop app starts the server with the app's own process id here
//! (`src-tauri/src/local_server.rs`). A normal close of the app kills the
//! server. A crash of the app kills nothing, so without this the server would
//! keep serving and converting media until the computer restarted. With it,
//! the server stops the way it stops on Ctrl-C or SIGTERM: the requests in
//! flight finish and ffmpeg is stopped (#1736).
//!
//! On Unix the server checks its parent process id every
//! [`PARENT_CHECK_INTERVAL`]. A process whose parent ends gets a new parent,
//! and nothing tells it so, so the server notices the change by asking. On
//! Windows the server waits on the parent's process handle, which the system
//! signals the moment the parent ends.

#[cfg(unix)]
use message_crate_serve_protocol::PARENT_CHECK_INTERVAL;

/// Resolve once the process `pid`, the server's parent, is gone. Resolves at
/// once when the server's parent is not `pid` to begin with, since that
/// parent ended before the server first looked.
#[cfg(unix)]
pub(crate) async fn parent_gone(pid: u32) {
    let mut checks = tokio::time::interval(PARENT_CHECK_INTERVAL);
    loop {
        // The first tick is immediate.
        checks.tick().await;
        if std::os::unix::process::parent_id() != pid {
            return;
        }
    }
}

/// Resolve once the process `pid` is gone. Resolves at once when no process
/// `pid` can be opened, which for the process that started the server means
/// it has already ended.
#[cfg(windows)]
pub(crate) async fn parent_gone(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    let (ended, gone) = tokio::sync::oneshot::channel::<()>();
    // A thread of its own rather than `spawn_blocking`: the wait has no end
    // while the parent runs, and the Tokio runtime waits for its blocking
    // tasks before it lets the process exit, so a server stopped by Ctrl-C
    // would never exit. A plain thread ends with the process.
    std::thread::spawn(move || {
        // SAFETY: `OpenProcess` takes no pointers, and a null handle is
        // checked before use.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if !handle.is_null() {
            // SAFETY: `handle` is an open process handle this thread owns,
            // waited on and then closed once.
            unsafe {
                WaitForSingleObject(handle, INFINITE);
                CloseHandle(handle);
            }
        }
        let _ = ended.send(());
    });
    let _ = gone.await;
}
