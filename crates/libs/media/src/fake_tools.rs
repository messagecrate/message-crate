//! A stand-in for ffmpeg and ffprobe, for tests of what the Media Stage and
//! the server's Thumbnail and Preview decide: which file is re-encoded, with
//! which arguments, and what is kept.
//!
//! It answers a fixed probe and writes fixed bytes where ffmpeg would write,
//! so those tests run without ffmpeg installed. This crate's tests use it,
//! and so do the tests of every crate that lists `media` with the
//! `testutil` feature (`media::testutil::FakeTools`), wherever the code under
//! test takes a [`MediaTools`]. What the real programs make of the arguments
//! is tested against the real programs ([`crate::testutil::real_ffmpeg`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use anyhow::{Result, anyhow, bail};

use crate::tools::ffmpeg_command_line;
use crate::{FfmpegTools, KillReason, Killed, MediaProbe, MediaTools, Reads};

/// What ffprobe reports about a small, low-bitrate 320x240 video in
/// `codec` at `fps` frames per second.
pub fn video_probe(codec: &str, fps: f32) -> MediaProbe {
    MediaProbe {
        codec: codec.to_string(),
        width: 320,
        height: 240,
        fps: Some(fps),
        bitrate: 100_000,
    }
}

/// ffmpeg and ffprobe as a test sets them up, with every ffmpeg run kept.
#[derive(Debug)]
pub struct FakeTools {
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
    runs: Mutex<Vec<Vec<String>>>,
    /// What every ffmpeg run read, in order: the file it read whole, or
    /// `None` for one frame.
    reads: Mutex<Vec<Option<PathBuf>>>,
}

impl FakeTools {
    /// Both programs present: every probe fails, and ffmpeg writes a few
    /// bytes and succeeds.
    #[must_use]
    pub fn new() -> Self {
        Self {
            available: true,
            probe: None,
            output: b"fake ffmpeg output".to_vec(),
            failing: Vec::new(),
            runs: Mutex::new(Vec::new()),
            reads: Mutex::new(Vec::new()),
        }
    }

    /// Neither program present.
    #[must_use]
    pub fn missing() -> Self {
        Self {
            available: false,
            ..Self::new()
        }
    }

    /// ffprobe answers `probe` for every file.
    #[must_use]
    pub fn probing(mut self, probe: MediaProbe) -> Self {
        self.probe = Some(probe);
        self
    }

    /// ffmpeg writes `output` to the file it is asked to write.
    #[must_use]
    pub fn writing(mut self, output: impl Into<Vec<u8>>) -> Self {
        self.output = output.into();
        self
    }

    /// A run whose arguments hold `arg` fails.
    #[must_use]
    pub fn failing_on(mut self, arg: &str) -> Self {
        self.failing.push(arg.to_string());
        self
    }

    /// The arguments of every ffmpeg run so far, in order.
    #[must_use]
    pub fn runs(&self) -> Vec<Vec<String>> {
        self.runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// What every ffmpeg run so far read, in order: the file it read whole,
    /// or `None` for a run that reads one frame.
    #[must_use]
    pub fn reads(&self) -> Vec<Option<PathBuf>> {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Default for FakeTools {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaTools for FakeTools {
    /// Both programs at made-up paths, or neither.
    fn find(&self) -> Result<FfmpegTools> {
        Ok(if self.available {
            FfmpegTools {
                ffmpeg: Some(PathBuf::from("fake/ffmpeg")),
                ffprobe: Some(PathBuf::from("fake/ffprobe")),
            }
        } else {
            FfmpegTools::default()
        })
    }

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
        self.runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(ffmpeg_command_line(args, dest));
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(reads.source().map(Path::to_path_buf));
        std::fs::write(dest, &self.output)?;
        if args.iter().any(|arg| self.failing.contains(arg)) {
            bail!("ffmpeg failed (fake) on {}", args.join(" "));
        }
        Ok(())
    }
}
