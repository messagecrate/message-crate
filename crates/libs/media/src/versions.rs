//! The two versions the server makes of an attachment beside its original,
//! which it never changes (`docs/architecture/media.md`, rule 3):
//!
//! - a **Thumbnail**, a JPEG at most [`THUMBNAIL_LONG_EDGE`] pixels on its
//!   long side: the image scaled down, or a video's first frame. Every image
//!   and video gets one.
//! - a **Preview**, a copy every browser can show or play: a JPEG, an MP4
//!   with H.264 video and AAC audio, or an MP3. Only a file of a type that
//!   [`browser_shows`] says no to gets one.
//!
//! Each is written to the path the caller names and nowhere else, so nothing
//! is ever written beside the original.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};

use crate::Kind;
use crate::tools::{probe_video, require_ffmpeg, run_ffmpeg_until};

/// The longest side of a Thumbnail, in pixels. A smaller original keeps its
/// own size.
pub const THUMBNAIL_LONG_EDGE: u32 = 560;

/// The longest side of a video Preview, in pixels: 1080p.
const PREVIEW_VIDEO_LONG_EDGE: u32 = 1920;

/// The media types every browser shows as they are (rule 2): JPEG, PNG, GIF
/// and WebP images, and MP3 audio. MP4 is one too, but only with H.264 video,
/// which only the file can tell ([`browser_shows`]).
const SHOWN_AS_IS: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "audio/mpeg",
];

/// The media type of an attachment, lowercased and without parameters, from
/// what is known about it, in the order [`kind_of`](crate::kind_of) reads
/// it: the stored file's extension, then the declared MIME type, then the
/// extension of a name the export gave it. A declared alias such as
/// `image/jpg` reads as its usual spelling, `image/jpeg`.
#[must_use]
pub fn media_type_of(
    path: &Path,
    mime: Option<&str>,
    name_hints: &[Option<&str>],
) -> Option<String> {
    let by_extension = |path: &Path| {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(crate::mime_for_ext)
            .map(str::to_string)
    };
    if let Some(found) = by_extension(path) {
        return Some(found);
    }
    if let Some(declared) = mime.map(str::trim).filter(|m| !m.is_empty()) {
        let base = declared
            .split(';')
            .next()
            .unwrap_or(declared)
            .trim()
            .to_ascii_lowercase();
        // The usual spelling of an alias, through the one table.
        let usual = crate::ext_for_mime(&base)
            .and_then(crate::mime_for_ext)
            .map_or(base, str::to_string);
        return Some(usual);
    }
    name_hints
        .iter()
        .flatten()
        .find_map(|hint| by_extension(Path::new(hint)))
}

/// Whether every browser shows `src`, of media type `media_type`, as it is:
/// a JPEG, PNG, GIF or WebP image, an MP3, or an MP4 whose video is H.264.
/// Anything else, a HEIC photo, a HEVC video, an AMR voice note, needs a
/// Preview.
///
/// Only an MP4 is opened, by ffprobe, to read its video codec. One ffprobe
/// cannot read counts as not shown, so it is given a Preview.
#[must_use]
pub fn browser_shows(src: &Path, media_type: Option<&str>) -> bool {
    let Some(media_type) = media_type else {
        return false;
    };
    if SHOWN_AS_IS.contains(&media_type) {
        return true;
    }
    media_type == "video/mp4" && probe_video(src).is_ok_and(|probe| probe.codec == "h264")
}

/// Write the Thumbnail of the image or video `src` to `dest`, a JPEG: the
/// image, or a video's first frame, scaled to at most
/// [`THUMBNAIL_LONG_EDGE`] pixels on its long side and never enlarged.
/// Setting `stop` kills ffmpeg and fails the call; what it wrote to `dest`
/// is the caller's to remove.
///
/// # Errors
///
/// Returns an error when ffmpeg is missing or cannot read `src`, or `stop`
/// is set.
pub fn make_thumbnail(src: &Path, dest: &Path, stop: &AtomicBool) -> Result<()> {
    require_ffmpeg()?;
    let edge = THUMBNAIL_LONG_EDGE;
    let args = vec![
        "-y".into(),
        "-i".into(),
        path_str(src),
        "-map".into(),
        "0:v:0".into(),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-vf".into(),
        format!("scale='min({edge},iw)':'min({edge},ih)':force_original_aspect_ratio=decrease"),
        "-q:v".into(),
        "5".into(),
        "-f".into(),
        "image2".into(),
        "-c:v".into(),
        "mjpeg".into(),
        path_str(dest),
    ];
    run_ffmpeg_until(&args, stop).with_context(|| format!("thumbnail of {}", src.display()))
}

/// The extension of the Preview of a `kind` file: what every browser shows.
#[must_use]
pub fn preview_extension(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => ".jpg",
        Kind::Video => ".mp4",
        Kind::Audio => ".mp3",
    }
}

/// Write the Preview of `src`, a `kind` file, to `dest`: a JPEG for an
/// image, an MP4 with H.264 video (at most 1080p, 8-bit 4:2:0, which every
/// browser plays) and AAC audio for a video, an MP3 for audio. `dest`
/// carries [`preview_extension`]'s extension. Setting `stop` kills ffmpeg
/// and fails the call; what it wrote to `dest` is the caller's to remove.
///
/// # Errors
///
/// Returns an error when ffmpeg is missing or cannot convert `src`, or
/// `stop` is set.
pub fn make_preview(src: &Path, kind: Kind, dest: &Path, stop: &AtomicBool) -> Result<()> {
    require_ffmpeg()?;
    let wanted = preview_extension(kind);
    if dest.extension().and_then(|e| e.to_str()) != Some(&wanted[1..]) {
        bail!(
            "a {kind:?} Preview is written as {wanted}, not to {}",
            dest.display()
        );
    }
    let mut args: Vec<String> = vec!["-y".into(), "-i".into(), path_str(src)];
    match kind {
        Kind::Image => args.extend([
            "-map".into(),
            "0:v:0".into(),
            "-frames:v".into(),
            "1".into(),
            "-update".into(),
            "1".into(),
            "-q:v".into(),
            "2".into(),
        ]),
        Kind::Audio => args.extend([
            "-map".into(),
            "0:a:0".into(),
            "-c:a".into(),
            "libmp3lame".into(),
            "-q:a".into(),
            "4".into(),
        ]),
        Kind::Video => {
            let edge = PREVIEW_VIDEO_LONG_EDGE;
            args.extend([
                "-map".into(),
                "0:v:0".into(),
                "-map".into(),
                "0:a:0?".into(),
                "-vf".into(),
                format!(
                    "scale='min({edge},iw)':'min({edge},ih)':force_original_aspect_ratio=decrease,\
                     scale=trunc(iw/2)*2:trunc(ih/2)*2"
                ),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "veryfast".into(),
                "-crf".into(),
                "23".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "128k".into(),
                "-movflags".into(),
                "+faststart".into(),
            ]);
        }
    }
    args.push(path_str(dest));
    run_ffmpeg_until(&args, stop).with_context(|| format!("preview of {}", src.display()))
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
