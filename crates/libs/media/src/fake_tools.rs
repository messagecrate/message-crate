//! A stand-in for ffmpeg and ffprobe, for tests of what the Media Stage and
//! the server's Thumbnail and Preview decide: which file is re-encoded, with
//! which arguments, and what is kept.
//!
//! It answers a fixed probe and writes fixed bytes where ffmpeg would write,
//! so those tests run without ffmpeg installed and without the lock over the
//! tool location ([`crate::testutil::real_ffmpeg_test_guard`]). What the
//! real programs make of the arguments is tested against the real programs.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow, bail};

use crate::tools::{MediaTools, Reads, ffmpeg_command_line};
use crate::{KillReason, Killed, MediaProbe};

/// What ffprobe reports about a small, low-bitrate 320x240 video in
/// `codec` at `fps` frames per second.
pub(crate) fn video_probe(codec: &str, fps: f32) -> MediaProbe {
    MediaProbe {
        codec: codec.to_string(),
        width: 320,
        height: 240,
        fps: Some(fps),
        bitrate: 100_000,
    }
}

/// ffmpeg and ffprobe as a test sets them up, with every ffmpeg run kept.
pub(crate) struct FakeTools {
    /// False for a computer with neither program.
    available: bool,
    /// What ffprobe answers for every file; `None` makes every probe fail.
    probe: Option<MediaProbe>,
    /// What ffmpeg writes to the file it is asked to write.
    output: Vec<u8>,
    /// A run whose arguments hold any of these fails, after writing its
    /// output, as an ffmpeg that fails partway leaves a partial file.
    failing: Vec<String>,
    /// The arguments of every ffmpeg run, in order.
    runs: RefCell<Vec<Vec<String>>>,
    /// What every ffmpeg run read, in order: the file it read whole, or
    /// `None` for one frame.
    reads: RefCell<Vec<Option<PathBuf>>>,
}

impl FakeTools {
    /// Both programs present: every probe fails, and ffmpeg writes a few
    /// bytes and succeeds.
    pub(crate) fn new() -> Self {
        Self {
            available: true,
            probe: None,
            output: b"fake ffmpeg output".to_vec(),
            failing: Vec::new(),
            runs: RefCell::new(Vec::new()),
            reads: RefCell::new(Vec::new()),
        }
    }

    /// Neither program present.
    pub(crate) fn missing() -> Self {
        Self {
            available: false,
            ..Self::new()
        }
    }

    /// ffprobe answers `probe` for every file.
    pub(crate) fn probing(mut self, probe: MediaProbe) -> Self {
        self.probe = Some(probe);
        self
    }

    /// ffmpeg writes `output` to the file it is asked to write.
    pub(crate) fn writing(mut self, output: impl Into<Vec<u8>>) -> Self {
        self.output = output.into();
        self
    }

    /// A run whose arguments hold `arg` fails.
    pub(crate) fn failing_on(mut self, arg: &str) -> Self {
        self.failing.push(arg.to_string());
        self
    }

    /// The arguments of every ffmpeg run so far, in order.
    pub(crate) fn runs(&self) -> Vec<Vec<String>> {
        self.runs.borrow().clone()
    }

    /// What every ffmpeg run so far read, in order: the file it read whole,
    /// or `None` for a run that reads one frame.
    pub(crate) fn reads(&self) -> Vec<Option<PathBuf>> {
        self.reads.borrow().clone()
    }
}

impl MediaTools for FakeTools {
    fn require(&self) -> Result<()> {
        if self.available {
            Ok(())
        } else {
            bail!("ffmpeg and ffprobe were not found (fake)")
        }
    }

    fn can_probe(&self) -> bool {
        self.available
    }

    fn probe(&self, path: &Path) -> Result<MediaProbe> {
        if !self.available {
            bail!("ffprobe not found (fake)");
        }
        self.probe
            .clone()
            .ok_or_else(|| anyhow!("ffprobe cannot read {} (fake)", path.display()))
    }

    fn transcode(
        &self,
        args: &[String],
        dest: &Path,
        reads: Reads<'_>,
        stop: Option<&AtomicBool>,
    ) -> Result<()> {
        self.require()?;
        if stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
            return Err(Killed::ffmpeg(KillReason::StoppedBeforeStart).into());
        }
        self.runs.borrow_mut().push(ffmpeg_command_line(args, dest));
        self.reads
            .borrow_mut()
            .push(reads.source().map(Path::to_path_buf));
        std::fs::write(dest, &self.output)?;
        if args.iter().any(|arg| self.failing.contains(arg)) {
            bail!("ffmpeg failed (fake) on {}", args.join(" "));
        }
        Ok(())
    }
}
