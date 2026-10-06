//! The server the app started, together with every process it starts.
//!
//! The server starts ffmpeg to convert a video, and a killed process cannot
//! stop its own children, so killing the server alone leaves ffmpeg running
//! until the video is done (#1737). The app therefore starts the server so
//! that its processes can be killed with it:
//!
//! - on Unix (Linux and macOS) the server leads a new process group, which
//!   every process it starts joins, and the app kills the group;
//! - on Windows the server runs in a Job Object set to kill every process in
//!   it when the job's last handle closes. The app kills the job, and the
//!   system closes the handle when the app ends any other way, so a crashed
//!   app ends the server and its ffmpeg too.
//!
//! A server that exits on its own, such as one that crashes, takes its
//! processes with it as well. On Unix the app kills the group once it finds
//! the server exited, before reaping it, while the server's process id still
//! names the group. On Windows closing the job does it.
//!
//! On Windows the server is put in the job just after it starts. It starts
//! nothing in that moment: ffmpeg runs only for media work, after the server
//! listens.

use std::io;
use std::process::{Child, Command, ExitStatus};

/// The server the app started, killed with every process it started.
#[derive(Debug)]
pub(super) struct ServerProcess {
    child: Child,
    /// The Job Object the server runs in, or `None` when the system refused
    /// one. The server is then killed alone.
    #[cfg(windows)]
    job: Option<std::os::windows::io::OwnedHandle>,
}

impl ServerProcess {
    /// Start `command` as the head of its own process tree.
    pub(super) fn spawn(command: &mut Command) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // 0: a new group whose id is the server's process id.
            command.process_group(0);
        }
        let child = command.spawn()?;
        #[cfg(windows)]
        let job = match windows_job::kill_on_close_job(&child) {
            Ok(job) => Some(job),
            Err(error) => {
                eprintln!("The server's processes will not end with it: {error}");
                None
            }
        };
        Ok(Self {
            child,
            #[cfg(windows)]
            job,
        })
    }

    /// The server process, for its process id and its output pipes.
    pub(super) const fn child(&mut self) -> &mut Child {
        &mut self.child
    }

    /// The server's process id.
    #[cfg(test)]
    pub(super) fn id(&self) -> u32 {
        self.child.id()
    }

    /// Whether the server has exited, without waiting. A server found
    /// exited takes every process it started with it, so an ffmpeg it left
    /// behind ends too. On Windows the job does that when this
    /// `ServerProcess` is dropped.
    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        #[cfg(unix)]
        if self.exited_unreaped() {
            // The server is not reaped yet, so its process id still names
            // its group.
            self.kill_tree();
        }
        self.child.try_wait()
    }

    /// Whether the server has exited and is not reaped yet.
    #[cfg(unix)]
    fn exited_unreaped(&self) -> bool {
        // `id_t` is the `u32` `Child::id` gives, on Linux and macOS alike.
        let pid: libc::id_t = self.child.id();
        // SAFETY: a `siginfo_t` of zeros is valid, and `si_pid` stays 0
        // when no process has exited.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is a `siginfo_t` the call may write. `WNOWAIT`
        // leaves the server unreaped, and `WNOHANG` returns at once.
        let found = unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                &raw mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        // SAFETY: `info` was written by `waitid`, or is still zeros.
        found == 0 && unsafe { info.si_pid() } != 0
    }

    /// Kill the server and every process it started, then wait for the
    /// server to end.
    pub(super) fn kill(mut self) {
        self.kill_tree();
        let _ = self.child.wait();
    }

    #[cfg(unix)]
    fn kill_tree(&mut self) {
        match libc::pid_t::try_from(self.child.id()) {
            // SAFETY: `killpg` takes no pointers. The group's id is the
            // server's process id, which is not given to another process
            // while the server is unwaited, as it is here.
            Ok(group) => unsafe {
                libc::killpg(group, libc::SIGKILL);
            },
            Err(_) => {
                let _ = self.child.kill();
            }
        }
    }

    #[cfg(windows)]
    fn kill_tree(&mut self) {
        match &self.job {
            Some(job) => windows_job::terminate(job),
            None => {
                let _ = self.child.kill();
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn kill_tree(&mut self) {
        let _ = self.child.kill();
    }
}

/// The Job Object calls, on Windows only.
#[cfg(windows)]
mod windows_job {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::process::Child;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };

    /// A new Job Object that kills its processes when its last handle
    /// closes, with `child` in it.
    pub(super) fn kill_on_close_job(child: &Child) -> io::Result<OwnedHandle> {
        // SAFETY: no security attributes and no name are valid arguments.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` is a handle just created and owned by nothing else.
        let job = unsafe { OwnedHandle::from_raw_handle(raw) };

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(std::mem::size_of_val(&limits))
            .map_err(|_| io::Error::other("job limits too large"))?;
        // SAFETY: `limits` is the structure the information class names,
        // and `size` is its size.
        let set = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                size,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both handles are open for as long as the call runs.
        let assigned =
            unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) };
        if assigned == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Kill every process in `job`.
    pub(super) fn terminate(job: &OwnedHandle) {
        // SAFETY: `job` is an open Job Object handle.
        unsafe {
            TerminateJobObject(job.as_raw_handle(), 1);
        }
    }
}
