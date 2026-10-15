//! Read one media file's shape with ffprobe: for the size forecast, the
//! Media Stage's compression, and the check whether a browser plays an MP4.
//! [`probe_media`] is the one place that reads a file's shape, so all three
//! judge a file the same way. [`played_for`] reads how long a file plays,
//! so the ffmpeg run that converts a video is stopped when it runs past
//! twice that, and one that converts audio gets a time limit by it.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Result, anyhow};

use crate::tools::InstalledTools;

/// What ffprobe reports about one media file's first video stream.
///
/// Stills have a stream too, with no frame rate.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MediaProbe {
    /// Codec as ffprobe spells it, lowercased: `hevc`, `h264`, `mjpeg`, `png`.
    pub codec: String,
    /// Pixel width. Zero only when ffprobe reported no dimensions.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Frames per second, `None` for stills.
    pub fps: Option<f32>,
    /// Bits per second, as ffprobe reports it. `0` when unreported (a still,
    /// or a stream ffprobe could not measure).
    pub bitrate: u64,
}

/// Ask ffprobe about `path`.
///
/// # Errors
///
/// Returns an error when ffprobe is missing, exits non-zero, or reports a
/// line this cannot read.
pub(crate) fn probe_media(tools: &InstalledTools, path: &Path) -> Result<MediaProbe> {
    let out = tools.run_ffprobe(
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,avg_frame_rate,bit_rate",
            "-of",
            "csv=p=0",
        ],
        path,
        None,
    )?;
    let text = String::from_utf8_lossy(&out);
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| anyhow!("ffprobe reported no stream for {}", path.display()))?;
    parse_probe_line(line.trim())
}

/// How long the media file at `path` plays, as its container says. `None`
/// when ffprobe is missing, fails, reports no length, as for a still,
/// reports only a length it estimated, or `stop` is set.
pub(crate) fn played_for(
    tools: &InstalledTools,
    path: &Path,
    stop: Option<&AtomicBool>,
) -> Option<Duration> {
    let (out, said) = tools
        .run_ffprobe_with_stderr(
            &[
                "-v",
                "warning",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
            ],
            path,
            stop,
        )
        .ok()?;
    length_read(&String::from_utf8_lossy(&out), &said)
}

/// What ffprobe writes when it estimates a file's length from the bitrate
/// of its first frames, as for an ADTS AAC file or a variable-bitrate MP3
/// without a header that counts its frames.
const ESTIMATED_LENGTH: &str = "Estimating duration from bitrate";

/// The length ffprobe answered on stdout, unless what it said on stderr
/// shows it only estimated it. An estimate from the first frames can come
/// out under half the real length, and a video conversion past twice the
/// length is stopped as looping (#2898).
fn length_read(answer: &str, said: &str) -> Option<Duration> {
    if said.contains(ESTIMATED_LENGTH) {
        return None;
    }
    parse_duration(answer)
}

/// Read ffprobe's `format=duration` answer, seconds with a fraction. `None`
/// for `N/A`, nothing, or a length that is not a positive finite number.
fn parse_duration(text: &str) -> Option<Duration> {
    let seconds: f64 = text.lines().next()?.trim().parse().ok()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Duration::try_from_secs_f64(seconds).ok()
}

/// Read one `codec,width,height,avg_frame_rate,bit_rate` line.
///
/// `bit_rate` is trailing and optional-in-practice: absent, ffprobe writes
/// `N/A`, or a still simply has none. All of those default to `0` rather
/// than erroring, same as a malformed width/height — only a genuinely
/// missing codec/width/height column fails the read.
fn parse_probe_line(line: &str) -> Result<MediaProbe> {
    let mut parts = line.split(',');
    let codec = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let width = parts
        .next()
        .ok_or_else(|| anyhow!("no width in {line:?}"))?;
    let height = parts
        .next()
        .ok_or_else(|| anyhow!("no height in {line:?}"))?;
    let rate = parts.next().unwrap_or_default();
    let bitrate = parts.next().unwrap_or_default();
    Ok(MediaProbe {
        codec,
        width: width.trim().parse().unwrap_or(0),
        height: height.trim().parse().unwrap_or(0),
        fps: parse_frame_rate(rate.trim()),
        bitrate: bitrate.trim().parse().unwrap_or(0),
    })
}

/// ffprobe writes frame rates as a rational: `30000/1001`, or `0/0` for a still.
fn parse_frame_rate(raw: &str) -> Option<f32> {
    let (num, den) = raw.split_once('/')?;
    let num: f32 = num.trim().parse().ok()?;
    let den: f32 = den.trim().parse().ok()?;
    if num <= 0.0 || den <= 0.0 {
        return None;
    }
    Some(num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_length_and_refuses_what_is_no_length() {
        assert_eq!(
            parse_duration("12.500000\n"),
            Some(Duration::from_millis(12_500))
        );
        for no_length in ["N/A", "", "0.000000", "-3", "nan", "inf", "1e400"] {
            assert_eq!(parse_duration(no_length), None, "{no_length:?}");
        }
    }

    /// A length ffprobe only estimated from the first frames' bitrate is no
    /// length, so a conversion of the file is never stopped as running past
    /// twice it (#2898). Other warnings leave the length as read.
    #[test]
    fn an_estimated_length_is_no_length() {
        let said = "[aac @ 0x5581] Estimating duration from bitrate, this may be inaccurate\n";
        assert_eq!(length_read("12.500000\n", said), None);
        assert_eq!(
            length_read("12.500000\n", "[mov @ 0x5581] some other warning\n"),
            Some(Duration::from_millis(12_500))
        );
        assert_eq!(
            length_read("12.500000\n", ""),
            Some(Duration::from_millis(12_500))
        );
    }

    #[test]
    fn parses_a_rational_frame_rate() {
        assert_eq!(parse_frame_rate("30000/1001"), Some(29.97003));
        assert_eq!(parse_frame_rate("30/1"), Some(30.0));
    }

    #[test]
    fn a_still_image_has_no_frame_rate() {
        // ffprobe reports 0/0 for a still: there is no rate, and dividing by
        // the denominator would be a panic on some inputs and a lie on others.
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate(""), None);
        assert_eq!(parse_frame_rate("N/A"), None);
    }

    #[test]
    fn reads_one_csv_line_into_a_probe() {
        let probe = parse_probe_line("hevc,3840,2160,30000/1001").unwrap();
        assert_eq!(probe.codec, "hevc");
        assert_eq!(probe.width, 3840);
        assert_eq!(probe.height, 2160);
        assert_eq!(probe.fps, Some(29.97003));
        // No bit_rate column on this line: defaults rather than errors, same
        // as a malformed width/height would.
        assert_eq!(probe.bitrate, 0);
    }

    #[test]
    fn a_short_csv_line_is_an_error_not_a_default() {
        // A defaulted probe reads as a 0x0 file, which the estimate would
        // divide by. Fail loudly instead.
        assert!(parse_probe_line("hevc,3840").is_err());
    }

    #[test]
    fn reads_the_bit_rate_column_when_present() {
        // The efficient-video skip (`is_efficient` in process.rs) gates on
        // bitrate, so this column has to come through, not just default.
        let probe = parse_probe_line("hevc,1920,1080,30/1,9000000").unwrap();
        assert_eq!(probe.bitrate, 9_000_000);
    }

    /// Write a short H.264 clip, `width` by `height` pixels at 25 fps, to `path`.
    fn make_h264_clip(tools: &InstalledTools, path: &Path, width: u32, height: u32) {
        let args: Vec<String> = [
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size={width}x{height}:rate=25"),
            "-frames:v",
            "5",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx264",
        ]
        .into_iter()
        .map(String::from)
        .chain(std::iter::once(path.to_string_lossy().into_owned()))
        .collect();
        tools.run_ffmpeg(&args).expect("generate probe fixture");
    }

    /// The parsing tests above never touch ffprobe itself: they hand-craft
    /// the CSV line. This exercises the real subprocess — the exact args,
    /// column order, and codec string ffprobe actually emits — against a
    /// generated fixture with a known codec, resolution, and frame rate.
    #[test]
    fn probes_a_real_video_file() {
        let Some(tools) = crate::testutil::real_ffmpeg() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        make_h264_clip(&tools, &path, 64, 48);

        let probe = probe_media(&tools, &path).expect("probe the generated fixture");
        assert_eq!(probe.codec, "h264");
        assert_eq!(probe.width, 64);
        assert_eq!(probe.height, 48);
        let fps = probe.fps.expect("a real video reports a frame rate");
        assert!((fps - 25.0).abs() < 0.01, "fps was {fps}");
    }

    /// A file name that is not UTF-8 is passed to ffprobe as it is. A probe
    /// that sent it as text once sent an empty name instead, so compression
    /// and the browser-playable check read such a video as unknown.
    ///
    /// Linux only: macOS file systems refuse a name that is not UTF-8.
    #[cfg(target_os = "linux")]
    #[test]
    fn probes_a_file_whose_name_is_not_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let Some(tools) = crate::testutil::real_ffmpeg() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        // ffmpeg is handed its output name as text, so the clip is made
        // under a UTF-8 name and then renamed.
        let made = dir.path().join("clip.mp4");
        make_h264_clip(&tools, &made, 32, 24);
        let path = dir.path().join(OsStr::from_bytes(b"clip-\xff.mp4"));
        std::fs::rename(&made, &path).unwrap();

        let probe = probe_media(&tools, &path).expect("probe a file with a non-UTF-8 name");
        assert_eq!(probe.codec, "h264");
        assert_eq!((probe.width, probe.height), (32, 24));
    }
}
