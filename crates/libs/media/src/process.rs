use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};

use crate::tools::{FfmpegFailed, MediaTools, Reads};
use crate::{CompressOptions, KillReason, Killed, MediaMode};

/// Why ffmpeg did not convert `file`, short enough to keep on the
/// attachment as `convert_failed: <reason>`, and naming neither `file` nor
/// any other file in its directory by its path:
///
/// - why ffmpeg was stopped ([`Killed`]);
/// - the line ffmpeg wrote that names the cause ([`cause_line`]), with
///   each file it was given written as `<input>` or `<output>`
///   ([`FfmpegFailed::without_files`]) and its tags without their
///   addresses ([`without_addresses`]);
/// - how ffmpeg exited, when it failed and wrote nothing;
/// - otherwise the error's first line, with `file` written as `<input>`,
///   and without each step of it that names another file in `file`'s
///   directory, such as the temporary file a conversion writes.
///
/// The desktop app's Media Stage records this on the attachment. The whole
/// error, with everything ffmpeg wrote, goes to the run's log.
#[must_use]
pub fn why_not_converted(err: &anyhow::Error, file: &Path) -> String {
    if let Some(killed) = err.downcast_ref::<Killed>() {
        return killed.to_string();
    }
    if let Some(failed) = err.downcast_ref::<FfmpegFailed>() {
        return match cause_line(failed.said()) {
            Some(cause) => failed.without_files(&without_addresses(cause)),
            None => failed.to_string(),
        };
    }
    let file_text = file.display().to_string();
    let dir = file
        .parent()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    let steps: Vec<String> = err
        .chain()
        .map(|step| {
            let step = step.to_string();
            if file_text.is_empty() {
                step
            } else {
                step.replace(&file_text, "<input>")
            }
        })
        .filter(|step| dir.is_empty() || !step.contains(&dir))
        .collect();
    steps
        .join(": ")
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// `line` with each of its leading `[<component> @ 0x…]` tags written as
/// `[<component>]`. The address is a pointer inside the ffmpeg process,
/// different on every run, so the same failure would otherwise read
/// differently each time (#3055). A tag without an address, and the text
/// after the tags, are kept as they are.
fn without_addresses(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some((tag, after)) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
        let component = match tag.rsplit_once(" @ 0x") {
            Some((component, address))
                if !address.is_empty() && address.chars().all(|c| c.is_ascii_hexdigit()) =>
            {
                component
            }
            _ => tag,
        };
        let text = after.trim_start();
        out.push('[');
        out.push_str(component);
        out.push(']');
        out.push_str(&after[..after.len() - text.len()]);
        rest = text;
    }
    out.push_str(rest);
    out
}

/// What ffmpeg writes after a failure that only follows from it: a task
/// or thread ending, an encoder or a file that then would not open, and
/// that nothing was written. Each is matched at the start of a line's text,
/// after its `[<component> @ …]` tags.
const CONSEQUENCES: [&str; 9] = [
    "Nothing was written into output file",
    "Task finished with error code",
    "Terminating thread with return code",
    "Could not open encoder before EOF",
    "Error sending frames to consumers",
    "Error while opening encoder",
    "Error selecting an encoder",
    "Error opening input",
    "Error opening output",
];

/// Whether `line` is libx265's own report, which it writes to stderr at
/// any `-loglevel` and which ends what ffmpeg wrote even when it failed:
/// an `x265 [info]:` line, or its summary, `encoded <n> frames …`.
fn is_x265_report(line: &str) -> bool {
    line.starts_with("x265 [info]:")
        || line
            .strip_prefix("encoded ")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// Whether `line` of what ffmpeg wrote names no cause: it only follows from
/// an earlier failure ([`CONSEQUENCES`]), or is libx265's own report
/// ([`is_x265_report`]).
fn is_consequence(line: &str) -> bool {
    if is_x265_report(line) {
        return true;
    }
    let mut text = line;
    while let Some((_, rest)) = text.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
        text = rest.trim_start();
    }
    CONSEQUENCES
        .iter()
        .any(|consequence| text.starts_with(consequence))
}

/// The line of what ffmpeg `said` that names why it failed, trimmed: the
/// last line that names a cause ([`is_consequence`]). When every line only
/// follows from a failure, the first line that is not libx265's report,
/// which comes nearest the cause. The last line when every line is
/// libx265's report, and `None` when ffmpeg said nothing.
///
/// ffmpeg writes the cause first and what follows from it after, such as
/// that the encoder did not open and the output muxer's line that nothing
/// was written, so the last line can be a consequence rather than the
/// cause (#2959). The search goes back from the end rather than forward
/// from the start, because a damaged file makes ffmpeg write errors it
/// carries on past, such as a frame it could not decode, before the one it
/// stopped on, and the start of what is kept can be any of them.
fn cause_line(said: &str) -> Option<&str> {
    let lines = said.lines().map(str::trim).filter(|line| !line.is_empty());
    lines
        .clone()
        .rev()
        .find(|line| !is_consequence(line))
        .or_else(|| lines.clone().find(|line| !is_x265_report(line)))
        .or_else(|| lines.clone().next_back())
}

/// `err` on one line, with its whole chain, and each file a failed ffmpeg
/// run in it was given written as `<input>` or `<output>`
/// ([`FfmpegFailed::without_files`]).
fn error_line_without_files(err: &anyhow::Error) -> String {
    let line = format!("{err:#}");
    match err.downcast_ref::<FfmpegFailed>() {
        Some(failed) => failed.without_files(&line),
        None => line,
    }
}

/// Why HEVC failed, kept from the first video in a run that was compressed
/// as H.264 instead, so the run says it once, however many videos fell back,
/// and a person can see why the videos came out larger than expected.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HevcFallback {
    reason: Option<String>,
}

impl HevcFallback {
    /// Keep `hevc_failure`, one file's reason from
    /// [`TranscodeOutcome::Produced`], unless a reason is already kept.
    /// Returns the warning to log when this is the run's first reason, and
    /// `None` otherwise, so a caller that logs what this returns logs once.
    pub fn record(&mut self, hevc_failure: Option<String>) -> Option<String> {
        if self.reason.is_some() {
            return None;
        }
        self.reason = hevc_failure;
        self.line()
    }

    /// Why HEVC failed, or `None` when no video fell back: everything
    /// ffmpeg wrote, with the files it was given written as `<input>` and
    /// `<output>` ([`FfmpegFailed::without_files`]), so the server's log can
    /// hold it.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// The warning saying why HEVC failed, or `None` when no video fell back.
    #[must_use]
    pub fn line(&self) -> Option<String> {
        self.reason.as_ref().map(|reason| {
            format!("HEVC encoding failed, so videos were compressed as H.264 instead: {reason}")
        })
    }
}

/// JPEGs at or under this size are left alone in compress mode: re-encoding
/// them buys nothing.
const JPEG_COMPRESS_FLOOR: u64 = 500 * 1024;
/// MP3s at or under this size are left alone in compress mode.
const MP3_COMPRESS_FLOOR: u64 = 100 * 1024;

/// A byte count as KB, MB, or GB with one decimal, for progress lines and
/// reports. Decimal units: that is how people read a file size.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    const MB: f64 = KB * 1000.0;
    const GB: f64 = MB * 1000.0;
    let n = bytes as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.1} KB", n / KB)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) use crate::mime::Kind;

/// Sidecar written by ffmpeg before [`replace_original`] (must never remain on disk).
fn is_msgmedia_temp(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains(".msgmedia.tmp."))
}

/// Delete ffmpeg scratch left beside `path` by an earlier interrupted run.
///
/// Matched on the exact name a transcode of `path` could have written (see
/// `temp_sibling`), not a stem prefix, and scoped to `path`'s own kind: a
/// given kind only ever writes one scratch extension (`jpg` for images,
/// `mp3` for audio, `mp4` for video). Precision here matters because two
/// source files can share a stem (an iOS Live Photo's `IMG_0001.HEIC` and
/// `IMG_0001.MOV`, for instance), and a coarser, stem-only match would delete
/// one file's in-flight scratch while converting the other.
///
/// `path` itself can never be swept: `classify` treats a `.msgmedia.tmp.`
/// path as having no kind, so a caller that passes scratch as `path` never
/// gets here with a kind to sweep by.
fn remove_temps_beside(path: &Path, kind: Kind) {
    let ext = match kind {
        Kind::Image => "jpg",
        Kind::Audio => "mp3",
        Kind::Video => "mp4",
    };
    let _ = fs::remove_file(temp_sibling(path, ext));
}

/// The temp path a conversion writes to next to `path`.
fn temp_sibling(path: &Path, ext: &str) -> PathBuf {
    path.with_extension(format!("msgmedia.tmp.{ext}"))
}

/// Run work that writes `tmp`. Deletes `tmp` on any error (success must rename it away).
fn with_temp_output<T>(tmp: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match f() {
        Ok(v) => Ok(v),
        Err(err) => {
            let _ = fs::remove_file(tmp);
            Err(err)
        }
    }
}

/// Remux into MP4 without re-encoding and move it to `dest`; `None` when ffmpeg cannot remux the container.
fn try_remux(
    tools: &dyn MediaTools,
    path: &Path,
    dest: &Path,
    stop: Option<&AtomicBool>,
) -> Result<Option<PathBuf>> {
    let tmp = temp_sibling(path, "mp4");
    if remux_mp4(tools, path, &tmp, stop).is_err() {
        let _ = fs::remove_file(&tmp);
        return Ok(None);
    }
    match commit_produced(dest, path, &tmp) {
        Ok(p) => Ok(Some(p)),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
}

/// Media [`Kind`] of a file, from its extension via the shared
/// extension table in `crate::mime`; `None` for unrecognized extensions
/// and for this crate's own in-progress temp files.
pub fn classify(path: &Path) -> Option<Kind> {
    if is_msgmedia_temp(path) {
        return None;
    }
    crate::mime::kind_for_ext(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

/// Media [`Kind`] of an attachment from everything the caller knows about it:
/// the file's own extension first, then the declared MIME type, then the
/// names the export supplied for it.
///
/// The server stores an Asset as `<directory>/<sha256>` with no extension, so
/// a row whose MIME type is missing would otherwise have no kind and never be
/// converted. `name_hints` (the attachment's original name and its path
/// inside the export) are read only when the path and the declared MIME say
/// nothing. A declared MIME is authoritative even when it names something that
/// is not media, so an `application/pdf` file called `clip.mp4` is not a video.
///
/// GIFs are animations and are never converted: a `.gif` name or an
/// `image/gif` MIME answers `None`. The MIME is read as `kind_for_mime` reads
/// it, so `image/GIF` and `image/gif; charset=binary` are GIFs too.
pub fn kind_of(path: &Path, mime: Option<&str>, name_hints: &[Option<&str>]) -> Option<Kind> {
    let declared = mime.map(str::trim).filter(|m| !m.is_empty());
    if declared.and_then(crate::mime::ext_for_mime) == Some(".gif") || has_gif_ext(path) {
        return None;
    }
    if let Some(kind) = classify(path) {
        return Some(kind);
    }
    if let Some(declared) = declared {
        return crate::mime::kind_for_mime(declared);
    }
    name_hints
        .iter()
        .flatten()
        .map(Path::new)
        .filter(|hint| !has_gif_ext(hint))
        .find_map(classify)
}

/// True for a `.gif` name, in any case.
fn has_gif_ext(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gif"))
}

/// What the caller asked of the media step for one file.
#[derive(Clone, Copy)]
struct Asked<'a> {
    /// `convert` or `compress`.
    mode: MediaMode,
    /// How to compress, in [`MediaMode::Compress`].
    compress: &'a CompressOptions,
    /// The run's stop: setting it kills the ffmpeg that runs.
    stop: Option<&'a AtomicBool>,
}

/// Run the media step over one file, writing what it produces to `dest`.
///
/// Returns the produced path, or `None` when the media step leaves this file
/// alone, either because the mode does not touch it, or because a same-format
/// re-encode came out no smaller. A format change keeps the derivative even
/// when it is larger, because the person chose the target format.
fn run_one(
    tools: &dyn MediaTools,
    path: &Path,
    kind: Kind,
    asked: Asked<'_>,
    dest: &Path,
    hevc_failure: &mut Option<String>,
) -> Result<Option<PathBuf>> {
    let Asked {
        mode,
        compress,
        stop,
    } = asked;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match (kind, mode) {
        (Kind::Image, MediaMode::Convert) => {
            // Keep GIF as-is (animation); jpg already in target form.
            if matches!(ext.as_str(), "jpg" | "jpeg" | "gif") {
                return Ok(None);
            }
            convert_image(tools, path, false, false, dest, stop)
        }
        (Kind::Image, MediaMode::Compress) => {
            if ext == "gif" {
                return Ok(None);
            }
            let same_format = matches!(ext.as_str(), "jpg" | "jpeg");
            if same_format && fs::metadata(path)?.len() <= JPEG_COMPRESS_FLOOR {
                return Ok(None);
            }
            convert_image(tools, path, true, same_format, dest, stop)
        }
        (Kind::Audio, MediaMode::Convert) => {
            if ext == "mp3" {
                return Ok(None);
            }
            convert_audio(tools, path, false, false, dest, stop)
        }
        (Kind::Audio, MediaMode::Compress) => {
            let same_format = ext == "mp3";
            if same_format && fs::metadata(path)?.len() <= MP3_COMPRESS_FLOOR {
                return Ok(None);
            }
            convert_audio(tools, path, true, same_format, dest, stop)
        }
        (Kind::Video, MediaMode::Convert) => convert_video(tools, path, dest, stop).map(Some),
        (Kind::Video, MediaMode::Compress) => {
            compress_video(tools, path, compress, dest, stop, hevc_failure)
        }
        (_, MediaMode::Clone | MediaMode::Disabled) => Ok(None),
    }
}

/// What [`transcode_file`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscodeOutcome {
    /// Nothing was written: the mode does not touch this file, or a
    /// same-format re-encode came out no smaller.
    Skipped,
    /// A derivative was written to the destination the caller named.
    Produced {
        /// Why HEVC failed, when a compressed video was written as H.264
        /// instead: the whole HEVC error, with the files ffmpeg was given
        /// written as `<input>` and `<output>`. Not only its last line,
        /// because ffmpeg writes why an encoder would not open before the
        /// lines that follow from it. A caller keeps it in a
        /// [`HevcFallback`] for the run and logs what that returns, so the
        /// run says it once.
        hevc_failure: Option<String>,
    },
}

/// File name the media step would produce for `src`, or `None` when it leaves
/// the file alone.
///
/// Reads the same decision tree as the pass itself, so the name a caller
/// patches into a conversation file is the name the pass writes when it does
/// write one. For video this is a forecast, not a promise: `derivative_name`
/// cannot see `CompressOptions`, so it always answers `mp4` for a video in
/// either mode, even though `compress_video` may skip a small or
/// already-efficient file and `try_remux` may fall through on a
/// remux failure. Callers must treat [`TranscodeOutcome::Skipped`] from
/// [`transcode_file`] as authoritative over whatever this function predicted.
///
/// Stats `src` for the two size floors (compress-mode same-format JPEG/MP3).
/// When `src` may not exist on disk, use [`derivative_name_for_missing`]
/// instead. A stat failure here silently reads as size 0, which is under
/// both floors and answers `None`, the wrong answer for "is there a
/// candidate name to look for", not "is this live file worth touching".
#[must_use]
pub fn derivative_name(src: &Path, mode: MediaMode) -> Option<String> {
    derivative_name_impl(src, mode, |floor| {
        fs::metadata(src).map_or(0, |m| m.len()) <= floor
    })
}

/// Same decision tree as [`derivative_name`], but never stats `src`. It is
/// for a recorded path already known to be missing from disk.
///
/// The two size floors exist to skip a small file that is still there to
/// measure; a missing file's size is unknowable and irrelevant to the
/// question this variant answers ("what name would a committed derivative of
/// this file carry, if one exists"), so both floors are treated as never
/// crossed and the candidate name is always produced. The caller is
/// expected to check the filesystem for that name itself.
#[must_use]
pub fn derivative_name_for_missing(src: &Path, mode: MediaMode) -> Option<String> {
    derivative_name_impl(src, mode, |_floor| false)
}

/// The file name a conversion would produce, or `None` when the file would be left as-is;
/// `under_floor` says whether a size is too small to bother with.
fn derivative_name_impl(
    src: &Path,
    mode: MediaMode,
    under_floor: impl Fn(u64) -> bool,
) -> Option<String> {
    let kind = classify(src)?;
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = src.file_stem().and_then(|s| s.to_str())?;
    let target = match (kind, mode) {
        (_, MediaMode::Clone | MediaMode::Disabled) => return None,
        (Kind::Image, MediaMode::Convert) => {
            if matches!(ext.as_str(), "jpg" | "jpeg" | "gif") {
                return None;
            }
            "jpg"
        }
        (Kind::Image, MediaMode::Compress) => {
            if ext == "gif" {
                return None;
            }
            if matches!(ext.as_str(), "jpg" | "jpeg") && under_floor(JPEG_COMPRESS_FLOOR) {
                return None;
            }
            "jpg"
        }
        (Kind::Audio, MediaMode::Convert) => {
            if ext == "mp3" {
                return None;
            }
            "mp3"
        }
        (Kind::Audio, MediaMode::Compress) => {
            if ext == "mp3" && under_floor(MP3_COMPRESS_FLOOR) {
                return None;
            }
            "mp3"
        }
        // Forecast only: whether a video is actually rewritten depends on
        // CompressOptions and probed efficiency, neither visible here. See
        // the function doc.
        (Kind::Video, _) => "mp4",
    };
    Some(format!("{stem}.{target}"))
}

/// Transcode `src` and write the derivative to exactly `dest`.
///
/// `src` is never modified or deleted: committing is the caller's, because it
/// has to patch whatever points at the original first. Scratch left beside
/// `src` by an interrupted run is cleared; scratch belonging to other files,
/// and any `.in_progress` marker, is left alone.
///
/// Setting `stop` kills the ffmpeg that runs and fails the call as stopped
/// ([`KillReason::Stopped`], or [`KillReason::StoppedBeforeStart`] when it
/// was set first).
///
/// # Errors
///
/// Returns an error when ffmpeg/ffprobe are missing or fail, IO fails, or
/// `stop` is set.
pub fn transcode_file(
    tools: &dyn MediaTools,
    src: &Path,
    dest: &Path,
    mode: MediaMode,
    compress: &CompressOptions,
    stop: Option<&AtomicBool>,
) -> Result<TranscodeOutcome> {
    let kind = classify(src);
    // Clear this file's own scratch before checking the mode: an interrupted
    // run can leave scratch beside a file regardless of what mode retries it.
    if let Some(kind) = kind {
        remove_temps_beside(src, kind);
    }
    if matches!(mode, MediaMode::Clone | MediaMode::Disabled) {
        return Ok(TranscodeOutcome::Skipped);
    }
    let kind = kind.context("unknown media kind")?;
    transcode_as(
        tools,
        src,
        kind,
        dest,
        Asked {
            mode,
            compress,
            stop,
        },
    )
}

/// [`transcode_file`] for a source whose kind the caller already knows,
/// because the file carries no extension to read it from: the server stores
/// an original under its SHA-256 alone, and knows the kind from the MIME type
/// the import declared. Use [`kind_for_mime`](crate::kind_for_mime) for that.
///
/// A source with no extension is never "already in the target format", so in
/// `Convert` mode every image becomes a JPEG, every video an MP4, and every
/// audio file an MP3; the same-format floors only apply when the extension
/// says so.
///
/// Setting `stop` kills the ffmpeg that runs and fails the call as stopped
/// ([`KillReason::Stopped`], or [`KillReason::StoppedBeforeStart`] when it
/// was set first).
///
/// # Errors
///
/// Returns an error when ffmpeg/ffprobe are missing or fail, IO fails, or
/// `stop` is set.
pub fn transcode_file_as(
    tools: &dyn MediaTools,
    src: &Path,
    kind: Kind,
    dest: &Path,
    mode: MediaMode,
    compress: &CompressOptions,
    stop: Option<&AtomicBool>,
) -> Result<TranscodeOutcome> {
    remove_temps_beside(src, kind);
    if matches!(mode, MediaMode::Clone | MediaMode::Disabled) {
        return Ok(TranscodeOutcome::Skipped);
    }
    transcode_as(
        tools,
        src,
        kind,
        dest,
        Asked {
            mode,
            compress,
            stop,
        },
    )
}

/// The shared tail of the two `transcode_file` entry points.
fn transcode_as(
    tools: &dyn MediaTools,
    src: &Path,
    kind: Kind,
    dest: &Path,
    asked: Asked<'_>,
) -> Result<TranscodeOutcome> {
    tools.require()?;
    let mut hevc_failure = None;
    match run_one(tools, src, kind, asked, dest, &mut hevc_failure)? {
        Some(_) => Ok(TranscodeOutcome::Produced { hevc_failure }),
        None => Ok(TranscodeOutcome::Skipped),
    }
}

/// Move the produced file to exactly `dest`, leaving the original alone.
///
/// The caller commits: it patches whatever points at the original, renames
/// the derivative into its final name, and only then deletes the original,
/// so the final name never exists before a conversation file points at it.
fn commit_produced(dest: &Path, original: &Path, produced: &Path) -> Result<PathBuf> {
    if dest == original {
        // The final name must not exist until the caller has patched
        // whatever points at the original and renamed this derivative into
        // place itself. A destination equal to the original would overwrite
        // it here, before any of that has happened. For example,
        // `derivative_name` returning the source's own name (a same-format
        // compress) joined onto the source's directory without a
        // caller-added suffix like `.in_progress`.
        bail!(
            "transcode destination {} is the original file: write the \
             derivative to a distinct temporary name (e.g. suffixed with \
             `.in_progress`) and rename it into place only after patching \
             whatever points at {}",
            dest.display(),
            original.display()
        );
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    move_file(produced, dest)?;
    Ok(dest.to_path_buf())
}

/// Move `from` to `to`: a rename when both sit on one filesystem, else a copy
/// and a delete. The scratch file is written beside the source, and a caller's
/// destination (a temp directory, say) need not share a device with it.
fn move_file(from: &Path, to: &Path) -> Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
    fs::remove_file(from).with_context(|| format!("remove {}", from.display()))?;
    Ok(())
}

/// Is `produced` actually smaller than `original`?
///
/// Only meaningful for a same-format re-encode. Where the format changes the
/// user asked for the target format, and a smaller file in the source format
/// is not a substitute for it.
fn is_smaller(produced: &Path, original: &Path) -> Result<bool> {
    Ok(fs::metadata(produced)?.len() < fs::metadata(original)?.len())
}

/// Convert an image to JPEG (compressing when asked), keeping the original when the result
/// is not smaller and `keep_smaller` is set.
fn convert_image(
    tools: &dyn MediaTools,
    path: &Path,
    compress: bool,
    keep_smaller: bool,
    dest: &Path,
    stop: Option<&AtomicBool>,
) -> Result<Option<PathBuf>> {
    let tmp = temp_sibling(path, "jpg");
    let quality = if compress { "5" } else { "2" }; // ffmpeg -q:v (2 best … 31 worst for mjpeg)
    // `-frames:v 1 -update 1`: animated GIF/WebP must write a single still, not an
    // image2 sequence (otherwise ffmpeg leaves a partial tmp and exits non-zero).
    let args = vec![
        "-y".into(),
        "-i".into(),
        path_str(path),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-q:v".into(),
        quality.into(),
    ];
    with_temp_output(&tmp, || {
        tools
            .transcode(&args, &tmp, Reads::OneFrame, stop)
            .with_context(|| format!("convert image {}", path.display()))?;
        if keep_smaller && !is_smaller(&tmp, path)? {
            let _ = fs::remove_file(&tmp);
            return Ok(None);
        }
        commit_produced(dest, path, &tmp).map(Some)
    })
}

/// Convert audio to MP3 (compressing when asked), keeping the original when the result
/// is not smaller and `keep_smaller` is set.
fn convert_audio(
    tools: &dyn MediaTools,
    path: &Path,
    compress: bool,
    keep_smaller: bool,
    dest: &Path,
    stop: Option<&AtomicBool>,
) -> Result<Option<PathBuf>> {
    let tmp = temp_sibling(path, "mp3");
    let mut args = vec![
        "-y".into(),
        "-i".into(),
        path_str(path),
        "-vn".into(),
        "-acodec".into(),
        "libmp3lame".into(),
    ];
    if compress {
        args.extend(["-ac".into(), "1".into(), "-b:a".into(), "96k".into()]);
    } else {
        args.extend(["-q:a".into(), "4".into()]);
    }
    with_temp_output(&tmp, || {
        tools
            .transcode(&args, &tmp, Reads::Audio(path), stop)
            .with_context(|| format!("convert audio {}", path.display()))?;
        if keep_smaller && !is_smaller(&tmp, path)? {
            let _ = fs::remove_file(&tmp);
            return Ok(None);
        }
        commit_produced(dest, path, &tmp).map(Some)
    })
}

/// Convert a video to MP4: remux when the codecs allow, else re-encode.
fn convert_video(
    tools: &dyn MediaTools,
    path: &Path,
    dest: &Path,
    stop: Option<&AtomicBool>,
) -> Result<PathBuf> {
    let tmp = temp_sibling(path, "mp4");

    with_temp_output(&tmp, || {
        // Prefer remux into mp4 when already a video file.
        if remux_mp4(tools, path, &tmp, stop).is_ok() {
            return commit_produced(dest, path, &tmp);
        }
        let _ = fs::remove_file(&tmp);

        // Light standardize encode (H.264, 30fps, no aggressive downscale).
        // scripts/check-windows-arm-ffmpeg.sh runs this encode on Windows on
        // ARM each night, so a change here is a change there too.
        let args = vec![
            "-y".into(),
            "-i".into(),
            path_str(path),
            "-vf".into(),
            "fps=30".into(),
            "-c:v".into(),
            "libx264".into(),
            "-crf".into(),
            "23".into(),
            "-preset".into(),
            "medium".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "128k".into(),
            "-movflags".into(),
            "+faststart".into(),
        ];
        tools
            .transcode(&args, &tmp, Reads::Video(path), stop)
            .with_context(|| format!("convert video {}", path.display()))?;
        commit_produced(dest, path, &tmp)
    })
}

/// Copy the streams into an MP4 container without re-encoding.
fn remux_mp4(
    tools: &dyn MediaTools,
    path: &Path,
    tmp: &Path,
    stop: Option<&AtomicBool>,
) -> Result<()> {
    let args = vec![
        "-y".into(),
        "-i".into(),
        path_str(path),
        "-c".into(),
        "copy".into(),
        "-movflags".into(),
        "+faststart".into(),
    ];
    tools.transcode(&args, tmp, Reads::Video(path), stop)
}

/// Leave an MP4 as it is; remux anything else into one, so every video the
/// server keeps shares a container.
fn keep_or_remux(
    tools: &dyn MediaTools,
    path: &Path,
    dest: &Path,
    stop: Option<&AtomicBool>,
) -> Result<Option<PathBuf>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "mp4" {
        return Ok(None);
    }
    try_remux(tools, path, dest, stop)
}

/// Re-encode a video under the resolution, frame-rate, and quality caps; `None` when it is already within them.
///
/// HEVC is tried first, and H.264 when HEVC failed in a way H.264 may not
/// ([`h264_follows`]). When H.264 then succeeds, why HEVC failed goes into
/// `hevc_failure`; when H.264 fails too, the error carries both reasons.
fn compress_video(
    tools: &dyn MediaTools,
    path: &Path,
    opts: &CompressOptions,
    dest: &Path,
    stop: Option<&AtomicBool>,
    hevc_failure: &mut Option<String>,
) -> Result<Option<PathBuf>> {
    let meta = fs::metadata(path)?;
    if meta.len() < opts.min_size_bytes {
        // Still remux non-mp4 small files for container consistency.
        return keep_or_remux(tools, path, dest, stop);
    }

    // A probe that fails yields empty codec and zero dimensions, which never
    // count as efficient, so the file goes through compression like any other.
    let probe = tools.probe(path).unwrap_or_default();
    if opts.skip_efficient
        && is_efficient(&probe.codec, probe.width, probe.height, probe.bitrate, opts)
    {
        return keep_or_remux(tools, path, dest, stop);
    }

    let max_edge = opts.max_resolution.max_long_edge();
    let scale = format!(
        "scale='if(gt(iw,ih),min({max_edge},iw),-2)':'if(gt(iw,ih),-2,min({max_edge},ih))'"
    );
    let vf = match frame_rate_cap(probe.fps, opts.max_fps) {
        Some(fps) => format!("{scale},fps={fps}"),
        None => scale,
    };
    let tmp = temp_sibling(path, "mp4");

    with_temp_output(&tmp, || {
        // Prefer libx265; fall back to libx264.
        // scripts/check-windows-arm-ffmpeg.sh runs this encode on Windows on
        // ARM each night, so a change here is a change there too.
        let mut hevc_args = base_video_args(path, &vf);
        hevc_args.extend([
            "-c:v".into(),
            "libx265".into(),
            "-crf".into(),
            "22".into(),
            "-preset".into(),
            "medium".into(),
            "-tag:v".into(),
            "hvc1".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "128k".into(),
            "-movflags".into(),
            "+faststart".into(),
        ]);
        if let Err(hevc_err) = tools.transcode(&hevc_args, &tmp, Reads::Video(path), stop) {
            let _ = fs::remove_file(&tmp);
            if !h264_follows(&hevc_err) {
                return Err(hevc_err.context(format!("compress video {}", path.display())));
            }
            let mut avc_args = base_video_args(path, &vf);
            avc_args.extend([
                "-c:v".into(),
                "libx264".into(),
                "-crf".into(),
                "28".into(),
                "-preset".into(),
                "medium".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "96k".into(),
                "-movflags".into(),
                "+faststart".into(),
            ]);
            tools
                .transcode(&avc_args, &tmp, Reads::Video(path), stop)
                .with_context(|| format!("HEVC failed ({hevc_err:#}), and then H.264 failed"))
                .with_context(|| format!("compress video {}", path.display()))?;
            *hevc_failure = Some(error_line_without_files(&hevc_err));
        }
        Ok(Some(commit_produced(dest, path, &tmp)?))
    })
}

/// Whether H.264 is tried after HEVC failed with `hevc_err` (#2898).
///
/// - ffmpeg exited with an error, as when the encoder is missing or refused
///   the file, or did not start: yes.
/// - It made no progress and had never moved from the start: yes, because
///   the HEVC encoder most likely hung, as a hardware encoder can.
/// - It made no progress after moving: no, because the file is most likely
///   damaged at that point, and H.264 would stall there too.
/// - It ran past twice the file's length: no, because a damaged file loops.
/// - It was stopped: no, because nothing is wrong, and the run is ending.
/// - A run over its time limit, a short step or an audio conversion, never
///   reaches here; it is not followed either.
fn h264_follows(hevc_err: &anyhow::Error) -> bool {
    match hevc_err.downcast_ref::<Killed>().map(Killed::reason) {
        None | Some(KillReason::StalledBeforeOutput { .. }) => true,
        Some(
            KillReason::StalledPartway { .. }
            | KillReason::RanPastLength { .. }
            | KillReason::Stopped
            | KillReason::StoppedBeforeStart
            | KillReason::TookTooLong { .. },
        ) => false,
    }
}

/// The frame rate to bring a video down to, or `None` to leave its rate alone.
///
/// The maximum is a ceiling: a video at or under it keeps its frame rate, so
/// a 24 fps video is not raised to 30. A maximum of zero means none was given
/// and stands for 30. A video whose rate ffprobe could not read gets the
/// maximum, because nothing shows it is under it.
fn frame_rate_cap(source_fps: Option<f32>, max_fps: f32) -> Option<f32> {
    let max = if max_fps > 0.0 { max_fps } else { 30.0 };
    match source_fps {
        Some(source) if source <= max => None,
        _ => Some(max),
    }
}

/// The ffmpeg arguments shared by every video re-encode: input, filter graph, and output settings.
fn base_video_args(path: &Path, vf: &str) -> Vec<String> {
    vec![
        "-y".into(),
        "-i".into(),
        path_str(path),
        "-vf".into(),
        vf.into(),
    ]
}

/// Would `compress_video` skip re-encoding this stream and only remux it?
///
/// `compress_video` and the size forecast in `estimate.rs` both call this
/// rather than copying its thresholds, so one place decides what counts as
/// "already efficient enough."
pub(crate) fn is_efficient(
    codec: &str,
    width: u32,
    height: u32,
    bitrate: u64,
    opts: &CompressOptions,
) -> bool {
    let hevc = matches!(codec, "hevc" | "h265");
    if !hevc {
        return false;
    }
    let long = width.max(height);
    if long > opts.max_resolution.max_long_edge() {
        return false;
    }
    // ~12 Mbps threshold (archive-tools style)
    if bitrate > 12_000_000 {
        return false;
    }
    true
}

/// A path as a string for a command line.
fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
