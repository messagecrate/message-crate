use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};

use crate::tools::{FfmpegFailed, MediaTools, Reads};
use crate::{CompressOptions, KillReason, Killed, MediaMode};

/// Aggregate counts and errors from one media convert/compress pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MediaReport {
    /// Number of files converted or compressed.
    pub processed: usize,
    /// Number of files left unchanged.
    pub skipped: usize,
    /// Total bytes under `attachments/` before convert/compress (non-temp files).
    pub bytes_before: u64,
    /// Total bytes under `attachments/` after convert/compress (non-temp files).
    pub bytes_after: u64,
    /// What the pass could not do, for the log, from either of its two
    /// writers. [`process_attachment_files`] puts one `path: error` line
    /// here for each file it could not convert or compress, with the whole
    /// error. Staging's own Media pass (`run_media_post_pass` in
    /// `message-staging`) puts one sentence counting the files it could not
    /// convert instead, since each one's conversation entry already says
    /// why.
    pub errors: Vec<String>,
    /// Each file [`process_attachment_files`] could not convert or
    /// compress, with the reason its attachment records.
    pub not_converted: Vec<NotConverted>,
    /// Why HEVC failed, when the pass compressed a video as H.264 instead.
    /// The caller logs it as a warning with [`HevcFallback::line`].
    pub hevc_fallback: HevcFallback,
}

/// A file a convert or compress pass could not convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotConverted {
    /// The file, as the pass was given it.
    pub path: PathBuf,
    /// Why, as [`why_not_converted`] words it.
    pub reason: String,
}

/// Why ffmpeg did not convert `file`, short enough to keep on the
/// attachment as `convert_failed: <reason>`, and naming neither `file` nor
/// any other file in its directory by its path:
///
/// - why ffmpeg was stopped ([`Killed`]);
/// - the line ffmpeg wrote that names the cause ([`cause_line`]), with
///   each file it was given written as `<input>` or `<output>`
///   ([`FfmpegFailed::without_files`]);
/// - how ffmpeg exited, when it failed and wrote nothing;
/// - otherwise the error's first line, with `file` written as `<input>`,
///   and without each step of it that names another file in `file`'s
///   directory, such as the temporary file a conversion writes.
///
/// The desktop app's Media Stage and the convert pass of an Import Run that
/// converts as it stages both record this, so one failure reads alike on
/// either path. The whole error, with everything ffmpeg wrote, goes to the
/// run's log.
#[must_use]
pub fn why_not_converted(err: &anyhow::Error, file: &Path) -> String {
    if let Some(killed) = err.downcast_ref::<Killed>() {
        return killed.to_string();
    }
    if let Some(failed) = err.downcast_ref::<FfmpegFailed>() {
        return match cause_line(failed.said()) {
            Some(cause) => failed.without_files(cause),
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

/// How often to write `…n/total` progress lines during convert/compress.
const MEDIA_PROGRESS_EVERY: usize = 100;

/// JPEGs at or under this size are left alone in compress mode: re-encoding
/// them buys nothing.
const JPEG_COMPRESS_FLOOR: u64 = 500 * 1024;
/// MP3s at or under this size are left alone in compress mode.
const MP3_COMPRESS_FLOOR: u64 = 100 * 1024;

/// Convert or compress the given attachment files in place.
///
/// The caller builds `files` (usually via [`collect_media_files`]), so a
/// resumed or scoped pass can name exactly the files it means instead of
/// sweeping the whole directory. Paths must live under `output_dir`'s
/// `attachments/` directory.
///
/// Returns a path remap (`old_rel` → `new_rel`, forward-slash relative to
/// `output_dir`) for callers that update IR / CSV themselves.
///
/// Setting `stop` kills the ffmpeg that runs and converts no other file.
///
/// # Errors
///
/// Returns an error when ffmpeg/ffprobe are missing or fail, an input path
/// escapes the output directory, IO fails, or `stop` is set ([`Killed`]).
pub fn process_attachment_files(
    tools: &dyn MediaTools,
    output_dir: &Path,
    files: &[PathBuf],
    mode: MediaMode,
    compress: &CompressOptions,
    mut log: Option<&mut dyn FnMut(&str)>,
    stop: Option<&AtomicBool>,
) -> Result<(MediaReport, HashMap<String, String>)> {
    if matches!(mode, MediaMode::Clone | MediaMode::Disabled) {
        return Ok((MediaReport::default(), HashMap::new()));
    }
    tools.require()?;

    let attachments = output_dir.join("attachments");
    if !attachments.is_dir() {
        return Ok((MediaReport::default(), HashMap::new()));
    }

    // Leftovers from a previous failed ffmpeg run.
    remove_msgmedia_temps(&attachments)?;

    let mut report = MediaReport::default();
    let mut remap = HashMap::new();
    let total = files.len();
    if total == 0 {
        return Ok((report, remap));
    }

    report.bytes_before = attachments_dir_bytes(&attachments)?;
    let verb = match mode {
        MediaMode::Compress => "Compressing",
        _ => "Converting",
    };
    emit(&mut log, "");
    emit(&mut log, &starting_line(verb, total, report.bytes_before));

    let asked = Asked {
        mode,
        compress,
        stop,
    };
    let mut done = 0usize;
    for path in files {
        let mut hevc_failure = None;
        let outcome = process_one(tools, output_dir, path, asked, &mut hevc_failure);
        report.hevc_fallback.record(hevc_failure);
        match outcome {
            Ok(Outcome::Changed { old_rel, new_rel }) => {
                report.processed += 1;
                remap.insert(old_rel, new_rel);
            }
            Ok(Outcome::Skipped) => report.skipped += 1,
            // A conversion the stop killed is not the file's failure, and
            // no other file is started.
            Err(_) if stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) => {
                let _ = remove_msgmedia_temps(&attachments);
                return Err(Killed::ffmpeg_stopped().into());
            }
            Err(err) => {
                report.errors.push(format!("{}: {err:#}", path.display()));
                report.not_converted.push(NotConverted {
                    path: path.clone(),
                    reason: why_not_converted(&err, path),
                });
            }
        }
        done += 1;
        if done.is_multiple_of(MEDIA_PROGRESS_EVERY) || done == total {
            emit(&mut log, &format!("  …{done}/{total}"));
        }
    }

    // Always sweep again so a failed convert cannot leave junk behind.
    let swept =
        remove_msgmedia_temps(&attachments).and_then(|()| attachments_dir_bytes(&attachments));
    report.bytes_after = match (swept, report.hevc_fallback.line()) {
        (Ok(bytes), _) => bytes,
        (Err(err), None) => return Err(err),
        // The report is lost with this error, so the error carries the
        // fallback its caller would have logged.
        (Err(err), Some(line)) => {
            return Err(err.context(format!("{line}. Then converting stopped")));
        }
    };

    emit(&mut log, &done_line_with_failures(mode, &report));

    Ok((report, remap))
}

/// `n` files, singular for one. This crate sits below `message-crate-core`
/// (core depends on it), so it words the count itself rather than through
/// `count_of_files`.
fn files(n: usize) -> String {
    if n == 1 {
        "1 file".to_string()
    } else {
        format!("{n} files")
    }
}

/// The line that starts the pass: `verb` is `Converting` or `Compressing`,
/// over `total` files of `bytes` in all.
fn starting_line(verb: &str, total: usize, bytes: u64) -> String {
    format!(
        "{verb} attachments ({}, {})…",
        files(total),
        format_bytes(bytes)
    )
}

/// The line that ends a Convert or Compress pass over staged attachments:
/// the files processed and skipped, and the attachments' size before and
/// after. The write queue's own pass ends with it too, so both passes word
/// it alike.
pub fn done_line(mode: MediaMode, report: &MediaReport) -> String {
    format!(
        "Attachment {mode} done: processed {}, skipped {}, size {} → {}",
        files(report.processed),
        files(report.skipped),
        format_bytes(report.bytes_before),
        format_bytes(report.bytes_after),
    )
}

/// [`done_line`] with the files that failed, when any did. The write queue
/// counts its failures as one note instead, so only this pass adds them.
fn done_line_with_failures(mode: MediaMode, report: &MediaReport) -> String {
    let mut line = done_line(mode, report);
    if !report.errors.is_empty() {
        let _ = write!(line, ", {} failed", files(report.errors.len()));
    }
    line
}

/// Send one line to the log callback, if there is one.
fn emit(log: &mut Option<&mut dyn FnMut(&str)>, line: &str) {
    if let Some(log) = log.as_mut() {
        log(line);
    }
}

/// Sum sizes of non-temp files under `attachments/` (directory-level total).
fn attachments_dir_bytes(attachments: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in files_under(attachments)? {
        let path = entry.path();
        if !is_msgmedia_temp(&path) {
            total = total.saturating_add(
                entry
                    .metadata()
                    .with_context(|| format!("stat {}", path.display()))?
                    .len(),
            );
        }
    }
    Ok(total)
}

/// Every regular file under `root`, recursively, reading each entry's type
/// without following a symlink.
///
/// Only real directories are descended into, and a symlink is skipped
/// whatever it points at. A link under `root` therefore never brings files
/// from elsewhere into a media pass, and a link to a parent directory cannot
/// make the walk loop (#2262).
///
/// A socket, pipe or device file is skipped too, because a conversion that
/// opened one could block on it.
///
/// # Errors
///
/// Returns an error when a directory under `root` cannot be read.
fn files_under(root: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry.with_context(|| format!("read {}", dir.display()))?;
            let file_type = entry
                .file_type()
                .with_context(|| format!("stat {}", entry.path().display()))?;
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                out.push(entry);
            }
        }
    }
    Ok(out)
}

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

enum Outcome {
    Changed { old_rel: String, new_rel: String },
    Skipped,
}

pub(crate) use crate::mime::Kind;

/// List the files media conversion would touch under `root`.
///
/// Every non-temp file [`classify`] recognizes, recursively, sorted so two
/// runs enumerate in the same order. A symlink is skipped and never followed,
/// so nothing outside `root` is listed. Callers hand the result — or a subset of
/// it — to [`process_attachment_files`].
///
/// # Errors
///
/// Returns an error when a directory under `root` cannot be read.
pub fn collect_media_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = files_under(root)?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| !is_msgmedia_temp(path) && classify(path).is_some())
        .collect();
    out.sort();
    Ok(out)
}

/// Sidecar written by ffmpeg before [`replace_original`] (must never remain on disk).
fn is_msgmedia_temp(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains(".msgmedia.tmp."))
}

/// Delete `*.msgmedia.tmp.*` files left by an interrupted run anywhere under `root`.
fn remove_msgmedia_temps(root: &Path) -> Result<()> {
    for entry in files_under(root)? {
        let path = entry.path();
        if is_msgmedia_temp(&path) {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

/// Delete ffmpeg scratch left beside `path` by an earlier interrupted run.
///
/// Matched on the exact name a transcode of `path` could have written (see
/// `temp_sibling`), not a stem prefix, and scoped to `path`'s own kind: a
/// given kind only ever writes one scratch extension (`jpg` for images,
/// `mp3` for audio, `mp4` for video). Precision here matters because two
/// source files can share a stem — an iOS Live Photo's `IMG_0001.HEIC` and
/// `IMG_0001.MOV`, for instance — and a coarser, stem-only match would delete
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

/// Remux into MP4 without re-encoding and commit it; `None` when ffmpeg cannot remux the container.
fn try_remux_replace(
    tools: &dyn MediaTools,
    path: &Path,
    commit: Commit<'_>,
    stop: Option<&AtomicBool>,
) -> Result<Option<PathBuf>> {
    let tmp = temp_sibling(path, "mp4");
    if remux_mp4(tools, path, &tmp, stop).is_err() {
        let _ = fs::remove_file(&tmp);
        return Ok(None);
    }
    match commit_produced(commit, path, &tmp) {
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
    /// Convert or compress.
    mode: MediaMode,
    /// How to compress, in [`MediaMode::Compress`].
    compress: &'a CompressOptions,
    /// The run's stop: setting it kills the ffmpeg that runs.
    stop: Option<&'a AtomicBool>,
}

/// Run the media step over one file, committing however `commit` says.
///
/// Returns the produced path, or `None` when the media step leaves this file
/// alone — either because the mode does not touch it, or because a same-format
/// re-encode came out no smaller. A format change keeps the derivative even
/// when it is larger, because the person chose the target format.
fn run_one(
    tools: &dyn MediaTools,
    path: &Path,
    kind: Kind,
    asked: Asked<'_>,
    commit: Commit<'_>,
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
            convert_image(tools, path, false, false, commit, stop)
        }
        (Kind::Image, MediaMode::Compress) => {
            if ext == "gif" {
                return Ok(None);
            }
            let same_format = matches!(ext.as_str(), "jpg" | "jpeg");
            if same_format && fs::metadata(path)?.len() <= JPEG_COMPRESS_FLOOR {
                return Ok(None);
            }
            convert_image(tools, path, true, same_format, commit, stop)
        }
        (Kind::Audio, MediaMode::Convert) => {
            if ext == "mp3" {
                return Ok(None);
            }
            convert_audio(tools, path, false, false, commit, stop)
        }
        (Kind::Audio, MediaMode::Compress) => {
            let same_format = ext == "mp3";
            if same_format && fs::metadata(path)?.len() <= MP3_COMPRESS_FLOOR {
                return Ok(None);
            }
            convert_audio(tools, path, true, same_format, commit, stop)
        }
        (Kind::Video, MediaMode::Convert) => convert_video(tools, path, commit, stop).map(Some),
        (Kind::Video, MediaMode::Compress) => {
            compress_video(tools, path, compress, commit, stop, hevc_failure)
        }
        (_, MediaMode::Clone | MediaMode::Disabled) => Ok(None),
    }
}

/// Convert or compress one file by its media kind and report what changed.
fn process_one(
    tools: &dyn MediaTools,
    output_dir: &Path,
    path: &Path,
    asked: Asked<'_>,
    hevc_failure: &mut Option<String>,
) -> Result<Outcome> {
    let old_rel = rel_path(output_dir, path)?;
    let kind = classify(path).context("unknown media kind")?;
    match run_one(tools, path, kind, asked, Commit::InPlace, hevc_failure)? {
        Some(new_path) => changed(output_dir, &old_rel, &new_path),
        None => Ok(Outcome::Skipped),
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
/// already-efficient file and `try_remux_replace` may fall through on a
/// remux failure. Callers must treat [`TranscodeOutcome::Skipped`] from
/// [`transcode_file`] as authoritative over whatever this function predicted.
///
/// Stats `src` for the two size floors (compress-mode same-format JPEG/MP3).
/// When `src` may not exist on disk, use [`derivative_name_for_missing`]
/// instead — a stat failure here silently reads as size 0, which is under
/// both floors and answers `None`, the wrong answer for "is there a
/// candidate name to look for", not "is this live file worth touching".
#[must_use]
pub fn derivative_name(src: &Path, mode: MediaMode) -> Option<String> {
    derivative_name_impl(src, mode, |floor| {
        fs::metadata(src).map_or(0, |m| m.len()) <= floor
    })
}

/// Same decision tree as [`derivative_name`], but never stats `src` — for a
/// recorded path already known to be missing from disk.
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
    match run_one(tools, src, kind, asked, Commit::To(dest), &mut hevc_failure)? {
        Some(_) => Ok(TranscodeOutcome::Produced { hevc_failure }),
        None => Ok(TranscodeOutcome::Skipped),
    }
}

/// A `Changed` outcome naming the new relative path. Always Changed, even when the path
/// is the same: callers must refresh digests after an in-place rewrite.
fn changed(output_dir: &Path, old_rel: &str, new_path: &Path) -> Result<Outcome> {
    let new_rel = rel_path(output_dir, new_path)?;
    // Always report Changed — even when the relative path is unchanged (e.g. JPG
    // recompressed in place). Callers must invalidate digest_sha256 for remapped
    // paths; treating same-path rewrites as Skipped left stale fingerprints in
    // JSON Lines files and caused message-crate-import sha256 mismatches after upload.
    Ok(Outcome::Changed {
        old_rel: old_rel.to_string(),
        new_rel,
    })
}

/// The path under `output_dir` with forward slashes.
fn rel_path(output_dir: &Path, path: &Path) -> Result<String> {
    let rel = path
        .strip_prefix(output_dir)
        .with_context(|| format!("{} not under {}", path.display(), output_dir.display()))?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// `path` with its extension replaced by `ext`; unchanged when that would be the same path.
fn sibling_with_ext(path: &Path, ext: &str) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default();
    let mut dest = path.with_file_name(stem);
    dest.set_extension(ext);
    if dest == path {
        return dest;
    }
    if !dest.exists() {
        return dest;
    }
    // collision: stem_{n}.ext, counting up from 1
    let mut n = 1u32;
    loop {
        let name = format!("{}_{n}.{ext}", stem.to_string_lossy());
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Where a freshly produced derivative goes.
#[derive(Debug, Clone, Copy)]
enum Commit<'a> {
    /// Replace the original in place, deleting it. The directory pass's
    /// behaviour, unchanged.
    InPlace,
    /// Move the derivative to exactly this path and leave the original alone.
    ///
    /// The caller commits: it patches whatever points at the original, renames
    /// this file into its final name, and only then deletes the original,
    /// so the final name never exists before a conversation file points at it.
    To(&'a Path),
}

/// Move the produced file to where the commit mode says: over the original, or to a named destination.
fn commit_produced(commit: Commit<'_>, original: &Path, produced: &Path) -> Result<PathBuf> {
    match commit {
        Commit::InPlace => replace_original(original, produced),
        Commit::To(dest) => {
            if dest == original {
                // The whole point of Commit::To is that the final name never
                // exists until the caller has patched whatever points at the
                // original and renamed this derivative into place itself.
                // A destination equal to the original would
                // overwrite it here, before any of that has happened — for
                // example `derivative_name` returning the source's own name
                // (a same-format compress) joined onto the source's directory
                // without a caller-added suffix like `.in_progress`.
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
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            move_file(produced, dest)?;
            Ok(dest.to_path_buf())
        }
    }
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

/// Replace the original with the produced file, renaming to the new extension and removing the old file.
fn replace_original(original: &Path, produced: &Path) -> Result<PathBuf> {
    if produced == original {
        return Ok(original.to_path_buf());
    }
    let target_ext = produced
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    // `rename_into_place` syncs the produced file before the rename, so a
    // power loss cannot leave an empty file where the original was.
    let final_path = if original.extension().and_then(|e| e.to_str()) == Some(target_ext) {
        // The rename replaces the original in one step.
        file_io::rename_into_place(produced, original)?;
        original.to_path_buf()
    } else {
        let dest = sibling_with_ext(original, target_ext);
        if dest.exists() && dest != produced {
            let _ = fs::remove_file(&dest);
        }
        file_io::rename_into_place(produced, &dest)?;
        let _ = fs::remove_file(original);
        dest
    };
    Ok(final_path)
}

/// Convert an image to JPEG (compressing when asked), keeping the original when the result
/// is not smaller and `keep_smaller` is set.
fn convert_image(
    tools: &dyn MediaTools,
    path: &Path,
    compress: bool,
    keep_smaller: bool,
    commit: Commit<'_>,
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
        commit_produced(commit, path, &tmp).map(Some)
    })
}

/// Convert audio to MP3 (compressing when asked), keeping the original when the result
/// is not smaller and `keep_smaller` is set.
fn convert_audio(
    tools: &dyn MediaTools,
    path: &Path,
    compress: bool,
    keep_smaller: bool,
    commit: Commit<'_>,
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
        commit_produced(commit, path, &tmp).map(Some)
    })
}

/// Convert a video to MP4: remux when the codecs allow, else re-encode.
fn convert_video(
    tools: &dyn MediaTools,
    path: &Path,
    commit: Commit<'_>,
    stop: Option<&AtomicBool>,
) -> Result<PathBuf> {
    let tmp = temp_sibling(path, "mp4");

    with_temp_output(&tmp, || {
        // Prefer remux into mp4 when already a video file.
        if remux_mp4(tools, path, &tmp, stop).is_ok() {
            return commit_produced(commit, path, &tmp);
        }
        let _ = fs::remove_file(&tmp);

        // Light standardize encode (H.264, 30fps, no aggressive downscale).
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
        commit_produced(commit, path, &tmp)
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
    commit: Commit<'_>,
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
    try_remux_replace(tools, path, commit, stop)
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
    commit: Commit<'_>,
    stop: Option<&AtomicBool>,
    hevc_failure: &mut Option<String>,
) -> Result<Option<PathBuf>> {
    let meta = fs::metadata(path)?;
    if meta.len() < opts.min_size_bytes {
        // Still remux non-mp4 small files for container consistency.
        return keep_or_remux(tools, path, commit, stop);
    }

    // A probe that fails yields empty codec and zero dimensions, which never
    // count as efficient, so the file goes through compression like any other.
    let probe = tools.probe(path).unwrap_or_default();
    if opts.skip_efficient
        && is_efficient(&probe.codec, probe.width, probe.height, probe.bitrate, opts)
    {
        return keep_or_remux(tools, path, commit, stop);
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
        Ok(Some(commit_produced(commit, path, &tmp)?))
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
