//! Typed progress events an exporter run reports while it works.
//!
//! Log lines ([`crate::LogSink`]) are for people. Progress events are for
//! whatever draws a progress bar: the desktop app renders them, and nothing
//! has to read counts back out of prose. Every event names the stage it
//! belongs to and carries the counts that stage has, so a caller can match
//! on the variant and use the fields without parsing anything.
//!
//! The events are emitted from the shared write layer (`message-staging`'s
//! `ExportWriter` and write queue, `message-ir-format`'s `FormatSink`, and
//! this crate's attachment stager) and from the few exporter-specific loops
//! that have progress worth showing (iMessage's message stream and its
//! backup-decrypt setup steps).

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::attachment_jobs::AttachmentProgress;

/// One progress report from an exporter run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// A setup step before any message is read, such as decrypting an iOS
    /// backup or caching chat tables. `step` of `total`, with a short label
    /// for the person watching ("Deriving backup keys").
    Setup {
        /// What this step does, without trailing punctuation.
        label: String,
        /// One-based index of this step.
        step: usize,
        /// How many setup steps this group has.
        total: usize,
    },
    /// Messages read from the backup so far.
    Parse {
        /// Messages read.
        done: usize,
        /// Messages the backup holds, or 0 when unknown.
        total: usize,
    },
    /// Attachment files staged so far, and the bytes they add up to.
    Attachments {
        /// Attachment jobs finished.
        done: usize,
        /// Attachment jobs in the run.
        total: usize,
        /// Bytes written so far.
        bytes_done: u64,
        /// Known byte total. Grows when a file had no size hint.
        bytes_total: u64,
    },
    /// Conversation files written so far.
    Prepare {
        /// Conversation files written (or skipped on a resumed run).
        done: usize,
        /// Conversation files the run will write.
        total: usize,
    },
    /// Attachments converted or compressed so far by the Media stage.
    Media {
        /// Files finished.
        done: usize,
        /// Files the Media stage covers.
        total: usize,
    },
    /// The write queue finished with one conversation file: it wrote the
    /// file, or a resumed run found it already written to the end and
    /// skipped it without reading the conversation again. The desktop app
    /// keeps a Staging row about a conversation apart until its file is
    /// written, because only until then does a resumed run report it again
    /// (#1688). Never held back, since each names its own file.
    FileWritten {
        /// The conversation file's name in the output directory, without
        /// the directory: the name the Upload gives the file too.
        file: String,
        /// Whether the write queue wrote the file now or skipped it.
        status: WriteStatus,
    },
}

/// What the write queue did with one conversation file
/// ([`ProgressEvent::FileWritten`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStatus {
    /// Written now.
    Written,
    /// A resumed run found the file already written to the end, and did not
    /// read its conversation again.
    Skipped,
}

impl ProgressEvent {
    /// Where this event's stage keeps its last-delivered time, or `None` for
    /// an event that is never held back. A setup step is one: each carries
    /// its own label, and there are only a handful of them. A finished
    /// conversation file is another: each names its own file.
    fn paced_stage(&self) -> Option<usize> {
        match self {
            Self::Setup { .. } | Self::FileWritten { .. } => None,
            Self::Parse { .. } => Some(0),
            Self::Attachments { .. } => Some(1),
            Self::Prepare { .. } => Some(2),
            Self::Media { .. } => Some(3),
        }
    }

    /// True for a stage's first and last counts, which are always delivered
    /// so the bar starts at zero and ends full. A `total` of 0 means the
    /// total is unknown, so no count is known to be the last.
    fn is_stage_boundary(&self) -> bool {
        match *self {
            Self::Setup { .. } | Self::FileWritten { .. } => true,
            Self::Parse { done, total }
            | Self::Attachments { done, total, .. }
            | Self::Prepare { done, total }
            | Self::Media { done, total } => done == 0 || (total > 0 && done >= total),
        }
    }
}

impl From<AttachmentProgress> for ProgressEvent {
    fn from(progress: AttachmentProgress) -> Self {
        Self::Attachments {
            done: progress.done,
            total: progress.total,
            bytes_done: progress.bytes_done,
            bytes_total: progress.bytes_total,
        }
    }
}

/// How long a stage waits between the counts it delivers. A run stages
/// thousands of files a second, and a count that changes that fast is
/// unreadable and floods whatever draws it.
pub const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// How many stages [`ProgressEvent::paced_stage`] names.
const PACED_STAGES: usize = 4;

/// Callback for typed progress events. The desktop app sets one; a run with
/// no bar to move passes [`ProgressSink::none`].
///
/// The sink paces what it delivers: each stage's counts reach the callback
/// at most once per [`PROGRESS_INTERVAL`], except a stage's first and last
/// counts and every setup step, which always do. Stages are paced apart
/// because the write queue reports conversations and attachments at the
/// same time.
#[derive(Clone)]
pub struct ProgressSink {
    callback: Arc<dyn Fn(ProgressEvent) + Send + Sync>,
    interval: Duration,
    last_delivered: Arc<Mutex<[Option<Instant>; PACED_STAGES]>>,
}

impl ProgressSink {
    /// Wrap a callback that receives paced events, one at a time.
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(ProgressEvent) + Send + Sync + 'static,
    {
        Self::with_interval(PROGRESS_INTERVAL, f)
    }

    /// A sink that drops every event. It is unpaced, so [`Self::emit`]
    /// answers `true` for each: with no bar, every event is due.
    pub fn none() -> Self {
        Self::unpaced(|_| {})
    }

    /// Wrap a callback that receives every event, for a caller (a test,
    /// mostly) that wants each count rather than a readable bar.
    pub fn unpaced<F>(f: F) -> Self
    where
        F: Fn(ProgressEvent) + Send + Sync + 'static,
    {
        Self::with_interval(Duration::ZERO, f)
    }

    fn with_interval<F>(interval: Duration, f: F) -> Self
    where
        F: Fn(ProgressEvent) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(f),
            interval,
            last_delivered: Arc::new(Mutex::new([None; PACED_STAGES])),
        }
    }

    /// Send one event to the callback if it is due, and say whether it was.
    pub fn emit(&self, event: ProgressEvent) -> bool {
        self.emit_at(Instant::now(), event)
    }

    fn emit_at(&self, now: Instant, event: ProgressEvent) -> bool {
        if let Some(stage) = event.paced_stage() {
            let mut last_delivered = self.last_delivered.lock().expect("progress pacing state");
            let held_back = !event.is_stage_boundary()
                && last_delivered[stage]
                    .is_some_and(|last| now.saturating_duration_since(last) < self.interval);
            if held_back {
                return false;
            }
            last_delivered[stage] = Some(now);
        }
        (self.callback)(event);
        true
    }
}

impl Default for ProgressSink {
    fn default() -> Self {
        Self::none()
    }
}

impl fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProgressSink")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paced sink that records every event delivered to it, for tests.
    pub(crate) fn recording_sink() -> (ProgressSink, Arc<Mutex<Vec<ProgressEvent>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = Arc::clone(&seen);
        let sink = ProgressSink::new(move |event| seen_clone.lock().unwrap().push(event));
        (sink, seen)
    }

    fn attachments(done: usize) -> ProgressEvent {
        ProgressEvent::Attachments {
            done,
            total: 100,
            bytes_done: 0,
            bytes_total: 0,
        }
    }

    #[test]
    fn a_stage_delivers_one_count_per_interval_plus_its_first_and_last() {
        let (sink, seen) = recording_sink();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);

        assert!(sink.emit_at(at(0), attachments(0)));
        assert!(!sink.emit_at(at(10), attachments(1)));
        assert!(!sink.emit_at(at(999), attachments(50)));
        assert!(sink.emit_at(at(1000), attachments(51)));
        assert!(!sink.emit_at(at(1500), attachments(99)));
        assert!(sink.emit_at(at(1501), attachments(100)));

        assert_eq!(
            seen.lock().unwrap().as_slice(),
            [attachments(0), attachments(51), attachments(100)]
        );
    }

    #[test]
    fn stages_are_paced_apart_and_setup_steps_are_never_held_back() {
        let (sink, seen) = recording_sink();
        let now = Instant::now();
        let setup = |step| ProgressEvent::Setup {
            label: "Deriving backup keys".into(),
            step,
            total: 3,
        };

        assert!(sink.emit_at(now, attachments(1)));
        assert!(sink.emit_at(now, ProgressEvent::Prepare { done: 1, total: 9 }));
        assert!(!sink.emit_at(now, ProgressEvent::Prepare { done: 2, total: 9 }));
        assert!(sink.emit_at(now, setup(1)));
        assert!(sink.emit_at(now, setup(2)));
        assert_eq!(seen.lock().unwrap().len(), 4);
    }

    #[test]
    fn an_unknown_total_has_no_last_count() {
        let (sink, _seen) = recording_sink();
        let now = Instant::now();
        assert!(sink.emit_at(now, ProgressEvent::Parse { done: 1, total: 0 }));
        assert!(!sink.emit_at(now, ProgressEvent::Parse { done: 2, total: 0 }));
    }

    #[test]
    fn an_unpaced_sink_delivers_every_count() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = Arc::clone(&seen);
        let sink = ProgressSink::unpaced(move |event| seen_clone.lock().unwrap().push(event));
        for done in 1..=5 {
            assert!(sink.emit(attachments(done)));
        }
        assert_eq!(seen.lock().unwrap().len(), 5);
    }

    /// A caller that writes a log line beside each count writes it when the
    /// count was due, so a sink with no bar must answer that every count is.
    #[test]
    fn a_sink_with_no_bar_answers_that_every_count_is_due() {
        let sink = ProgressSink::none();
        for done in 1..=5 {
            assert!(sink.emit(attachments(done)));
        }
    }

    #[test]
    fn attachment_progress_maps_onto_the_attachments_event() {
        let event = ProgressEvent::from(AttachmentProgress {
            done: 2,
            total: 3,
            bytes_done: 100,
            bytes_total: 500,
        });
        assert_eq!(
            event,
            ProgressEvent::Attachments {
                done: 2,
                total: 3,
                bytes_done: 100,
                bytes_total: 500,
            }
        );
    }
}
