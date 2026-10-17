use super::*;
use crate::write_queue::conversation_file;
use media::testutil::PNG_1X1_RGB;
use std::sync::atomic::Ordering;

/// A run directory holding one conversation and one attachment.
///
/// Writes `attachments/<name>` with `bytes`, and one `.jsonl` whose
/// single message has non-empty text and one attachment pointing at
/// `attachments/<name>`. Built with `ir::testutil::sample_document`
/// and written with `write_conversation_jsonl_to`, so the fixture and the
/// code under test agree on the on-disk shape.
///
/// Returns (run directory, conversation file path, attachment path).
fn staged_one(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let attachments_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments_dir).unwrap();
    let rel = format!("attachments/{name}");
    let original = dir.path().join(&rel);
    std::fs::write(&original, bytes).unwrap();

    let mut document = ir::testutil::sample_document("hello from the fixture");
    document.messages[0].attachments = vec![Attachment {
        path: Some(rel),
        original_name: Some(name.to_string()),
        mime_type: None,
        size_bytes: Some(bytes.len() as u64),
        ..ir::testutil::sample_attachment()
    }];
    document.finalize_stats();

    let jsonl = conversation_file(dir.path(), &document);
    write_conversation_jsonl_to(&jsonl, &document).unwrap();
    (dir, jsonl, original)
}

/// An issue sink, and the rows it receives in the order the Media Stage sent them.
fn collecting_sink() -> (IssueSink, std::sync::Arc<std::sync::Mutex<Vec<RunIssue>>>) {
    let issues = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink_issues = std::sync::Arc::clone(&issues);
    let sink = IssueSink::new(move |issue| sink_issues.lock().unwrap().push(issue));
    (sink, issues)
}

fn options(mode: MediaMode, limit: u64) -> TranscodeOptions {
    TranscodeOptions {
        mode,
        compress: CompressOptions::default(),
        asset_max_bytes: limit,
    }
}

#[test]
fn a_converted_attachment_is_patched_before_its_final_name_exists() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, original) = staged_one("photo.png", PNG_1X1_RGB);
    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let attachment = &document.messages[0].attachments[0];
    assert_eq!(attachment.path.as_deref(), Some("attachments/photo-mv.jpg"));
    assert!(
        !original.exists(),
        "original deleted after the patch committed"
    );
    assert!(dir.path().join("attachments/photo-mv.jpg").exists());
    assert!(
        std::fs::read_dir(dir.path().join("attachments"))
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".in_progress")),
        "no marker survives a completed file"
    );
}

#[test]
fn the_digest_and_size_are_recomputed_from_the_derivative() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // ffmpeg output is not byte-identical across runs, so a
    // replayed digest would be a silent corruption — the server dedupes
    // assets by sha256.
    let (dir, jsonl, _) = staged_one("photo.png", PNG_1X1_RGB);
    transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    let document = read_conversation_jsonl(&jsonl).unwrap();
    let attachment = &document.messages[0].attachments[0];
    let derivative = dir.path().join("attachments/photo-mv.jpg");
    let on_disk = std::fs::read(&derivative).unwrap();
    assert_eq!(
        attachment.digest_sha256.as_deref(),
        Some(file_io::sha256_hex(&on_disk).as_str())
    );
    assert_eq!(attachment.size_bytes, Some(on_disk.len() as u64));
    assert_eq!(attachment.mime_type.as_deref(), Some("image/jpeg"));
}

#[test]
fn an_interrupted_file_is_re_transcoded_not_adopted() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // Nothing distinguishes a complete .in_progress from a
    // truncated one without hashing it, so the marker's bytes are never used.
    let (dir, jsonl, _) = staged_one("photo.png", PNG_1X1_RGB);
    let marker = dir.path().join("attachments/photo-mv.jpg.in_progress");
    std::fs::write(&marker, b"truncated garbage from a killed run").unwrap();

    transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    let derivative = dir.path().join("attachments/photo-mv.jpg");
    assert_ne!(
        std::fs::read(&derivative).unwrap(),
        b"truncated garbage from a killed run".to_vec(),
        "the marker's bytes must never be adopted"
    );
    let document = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        document.messages[0].attachments[0].path.as_deref(),
        Some("attachments/photo-mv.jpg")
    );
}

#[test]
fn an_already_converted_attachment_is_left_alone_on_a_second_run() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, _) = staged_one("photo.png", PNG_1X1_RGB);
    transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();
    let after_first = std::fs::read(dir.path().join("attachments/photo-mv.jpg")).unwrap();

    let second = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(second.converted, 0, "resume must not redo finished work");
    assert_eq!(
        std::fs::read(dir.path().join("attachments/photo-mv.jpg")).unwrap(),
        after_first
    );
    let document = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        document.messages[0].attachments[0].path.as_deref(),
        Some("attachments/photo-mv.jpg")
    );
}

#[test]
fn a_derivative_over_the_limit_becomes_too_large_and_keeps_the_message() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // Skipped, not reverted. Falling back to the original would store the
    // format the person asked to be rid of.
    let (dir, jsonl, original) = staged_one("photo.png", PNG_1X1_RGB);
    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, 1),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.too_large, 1);
    assert_eq!(report.converted, 0);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let message = &document.messages[0];
    assert!(!message.text.is_empty(), "the message keeps its text");
    let attachment = &message.attachments[0];
    assert_eq!(attachment.missing_reason.as_deref(), Some("too_large"));
    assert_eq!(attachment.path, None, "nothing to upload");
    assert!(!original.exists(), "the original is not kept as a fallback");
    assert!(!dir.path().join("attachments/photo-mv.jpg").exists());
}

#[test]
fn a_conversion_failure_becomes_a_per_item_reason_carrying_the_detail() {
    // Needs ffmpeg present and failing on this specific input: after the
    // ffmpeg preflight check, an *absent* ffmpeg now fails the whole
    // Media Stage (see without_ffmpeg_the_whole_media_stage_fails_and_touches_nothing)
    // rather than reaching this per-item path.
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, original) = staged_one("broken.png", b"not a png at all");
    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    );

    // ffmpeg failing on one file is an issue, never a failed Media Stage.
    let report = report.unwrap();
    assert_eq!(report.failed, 1);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let reason = document.messages[0].attachments[0]
        .missing_reason
        .clone()
        .unwrap();
    assert!(
        reason.starts_with("convert_failed: "),
        "reason must stay inside the closed set: {reason}"
    );
    assert!(
        reason.len() > "convert_failed: ".len(),
        "the detail must survive"
    );
    assert!(
        original.exists(),
        "a file that failed to convert is still there"
    );
}

/// A file ffmpeg cannot convert is a `skip` Import Error naming the
/// conversation file and the attachment, sent while the Media Stage runs (#1639).
/// Before, the Media Stage only counted it.
#[test]
fn a_file_the_media_stage_cannot_convert_is_sent_as_a_skip_import_error() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, _original) = staged_one("broken.png", b"not a png at all");
    let (sink, issues) = collecting_sink();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &sink,
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    let issues = issues.lock().unwrap();
    assert_eq!(issues.len(), 1, "{issues:?}");
    let conversation = jsonl.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        (issues[0].kind, issues[0].step, issues[0].item.clone()),
        (
            RunIssueKind::Skip,
            RunIssueStep::Media,
            format!("{conversation}:attachments/broken.png")
        )
    );
    assert!(
        issues[0]
            .reason
            .starts_with("broken.png could not be converted, so the original file is kept: "),
        "{}",
        issues[0].reason
    );
    assert!(
        issues[0].reason.len()
            > "broken.png could not be converted, so the original file is kept: ".len(),
        "the reason carries ffmpeg's detail: {}",
        issues[0].reason
    );
}

/// An attachment whose converted file is over the limit is left out, and
/// the Media Stage says so as a `skip` row. The Upload sends no row for it, since
/// the conversation file records why it has no file.
#[test]
fn a_file_left_out_as_too_large_is_sent_as_a_skip_import_error() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, _original) = staged_one("photo.png", PNG_1X1_RGB);
    let (sink, issues) = collecting_sink();

    transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, 1),
        &tools,
        &CancelFlag::default(),
        &sink,
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    let issues = issues.lock().unwrap();
    let conversation = jsonl.file_name().unwrap().to_str().unwrap();
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(
        (issues[0].kind, issues[0].item.clone()),
        (
            RunIssueKind::Skip,
            format!("{conversation}:attachments/photo.png")
        )
    );
    assert!(
        issues[0]
            .reason
            .ends_with("over the attachment size limit, so it was left out"),
        "{}",
        issues[0].reason
    );
}

/// A resumed Media Stage tries a file again that an earlier attempt could not
/// convert. It says first that the earlier row no longer holds, then
/// reports the new outcome, so a file converted on the second try keeps no
/// row.
#[test]
fn a_file_tried_again_resolves_its_earlier_row_first() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, _original) = staged_one("broken.png", b"not a png at all");
    let opts = options(MediaMode::Convert, u64::MAX);
    transcode_staged(
        dir.path(),
        &opts,
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();
    let (sink, issues) = collecting_sink();

    transcode_staged(
        dir.path(),
        &opts,
        &tools,
        &CancelFlag::default(),
        &sink,
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    let conversation = jsonl.file_name().unwrap().to_str().unwrap();
    let rows: Vec<(RunIssueKind, String)> = issues
        .lock()
        .unwrap()
        .iter()
        .map(|issue| (issue.kind, issue.item.clone()))
        .collect();
    let item = format!("{conversation}:attachments/broken.png");
    assert_eq!(
        rows,
        [
            (RunIssueKind::Resolved, item.clone()),
            (RunIssueKind::Skip, item)
        ]
    );
}

/// An attachment an earlier attempt could not convert can be settled without a
/// conversion of its own: here another conversation sharing the file
/// converted it since, so this one is repointed. Its earlier row is
/// resolved all the same.
#[test]
fn a_failed_file_settled_by_a_repoint_resolves_its_earlier_row() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, _jsonl, _original) = staged_one("photo.png", PNG_1X1_RGB);
    let jsonl_b = second_document_sharing(
        dir.path(),
        "attachments/photo.png",
        PNG_1X1_RGB.len() as u64,
    );
    let mut document_b = read_conversation_jsonl(&jsonl_b).unwrap();
    document_b.messages[0].attachments[0].missing_reason = Some("convert_failed: earlier".into());
    write_conversation_jsonl_to(&jsonl_b, &document_b).unwrap();
    let opts = options(MediaMode::Convert, u64::MAX);
    let (sink, issues) = collecting_sink();

    let report = transcode_staged(
        dir.path(),
        &opts,
        &tools,
        &CancelFlag::default(),
        &sink,
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted + report.repointed, 2, "{report:?}");
    let conversation_b = jsonl_b.file_name().unwrap().to_str().unwrap();
    let rows: Vec<(RunIssueKind, String)> = issues
        .lock()
        .unwrap()
        .iter()
        .map(|issue| (issue.kind, issue.item.clone()))
        .collect();
    assert_eq!(
        rows,
        [(
            RunIssueKind::Resolved,
            format!("{conversation_b}:attachments/photo.png")
        )]
    );
}

#[test]
fn a_convert_failed_attachment_keeps_its_path_and_is_retried_on_resume() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // The original is still on disk after a transient ffmpeg failure;
    // clearing `path` would sever the only reference to bytes that still
    // exist and stop `pending_in` from ever retrying it.
    let (dir, jsonl, original) = staged_one("broken.png", b"not a png at all");
    let first = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(first.failed, 1);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let attachment = &document.messages[0].attachments[0];
    assert_eq!(
        attachment.path.as_deref(),
        Some("attachments/broken.png"),
        "the path survives a transient failure"
    );
    assert!(original.exists());

    // Resume: pending_in must still see this as work, because the path
    // exists, derivative_name says Some, and the stem carries no -mv.
    let second = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        second.failed, 1,
        "a resume must retry a convert_failed file, not skip it"
    );
}

#[test]
fn canceling_stops_the_media_stage_without_corrupting_the_directory() {
    let tools = media::testutil::FakeTools::new();
    let (dir, jsonl, _) = staged_one("photo.png", PNG_1X1_RGB);
    let cancel = CancelFlag::default();
    cancel.store(true, Ordering::Relaxed);

    let err = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &cancel,
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    );

    let err = err.expect_err("a cancel requested before the call must surface as Err");
    assert_eq!(
        err.to_string(),
        "canceled",
        "the same word every other canceled stage returns; the import screen matches it"
    );
    let document = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        document.messages[0].attachments[0].path.as_deref(),
        Some("attachments/photo.png"),
        "an untouched attachment still points at its original"
    );
}

#[test]
fn progress_counts_the_work_it_actually_has() {
    // Convert mode still probes for ffmpeg up front even though a PDF
    // alone needs no transcode, so this needs the tools present to reach
    // that far.
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, _, _) = staged_one("notes.pdf", b"%PDF-1.4");
    let mut seen = Vec::new();
    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |p| seen.push((p.done, p.total)),
    )
    .unwrap();
    // A file the Media Stage does not handle is not work.
    assert_eq!(report.converted, 0);
    assert!(seen.iter().all(|(_, total)| *total == 0));
}

#[test]
fn a_crash_between_the_patch_and_the_rename_heals_by_re_transcoding_the_original() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // Hand-simulate the crash window between the conversation file being
    // patched and written and the marker being renamed into its final
    // name: the document already points at the -mv
    // name, a marker sits under .in_progress, and the original is still
    // on disk under its old name because the delete never ran.
    let (dir, jsonl, original) = staged_one("photo.png", PNG_1X1_RGB);
    let mut document = read_conversation_jsonl(&jsonl).unwrap();
    {
        let attachment = &mut document.messages[0].attachments[0];
        attachment.path = Some("attachments/photo-mv.jpg".into());
        attachment.digest_sha256 = Some("deadbeef".repeat(8));
        attachment.size_bytes = Some(1234);
        attachment.mime_type = Some("image/jpeg".into());
    }
    write_conversation_jsonl_to(&jsonl, &document).unwrap();
    let marker = dir.path().join("attachments/photo-mv.jpg.in_progress");
    std::fs::write(&marker, b"leftover bytes from the crashed run").unwrap();
    assert!(
        original.exists(),
        "the original is still there before the Media Stage runs"
    );

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1);
    let derivative = dir.path().join("attachments/photo-mv.jpg");
    assert!(
        derivative.exists(),
        "the derivative exists under the -mv name"
    );
    assert_ne!(
        std::fs::read(&derivative).unwrap(),
        b"leftover bytes from the crashed run".to_vec(),
        "the heal re-transcodes rather than adopting the marker's bytes"
    );
    assert!(!marker.exists(), "no marker survives a completed heal");
    assert!(
        !original.exists(),
        "the original is gone once the heal commits"
    );
    let document = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        document.messages[0].attachments[0].path.as_deref(),
        Some("attachments/photo-mv.jpg"),
        "the document points at the healed derivative"
    );
}

#[test]
fn a_heal_that_fails_to_transcode_repoints_at_the_original_before_recording_the_failure() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // Same crash-window simulation as the other heal tests, but this
    // time the recovered original is garbage ffmpeg will fail on. The
    // Err arm must not simply "keep the path" the way a non-heal
    // failure does: `recorded_rel` here is the phantom -mv name from the
    // crashed run, which will never exist. It must repoint at the
    // recovered original first, then record the failure on it.
    let (dir, jsonl, original) = staged_one("broken.png", b"not a png at all");
    let mut document = read_conversation_jsonl(&jsonl).unwrap();
    {
        let attachment = &mut document.messages[0].attachments[0];
        attachment.path = Some("attachments/broken-mv.jpg".into());
        attachment.digest_sha256 = Some("deadbeef".repeat(8));
        attachment.size_bytes = Some(1234);
        attachment.mime_type = Some("image/jpeg".into());
    }
    write_conversation_jsonl_to(&jsonl, &document).unwrap();
    assert!(
        original.exists(),
        "the original is still there before the Media Stage runs"
    );

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let attachment = &document.messages[0].attachments[0];
    assert_eq!(
        attachment.path.as_deref(),
        Some("attachments/broken.png"),
        "repointed at the recovered original, not left on the phantom -mv name"
    );
    let reason = attachment.missing_reason.clone().unwrap();
    assert!(
        reason.starts_with("convert_failed: "),
        "reason must stay inside the closed set: {reason}"
    );
    assert_eq!(
        attachment.digest_sha256.as_deref(),
        Some(file_io::sha256_hex(&std::fs::read(&original).unwrap()).as_str()),
        "digest recomputed from the recovered original, not the stale pre-crash value"
    );
    assert!(
        original.exists(),
        "the recovered original is untouched by a failed transcode"
    );
}

#[test]
fn a_heal_that_the_media_step_skips_repoints_at_the_original_deterministically() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // A small mp4 under compress's min_size_bytes returns
    // TranscodeOutcome::Skipped without looking at the video's content
    // at all — compress_video's `ext == "mp4"` branch short-circuits
    // before any ffmpeg probe or encode — so this Skip is deterministic
    // regardless of the installed ffmpeg's version or behavior, unlike
    // (say) an already-efficient-codec skip, which depends on what that
    // ffmpeg actually reports.
    let (dir, jsonl, original) = staged_one("clip.mp4", b"not really a video, but small");
    let mut document = read_conversation_jsonl(&jsonl).unwrap();
    {
        let attachment = &mut document.messages[0].attachments[0];
        attachment.path = Some("attachments/clip-mv.mp4".into());
        attachment.digest_sha256 = Some("deadbeef".repeat(8));
        attachment.size_bytes = Some(1234);
        attachment.mime_type = Some("video/mp4".into());
    }
    write_conversation_jsonl_to(&jsonl, &document).unwrap();
    assert!(
        original.exists(),
        "the original is still there before the Media Stage runs"
    );
    // Default min_size_bytes is 20 MB; our fixture is a few dozen bytes.
    assert!(CompressOptions::default().min_size_bytes > 1000);

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Compress, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.skipped, 1);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let attachment = &document.messages[0].attachments[0];
    assert_eq!(
        attachment.path.as_deref(),
        Some("attachments/clip.mp4"),
        "repointed at the recovered original, not left on the phantom -mv name"
    );
    assert!(attachment.missing_reason.is_none());
    assert_eq!(
        attachment.digest_sha256.as_deref(),
        Some(file_io::sha256_hex(&std::fs::read(&original).unwrap()).as_str())
    );
    assert!(original.exists(), "a skipped file's original is left alone");
}

#[test]
fn a_crash_that_lost_both_the_marker_and_the_original_is_unrecoverable() {
    // The whole Media Stage still needs ffmpeg present up front (the preflight
    // check runs before any per-attachment classification), even though
    // no transcode is ever attempted for this particular attachment.
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, original) = staged_one("photo.png", PNG_1X1_RGB);
    let mut document = read_conversation_jsonl(&jsonl).unwrap();
    {
        let attachment = &mut document.messages[0].attachments[0];
        attachment.path = Some("attachments/photo-mv.jpg".into());
        // Seed non-None digest/size so the clearing assertions below can
        // actually fail if `apply_unrecoverable` stops clearing them.
        attachment.digest_sha256 = Some("cafebabe".repeat(8));
        attachment.size_bytes = Some(999);
    }
    write_conversation_jsonl_to(&jsonl, &document).unwrap();
    // Nothing recoverable is left: no marker, and the original itself is gone too.
    std::fs::remove_file(&original).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.missing, 1);
    assert_eq!(report.converted, 0);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let attachment = &document.messages[0].attachments[0];
    assert_eq!(attachment.missing_reason.as_deref(), Some("file_missing"));
    assert_eq!(attachment.path, None);
    assert_eq!(attachment.digest_sha256, None);
}

#[test]
fn two_attachments_in_one_document_sharing_a_path_are_patched_together() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, original) = staged_one("photo.png", PNG_1X1_RGB);
    // A second message in the same document, carrying an attachment
    // recorded at the exact same content-addressed path — a legitimate
    // state, not a fixture error.
    let mut document = read_conversation_jsonl(&jsonl).unwrap();
    let mut second_message = document.messages[0].clone();
    second_message.guid = "second-message-guid".into();
    second_message.timestamp_unix_ms += 1000;
    document.messages.push(second_message);
    document.finalize_stats();
    write_conversation_jsonl_to(&jsonl, &document).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(
        report.converted, 1,
        "one physical file, one transcode, however many attachments reference it"
    );
    assert!(!original.exists());
    let document = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(document.messages.len(), 2);
    for message in &document.messages {
        let attachment = &message.attachments[0];
        assert_eq!(
            attachment.path.as_deref(),
            Some("attachments/photo-mv.jpg"),
            "every attachment sharing the path gets patched"
        );
        assert!(attachment.digest_sha256.is_some());
    }
}

#[test]
fn two_documents_sharing_one_original_both_end_pointing_at_the_committed_derivative() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl_a, original) = staged_one("shared.png", PNG_1X1_RGB);

    // A second, independent conversation staged in the same directory whose
    // attachment happens to record the identical path — two different
    // chats that received the same bytes.
    let mut document_b = ir::testutil::sample_document("second conversation, same photo");
    document_b.conversation.chat_identifier = "+15555550199".into();
    document_b.messages[0].guid = "doc-b-guid".into();
    document_b.messages[0].attachments = vec![Attachment {
        path: Some("attachments/shared.png".into()),
        original_name: Some("shared.png".into()),
        mime_type: None,
        size_bytes: Some(PNG_1X1_RGB.len() as u64),
        ..ir::testutil::sample_attachment()
    }];
    document_b.finalize_stats();
    let jsonl_b = conversation_file(dir.path(), &document_b);
    write_conversation_jsonl_to(&jsonl_b, &document_b).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1, "one physical file is transcoded once");
    assert_eq!(
        report.repointed, 1,
        "the second document is repointed, not re-transcoded"
    );
    assert!(!original.exists());

    let final_document_a = read_conversation_jsonl(&jsonl_a).unwrap();
    let final_document_b = read_conversation_jsonl(&jsonl_b).unwrap();
    let attachment_a = &final_document_a.messages[0].attachments[0];
    let attachment_b = &final_document_b.messages[0].attachments[0];
    assert_eq!(
        attachment_a.path.as_deref(),
        Some("attachments/shared-mv.jpg")
    );
    assert_eq!(
        attachment_b.path.as_deref(),
        Some("attachments/shared-mv.jpg")
    );
    assert!(attachment_a.digest_sha256.is_some());
    assert_eq!(
        attachment_a.digest_sha256, attachment_b.digest_sha256,
        "both documents recompute the same digest from the same on-disk derivative"
    );
}

#[cfg(unix)]
#[test]
fn a_write_failure_leaves_the_final_name_uncommitted_and_the_original_untouched() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // The headline "patched before the final name exists" test only
    // checks terminal state, which would pass even if the patch and the
    // rename were swapped. This makes the ordering falsifiable: force
    // the conversation-file write to fail (a read-only run directory, so
    // `write_conversation_jsonl_to`'s `.tmp` sibling can't be created)
    // after the transcode has already produced a derivative, and assert
    // the final name was never created and the original is untouched.
    use std::os::unix::fs::PermissionsExt;
    let (dir, _jsonl, original) = staged_one("photo.png", PNG_1X1_RGB);
    let mut perms = std::fs::metadata(dir.path()).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(dir.path(), perms).unwrap();

    let result = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    );

    // Restore before any assertion can panic, so the TempDir can still
    // clean itself up.
    let mut restore = std::fs::metadata(dir.path()).unwrap().permissions();
    restore.set_mode(0o755);
    std::fs::set_permissions(dir.path(), restore).unwrap();

    assert!(
        result.is_err(),
        "the conversation-file write failure must surface, not be swallowed"
    );
    assert!(
        !dir.path().join("attachments/photo-mv.jpg").exists(),
        "the final name must never exist without a committed patch"
    );
    assert!(
        original.exists(),
        "the original is untouched when the patch never committed"
    );
}

/// Write a JPEG through ffmpeg at `-q:v 2` (low compression), sized well
/// over the media crate's compress-mode same-format floor (500 KB) and
/// reliably smaller when re-encoded at compress mode's finer `-q:v 5` —
/// the ordinary "worse quality shrinks" direction, calibrated locally
/// against this repo's ffmpeg build (1024x768 random noise: ~856 KB at
/// `-q:v 2`, ~646 KB re-encoded at `-q:v 5`). See `media::process`'s
/// `write_jpeg_that_grows_on_finer_reencode` for the opposite,
/// incompressible-noise calibration this deliberately avoids.
fn jpeg_over_compress_floor_bytes() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "nullsrc=size=1024x768,geq=random(1)*255:random(1)*255:random(1)*255",
            "-frames:v",
            "1",
            "-update",
            "1",
            "-q:v",
            "2",
        ])
        .arg(&path)
        .output()
        .expect("run ffmpeg for jpeg fixture");
    assert!(
        output.status.success(),
        "ffmpeg jpeg fixture generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read(&path).unwrap()
}

#[test]
fn two_documents_sharing_one_compressed_original_both_end_pointing_at_the_committed_derivative() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // The compress-mode variant of the convert-mode test above: this is
    // the exact bug the final review caught. `final_derivative_name`
    // used to stat the (already-deleted) shared original for the
    // compress-mode JPEG floor, read size 0, read that as "under the
    // floor", and answered `None` — so document B's repoint never
    // queued and it was left pointing at a file that no longer existed,
    // with no `missing_reason`.
    let bytes = jpeg_over_compress_floor_bytes();
    assert!(
        bytes.len() as u64 > 500 * 1024,
        "fixture must clear the compress-mode same-format floor"
    );
    let (dir, jsonl_a, original) = staged_one("shared.jpg", &bytes);

    let mut document_b = ir::testutil::sample_document("second conversation, same photo");
    document_b.conversation.chat_identifier = "+15555550199".into();
    document_b.messages[0].guid = "doc-b-guid".into();
    document_b.messages[0].attachments = vec![Attachment {
        path: Some("attachments/shared.jpg".into()),
        original_name: Some("shared.jpg".into()),
        mime_type: None,
        size_bytes: Some(bytes.len() as u64),
        ..ir::testutil::sample_attachment()
    }];
    document_b.finalize_stats();
    let jsonl_b = conversation_file(dir.path(), &document_b);
    write_conversation_jsonl_to(&jsonl_b, &document_b).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Compress, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1, "one physical file is compressed once");
    assert_eq!(
        report.repointed, 1,
        "the second document is repointed, not left dangling or re-compressed"
    );
    assert!(!original.exists());

    let final_document_a = read_conversation_jsonl(&jsonl_a).unwrap();
    let final_document_b = read_conversation_jsonl(&jsonl_b).unwrap();
    let attachment_a = &final_document_a.messages[0].attachments[0];
    let attachment_b = &final_document_b.messages[0].attachments[0];
    assert_eq!(
        attachment_a.path.as_deref(),
        Some("attachments/shared-mv.jpg")
    );
    assert_eq!(
        attachment_b.path.as_deref(),
        Some("attachments/shared-mv.jpg")
    );
    assert!(attachment_a.missing_reason.is_none());
    assert!(attachment_b.missing_reason.is_none());
    assert!(attachment_a.digest_sha256.is_some());
    assert_eq!(
        attachment_a.digest_sha256, attachment_b.digest_sha256,
        "both documents recompute the same digest from the same on-disk derivative"
    );
}

#[test]
fn a_missing_original_with_no_committed_derivative_becomes_file_missing() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    // Covers the other half of the same bug: a recorded path that is
    // gone for good (nothing shares it, no committed derivative exists,
    // and no too-large note says the Media Stage dropped it). Before the fix
    // this fell through the repoint branch's `if let Some(name) = …`
    // silently, leaving the attachment dangling with no `missing_reason`
    // at all.
    let (dir, jsonl, original) = staged_one(
        "ghost.jpg",
        b"content is irrelevant; deleted before the Media Stage looks",
    );
    std::fs::remove_file(&original).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Compress, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.missing, 1);
    assert_eq!(report.repointed, 0);
    assert_eq!(report.converted, 0);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let attachment = &document.messages[0].attachments[0];
    assert_eq!(attachment.missing_reason.as_deref(), Some("file_missing"));
    assert_eq!(attachment.path, None);
    assert_eq!(attachment.digest_sha256, None);
}

/// Write a second conversation into `dir` whose one attachment records
/// `rel`, the path the first conversation's attachment already records:
/// two chats that received the same bytes in the same second.
///
/// Returns the second conversation file's path.
fn second_document_sharing(dir: &Path, rel: &str, size: u64) -> PathBuf {
    let mut document_b = ir::testutil::sample_document("second conversation, same file");
    document_b.conversation.chat_identifier = "+15555550199".into();
    document_b.messages[0].guid = "doc-b-guid".into();
    document_b.messages[0].attachments = vec![Attachment {
        path: Some(rel.into()),
        original_name: Some("shared.png".into()),
        mime_type: None,
        size_bytes: Some(size),
        ..ir::testutil::sample_attachment()
    }];
    document_b.finalize_stats();
    let jsonl_b = conversation_file(dir, &document_b);
    write_conversation_jsonl_to(&jsonl_b, &document_b).unwrap();
    jsonl_b
}

/// The one attachment of each conversation file, in the order given.
fn only_attachments(jsonls: &[&Path]) -> Vec<Attachment> {
    jsonls
        .iter()
        .map(|jsonl| read_conversation_jsonl(jsonl).unwrap().messages[0].attachments[0].clone())
        .collect()
}

#[test]
fn two_documents_sharing_one_original_that_converts_too_large_both_record_too_large() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl_a, original) = staged_one("shared.png", PNG_1X1_RGB);
    let jsonl_b = second_document_sharing(
        dir.path(),
        "attachments/shared.png",
        PNG_1X1_RGB.len() as u64,
    );

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, 1),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.too_large, 2, "each conversation records the drop");
    assert_eq!(report.missing, 0, "the shared file was dropped, not lost");
    assert!(!original.exists());
    let attachments = only_attachments(&[&jsonl_a, &jsonl_b]);
    for attachment in &attachments {
        assert_eq!(attachment.missing_reason.as_deref(), Some("too_large"));
        assert_eq!(attachment.path, None);
        assert_eq!(attachment.digest_sha256, None);
    }
    assert!(
        attachments[0].size_bytes.is_some_and(|size| size > 1),
        "the converted size, over the limit of 1 byte"
    );
    assert_eq!(
        attachments[0].size_bytes, attachments[1].size_bytes,
        "both conversations carry the converted size"
    );
}

#[test]
fn a_too_large_drop_survives_a_stop_and_a_resume() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl_a, original) = staged_one("shared.png", PNG_1X1_RGB);
    let jsonl_b = second_document_sharing(
        dir.path(),
        "attachments/shared.png",
        PNG_1X1_RGB.len() as u64,
    );

    // Stop the Media Stage right after the first conversation's attachment.
    let cancel = CancelFlag::default();
    let err = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, 1),
        &tools,
        &cancel,
        &IssueSink::none(),
        &LogSink::none(),
        &mut |progress| {
            if progress.done == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    )
    .expect_err("the Media Stage stops after the first attachment");
    assert_eq!(err.to_string(), "canceled");
    assert!(
        !original.exists(),
        "the first conversation dropped the file"
    );

    let resumed = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, 1),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(
        resumed.too_large, 1,
        "the resume settles the second conversation"
    );
    assert_eq!(resumed.missing, 0);
    let attachments = only_attachments(&[&jsonl_a, &jsonl_b]);
    for attachment in &attachments {
        assert_eq!(attachment.missing_reason.as_deref(), Some("too_large"));
        assert_eq!(attachment.path, None);
    }
    assert!(attachments[0].size_bytes.is_some_and(|size| size > 1));
    assert_eq!(attachments[0].size_bytes, attachments[1].size_bytes);
}

#[test]
fn the_committed_suffix_guard_excludes_an_already_final_video_from_pending() {
    // No ffmpeg needed: pending_in decides this from names alone.
    // media::derivative_name always answers Some("…mp4") for a video in
    // either mode (it cannot see CompressOptions), so without the -mv
    // exclusion a committed video derivative would look pending forever
    // and get re-degraded on every resume.
    let (dir, jsonl, _original) = staged_one("clip-mv.mp4", b"");
    let document = read_conversation_jsonl(&jsonl).unwrap();

    let work = pending_in(dir.path(), &document, MediaMode::Convert).unwrap();

    assert!(
        work.is_empty(),
        "a committed -mv name must never re-enter the pending list"
    );
}

/// Every file under `dir`, keyed by its path relative to `dir`, with its bytes.
fn snapshot_tree(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut directories = vec![dir.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else {
                let rel = path.strip_prefix(dir).unwrap().to_path_buf();
                files.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

#[test]
fn without_ffmpeg_the_whole_media_stage_fails_and_touches_nothing() {
    // The module's contract: when ffmpeg/ffprobe are missing, the Media Stage fails
    // before any document is touched and never brands an attachment
    // `convert_failed`. The directory holds work for both modes: an image, a
    // video, and an audio file, across two conversation files.
    let (dir, jsonl, _) = staged_one("photo.png", PNG_1X1_RGB);
    let mut document = read_conversation_jsonl(&jsonl).unwrap();
    let image = document.messages[0].attachments[0].clone();
    for (name, bytes) in [
        ("clip.mp4", b"not really a video".as_slice()),
        ("voice.m4a", b"not really audio".as_slice()),
    ] {
        let rel = format!("attachments/{name}");
        std::fs::write(dir.path().join(&rel), bytes).unwrap();
        document.messages[0].attachments.push(Attachment {
            path: Some(rel),
            original_name: Some(name.to_string()),
            size_bytes: Some(bytes.len() as u64),
            ..image.clone()
        });
    }
    write_conversation_jsonl_to(&jsonl, &document).unwrap();
    write_conversation_jsonl_to(&dir.path().join("second.jsonl"), &document).unwrap();
    let before = snapshot_tree(dir.path());
    assert_eq!(
        before.len(),
        5,
        "two conversation files and three originals"
    );

    let tools = media::testutil::no_ffmpeg();
    for mode in [MediaMode::Convert, MediaMode::Compress] {
        let mut progress_calls = 0usize;
        let err = transcode_staged(
            dir.path(),
            &options(mode, u64::MAX),
            &tools,
            &CancelFlag::default(),
            &IssueSink::none(),
            &LogSink::none(),
            &mut |_| {
                progress_calls += 1;
            },
        )
        .expect_err("a missing ffmpeg fails the whole Media Stage");

        let message = err.to_string();
        assert!(
            message
                .starts_with("ffmpeg and ffprobe are required to convert or compress attachments"),
            "{mode:?}: {message}"
        );
        assert!(
            message.contains("ffmpeg and ffprobe were not found on PATH"),
            "{mode:?}: {message}"
        );
        assert_eq!(
            progress_calls, 0,
            "{mode:?}: the Media Stage reported progress"
        );
        assert_eq!(
            snapshot_tree(dir.path()),
            before,
            "{mode:?}: a conversation file or an original changed"
        );
        for file in conversation_files(dir.path()).unwrap() {
            let document = read_conversation_jsonl(&file).unwrap();
            for attachment in document.messages.iter().flat_map(|m| &m.attachments) {
                assert_eq!(
                    attachment.missing_reason,
                    None,
                    "{mode:?}: {} marked an attachment",
                    file.display()
                );
            }
        }
    }
}

/// Healing `a-mv.jpg` must re-transcode `a.*`. Any other file would put
/// someone else's photo on the message.
#[test]
fn crash_recovery_finds_only_an_original_with_the_same_stem() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments).unwrap();
    for name in ["a.png", "b.png", "c.gif"] {
        std::fs::write(attachments.join(name), b"image").unwrap();
    }

    assert_eq!(
        find_recoverable_original(dir.path(), "a", MediaMode::Convert).unwrap(),
        Some(attachments.join("a.png"))
    );
    // `c.gif` has the stem but the Media Stage never touches a GIF, and
    // `a.png` and `b.png` are convertible but belong to other attachments.
    assert_eq!(
        find_recoverable_original(dir.path(), "c", MediaMode::Convert).unwrap(),
        None
    );
    assert_eq!(
        find_recoverable_original(dir.path(), "d", MediaMode::Convert).unwrap(),
        None
    );
}

/// A stop after a conversation file was patched to `x-mv.mp4`, while another
/// conversation sharing `x.mov` dropped it for size: the resume finds the
/// note the drop left and records `too_large`, not `file_missing`.
#[test]
fn a_crash_heal_whose_original_was_dropped_too_large_records_too_large() {
    let Some(tools) = media::testutil::real_ffmpeg() else {
        return;
    };
    let (dir, jsonl, original) = staged_one("x.mov", b"not really a video");
    let mut document = read_conversation_jsonl(&jsonl).unwrap();
    {
        let attachment = &mut document.messages[0].attachments[0];
        attachment.path = Some("attachments/x-mv.mp4".into());
        attachment.digest_sha256 = Some("cafebabe".repeat(8));
    }
    write_conversation_jsonl_to(&jsonl, &document).unwrap();
    std::fs::remove_file(&original).unwrap();
    std::fs::write(too_large_note(&original), "12345").unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, 1),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.too_large, 1);
    assert_eq!(report.missing, 0, "the shared file was dropped, not lost");
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let attachment = &document.messages[0].attachments[0];
    assert_eq!(attachment.missing_reason.as_deref(), Some("too_large"));
    assert_eq!(attachment.size_bytes, Some(12345));
    assert_eq!(attachment.path, None);
    assert_eq!(attachment.digest_sha256, None);
}

/// The note must belong to the original `x-mv.mp4` was converted from: same
/// stem, and an original the mode would convert to that name.
#[test]
fn crash_recovery_reads_only_the_too_large_note_of_its_own_original() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments).unwrap();
    for (name, size) in [("y.mov", "1"), ("x.png", "2"), ("x.mov", "3")] {
        std::fs::write(too_large_note(&attachments.join(name)), size).unwrap();
    }

    let note = |name: &str| {
        find_too_large_note(dir.path(), &attachments.join(name), MediaMode::Convert).unwrap()
    };
    assert_eq!(note("x-mv.mp4"), Some(3));
    assert_eq!(note("x-mv.jpg"), Some(2));
    assert_eq!(note("z-mv.mp4"), None);
}

/// A cancel while ffmpeg converts a file kills it and ends the Media Stage
/// as canceled: the file is not recorded as one that could not be
/// converted, and no Import Error is sent for it (#2898).
#[cfg(unix)]
#[test]
fn a_cancel_while_ffmpeg_runs_ends_the_stage_as_canceled_not_as_a_failed_file() {
    use std::os::unix::fs::PermissionsExt;
    let mocks = tempfile::tempdir().unwrap();
    for name in ["ffmpeg", "ffprobe"] {
        let program = mocks.path().join(name);
        std::fs::write(
            &program,
            "#!/bin/sh\n[ \"$1\" = -version ] && exit 0\n\
             case \"$*\" in *cancel-me.png*) ;; *) exit 1 ;; esac\n\
             touch \"$(dirname \"$0\")/started\"\nexec sleep 600\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let tools = media::InstalledTools::new(Some(mocks.path().to_path_buf())).searching("");
    let (dir, jsonl, original) = staged_one("cancel-me.png", PNG_1X1_RGB);
    let (issues, rows) = collecting_sink();
    let cancel = CancelFlag::default();
    let canceler = {
        let cancel = std::sync::Arc::clone(&cancel);
        let started = mocks.path().join("started");
        std::thread::spawn(move || {
            while !started.exists() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            cancel.store(true, Ordering::Relaxed);
        })
    };

    let err = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &cancel,
        &issues,
        &LogSink::none(),
        &mut |_| {},
    )
    .expect_err("a canceled Media Stage fails");
    canceler.join().unwrap();

    assert_eq!(err.to_string(), "canceled");
    assert!(
        rows.lock().unwrap().is_empty(),
        "{:?}",
        rows.lock().unwrap()
    );
    let document = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(document.messages[0].attachments[0].missing_reason, None);
    assert!(original.exists(), "the original is kept");
}

/// The reason recorded on an attachment ffmpeg could not convert carries why
/// ffmpeg failed or was stopped, not only which file it was working on, so
/// a stalled or looping video says so (#2898). Of what ffmpeg wrote, only
/// the line that names the cause, which here is its last line, is kept, so
/// a damaged file's thousands of warnings do not ride along on the
/// attachment.
#[cfg(unix)]
#[test]
fn a_failed_conversion_records_why_ffmpeg_failed() {
    use std::os::unix::fs::PermissionsExt;
    let mocks = tempfile::tempdir().unwrap();
    for name in ["ffmpeg", "ffprobe"] {
        let program = mocks.path().join(name);
        std::fs::write(
            &program,
            "#!/bin/sh\n[ \"$1\" = -version ] && exit 0\n\
             case \"$*\" in *fail-me.png*) ;; *) exit 1 ;; esac\n\
             echo 'warning for packet 1' >&2\n\
             echo 'the encoder made no progress' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let tools = media::InstalledTools::new(Some(mocks.path().to_path_buf())).searching("");
    let (dir, jsonl, _) = staged_one("fail-me.png", PNG_1X1_RGB);

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    let reason = document.messages[0].attachments[0]
        .missing_reason
        .clone()
        .unwrap();
    assert_eq!(reason, "convert_failed: the encoder made no progress");
}

/// The reason an attachment records when ffmpeg failed on it is the line
/// ffmpeg wrote that names the cause, which here is its last line, with no
/// path in it (#2801, #2959). The whole error, path and all, goes in the
/// row for the log.
#[test]
fn a_failed_conversion_records_the_line_that_names_the_cause() {
    let tools = media::testutil::FakeTools::new().failing_on("-q:v");
    let (dir, jsonl, original) = staged_one("photo.png", PNG_1X1_RGB);
    let (issues, rows) = collecting_sink();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        &tools,
        &CancelFlag::default(),
        &issues,
        &LogSink::none(),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    let document = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        document.messages[0].attachments[0]
            .missing_reason
            .as_deref(),
        Some("convert_failed: -q:v: fake failure on '<input>'")
    );
    let rows = rows.lock().unwrap();
    let row = rows.last().expect("a row for the failed file");
    assert!(
        row.reason.contains(&original.display().to_string()),
        "the row keeps the whole error: {}",
        row.reason
    );
}

/// A video staged under `attachments/` with compress options that send it
/// to the encoder, and a fake ffmpeg whose HEVC run fails and H.264 run
/// works.
fn staged_video_hevc_fails() -> (
    tempfile::TempDir,
    media::testutil::FakeTools,
    TranscodeOptions,
) {
    let (dir, _, _) = staged_one("clip.mov", b"an h264 video");
    let tools = media::testutil::FakeTools::new()
        .probing(media::testutil::video_probe("h264", 30.0))
        .failing_on("libx265");
    let options = TranscodeOptions {
        mode: MediaMode::Compress,
        compress: CompressOptions {
            min_size_bytes: 0,
            ..CompressOptions::default()
        },
        asset_max_bytes: u64::MAX,
    };
    (dir, tools, options)
}

/// The Media Stage sends why HEVC failed to the log's warnings, once, the
/// way the desktop app's Import Run log takes it (#2801).
#[test]
fn the_media_stage_warns_why_hevc_failed() {
    let (dir, tools, options) = staged_video_hevc_fails();
    let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink_warnings = std::sync::Arc::clone(&warnings);
    let log =
        LogSink::none().with_warnings(move |w| sink_warnings.lock().unwrap().push(w.to_string()));

    let report = transcode_staged(
        dir.path(),
        &options,
        &tools,
        &CancelFlag::default(),
        &IssueSink::none(),
        &log,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1);
    let warnings = warnings.lock().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0]
            .starts_with("HEVC encoding failed, so videos were compressed as H.264 instead: ")
            && warnings[0].ends_with("libx265: fake failure on '<input>'"),
        "{warnings:?}"
    );
    assert!(!warnings[0].contains("clip"), "{warnings:?}");
}
