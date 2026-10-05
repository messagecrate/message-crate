//! Shared state for the desktop process.
//!
//! The UI starts a long job and may later press Cancel. Those are separate
//! commands, so the job that is running, and its cancel flag, live here.

use message_crate_core::CancelFlag;

/// Data every command can reach through Tauri's managed state.
#[derive(Debug, Default)]
pub struct AppState {
    /// The desktop job that is running, if any. One runs at a time: a job
    /// command refuses to start while this is set (`commands::jobs`).
    pub job: Option<RunningJob>,
}

/// What a desktop job is, as the screens and `CONTEXT.md` name it. "Job" is
/// a word for the code only, so the refusal of a second job names both jobs
/// by this, and a job that panics is named by it too (`commands::jobs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobName {
    /// The `extract` command: the Staging Stage of an Import Run.
    Staging,
    /// The `transcode_staging` command: the Media Stage of an Import Run.
    Media,
    /// The `upload` command: the Upload Stage of an Import Run.
    Upload,
    /// The `pull` command, and the `format` command when it is the second
    /// step of an Export.
    Export,
    /// The `format` command started from Settings → Convert.
    Convert,
}

impl JobName {
    /// The name as it starts a sentence.
    pub fn label(self) -> &'static str {
        match self {
            Self::Staging => "Staging",
            Self::Media => "The Media Stage",
            Self::Upload => "The Upload",
            Self::Export => "An Export",
            Self::Convert => "A Convert",
        }
    }
}

/// The one desktop job that is running.
#[derive(Debug)]
pub struct RunningJob {
    /// What the job is, for the error that refuses a second job.
    pub name: JobName,
    /// This job's own cancel flag. The `cancel` command sets it, and the job
    /// reads it between steps and stops when it is true. A new flag is made
    /// for each job, so a Cancel never reaches a job started after it.
    pub cancel: CancelFlag,
}

impl AppState {
    /// Create state with no job running.
    pub fn new() -> Self {
        Self::default()
    }
}
