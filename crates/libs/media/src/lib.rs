//! Convert or compress one attachment file at a time ([`transcode_file`],
//! [`transcode_file_as`]). The desktop app's Media Stage, which walks a run's
//! conversations and their attachments, is `message_staging::transcode_staged`.
//!
//! Modes:
//! - **Disabled**: do not copy or write attachment files (the Import form's
//!   **Skip**)
//! - **Clone**: leave exported files as-is, a no-op after export (**Copy**)
//! - **Convert**: rewrite images to `.jpg`, videos to `.mp4`, audio to `.mp3`
//!   (**Convert media**)
//! - **Compress**: convert, and re-encode to shrink files, with optional video
//!   settings (**Convert & Compress media**)
//!
//! The `convert` and `compress` modes need `ffmpeg` / `ffprobe`, both on
//! `PATH` or both in the Tools Directory, and nowhere else. The caller says
//! where the Tools Directory is by the [`InstalledTools`] it passes in; a test
//! passes a fake [`MediaTools`] instead (`testutil::FakeTools`).

mod estimate;
#[cfg(any(test, feature = "testutil"))]
mod fake_tools;
mod mime;
mod probe;
mod process;
mod signature;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
mod tools;
mod versions;

pub use estimate::{SizeVerdict, classify_probed, estimate_bytes, needs_probe};
pub use mime::{
    Kind, MEDIA_TYPE_MAX_LEN, checked_media_type, ext_for_mime, is_media_type, kind_for_ext,
    kind_for_mime, mime_for_ext, usual_media_type,
};
pub use probe::MediaProbe;
pub use process::{
    HevcFallback, TranscodeOutcome, classify, derivative_name, derivative_name_for_missing,
    format_bytes, kind_of, transcode_file, transcode_file_as, why_not_converted,
};
pub use signature::{SIGNATURE_LEN, media_type_from_bytes, media_type_in_file};
pub use tools::{
    DEFAULT_STALL_WAIT, FfmpegFailed, FfmpegTools, InstalledTools, KillReason, Killed, MediaTools,
    NoTools, Reads, file_has_sha256, is_executable, tool_in_dir,
};
pub use versions::{
    THUMBNAIL_LONG_EDGE, browser_shows, make_preview, make_thumbnail, media_type_of,
    preview_extension, shown_as_is_by_type,
};

use std::fmt;
use std::ops::RangeInclusive;
use std::str::FromStr;

/// Attachment media handling after export.
///
/// Serialized under the names [`MediaMode::as_str`] gives. The Import form
/// names two modes differently ([`MediaMode::form_name`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaMode {
    /// Do not write attachment files during export.
    Disabled,
    /// Copy attachment files through unchanged; the default (a no-op after export).
    #[default]
    Clone,
    /// Rewrite images to `.jpg`, videos to `.mp4`, audio to `.mp3`.
    Convert,
    /// Re-encode attachments to shrink them per `CompressOptions`.
    Compress,
}

impl MediaMode {
    /// Every mode, so a check over all of them covers one added later.
    pub const ALL: [Self; 4] = [Self::Disabled, Self::Clone, Self::Convert, Self::Compress];

    /// Canonical lowercase name (`disabled` / `clone` / `convert` / `compress`),
    /// the value the export form and the desktop commands pass as text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Clone => "clone",
            Self::Convert => "convert",
            Self::Compress => "compress",
        }
    }

    /// Parse a mode name (case- and whitespace-insensitive; `none`, `skip` and
    /// `copy` are accepted aliases); `None` for unknown input.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "disabled" | "none" | "skip" => Some(Self::Disabled),
            "clone" | "copy" => Some(Self::Clone),
            "convert" => Some(Self::Convert),
            "compress" => Some(Self::Compress),
            _ => None,
        }
    }

    /// The name the Import form gives the mode (`skip` / `copy` / `convert` /
    /// `compress`): the inverse of the `skip` and `copy` aliases [`parse`]
    /// accepts, kept beside them so the two vocabularies meet in one place.
    ///
    /// [`parse`]: Self::parse
    pub fn form_name(self) -> &'static str {
        match self {
            Self::Disabled => "skip",
            Self::Clone => "copy",
            Self::Convert => "convert",
            Self::Compress => "compress",
        }
    }

    /// True when the mode requires ffmpeg/ffprobe (`convert` or `compress`).
    pub fn needs_tools(self) -> bool {
        matches!(self, Self::Convert | Self::Compress)
    }

    /// Whether exporters should write bytes under `attachments/`.
    pub fn copies_attachments(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

impl fmt::Display for MediaMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MediaMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| {
            format!(
                "unknown attachment setting '{s}' (expected disabled, clone, convert, or compress)"
            )
        })
    }
}

/// Max long-edge cap for video compress (no upscale).
///
/// Serialized under the names [`MaxResolution::as_str`] gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum MaxResolution {
    /// Cap the video long edge at 1280 px; the default, as on the Import
    /// form.
    #[default]
    #[serde(rename = "720p")]
    P720,
    /// Cap the video long edge at 1920 px.
    #[serde(rename = "1080p")]
    P1080,
    /// Cap the video long edge at 3840 px.
    #[serde(rename = "4k")]
    P4k,
}

impl MaxResolution {
    /// Canonical string (`720p` / `1080p` / `4k`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P720 => "720p",
            Self::P1080 => "1080p",
            Self::P4k => "4k",
        }
    }

    /// Pixel length of the long-edge cap.
    pub fn max_long_edge(self) -> u32 {
        match self {
            Self::P720 => 1280,
            Self::P1080 => 1920,
            Self::P4k => 3840,
        }
    }

    /// Parse `720p`/`1080p`/`4k` (or bare numbers); `None` for unknown input.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "720p" | "720" => Some(Self::P720),
            "1080p" | "1080" => Some(Self::P1080),
            "4k" | "2160p" | "2160" => Some(Self::P4k),
            _ => None,
        }
    }
}

impl fmt::Display for MaxResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MaxResolution {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| {
            format!("invalid media-max-resolution '{s}' (expected 720p, 1080p, or 4k)")
        })
    }
}

/// The whole numbers Max FPS accepts, in frames per second.
///
/// The Import form checks the same range as the person types
/// (`web/src/lib/compressFields.ts`), with the same message, so a change
/// here is made there too.
pub const MAX_FPS_RANGE: RangeInclusive<u32> = 1..=240;

/// The whole numbers Minimum Video File Size accepts, in megabytes of
/// 1024 × 1024 bytes: up to 1 TB.
///
/// The Import form checks the same range as the person types
/// (`web/src/lib/compressFields.ts`), with the same message, so a change
/// here is made there too.
pub const MIN_VIDEO_SIZE_MB_RANGE: RangeInclusive<u64> = 1..=1_048_576;

/// The refusal for a Max FPS outside [`MAX_FPS_RANGE`], word for word the
/// Import form's.
pub const MAX_FPS_INVALID: &str = "Max FPS must be 1–240.";

/// The refusal for a Minimum Video File Size outside
/// [`MIN_VIDEO_SIZE_MB_RANGE`], word for word the Import form's.
pub const MIN_VIDEO_SIZE_INVALID: &str = "Minimum Video File Size must be 1–1,048,576.";

/// A typed whole number inside `range`, or `None` for an empty field. Only
/// digits count, so `+5`, `-5`, `29.97` and `20MB` are refused along with
/// a number outside the range.
fn whole_number_in<T>(
    typed: &str,
    range: &RangeInclusive<T>,
    invalid: &str,
) -> anyhow::Result<Option<T>>
where
    T: FromStr + PartialOrd,
{
    let typed = typed.trim();
    if typed.is_empty() {
        return Ok(None);
    }
    typed
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| typed.parse::<T>().ok())
        .flatten()
        .filter(|n| range.contains(n))
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("{invalid}"))
}

/// Build [`CompressOptions`] from the Import form's values: the resolution
/// cap, the frame-rate cap as the person typed it (`30`), the minimum video
/// size as the whole number of megabytes the person typed (`20` is 20 MiB),
/// and whether already-efficient videos are skipped.
///
/// An empty field means no limit: an empty Max FPS keeps each video's frame
/// rate, and an empty Minimum Video File Size re-encodes every video, whatever
/// its size. Any other value must be a whole number in [`MAX_FPS_RANGE`] or
/// [`MIN_VIDEO_SIZE_MB_RANGE`].
///
/// The Import form checks the same rules as the person types
/// (`web/src/lib/compressFields.ts`), so these refusals are the last guard.
/// They name the form's fields by their labels, Max FPS and Minimum Video
/// File Size, in the form's own words, because the desktop app shows them to
/// the person as they are.
///
/// # Errors
///
/// Returns [`MAX_FPS_INVALID`] or [`MIN_VIDEO_SIZE_INVALID`] for a value that
/// is not empty and not a whole number in its range.
pub fn compress_options_from_form(
    max_resolution: MaxResolution,
    max_fps: &str,
    min_size: &str,
    skip_efficient: bool,
) -> anyhow::Result<CompressOptions> {
    let max_fps = whole_number_in(max_fps, &MAX_FPS_RANGE, MAX_FPS_INVALID)?;
    let megabytes = whole_number_in(min_size, &MIN_VIDEO_SIZE_MB_RANGE, MIN_VIDEO_SIZE_INVALID)?;
    Ok(CompressOptions {
        max_resolution,
        max_fps,
        min_size_bytes: megabytes.map(|mb| mb * 1024 * 1024),
        skip_efficient,
    })
}

/// Options applied only when [`MediaMode::Compress`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompressOptions {
    /// Long-edge cap applied when compressing video.
    pub max_resolution: MaxResolution,
    /// The highest frame rate a compressed video keeps, or `None` for no
    /// limit, so each video keeps its own.
    pub max_fps: Option<u32>,
    /// Videos smaller than this are not compressed. `None` is no minimum, so
    /// every video goes on to the efficiency check and the re-encode.
    pub min_size_bytes: Option<u64>,
    /// Skip already-efficient (HEVC, low bitrate) videos.
    pub skip_efficient: bool,
}

impl CompressOptions {
    /// True when a video of `size_bytes` is under the minimum size, so it is
    /// not re-encoded. With no minimum, no video is under it.
    pub fn under_min_size(&self, size_bytes: u64) -> bool {
        self.min_size_bytes.is_some_and(|min| size_bytes < min)
    }
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            max_resolution: MaxResolution::P720,
            max_fps: Some(30),
            min_size_bytes: Some(20 * 1024 * 1024),
            skip_efficient: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_and_resolution() {
        assert_eq!(MediaMode::parse("Convert"), Some(MediaMode::Convert));
        assert_eq!(MaxResolution::parse("4k"), Some(MaxResolution::P4k));
        assert_eq!(MaxResolution::P720.max_long_edge(), 1280);
    }

    #[test]
    fn every_form_name_parses_back_to_its_mode() {
        for mode in MediaMode::ALL {
            assert_eq!(MediaMode::parse(mode.form_name()), Some(mode));
        }
    }

    /// The Import form starts at 720p and the user guide says 720, so a
    /// caller that leaves the resolution out gets the same cap.
    #[test]
    fn the_default_resolution_is_the_import_forms_720p() {
        assert_eq!(MaxResolution::default(), MaxResolution::P720);
        assert_eq!(
            CompressOptions::default().max_resolution,
            MaxResolution::P720
        );
    }

    #[test]
    fn every_mode_alias_maps_to_its_mode() {
        for (alias, mode) in [
            ("disabled", MediaMode::Disabled),
            ("none", MediaMode::Disabled),
            ("skip", MediaMode::Disabled),
            ("clone", MediaMode::Clone),
            ("copy", MediaMode::Clone),
            ("convert", MediaMode::Convert),
            (" COMPRESS ", MediaMode::Compress),
        ] {
            assert_eq!(MediaMode::parse(alias), Some(mode), "{alias}");
        }
        assert_eq!(MediaMode::parse("shrink"), None);
        assert!("shrink".parse::<MediaMode>().is_err());
    }

    #[test]
    fn every_mode_reads_back_what_it_writes() {
        // (mode, needs ffmpeg, copies attachments)
        for (mode, needs_tools, copies) in [
            (MediaMode::Disabled, false, false),
            (MediaMode::Clone, false, true),
            (MediaMode::Convert, true, true),
            (MediaMode::Compress, true, true),
        ] {
            assert_eq!(mode.to_string().parse::<MediaMode>(), Ok(mode));
            assert_eq!(mode.needs_tools(), needs_tools, "{mode}");
            assert_eq!(mode.copies_attachments(), copies, "{mode}");
        }
    }

    #[test]
    fn every_resolution_reads_back_what_it_writes() {
        for cap in [
            MaxResolution::P720,
            MaxResolution::P1080,
            MaxResolution::P4k,
        ] {
            assert_eq!(cap.to_string().parse::<MaxResolution>(), Ok(cap));
        }
        assert!("480p".parse::<MaxResolution>().is_err());
    }

    #[test]
    fn every_resolution_alias_maps_to_its_cap() {
        for (alias, cap) in [
            ("720p", MaxResolution::P720),
            ("720", MaxResolution::P720),
            ("1080p", MaxResolution::P1080),
            ("1080", MaxResolution::P1080),
            ("4K", MaxResolution::P4k),
            ("2160p", MaxResolution::P4k),
            ("2160", MaxResolution::P4k),
        ] {
            assert_eq!(MaxResolution::parse(alias), Some(cap), "{alias}");
        }
        assert_eq!(MaxResolution::parse("480p"), None);
    }

    #[test]
    fn compress_options_from_form_reads_every_field() {
        let options = compress_options_from_form(MaxResolution::P720, "24", "2048", false).unwrap();
        assert_eq!(
            options,
            CompressOptions {
                max_resolution: MaxResolution::P720,
                max_fps: Some(24),
                min_size_bytes: Some(2 * 1024 * 1024 * 1024),
                skip_efficient: false,
            }
        );
    }

    /// #3167: an empty field is no limit, not a refusal.
    #[test]
    fn an_empty_field_means_no_limit() {
        for raw in ["", "  "] {
            let options = compress_options_from_form(MaxResolution::P720, raw, raw, true).unwrap();
            assert_eq!(options.max_fps, None, "{raw:?}");
            assert_eq!(options.min_size_bytes, None, "{raw:?}");
            assert!(!options.under_min_size(0));
        }
    }

    /// #3167: Max FPS is a whole number from 1 to 240, and one message,
    /// the Import form's, covers every other value.
    #[test]
    fn the_max_fps_is_a_whole_number_from_1_to_240() {
        let max_fps = |raw: &str| {
            compress_options_from_form(MaxResolution::P720, raw, "20", true).map(|o| o.max_fps)
        };
        assert_eq!(max_fps("1").unwrap(), Some(1));
        assert_eq!(max_fps(" 24 ").unwrap(), Some(24));
        assert_eq!(max_fps("240").unwrap(), Some(240));
        for raw in [
            "0", "241", "300", "20aaa", "29.97", "-5", "+5", "fast", "NaN", "inf",
        ] {
            assert_eq!(
                format!("{:#}", max_fps(raw).unwrap_err()),
                "Max FPS must be 1–240.",
                "{raw:?}"
            );
        }
    }

    /// #1469 and #3167: the Import form labels the field in megabytes, so
    /// `20` is 20 MiB, a unit typed after it is refused, and the size runs
    /// from 1 megabyte to 1 TB.
    #[test]
    fn the_minimum_size_is_a_whole_number_of_megabytes_up_to_1_tb() {
        let min_size = |raw: &str| {
            compress_options_from_form(MaxResolution::P720, "30", raw, true)
                .map(|o| o.min_size_bytes)
        };
        assert_eq!(min_size("1").unwrap(), Some(1024 * 1024));
        assert_eq!(min_size(" 20 ").unwrap(), Some(20 * 1024 * 1024));
        assert_eq!(
            min_size("1048576").unwrap(),
            Some(1024 * 1024 * 1024 * 1024)
        );
        for raw in [
            "0", "1048577", "20aaa", "29.97", "20M", "20MB", "512k", "-1", "+5",
        ] {
            assert_eq!(
                format!("{:#}", min_size(raw).unwrap_err()),
                "Minimum Video File Size must be 1–1,048,576.",
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_video_under_the_minimum_is_not_re_encoded() {
        let options = CompressOptions {
            min_size_bytes: Some(10),
            ..CompressOptions::default()
        };
        assert!(options.under_min_size(9));
        assert!(!options.under_min_size(10));
    }
}
