//! Where a long piece of work, an import, a dedupe or a pass over the
//! Assets, says how far it has got.
//!
//! A command run from a shell (`import`, `dedupe-cross-source`,
//! `process-assets`) prints its progress on standard output, because that
//! progress is its output. The same work under `serve` writes it through
//! `tracing`, as every other runtime message of the server is written
//! (`crate::logging`), so it reaches the server's log and the owner's Logs
//! panel, and a standard output nobody reads any more cannot stop it
//! (`docs/architecture/server-log.md`). The caller picks one and passes it
//! down; nothing on the way prints a line of its own.

use std::fmt;
use std::io::{self, Write};

/// Where progress lines go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Standard output, flushed after each line so a long run shows movement
    /// even through a pipe; warnings go to standard error. A write that
    /// fails, to a pipe whose reader is gone, is dropped: progress is never
    /// worth stopping the work for.
    Print,
    /// `tracing`, at `info` and `warn`: standard error, and the server's log
    /// under `serve`.
    Log,
}

impl Progress {
    /// Say how far the work has got.
    pub fn say(self, line: impl fmt::Display) {
        match self {
            Self::Print => {
                let mut out = io::stdout().lock();
                let _ = writeln!(out, "{line}");
                let _ = out.flush();
            }
            Self::Log => tracing::info!("{line}"),
        }
    }

    /// Say that something went wrong that the work carries on past.
    pub fn warn(self, line: impl fmt::Display) {
        match self {
            Self::Print => {
                let _ = writeln!(io::stderr().lock(), "{line}");
            }
            Self::Log => tracing::warn!("{line}"),
        }
    }
}
