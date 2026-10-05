//! Convert or compress a Staging Directory, patching the conversation files it wrote.
//!
//! This runs after the Staging Directory is complete and before anything is
//! uploaded, so the import can stop and ask between the two. It commits one
//! attachment at a time through a rename, which is what makes it resumable
//! with no progress record: a file under its final derivative name is fully
//! patched, and an original still on disk means work remains.
//!
//! ## Naming and resume
//!
//! The final name for a converted attachment is `{original_stem}-mv.{target_ext}`
//! — never the bare name [`media::derivative_name`] returns. A same-format
//! compress (`photo.jpg` → `photo.jpg`) or a video in either mode
//! (`clip.mp4` → `clip.mp4`) would otherwise produce a final name equal to
//! the source's own name, or equal to an already-committed derivative's own
//! name on a later resume — both break the "an original on disk means work
//! remains" invariant this pass depends on for resume. The `-mv` suffix
//! cannot collide with a staged original's own name: staged names come from
//! `attachment_dest_name` (`{utc-date}-{digest16}{ext}`), whose stem is
//! hex, and hex digits never include `m` or `v`.
//!
//! An attachment is pending when its recorded path exists on disk,
//! [`media::derivative_name`] returns `Some` for that file, and the file's
//! stem does not end in `-mv` (a `-mv` stem marks an already-committed
//! derivative, which must never re-enter the pending list — for a video, or
//! a compressed same-format file, `derivative_name` would otherwise call it
//! pending forever and re-degrade it on every resume).
//!
//! When the recorded path's stem ends in `-mv` but the file is *not* on
//! disk, the previous run crashed between patching the conversation file and
//! renaming the derivative into place. The original is still there under its
//! old name, so the pass heals: it strips the `-mv` suffix and looks under
//! `attachments/` for a file with that stem (any extension) that
//! `derivative_name` still wants, and re-transcodes it — the recorded
//! digest/size/path are stale regardless, so a heal re-patches
//! exactly like a fresh conversion. When no such file exists but another
//! attachment sharing the original dropped it for size, the note it left
//! (see Aliasing) makes the attachment record `too_large` with the size.
//! When neither exists, the attachment is unrecoverable and is marked
//! `file_missing`.
//!
//! ## Aliasing
//!
//! Staged names are content-addressed (`attachment_dest_name`), and both
//! writers are idempotent by name, so two attachments — in the same document
//! or in different ones — can legitimately record the identical `path` when
//! they carry the same bytes. Every patch here therefore applies to *every*
//! attachment recorded at a given path, not just the one that happened to be
//! scanned first, and a recorded path that has gone missing without a `-mv`
//! stem is checked against its would-be derivative before being written off:
//! another attachment (in this document or another) may already have
//! transcoded and deleted the shared original, in which case this one is
//! repointed at the existing derivative rather than failing or re-encoding.
//!
//! A shared original whose derivative comes out over the size limit is
//! deleted together with the derivative, so a later attachment
//! recording the same path finds neither. Before deleting, the pass writes a
//! note beside the original's name, `{original_name}.too_large`, holding the
//! derivative's size. A recorded path that is gone, has no committed
//! derivative, and has that note records `too_large` with the size, in this
//! run or a resumed one. Only a path with none of the three records
//! `file_missing`.
//!
//! ## Tool availability and failure
//!
//! ffmpeg/ffprobe are probed once, before any document is touched — parity
//! with `media::process_attachment_files`. A missing pair fails the whole
//! pass; it must never brand every attachment `convert_failed: ffmpeg not
//! found`. An attachment ffmpeg genuinely fails on, by contrast, is an
//! item-level issue: `missing_reason` gets `convert_failed: <detail>` and the
//! original — still on disk, untouched — keeps its `path` and
//! `digest_sha256` so a resume retries it rather than treating a transient
//! failure as permanent loss.
//!
//! ## Issues
//!
//! Every attachment the pass leaves without a converted file goes to the
//! issue sink as a `skip` row naming the conversation file and the
//! attachment: one it could not convert, one whose converted file is over
//! the size limit, and one an interrupted earlier pass lost. Each row is sent
//! before the conversation file is written, so a stop between the two leaves
//! the work pending and the resumed pass reports it again. A row's item is
//! the conversation file and the original's staged path, which no other
//! file in the conversation shares. A file an earlier pass could not convert
//! is settled again by a later pass, which first sends a [`RESOLVED`] row
//! for it: the earlier row no longer holds, whatever the later pass does.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use media::{CompressOptions, MediaMode, TranscodeOutcome};
use message_crate_core::{CancelFlag, IssueSink, RunIssue, check_cancel, emit_issue, mime_for_rel};
use message_ir::{ConversationDocument, IrAttachment};

use message_ir_format::read_conversation_jsonl;
use message_ir_format::write_conversation_jsonl_to;

/// Suffix on a derivative that is written but not yet committed.
///
/// Named so it survives the media crate's ffmpeg-scratch sweep, which matches
/// `.msgmedia.tmp.` — deleting this file would delete the resume signal.
const IN_PROGRESS_SUFFIX: &str = ".in_progress";

/// Suffix on the note left beside a dropped original's name: the original
/// converted over the size limit, and the note holds the derivative's size in
/// bytes as decimal text. See the module docs on aliasing.
const TOO_LARGE_SUFFIX: &str = ".too_large";

/// Suffix on a committed derivative's stem, marking it as already converted.
///
/// Staged attachment names come from `attachment_dest_name`
/// (`{utc-date}-{digest16}{ext}`), whose stem never ends in `-mv`: `m` and
/// `v` are not hex digits. That makes this suffix an unambiguous "already
/// done" marker no staged original can wear by coincidence.
///
/// `pub(crate)`: shared with `staging_summary`, which must judge a committed
/// derivative on its own size rather than forecast a transcode that
/// `pending_in` will never queue for it again.
pub(crate) const COMMITTED_SUFFIX: &str = "-mv";

/// What the media pass should do.
///
/// Staging records these in the Staging Directory
/// ([`write_media_settings`](crate::write_media_settings)), and the summary
/// and the pass read them back from there, so the whole Import Run works to
/// one set of values.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TranscodeOptions {
    /// Convert or Compress. Clone and Disabled make the pass a no-op.
    pub mode: MediaMode,
    /// Video targets, from the import form.
    pub compress: CompressOptions,
    /// A derivative larger than this is dropped rather than uploaded.
    pub asset_max_bytes: u64,
}

/// How far the pass has got.
#[derive(Debug, Clone, Copy)]
pub struct TranscodeProgress {
    /// Files finished, however they finished.
    pub done: usize,
    /// Files the pass found work for.
    pub total: usize,
}

/// What the pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscodeReport {
    /// Attachments replaced by a derivative.
    pub converted: usize,
    /// Attachments the media step left alone.
    pub skipped: usize,
    /// Attachments dropped because their derivative came out over the size
    /// limit, counted once per conversation that records the dropped file.
    pub too_large: usize,
    /// Attachments ffmpeg could not process.
    pub failed: usize,
    /// Attachments an interrupted prior run lost beyond recovery — neither
    /// the original nor a usable derivative survived. `missing_reason` is set
    /// to `file_missing`.
    pub missing: usize,
    /// Attachments repointed at an already-existing derivative without a
    /// transcode — the aliasing case: another attachment (this document or
    /// another) already converted and deleted the shared original.
    pub repointed: usize,
    /// Total bytes of the originals the pass replaced.
    pub bytes_before: u64,
    /// Total bytes of the derivatives it wrote.
    pub bytes_after: u64,
}

/// Convert or compress every original still staged under `staging_dir`.
///
/// Safe to call again after an interruption: it re-reads the directory and does
/// whatever is left.
///
/// # Errors
///
/// Returns an error when the pass is cancelled, ffmpeg/ffprobe are
/// unavailable, or the directory cannot be read, or a conversation file cannot
/// be parsed or written. A single attachment ffmpeg cannot process is an
/// item-level issue, counted in the report and sent to `issues` as it
/// happens, never an error.
pub fn transcode_staged(
    staging_dir: &Path,
    options: &TranscodeOptions,
    cancel: Option<&CancelFlag>,
    issues: Option<&IssueSink>,
    on_progress: &mut dyn FnMut(TranscodeProgress),
) -> Result<TranscodeReport> {
    if matches!(options.mode, MediaMode::Clone | MediaMode::Disabled) {
        return Ok(TranscodeReport::default());
    }
    // A cancel already requested short-circuits before we even ask whether
    // the tools are there.
    check_cancel(cancel)?;
    // Parity with `process_attachment_files`: fail the whole pass up front
    // when the tools are missing, rather than branding every attachment
    // `convert_failed: ffmpeg not found`.
    if !media::ffmpeg_available() {
        let probe = media::probe_ffmpeg_tools(None);
        anyhow::bail!(
            "ffmpeg/ffprobe are required to convert or compress attachments: {}",
            probe.error.unwrap_or_else(|| "not found".to_string())
        );
    }

    let files = conversation_files(staging_dir)?;
    // Counting up front costs a second parse of each conversation file and
    // buys an honest progress total, which is worth the re-read.
    let total = count_remaining(staging_dir, &files, options.mode)?;
    on_progress(TranscodeProgress { done: 0, total });

    let mut report = TranscodeReport::default();
    let mut done = 0usize;
    for jsonl in &files {
        check_cancel(cancel)?;
        let mut doc = read_conversation_jsonl(jsonl)?;
        let work = pending_in(staging_dir, &doc, options.mode)?;
        for item in work {
            check_cancel(cancel)?;
            // An earlier pass could not convert this file, and this pass
            // settles it now, however: the earlier row no longer holds. A
            // new failure sends a fresh row after this one.
            let recorded_rel = item.recorded_rel();
            if had_convert_failure(&doc, recorded_rel) {
                emit_issue(
                    issues,
                    media_issue(
                        jsonl,
                        &doc,
                        recorded_rel,
                        recorded_rel,
                        RESOLVED,
                        "is tried again",
                    ),
                );
            }
            match item {
                PendingWork::Transcode { recorded_rel, src } => {
                    apply_transcode(
                        staging_dir,
                        jsonl,
                        &mut doc,
                        &recorded_rel,
                        &src,
                        false,
                        options,
                        issues,
                        &mut report,
                    )?;
                }
                PendingWork::HealTranscode { recorded_rel, src } => {
                    apply_transcode(
                        staging_dir,
                        jsonl,
                        &mut doc,
                        &recorded_rel,
                        &src,
                        true,
                        options,
                        issues,
                        &mut report,
                    )?;
                }
                PendingWork::Repoint {
                    recorded_rel,
                    derivative,
                } => {
                    apply_repoint(jsonl, &mut doc, &recorded_rel, &derivative, &mut report)?;
                }
                PendingWork::DroppedTooLarge { recorded_rel, size } => {
                    apply_too_large(
                        jsonl,
                        &mut doc,
                        &recorded_rel,
                        &recorded_rel,
                        size,
                        issues,
                        &mut report,
                    )?;
                }
                PendingWork::Unrecoverable { recorded_rel } => {
                    apply_unrecoverable(jsonl, &mut doc, &recorded_rel, issues, &mut report)?;
                }
            }
            done += 1;
            on_progress(TranscodeProgress { done, total });
        }
    }
    Ok(report)
}

/// `*.jsonl` files directly under `staging_dir`, sorted, non-recursive — the
/// sink writes them flat (`FormatSink::open_prepared`).
///
/// `pub(crate)`: shared with `staging_summary`, which walks the same list.
pub(crate) fn conversation_files(staging_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(staging_dir)
        .with_context(|| format!("read {}", staging_dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    files.sort();
    Ok(files)
}

/// Sum of [`pending_in`] over every conversation file — the progress total.
fn count_remaining(staging_dir: &Path, files: &[PathBuf], mode: MediaMode) -> Result<usize> {
    let mut total = 0usize;
    for jsonl in files {
        let doc = read_conversation_jsonl(jsonl)?;
        total += pending_in(staging_dir, &doc, mode)?.len();
    }
    Ok(total)
}

impl PendingWork {
    /// The path the conversation file records for the attachment.
    fn recorded_rel(&self) -> &str {
        match self {
            Self::Transcode { recorded_rel, .. }
            | Self::HealTranscode { recorded_rel, .. }
            | Self::Repoint { recorded_rel, .. }
            | Self::DroppedTooLarge { recorded_rel, .. }
            | Self::Unrecoverable { recorded_rel } => recorded_rel,
        }
    }
}

/// One deduplicated unit of work found in a document.
///
/// A recorded path is a literal string: every attachment carrying the same
/// one (in the same document) names the same physical file and therefore
/// shares the same fate, so [`pending_in`] queues each recorded path once.
enum PendingWork {
    /// Transcode `src` — untouched, still at its recorded name — and commit.
    Transcode { recorded_rel: String, src: PathBuf },
    /// The recorded name is a crash-heal `-mv` name with nothing on disk;
    /// `src` is the recovered original.
    HealTranscode { recorded_rel: String, src: PathBuf },
    /// The recorded path is gone, but its would-be derivative already
    /// exists — the aliasing case, or the crash window right after the
    /// shared original's delete. Repoint, no transcode needed.
    Repoint {
        recorded_rel: String,
        derivative: PathBuf,
    },
    /// The recorded path is gone because an earlier attachment sharing it
    /// converted over the size limit; `size` is that derivative's size, read
    /// from the note the drop left.
    DroppedTooLarge { recorded_rel: String, size: u64 },
    /// Nothing recoverable survived.
    Unrecoverable { recorded_rel: String },
}

/// Attachments in `doc` still needing the media step, deduplicated by
/// recorded path.
///
/// See the module docs for the pending rule, the crash-heal rule, and the
/// aliasing repoint rule.
fn pending_in(
    staging_dir: &Path,
    doc: &ConversationDocument,
    mode: MediaMode,
) -> Result<Vec<PendingWork>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for msg in &doc.messages {
        for att in &msg.attachments {
            let Some(rel) = att.path.as_deref() else {
                continue;
            };
            if !seen.insert(rel.to_string()) {
                // Already classified via an earlier attachment recorded at
                // the same path in this document.
                continue;
            }
            let abs = message_ir::safe_attachment_path(staging_dir, rel)?;
            let stem = abs.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let committed = stem.ends_with(COMMITTED_SUFFIX);

            if abs.is_file() {
                // A committed derivative (or anything the media step does
                // not touch) is not work; only a fresh, still-original file
                // is.
                if !committed && media::derivative_name(&abs, mode).is_some() {
                    out.push(PendingWork::Transcode {
                        recorded_rel: rel.to_string(),
                        src: abs,
                    });
                }
                continue;
            }

            if committed {
                // The recorded path is a committed derivative's name, but
                // nothing is there: the previous run crashed between
                // patching the conversation file and renaming the
                // derivative into place.
                // The original may instead have been dropped for size by
                // another attachment sharing it, which leaves its note. The
                // pass reaches that state only when a conversation sorted
                // before this one failed to convert the original in the
                // interrupted run, and its retry on resume came out over the
                // limit where this attachment's conversion had not (a
                // derivative whose size varies right at the limit).
                let orig_stem = &stem[..stem.len() - COMMITTED_SUFFIX.len()];
                if let Some(found) = find_recoverable_original(staging_dir, orig_stem, mode)? {
                    out.push(PendingWork::HealTranscode {
                        recorded_rel: rel.to_string(),
                        src: found,
                    });
                } else if let Some(size) = find_too_large_note(staging_dir, &abs, mode)? {
                    out.push(PendingWork::DroppedTooLarge {
                        recorded_rel: rel.to_string(),
                        size,
                    });
                } else {
                    out.push(PendingWork::Unrecoverable {
                        recorded_rel: rel.to_string(),
                    });
                }
                continue;
            }

            // Not committed, not on disk: maybe another attachment sharing
            // the same bytes (this document or another) already converted
            // it and deleted the shared original — or the shared original
            // was dropped for good (too_large deletes both the derivative
            // and the original, and leaves a note saying so).
            // The recorded file has no
            // bytes to measure, so the candidate name is derived stat-free:
            // the size floors exist to skip a small *live* file, and are
            // meaningless against a file that is not there. When
            // `derivative_name` returns `None`, the mode has no media step
            // for this kind of file at all — its absence has nothing to do
            // with the media pass, and it is left alone.
            if let Some(name) = final_derivative_name_for_missing(&abs, mode) {
                let derivative = staging_dir.join("attachments").join(&name);
                if derivative.is_file() {
                    out.push(PendingWork::Repoint {
                        recorded_rel: rel.to_string(),
                        derivative,
                    });
                } else if let Some(size) = read_too_large_note(&abs) {
                    out.push(PendingWork::DroppedTooLarge {
                        recorded_rel: rel.to_string(),
                        size,
                    });
                } else {
                    // No committed derivative and no too-large note: nothing
                    // recoverable survived, so the attachment is settled
                    // rather than left dangling with no `missing_reason`.
                    out.push(PendingWork::Unrecoverable {
                        recorded_rel: rel.to_string(),
                    });
                }
            }
        }
    }
    Ok(out)
}

/// Find a file under `staging_dir/attachments` whose stem is `orig_stem` and
/// that the media step would still touch — the crash-heal search.
fn find_recoverable_original(
    staging_dir: &Path,
    orig_stem: &str,
    mode: MediaMode,
) -> Result<Option<PathBuf>> {
    find_in_attachments(staging_dir, |path| {
        (path.is_file()
            && path.file_stem().and_then(|s| s.to_str()) == Some(orig_stem)
            && media::derivative_name(path, mode).is_some())
        .then(|| path.to_path_buf())
    })
}

/// The derivative size recorded in the too-large note of the original that
/// `committed` — a missing `-mv` name — was converted from, or `None` when
/// `attachments/` holds no such note.
///
/// The note is named `{original_name}.too_large`, and the original's
/// extension is not in the `-mv` name, so the search reads `attachments/`
/// for a note whose original would get exactly `committed`'s name.
fn find_too_large_note(
    staging_dir: &Path,
    committed: &Path,
    mode: MediaMode,
) -> Result<Option<u64>> {
    let Some(committed_name) = committed.file_name().and_then(|n| n.to_str()) else {
        return Ok(None);
    };
    find_in_attachments(staging_dir, |path| {
        let original_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(TOO_LARGE_SUFFIX))?;
        let original = path.with_file_name(original_name);
        if final_derivative_name_for_missing(&original, mode).as_deref() != Some(committed_name) {
            return None;
        }
        read_too_large_note(&original)
    })
}

/// The first `Some` that `found` returns for an entry of
/// `staging_dir/attachments`, or `None` when no entry gives one or the directory
/// does not exist.
fn find_in_attachments<T>(
    staging_dir: &Path,
    mut found: impl FnMut(&Path) -> Option<T>,
) -> Result<Option<T>> {
    let attachments_dir = staging_dir.join("attachments");
    if !attachments_dir.is_dir() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(&attachments_dir)
        .with_context(|| format!("read {}", attachments_dir.display()))?
    {
        let path = entry
            .with_context(|| format!("read entry in {}", attachments_dir.display()))?
            .path();
        if let Some(value) = found(&path) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// The name a committed derivative of `src` gets: `{original_stem}-mv.{target_ext}`.
///
/// Never the bare name [`media::derivative_name`] returns — see the module
/// docs for why a same-format compress or a video in either mode makes that
/// name collide with the source, or with an already-committed derivative on
/// a later resume.
fn final_derivative_name(src: &Path, mode: MediaMode) -> Option<String> {
    committed_name_from(src, media::derivative_name(src, mode))
}

/// Same as [`final_derivative_name`], but for a recorded path already known
/// to be missing from disk: the forecast is derived stat-free via
/// [`media::derivative_name_for_missing`], since there is no live file left
/// to check the compress-mode size floors against.
fn final_derivative_name_for_missing(src: &Path, mode: MediaMode) -> Option<String> {
    committed_name_from(src, media::derivative_name_for_missing(src, mode))
}

/// Turn a bare `derivative_name`-shaped `forecast` (`"{stem}.{ext}"`) into the
/// committed `-mv` name, keyed off `src`'s own stem rather than the
/// forecast's — see the module docs on why the committed name must never
/// equal the bare forecast.
fn committed_name_from(src: &Path, forecast: Option<String>) -> Option<String> {
    let forecast = forecast?;
    let target_ext = Path::new(&forecast)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let orig_stem = src.file_stem().and_then(|s| s.to_str())?;
    Some(format!("{orig_stem}{COMMITTED_SUFFIX}.{target_ext}"))
}

/// Where the note for a dropped `original` goes: `{original_name}.too_large`
/// beside it.
fn too_large_note(original: &Path) -> PathBuf {
    let mut name = original.file_name().unwrap_or_default().to_os_string();
    name.push(TOO_LARGE_SUFFIX);
    original.with_file_name(name)
}

/// The derivative size the note beside `original` records, or `None` when
/// there is no note or it does not hold a number.
fn read_too_large_note(original: &Path) -> Option<u64> {
    std::fs::read_to_string(too_large_note(original))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// `attachments/{file name of path}` — the doc-relative form every patch
/// records.
fn attachment_rel(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| format!("{} has no valid UTF-8 file name", path.display()))?;
    Ok(format!("attachments/{name}"))
}

/// Apply `patch` to every attachment across `doc` recorded at `recorded_rel`.
///
/// Content-addressed staging means more than one attachment — in the same
/// document — can legitimately share a path; every one of them must move
/// together; see the module docs.
fn patch_all_matching(
    doc: &mut ConversationDocument,
    recorded_rel: &str,
    patch: impl Fn(&mut IrAttachment),
) {
    for msg in &mut doc.messages {
        for att in &mut msg.attachments {
            if att.path.as_deref() == Some(recorded_rel) {
                patch(att);
            }
        }
    }
}

/// What a repoint writes into every matching attachment: the target file's
/// doc-relative path, digest, size, and MIME type, read fresh off disk.
struct DiskAttachmentFields {
    rel: String,
    digest: String,
    size: u64,
    mime: Option<String>,
}

/// Compute the fields a repoint writes, from `src` on disk.
///
/// Used by [`apply_repoint`] (aiming at an existing derivative) and by both
/// places a heal has to fall back to the recovered original rather than a
/// derivative: the media step declining the file (`Skipped`), and ffmpeg
/// failing on it (`Err`). In the heal cases `recorded_rel` — the phantom
/// `-mv` name a crashed prior run already wrote into the document — must not
/// be left standing, since nothing will ever exist under it.
fn disk_attachment_fields(src: &Path) -> Result<DiskAttachmentFields> {
    let rel = attachment_rel(src)?;
    let digest = media::file_sha256(src)?;
    let size = std::fs::metadata(src)
        .with_context(|| format!("stat {}", src.display()))?
        .len();
    let mime = mime_for_rel(&rel);
    Ok(DiskAttachmentFields {
        rel,
        digest,
        size,
        mime,
    })
}

/// Transcode `src` and commit it, in this order: derivative
/// written, conversation file patched, derivative renamed into its final
/// name, original deleted. Reversing any pair leaves the directory lying about
/// itself.
///
/// `is_heal` marks a crash-heal recovery: `recorded_rel` currently points at
/// a `-mv` name nothing produced yet, rather than at `src` itself. That
/// distinction only matters when the media step declines the file (see the
/// `Skipped` arm) — everywhere else a heal behaves exactly like a fresh
/// transcode.
#[allow(clippy::too_many_arguments)]
fn apply_transcode(
    staging_dir: &Path,
    jsonl: &Path,
    doc: &mut ConversationDocument,
    recorded_rel: &str,
    src: &Path,
    is_heal: bool,
    options: &TranscodeOptions,
    issues: Option<&IssueSink>,
    report: &mut TranscodeReport,
) -> Result<()> {
    let Some(name) = final_derivative_name(src, options.mode) else {
        report.skipped += 1;
        return Ok(());
    };
    let attachments_dir = staging_dir.join("attachments");
    let final_path = attachments_dir.join(&name);
    let marker = attachments_dir.join(format!("{name}{IN_PROGRESS_SUFFIX}"));
    let original_len = std::fs::metadata(src).map_or(0, |m| m.len());
    // A row names the original's staged path: a heal's `recorded_rel` is a
    // name nothing will ever have, and a failure records the original's.
    let item_rel = if is_heal {
        attachment_rel(src)?
    } else {
        recorded_rel.to_string()
    };

    match media::transcode_file(src, &marker, options.mode, &options.compress) {
        Err(err) => {
            emit_issue(
                issues,
                media_issue(
                    jsonl,
                    doc,
                    recorded_rel,
                    &item_rel,
                    SKIP,
                    &format!("could not be converted, so the original file is kept: {err:#}"),
                ),
            );
            let reason = format!("convert_failed: {err}");
            if is_heal {
                // `recorded_rel` is the phantom `-mv` name a crashed prior
                // run already wrote into the document; nothing will ever
                // exist under it. Keeping it here — the way a non-heal
                // failure keeps its path — would strand the document
                // pointing at a name that can never exist. Repoint at the
                // recovered original first, exactly like the `Skipped` heal
                // arm below, then record the failure on it.
                let r = disk_attachment_fields(src)?;
                patch_all_matching(doc, recorded_rel, |att| {
                    att.path = Some(r.rel.clone());
                    att.digest_sha256 = Some(r.digest.clone());
                    att.size_bytes = Some(r.size);
                    att.mime_type.clone_from(&r.mime);
                    att.missing_reason = Some(reason.clone());
                });
            } else {
                // The original is untouched on disk. Clearing `path` here
                // would sever the only reference to bytes that still exist,
                // turning a transient failure into permanent loss, and
                // `pending_in` (which skips `path == None`) would never
                // retry it — so only the reason changes; everything else
                // (path, digest, size, mime) stays exactly as recorded.
                patch_all_matching(doc, recorded_rel, |att| {
                    att.missing_reason = Some(reason.clone());
                });
            }
            write_conversation_jsonl_to(jsonl, doc)?;
            report.failed += 1;
            Ok(())
        }
        Ok(TranscodeOutcome::Skipped) => {
            if is_heal {
                // The doc currently points at a phantom `-mv` name that
                // nothing will ever produce — the media step declined this
                // file. Repoint it back at the recovered original so a
                // resume does not chase a name that can never exist.
                let r = disk_attachment_fields(src)?;
                patch_all_matching(doc, recorded_rel, |att| {
                    att.path = Some(r.rel.clone());
                    att.digest_sha256 = Some(r.digest.clone());
                    att.size_bytes = Some(r.size);
                    att.mime_type.clone_from(&r.mime);
                    att.missing_reason = None;
                });
                write_conversation_jsonl_to(jsonl, doc)?;
            }
            report.skipped += 1;
            Ok(())
        }
        Ok(TranscodeOutcome::Produced) => {
            let produced_len = std::fs::metadata(&marker)
                .with_context(|| format!("stat {}", marker.display()))?
                .len();
            if produced_len > options.asset_max_bytes {
                // Skipped, not reverted: falling back to the original would
                // store the format the person asked to be rid of. Both the derivative and
                // the original go, so nothing survives to point at. The note
                // is written first, while the original is still on disk: a
                // stop before the delete leaves the original, and the next
                // run converts it again and rewrites the note.
                let note = too_large_note(src);
                std::fs::write(&note, produced_len.to_string())
                    .with_context(|| format!("write {}", note.display()))?;
                apply_too_large(
                    jsonl,
                    doc,
                    recorded_rel,
                    &item_rel,
                    produced_len,
                    issues,
                    report,
                )?;
                let _ = std::fs::remove_file(&marker);
                let _ = std::fs::remove_file(src);
                return Ok(());
            }
            // Read the file on disk. A replayed digest can be
            // stale, and the server dedupes assets by sha256.
            let digest = media::file_sha256(&marker)?;
            let rel = attachment_rel(&final_path)?;
            let mime = mime_for_rel(&rel);
            patch_all_matching(doc, recorded_rel, |att| {
                att.path = Some(rel.clone());
                att.digest_sha256 = Some(digest.clone());
                att.size_bytes = Some(produced_len);
                att.mime_type.clone_from(&mime);
                att.missing_reason = None;
            });
            write_conversation_jsonl_to(jsonl, doc)?;
            // The `-mv` suffix in `final_derivative_name` makes this
            // structurally impossible, but a future naming change must fail
            // loudly here rather than silently destroy the original before
            // it is committed. `fs::rename` itself replaces an existing
            // destination atomically, so there is nothing to remove first.
            assert_ne!(
                final_path.as_path(),
                src,
                "final derivative name collided with the source"
            );
            // Synced before the original goes, so a power loss cannot
            // leave an empty derivative and no original.
            message_ir::rename_into_place(&marker, &final_path)
                .with_context(|| format!("commit {}", final_path.display()))?;
            let _ = std::fs::remove_file(src);
            // A note from an earlier run under a lower limit no longer
            // describes this file: the committed derivative replaces it.
            let _ = std::fs::remove_file(too_large_note(src));
            report.converted += 1;
            report.bytes_before += original_len;
            report.bytes_after += produced_len;
            Ok(())
        }
    }
}

/// The `kind` of a row for an attachment the pass left without a converted
/// file.
pub const SKIP: &str = "skip";

/// The `kind` of a row that says an earlier row about the same item no
/// longer holds. The window drops the earlier row and keeps no row for it.
pub const RESOLVED: &str = "resolved";

/// The attachments in `doc` recorded at `recorded_rel`.
fn recorded_at<'a>(
    doc: &'a ConversationDocument,
    recorded_rel: &'a str,
) -> impl Iterator<Item = &'a IrAttachment> + 'a {
    doc.messages
        .iter()
        .flat_map(|msg| &msg.attachments)
        .filter(move |att| att.path.as_deref() == Some(recorded_rel))
}

/// True when an earlier pass recorded that it could not convert the file at
/// `recorded_rel`.
fn had_convert_failure(doc: &ConversationDocument, recorded_rel: &str) -> bool {
    recorded_at(doc, recorded_rel).any(|att| {
        att.missing_reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("convert_failed"))
    })
}

/// A Media row about the attachment at `recorded_rel`. Its `item` is the
/// conversation file and the original's staged path `item_rel`, which no
/// other file in the conversation shares, and which a row the pass reports
/// again on a resume has too. Its `reason` opens with the attachment's
/// original name, when it has one, and goes on with `what`.
fn media_issue(
    jsonl: &Path,
    doc: &ConversationDocument,
    recorded_rel: &str,
    item_rel: &str,
    kind: &str,
    what: &str,
) -> RunIssue {
    let conversation = jsonl.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let name = recorded_at(doc, recorded_rel)
        .find_map(|att| att.original_name.as_deref().and_then(message_ir::trimmed))
        .unwrap_or("The attachment");
    RunIssue {
        kind: kind.into(),
        step: "media".into(),
        item: format!("{conversation}:{item_rel}"),
        reason: format!("{name} {what}"),
    }
}

/// The row for an attachment the pass left out because its converted file is
/// `size` bytes, over the server's attachment size limit.
fn too_large_issue(
    jsonl: &Path,
    doc: &ConversationDocument,
    recorded_rel: &str,
    item_rel: &str,
    size: u64,
) -> RunIssue {
    media_issue(
        jsonl,
        doc,
        recorded_rel,
        item_rel,
        SKIP,
        &format!(
            "is {size} bytes ({} MiB) after conversion, over the attachment size limit, so it \
             was left out",
            size / message_ir::MIB
        ),
    )
}

/// Repoint every attachment recorded at `recorded_rel` to `derivative`,
/// which already exists — no transcode needed. The aliasing case: another
/// attachment already converted and deleted the shared original.
fn apply_repoint(
    jsonl: &Path,
    doc: &mut ConversationDocument,
    recorded_rel: &str,
    derivative: &Path,
    report: &mut TranscodeReport,
) -> Result<()> {
    let r = disk_attachment_fields(derivative)?;
    patch_all_matching(doc, recorded_rel, |att| {
        att.path = Some(r.rel.clone());
        att.digest_sha256 = Some(r.digest.clone());
        att.size_bytes = Some(r.size);
        att.mime_type.clone_from(&r.mime);
        att.missing_reason = None;
    });
    write_conversation_jsonl_to(jsonl, doc)?;
    report.repointed += 1;
    Ok(())
}

/// Mark every attachment recorded at `recorded_rel` `too_large` with the
/// derivative size `size`: an earlier attachment sharing the path converted
/// over the limit and the pass dropped the shared original.
fn apply_too_large(
    jsonl: &Path,
    doc: &mut ConversationDocument,
    recorded_rel: &str,
    item_rel: &str,
    size: u64,
    issues: Option<&IssueSink>,
    report: &mut TranscodeReport,
) -> Result<()> {
    emit_issue(
        issues,
        too_large_issue(jsonl, doc, recorded_rel, item_rel, size),
    );
    patch_all_matching(doc, recorded_rel, |att| {
        att.path = None;
        att.digest_sha256 = None;
        att.missing_reason = Some("too_large".to_string());
        att.size_bytes = Some(size);
    });
    write_conversation_jsonl_to(jsonl, doc)?;
    report.too_large += 1;
    Ok(())
}

/// Mark every attachment recorded at `recorded_rel` `file_missing`: nothing
/// survived an interrupted prior run's crash window closely enough to
/// recover.
fn apply_unrecoverable(
    jsonl: &Path,
    doc: &mut ConversationDocument,
    recorded_rel: &str,
    issues: Option<&IssueSink>,
    report: &mut TranscodeReport,
) -> Result<()> {
    emit_issue(
        issues,
        media_issue(
            jsonl,
            doc,
            recorded_rel,
            recorded_rel,
            SKIP,
            "was lost when an earlier Media Stage stopped partway, so it was left out",
        ),
    );
    patch_all_matching(doc, recorded_rel, |att| {
        att.path = None;
        att.digest_sha256 = None;
        att.missing_reason = Some("file_missing".to_string());
        att.size_bytes = None;
    });
    write_conversation_jsonl_to(jsonl, doc)?;
    report.missing += 1;
    Ok(())
}

#[cfg(test)]
mod tests;
