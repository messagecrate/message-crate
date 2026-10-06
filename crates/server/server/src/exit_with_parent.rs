//! `serve --exit-with-parent <pid>`: the server stops once the process that
//! started it is gone (#1934).
//!
//! The desktop app starts the server with the app's own process id here
//! (`src-tauri/src/local_server.rs`). A normal close of the app kills the
//! server with its ffmpeg. A crash of the app kills nothing on Unix, so
//! without this the server would keep serving and converting media until the
//! computer restarted. With it, the server stops the way it stops on Ctrl-C
//! or SIGTERM: the requests in flight finish and ffmpeg is stopped (#1736).
//! On Windows the Job Object the app puts the server in usually kills it
//! first, when the system closes the crashed app's handle to the job (#1737);
//! this watch stops a server the system would not put in a job.
//!
//! On Unix the server checks its parent process id every
//! [`PARENT_CHECK_INTERVAL`]. A process whose parent ends gets a new parent,
//! and nothing tells it so, so the server notices the change by asking. On
//! Windows the server waits on the parent's process handle, which the system
//! signals the moment the parent ends.
//!
//! The app reads the server's standard output and standard error through
//! pipes, and a crashed app takes their read ends with it. A write to either
//! then fails, and `println!` and `eprintln!` panic on a failed write, which
//! would end the stop part-way. So once the parent is gone, both point at the
//! null device before the stop begins. The server's log, a file, still gets
//! every line.

#[cfg(unix)]
use message_crate_serve_protocol::PARENT_CHECK_INTERVAL;
use tokio::sync::oneshot;

/// A watch on the process that started the server, begun by [`Self::start`].
pub(crate) struct ParentWatch {
    /// The process watched.
    pid: u32,
    /// Sent to once the process is gone.
    gone: oneshot::Receiver<()>,
}

impl ParentWatch {
    /// Begin watching the process `pid`. `serve` calls it before anything
    /// else, so that on Windows the process is opened before the system could
    /// give its id to another process.
    pub(crate) fn start(pid: u32) -> Self {
        let (ended, gone) = oneshot::channel();
        watch(pid, ended);
        Self { pid, gone }
    }

    /// Resolve once the process is gone, with standard output and standard
    /// error pointing at the null device.
    pub(crate) async fn gone(self) {
        // A dropped sender means the watch itself ended, which happens only
        // as the process exits.
        let _ = self.gone.await;
        tracing::info!(
            "The process that started the server, process {}, has ended",
            self.pid
        );
    }
}

/// Check every [`PARENT_CHECK_INTERVAL`], the first time at once, whether
/// the server's parent is still `pid`, and send on `ended` once it is not.
/// A parent that is not `pid` to begin with ended before the server first
/// looked.
#[cfg(unix)]
fn watch(pid: u32, ended: oneshot::Sender<()>) {
    tokio::spawn(async move {
        let mut checks = tokio::time::interval(PARENT_CHECK_INTERVAL);
        loop {
            checks.tick().await;
            if std::os::unix::process::parent_id() != pid {
                break;
            }
        }
        silence_stdio();
        let _ = ended.send(());
    });
}

/// Wait on the process `pid` and send on `ended` once it is gone. A process
/// that cannot be opened has already ended.
#[cfg(windows)]
fn watch(pid: u32, ended: oneshot::Sender<()>) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    // SAFETY: `OpenProcess` takes no pointers. The handle it returns is
    // checked for null before use.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    // A raw handle is a pointer, which cannot cross to another thread; its
    // value can.
    let handle = handle as usize;
    // A thread of its own rather than `spawn_blocking`: the wait has no end
    // while the parent runs, and the Tokio runtime waits for its blocking
    // tasks before it lets the process exit, so a server stopped by Ctrl-C
    // would never exit. A plain thread ends with the process.
    std::thread::spawn(move || {
        let handle = handle as windows_sys::Win32::Foundation::HANDLE;
        if !handle.is_null() {
            // SAFETY: `handle` is an open process handle this thread owns,
            // waited on and then closed once.
            unsafe {
                WaitForSingleObject(handle, INFINITE);
                CloseHandle(handle);
            }
        }
        silence_stdio();
        let _ = ended.send(());
    });
}

/// Point standard output and standard error at the null device, so a write
/// to either no longer fails once the parent has taken its pipes with it.
/// Left as they are when the null device cannot be opened.
#[cfg(unix)]
fn silence_stdio() {
    use std::os::fd::AsRawFd;

    let Ok(null) = std::fs::OpenOptions::new().write(true).open("/dev/null") else {
        return;
    };
    // SAFETY: `dup2` takes no pointers. Descriptors 1 and 2 are replaced
    // atomically, so a write on another thread goes to the old pipe or to the
    // null device, never to a closed descriptor.
    unsafe {
        libc::dup2(null.as_raw_fd(), libc::STDOUT_FILENO);
        libc::dup2(null.as_raw_fd(), libc::STDERR_FILENO);
    }
}

/// Point standard output and standard error at the null device, so a write
/// to either no longer fails once the parent has taken its pipes with it.
/// Left as they are when the null device cannot be opened. The standard
/// library asks the system for the standard handles on every write, so the
/// change applies at once.
#[cfg(windows)]
fn silence_stdio() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};

    let Ok(null) = std::fs::OpenOptions::new().write(true).open("NUL") else {
        return;
    };
    // Kept open for the rest of the process, as the standard handles are.
    let null = null.into_raw_handle();
    // SAFETY: `null` is an open file handle that is never closed.
    unsafe {
        SetStdHandle(STD_OUTPUT_HANDLE, null);
        SetStdHandle(STD_ERROR_HANDLE, null);
    }
}
