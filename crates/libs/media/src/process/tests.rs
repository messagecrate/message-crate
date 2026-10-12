use super::*;
use crate::MediaProbe;
use crate::fake_tools::{FakeTools, video_probe};
use crate::kind_for_mime;
use crate::testutil::PNG_1X1_RGB;
use crate::testutil::{clock_from, progress_then_hang};
use crate::tools::{InstalledTools, Limit, MediaTools, Reads, StallWatch, ffmpeg_command_line};

/// The one ffmpeg run `tools` made, failing the test when there were more
/// or none.
fn only_run(tools: &FakeTools) -> Vec<String> {
    let runs = tools.runs();
    assert_eq!(runs.len(), 1, "ffmpeg runs: {runs:?}");
    runs.into_iter().next().unwrap()
}

/// True when `args` hold `arg`.
fn has(args: &[String], arg: &str) -> bool {
    args.iter().any(|a| a == arg)
}

/// The `-vf` filter graph in `args`.
fn filter(args: &[String]) -> &str {
    let at = args
        .iter()
        .position(|a| a == "-vf")
        .expect("a -vf argument");
    &args[at + 1]
}

#[test]
fn compress_keeps_the_original_jpeg_when_the_re_encode_is_not_smaller() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    // Over the floor, so the size gate does not skip it before ffmpeg runs.
    let jpeg = attachments.join("already-tight.jpg");
    let before = vec![0xa5; JPEG_COMPRESS_FLOOR as usize + 1];
    fs::write(&jpeg, &before).unwrap();
    // The re-encode comes out the same size: not smaller, so not kept.
    let tools = FakeTools::new().writing(vec![0; before.len()]);

    let (report, remap) = process_attachment_files(
        &tools,
        dir.path(),
        std::slice::from_ref(&jpeg),
        MediaMode::Compress,
        &CompressOptions::default(),
        None,
        None,
    )
    .unwrap();

    assert_eq!(
        tools.runs().len(),
        1,
        "the JPEG must reach ffmpeg: otherwise every assertion below holds \
         whether or not the keep-smaller guard exists"
    );
    assert_eq!(fs::read(&jpeg).unwrap(), before, "original bytes replaced");
    assert!(
        !remap.contains_key("attachments/already-tight.jpg"),
        "a kept file must not be remapped: a remap entry tells the caller to \
         recompute a digest that did not change"
    );
    assert_eq!(report.processed, 0);
    assert_eq!(report.skipped, 1);
    assert_eq!(
        collect_media_files(&attachments).unwrap(),
        vec![jpeg],
        "the discarded re-encode was left behind"
    );
}

#[test]
fn transcode_file_writes_the_derivative_and_leaves_the_original_alone() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.png");
    fs::write(&src, PNG_1X1_RGB).unwrap();
    let before = fs::read(&src).unwrap();

    let name = derivative_name(&src, MediaMode::Convert).expect("png is converted");
    assert_eq!(name, "photo.jpg");
    let dest = dir.path().join(format!("{name}.in_progress"));
    let tools = FakeTools::new().writing("the jpeg");

    let outcome = transcode_file(
        &tools,
        &src,
        &dest,
        MediaMode::Convert,
        &CompressOptions::default(),
        None,
    )
    .unwrap();

    assert_eq!(outcome, TranscodeOutcome::Produced { hevc_failure: None });
    assert_eq!(
        fs::read(&dest).unwrap(),
        b"the jpeg",
        "derivative written where the caller asked"
    );
    assert!(
        !dir.path().join("photo.jpg").exists(),
        "the final name must not exist until the caller renames it: a file \
         under its final name means fully patched"
    );
    assert_eq!(
        fs::read(&src).unwrap(),
        before,
        "the original is the caller's to delete, after it commits"
    );
    assert!(
        !dir.path().join("photo.msgmedia.tmp.jpg").exists(),
        "the scratch file was moved to the destination, not copied"
    );
}

#[test]
fn derivative_name_is_none_for_a_file_the_mode_leaves_alone() {
    let dir = tempfile::tempdir().unwrap();
    let gif = dir.path().join("loop.gif");
    fs::write(&gif, b"not really a gif").unwrap();
    assert_eq!(derivative_name(&gif, MediaMode::Convert), None);

    let jpeg = dir.path().join("photo.jpg");
    fs::write(&jpeg, b"not really a jpeg").unwrap();
    assert_eq!(derivative_name(&jpeg, MediaMode::Convert), None);

    let doc = dir.path().join("notes.pdf");
    fs::write(&doc, b"%PDF").unwrap();
    assert_eq!(derivative_name(&doc, MediaMode::Convert), None);
}

#[test]
fn derivative_name_matches_what_the_media_step_actually_produces() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    // The forecast and the patch both trust derivative_name. If it disagrees
    // with the pass, a conversation file points at a name nothing wrote.
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.png");
    fs::write(&src, PNG_1X1_RGB).unwrap();
    let name = derivative_name(&src, MediaMode::Convert).unwrap();
    let dest = dir.path().join("out").join(&name);
    let outcome = transcode_file(
        &tools,
        &src,
        &dest,
        MediaMode::Convert,
        &CompressOptions::default(),
        None,
    )
    .unwrap();
    // dest is built from name, so the file-name equality below would hold
    // even if transcode_file wrote nothing. Pin down that it actually ran.
    assert_eq!(outcome, TranscodeOutcome::Produced { hevc_failure: None });
    assert!(
        dest.exists(),
        "derivative_name promised a name nothing wrote"
    );
    assert_eq!(
        dest.file_name().and_then(|n| n.to_str()),
        Some(name.as_str())
    );
}

#[test]
fn transcode_file_clears_scratch_beside_the_source_only() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.png");
    fs::write(&src, PNG_1X1_RGB).unwrap();
    let own_scratch = dir.path().join("photo.msgmedia.tmp.jpg");
    fs::write(&own_scratch, b"leftover").unwrap();
    let other_scratch = dir.path().join("other.msgmedia.tmp.jpg");
    fs::write(&other_scratch, b"in flight").unwrap();
    // Same stem as `src`, but the scratch extension a video producer
    // would write (e.g. an iOS Live Photo's IMG_0001.MOV, mid-encode,
    // sharing photo's stem). A stem-only match would wrongly sweep this;
    // photo.png is Kind::Image, so only its own "jpg" scratch is a
    // candidate.
    let same_stem_video_scratch = dir.path().join("photo.msgmedia.tmp.mp4");
    fs::write(&same_stem_video_scratch, b"another kind, in flight").unwrap();
    let marker = dir.path().join("photo.jpg.in_progress");
    fs::write(&marker, b"a previous attempt").unwrap();

    // Clone mode returns before any ffmpeg work, which is enough to show what
    // the entry point sweeps.
    let _ = transcode_file(
        &FakeTools::new(),
        &src,
        &dir.path().join("photo.jpg.in_progress"),
        MediaMode::Clone,
        &CompressOptions::default(),
        None,
    );

    assert!(!own_scratch.exists(), "this file's own leftovers go");
    assert!(
        other_scratch.exists(),
        "another file's in-flight scratch must survive: a directory-wide sweep \
         destroys work that is still running"
    );
    assert!(
        same_stem_video_scratch.exists(),
        "a same-stem sibling's scratch of a different kind must survive: a \
         stem-only match would delete an in-flight Live-Photo pair's video \
         scratch while converting the image half"
    );
    assert!(
        marker.exists(),
        "the .in_progress marker is the resume signal and must survive the \
         scratch sweep"
    );
}

#[test]
fn commit_produced_refuses_a_destination_equal_to_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.jpg");
    fs::write(&original, b"jpeg-bytes").unwrap();
    let produced = dir.path().join("photo.msgmedia.tmp.jpg");
    fs::write(&produced, b"re-encoded-bytes").unwrap();

    // A caller that joined `derivative_name`'s output onto the source
    // directory without adding a distinct temp suffix (e.g. forgot
    // `.in_progress`) would ask to overwrite the original before any
    // commit has happened. That must be refused, not silently done.
    let err = commit_produced(Commit::To(&original), &original, &produced).unwrap_err();
    assert!(
        err.to_string().contains("original file"),
        "error should explain why: {err}"
    );
    assert!(original.exists(), "original must be untouched");
    assert_eq!(fs::read(&original).unwrap(), b"jpeg-bytes");
    assert!(
        produced.exists(),
        "the would-be derivative is left for the caller to clean up"
    );
}

#[test]
fn classify_kinds() {
    assert!(matches!(classify(Path::new("a.HEIC")), Some(Kind::Image)));
    assert!(matches!(classify(Path::new("v.mov")), Some(Kind::Video)));
    assert!(matches!(classify(Path::new("x.caf")), Some(Kind::Audio)));
    assert!(classify(Path::new("doc.pdf")).is_none());
    assert!(classify(Path::new("a.msgmedia.tmp.jpg")).is_none());
}

#[test]
fn detects_msgmedia_temp_names() {
    assert!(is_msgmedia_temp(Path::new(
        "20150917_095137-I_1.msgmedia.tmp.jpg"
    )));
    assert!(!is_msgmedia_temp(Path::new("20150917_095137-I_1.jpg")));
}

#[test]
fn sweeps_leftover_msgmedia_temps() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    let junk = att.join("photo.msgmedia.tmp.jpg");
    fs::write(&junk, b"partial").unwrap();
    fs::write(att.join("keep.jpg"), b"ok").unwrap();

    remove_msgmedia_temps(&att).unwrap();
    assert!(!junk.exists());
    assert!(att.join("keep.jpg").exists());
}

#[test]
fn clone_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let (report, remap) = process_attachment_files(
        &FakeTools::new(),
        dir.path(),
        &[],
        MediaMode::Clone,
        &CompressOptions::default(),
        None,
        None,
    )
    .unwrap();
    assert_eq!(report.processed, 0);
    assert!(remap.is_empty());
}

#[test]
fn same_path_rewrite_reports_changed() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    let file = att.join("photo.jpg");
    fs::write(&file, b"jpeg-bytes").unwrap();
    let outcome = changed(dir.path(), "attachments/photo.jpg", &file).unwrap();
    match outcome {
        Outcome::Changed { old_rel, new_rel } => {
            assert_eq!(old_rel, "attachments/photo.jpg");
            assert_eq!(new_rel, "attachments/photo.jpg");
        }
        Outcome::Skipped => panic!("in-place rewrite must not look like Skipped"),
    }
}

#[test]
fn format_bytes_scales() {
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(12_500), "12.5 KB");
    assert_eq!(format_bytes(1_500_000), "1.5 MB");
    assert_eq!(format_bytes(2_500_000_000), "2.5 GB");
}

#[test]
fn attachments_dir_bytes_sums_non_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    fs::write(att.join("a.jpg"), vec![0u8; 1000]).unwrap();
    fs::write(att.join("b.mp4"), vec![0u8; 2500]).unwrap();
    fs::write(att.join("orphan.msgmedia.tmp.jpg"), vec![0u8; 9999]).unwrap();
    assert_eq!(attachments_dir_bytes(&att).unwrap(), 3500);
}

#[test]
fn clone_with_log_emits_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut lines = Vec::new();
    let mut log = |line: &str| lines.push(line.to_string());
    let _ = process_attachment_files(
        &FakeTools::new(),
        dir.path(),
        &[],
        MediaMode::Clone,
        &CompressOptions::default(),
        Some(&mut log),
        None,
    )
    .unwrap();
    assert!(lines.is_empty());
}

#[test]
fn is_efficient_accepts_only_hevc_within_the_resolution_and_bitrate_caps() {
    let opts = CompressOptions {
        max_resolution: crate::MaxResolution::P1080, // long edge 1920
        ..CompressOptions::default()
    };
    let cap = 12_000_000;
    assert!(is_efficient("hevc", 1920, 1080, cap, &opts));
    assert!(is_efficient("h265", 1080, 1920, 1_000_000, &opts));
    assert!(!is_efficient("hevc", 1921, 1080, 1_000_000, &opts));
    assert!(!is_efficient("hevc", 1080, 1921, 1_000_000, &opts));
    assert!(!is_efficient("hevc", 1920, 1080, cap + 1, &opts));
    assert!(!is_efficient("h264", 640, 480, 1_000_000, &opts));
    assert!(!is_efficient("", 0, 0, 0, &opts));
    let p720 = CompressOptions {
        max_resolution: crate::MaxResolution::P720,
        ..CompressOptions::default()
    };
    assert!(is_efficient("hevc", 1280, 720, 1_000_000, &p720));
    assert!(!is_efficient("hevc", 1281, 720, 1_000_000, &p720));
}

#[test]
fn frame_rate_cap_applies_the_maximum_when_the_source_rate_is_unknown() {
    assert_eq!(frame_rate_cap(None, 30.0), Some(30.0));
    assert_eq!(frame_rate_cap(Some(60.0), 30.0), Some(30.0));
    assert_eq!(frame_rate_cap(Some(30.0), 30.0), None);
    assert_eq!(frame_rate_cap(Some(24.0), 30.0), None);
    // Zero means no maximum was given, and stands for 30.
    assert_eq!(frame_rate_cap(Some(60.0), 0.0), Some(30.0));
    assert_eq!(frame_rate_cap(None, 0.0), Some(30.0));
}

/// Write a one-second 320x240 test pattern at 10 fps with the given video codec.
fn write_test_video(path: &Path, codec: &[&str]) {
    write_test_video_at(path, codec, 10);
}

/// Write a one-second 320x240 test pattern at `rate` frames per second.
fn write_test_video_at(path: &Path, codec: &[&str], rate: u32) {
    let source = format!("testsrc=size=320x240:rate={rate}:duration=1");
    let mut args: Vec<String> = ["-y", "-f", "lavfi", "-i", &source, "-pix_fmt", "yuv420p"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    args.extend(codec.iter().map(|s| (*s).to_string()));
    args.push(path_str(path));
    InstalledTools::default()
        .run_ffmpeg(&args)
        .expect("generate test video");
}

/// One file through media conversion with `tools`: its remap target, or
/// `None` when skipped.
fn process_single(
    tools: &dyn MediaTools,
    dir: &Path,
    file: &Path,
    mode: MediaMode,
    opts: &CompressOptions,
) -> Option<String> {
    let (report, mut remap) =
        process_attachment_files(tools, dir, &[file.to_path_buf()], mode, opts, None, None)
            .unwrap();
    assert_eq!(report.errors, Vec::<String>::new());
    let old_rel = rel_path(dir, file).unwrap();
    let new_rel = remap.remove(&old_rel);
    assert_eq!(report.processed, usize::from(new_rel.is_some()));
    assert_eq!(report.skipped, usize::from(new_rel.is_none()));
    new_rel
}

#[test]
fn convert_turns_a_mov_into_an_mp4_and_removes_the_original() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let mov = attachments.join("clip.mov");
    write_test_video(&mov, &["-c:v", "libx264"]);

    let new_rel = process_single(
        &tools,
        dir.path(),
        &mov,
        MediaMode::Convert,
        &CompressOptions::default(),
    );

    assert_eq!(new_rel.as_deref(), Some("attachments/clip.mp4"));
    assert!(!mov.exists(), "the .mov was left behind");
    let probe = tools.probe(&attachments.join("clip.mp4")).unwrap();
    assert_eq!(probe.codec, "h264", "a remux keeps the stream");
}

#[test]
fn compress_only_remuxes_a_video_under_the_minimum_size() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let mov = attachments.join("small.mov");
    fs::write(&mov, b"a small mov").unwrap();
    let mp4 = attachments.join("other.mp4");
    fs::write(&mp4, b"a small mp4").unwrap();
    let opts = CompressOptions {
        min_size_bytes: 100 * 1024 * 1024,
        ..CompressOptions::default()
    };
    let tools = FakeTools::new().writing("the remuxed mp4");

    let new_rel = process_single(&tools, dir.path(), &mov, MediaMode::Compress, &opts);
    assert_eq!(new_rel.as_deref(), Some("attachments/small.mp4"));
    assert!(!mov.exists());
    assert_eq!(
        fs::read(attachments.join("small.mp4")).unwrap(),
        b"the remuxed mp4"
    );
    let remux = only_run(&tools);
    assert!(
        has(&remux, "copy") && !has(&remux, "libx265") && !has(&remux, "libx264"),
        "a small video is remuxed, not re-encoded: {remux:?}"
    );

    // A small MP4 is already in the target container: nothing to do.
    assert_eq!(
        process_single(&tools, dir.path(), &mp4, MediaMode::Compress, &opts),
        None
    );
    assert_eq!(tools.runs().len(), 1, "ffmpeg ran for the small mp4");
    assert_eq!(fs::read(&mp4).unwrap(), b"a small mp4");
}

#[test]
fn compress_re_encodes_a_large_h264_video_to_hevc_at_the_capped_frame_rate() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let avc = attachments.join("avc.mp4");
    write_test_video(&avc, &["-c:v", "libx264"]);
    let opts = CompressOptions {
        min_size_bytes: 0,
        max_fps: 5.0,
        ..CompressOptions::default()
    };

    let new_rel = process_single(&tools, dir.path(), &avc, MediaMode::Compress, &opts);

    assert_eq!(new_rel.as_deref(), Some("attachments/avc.mp4"));
    let probe = tools.probe(&avc).unwrap();
    assert_eq!(probe.codec, "hevc", "a large H.264 video is re-encoded");
    assert_eq!(probe.fps, Some(5.0), "the frame rate is capped at max_fps");
}

/// What `compress_video` reads from the probe: an efficient HEVC video is
/// left alone unless the skip is turned off, and a file of exactly the
/// minimum size is large enough to re-encode.
#[test]
fn compress_skips_an_efficient_video_unless_told_not_to() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let hevc = attachments.join("hevc.mp4");
    fs::write(&hevc, b"a small, low-bitrate hevc video").unwrap();
    let efficient = video_probe("hevc", 10.0);
    let opts = CompressOptions {
        min_size_bytes: fs::metadata(&hevc).unwrap().len(),
        ..CompressOptions::default()
    };
    let tools = FakeTools::new()
        .probing(efficient.clone())
        .writing("the re-encode");

    // Already HEVC, small and low bitrate: re-encoding would buy nothing.
    assert_eq!(
        process_single(&tools, dir.path(), &hevc, MediaMode::Compress, &opts),
        None
    );
    assert_eq!(tools.runs(), Vec::<Vec<String>>::new());

    // Over the resolution cap, the same codec is not efficient.
    let tools = FakeTools::new()
        .probing(MediaProbe {
            width: 1920,
            height: 1080,
            ..efficient.clone()
        })
        .writing("the re-encode");
    assert_eq!(
        process_single(&tools, dir.path(), &hevc, MediaMode::Compress, &opts).as_deref(),
        Some("attachments/hevc.mp4")
    );
    fs::write(&hevc, b"a small, low-bitrate hevc video").unwrap();

    // With the skip turned off, the efficient file is re-encoded too. A max
    // fps of zero means no cap was given, and the pass uses 30, which this
    // 10 fps video is already under, so the filter adds no fps step.
    let tools = FakeTools::new().probing(efficient).writing("the re-encode");
    let opts = CompressOptions {
        skip_efficient: false,
        max_fps: 0.0,
        ..opts
    };
    assert_eq!(
        process_single(&tools, dir.path(), &hevc, MediaMode::Compress, &opts).as_deref(),
        Some("attachments/hevc.mp4")
    );
    assert_eq!(fs::read(&hevc).unwrap(), b"the re-encode");
    let run = only_run(&tools);
    assert!(has(&run, "libx265"), "{run:?}");
    assert!(!filter(&run).contains("fps="), "{run:?}");
}

/// A video over the maximum frame rate is brought down to it, and libx264
/// stands in when libx265 fails.
#[test]
fn compress_caps_the_frame_rate_and_falls_back_to_h264() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let clip = attachments.join("clip.mov");
    fs::write(&clip, b"a 60 fps h264 video").unwrap();
    let tools = FakeTools::new()
        .probing(video_probe("h264", 60.0))
        .failing_on("libx265")
        .writing("the h264 re-encode");
    let opts = CompressOptions {
        min_size_bytes: 0,
        max_fps: 30.0,
        ..CompressOptions::default()
    };

    let new_rel = process_single(&tools, dir.path(), &clip, MediaMode::Compress, &opts);

    assert_eq!(new_rel.as_deref(), Some("attachments/clip.mp4"));
    assert!(!clip.exists());
    assert_eq!(
        fs::read(attachments.join("clip.mp4")).unwrap(),
        b"the h264 re-encode"
    );
    let runs = tools.runs();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(
        has(&runs[0], "libx265") && has(&runs[1], "libx264"),
        "{runs:?}"
    );
    for run in &runs {
        assert!(filter(run).ends_with(",fps=30"), "{run:?}");
    }
}

/// Two large videos in `attachments/` under `dir`, and compress options that
/// send both to the encoder.
fn two_large_videos(dir: &Path) -> (Vec<PathBuf>, CompressOptions) {
    let attachments = dir.join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let files: Vec<PathBuf> = ["one.mov", "two.mov"]
        .iter()
        .map(|name| {
            let path = attachments.join(name);
            fs::write(&path, b"an h264 video").unwrap();
            path
        })
        .collect();
    let opts = CompressOptions {
        min_size_bytes: 0,
        ..CompressOptions::default()
    };
    (files, opts)
}

/// When H.264 stands in for a failed HEVC run, the report says why HEVC
/// failed, for its caller to log as a warning (#2256).
#[test]
fn compress_reports_why_hevc_failed_when_h264_stands_in() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let tools = FakeTools::new()
        .probing(video_probe("h264", 30.0))
        .failing_on("libx265");

    let (report, _) = process_attachment_files(
        &tools,
        dir.path(),
        &files,
        MediaMode::Compress,
        &opts,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.processed, 2, "{:?}", report.errors);
    let line = report.hevc_fallback.line().expect("a fallback reason");
    assert!(
        line.contains("ffmpeg failed (fake)") && line.contains("libx265"),
        "the line carries ffmpeg's HEVC error: {line}"
    );
}

/// A pass where HEVC works reports no fallback.
#[test]
fn compress_reports_no_fallback_when_hevc_works() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let tools = FakeTools::new().probing(video_probe("h264", 30.0));

    let (report, _) = process_attachment_files(
        &tools,
        dir.path(),
        &files,
        MediaMode::Compress,
        &opts,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.hevc_fallback.line(), None);
}

/// When H.264 fails after HEVC, the error names both failures, and no
/// fallback is reported, since no video was compressed as H.264.
#[test]
fn compress_reports_both_errors_when_h264_fails_after_hevc() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let tools = FakeTools::new()
        .probing(video_probe("h264", 30.0))
        .failing_on("libx265")
        .failing_on("libx264");

    let (report, _) = process_attachment_files(
        &tools,
        dir.path(),
        &files[..1],
        MediaMode::Compress,
        &opts,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    let error = &report.errors[0];
    assert!(error.contains("HEVC failed"), "{error}");
    assert!(error.contains("libx265"), "HEVC's reason is kept: {error}");
    assert!(error.contains("libx264"), "H.264's reason is kept: {error}");
    assert_eq!(report.hevc_fallback.line(), None);
}

/// A run's [`HevcFallback`] hands back a warning for its first reason only,
/// so a caller that logs what it returns logs once per run.
#[test]
fn hevc_fallback_returns_a_warning_for_the_first_reason_only() {
    let mut fallback = HevcFallback::default();

    assert_eq!(fallback.record(None), None, "no fallback, no warning");
    let first = fallback.record(Some("no libx265".into()));
    assert_eq!(
        first.as_deref(),
        Some("HEVC encoding failed, so videos were compressed as H.264 instead: no libx265")
    );
    assert_eq!(fallback.record(Some("another reason".into())), None);
    assert_eq!(fallback.record(None), None);
    assert_eq!(fallback.line(), first, "the first reason is kept");
}

/// [`transcode_file`] hands why HEVC failed to its caller, which logs it.
#[test]
fn transcode_file_returns_why_hevc_failed_when_h264_stands_in() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let dest = dir.path().join("out.mp4");
    let tools = FakeTools::new()
        .probing(video_probe("h264", 30.0))
        .failing_on("libx265");

    let outcome =
        transcode_file(&tools, &files[0], &dest, MediaMode::Compress, &opts, None).unwrap();

    let TranscodeOutcome::Produced {
        hevc_failure: Some(reason),
    } = outcome
    else {
        panic!("expected a produced file with HEVC's reason, got {outcome:?}");
    };
    assert!(reason.contains("libx265"), "{reason}");
    assert!(dest.exists());
}

/// Max FPS is a ceiling: it never adds frames to a slower video (#965).
#[test]
fn compress_keeps_a_frame_rate_under_the_maximum_and_lowers_one_over_it() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let opts = CompressOptions {
        min_size_bytes: 0,
        max_fps: 30.0,
        ..CompressOptions::default()
    };

    let slow = attachments.join("slow.mp4");
    write_test_video_at(&slow, &["-c:v", "libx264"], 24);
    let new_rel = process_single(&tools, dir.path(), &slow, MediaMode::Compress, &opts);
    assert_eq!(new_rel.as_deref(), Some("attachments/slow.mp4"));
    let probe = tools.probe(&slow).unwrap();
    assert_eq!(probe.codec, "hevc", "the video was re-encoded");
    assert_eq!(probe.fps, Some(24.0), "a 24 fps video stays at 24");

    let fast = attachments.join("fast.mp4");
    write_test_video_at(&fast, &["-c:v", "libx264"], 60);
    let new_rel = process_single(&tools, dir.path(), &fast, MediaMode::Compress, &opts);
    assert_eq!(new_rel.as_deref(), Some("attachments/fast.mp4"));
    let probe = tools.probe(&fast).unwrap();
    assert_eq!(probe.codec, "hevc", "the video was re-encoded");
    assert_eq!(probe.fps, Some(30.0), "a 60 fps video comes down to 30");
}

#[test]
fn compress_re_encodes_a_large_mp3_when_the_result_is_smaller() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let mp3 = attachments.join("song.mp3");
    tools
        .run_ffmpeg(&[
            "-y".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "sine=frequency=440:duration=10".into(),
            "-ac".into(),
            "2".into(),
            "-b:a".into(),
            "320k".into(),
            path_str(&mp3),
        ])
        .expect("generate mp3 fixture");
    let before = fs::metadata(&mp3).unwrap().len();
    assert!(before > MP3_COMPRESS_FLOOR, "fixture must clear the floor");

    let new_rel = process_single(
        &tools,
        dir.path(),
        &mp3,
        MediaMode::Compress,
        &CompressOptions::default(),
    );

    assert_eq!(new_rel.as_deref(), Some("attachments/song.mp3"));
    assert!(fs::metadata(&mp3).unwrap().len() < before);

    // Convert leaves an MP3 alone, whatever its size.
    let bytes = fs::read(&mp3).unwrap();
    assert_eq!(
        process_single(
            &tools,
            dir.path(),
            &mp3,
            MediaMode::Convert,
            &CompressOptions::default()
        ),
        None
    );
    assert_eq!(fs::read(&mp3).unwrap(), bytes);
}

#[test]
fn process_attachment_files_touches_only_the_listed_files() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let listed = attachments.join("a.png");
    let unlisted = attachments.join("b.png");
    fs::write(&listed, PNG_1X1_RGB).unwrap();
    fs::write(&unlisted, PNG_1X1_RGB).unwrap();
    let unlisted_before = fs::read(&unlisted).unwrap();
    let tools = FakeTools::new();

    let (_report, remap) = process_attachment_files(
        &tools,
        dir.path(),
        std::slice::from_ref(&listed),
        MediaMode::Convert,
        &CompressOptions::default(),
        None,
        None,
    )
    .unwrap();

    assert!(
        remap.contains_key("attachments/a.png"),
        "the listed file must be converted"
    );
    assert!(
        !remap.contains_key("attachments/b.png"),
        "a file the caller did not list must be left alone: scoping the pass \
         to an explicit list is the whole point of taking one"
    );
    assert!(
        !only_run(&tools).contains(&path_str(&unlisted)),
        "ffmpeg read the unlisted file"
    );
    assert!(unlisted.is_file(), "unlisted file must survive the pass");
    assert_eq!(
        fs::read(&unlisted).unwrap(),
        unlisted_before,
        "unlisted file was rewritten"
    );
}

#[test]
fn transcode_file_as_converts_an_extensionless_source_by_the_given_kind() {
    let dir = tempfile::tempdir().unwrap();
    // The server stores originals under their fingerprint alone.
    let src = dir.path().join("3b1f");
    fs::write(&src, PNG_1X1_RGB).unwrap();
    assert_eq!(classify(&src), None, "no extension, so no kind to read");
    let dest = dir.path().join("3b1f.jpg.in_progress");
    let tools = FakeTools::new().writing("the jpeg");

    let outcome = transcode_file_as(
        &tools,
        &src,
        Kind::Image,
        &dest,
        MediaMode::Convert,
        &CompressOptions::default(),
        None,
    )
    .unwrap();

    assert_eq!(outcome, TranscodeOutcome::Produced { hevc_failure: None });
    assert_eq!(fs::read(&dest).unwrap(), b"the jpeg");
    assert!(src.exists(), "the original is never touched");
    assert_eq!(
        transcode_file_as(
            &tools,
            &src,
            Kind::Image,
            &dest,
            MediaMode::Clone,
            &CompressOptions::default(),
            None
        )
        .unwrap(),
        TranscodeOutcome::Skipped,
        "clone mode never converts"
    );
    assert_eq!(tools.runs().len(), 1, "clone mode ran ffmpeg");
}

#[test]
fn kind_for_mime_reads_the_top_level_type() {
    assert_eq!(kind_for_mime("image/heic"), Some(Kind::Image));
    assert_eq!(
        kind_for_mime("video/quicktime; codecs=hvc1"),
        Some(Kind::Video)
    );
    assert_eq!(kind_for_mime("AUDIO/amr"), Some(Kind::Audio));
    assert_eq!(kind_for_mime("application/pdf"), None);
    assert_eq!(kind_for_mime(""), None);
}

#[test]
fn kind_of_reads_the_extension_first_and_never_converts_a_gif() {
    assert_eq!(kind_of(Path::new("x.jpg"), None, &[]), Some(Kind::Image));
    assert_eq!(kind_of(Path::new("x.mp4"), None, &[]), Some(Kind::Video));
    assert_eq!(kind_of(Path::new("x.m4a"), None, &[]), Some(Kind::Audio));
    assert_eq!(kind_of(Path::new("x.gif"), None, &[]), None);
    assert_eq!(kind_of(Path::new("x.GIF"), Some("image/png"), &[]), None);
    assert_eq!(kind_of(Path::new("x.png"), Some("image/gif"), &[]), None);
    assert_eq!(
        kind_of(Path::new("x.bin"), Some("image/png"), &[]),
        Some(Kind::Image)
    );
    assert_eq!(kind_of(Path::new("x.bin"), Some("  "), &[]), None);
}

#[test]
fn a_gif_declared_in_capitals_or_with_parameters_is_not_converted() {
    for mime in ["image/GIF", "image/gif; charset=binary", " Image/Gif "] {
        assert_eq!(
            kind_of(Path::new("blob"), Some(mime), &[]),
            None,
            "{mime} names a GIF, and a GIF is never converted"
        );
    }
}

const SHA: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

#[test]
fn kind_of_falls_back_to_the_attachment_names_for_an_extensionless_asset() {
    let canonical = PathBuf::from(format!("ab/{SHA}"));
    for (name, expected) in [
        ("voice-note.amr", Some(Kind::Audio)),
        ("memo.wav", Some(Kind::Audio)),
        ("podcast.ogg", Some(Kind::Audio)),
        ("clip.3gp", Some(Kind::Video)),
        ("clip.webm", Some(Kind::Video)),
        ("movie.mkv", Some(Kind::Video)),
        ("scan.tiff", Some(Kind::Image)),
        ("notes.txt", None),
    ] {
        assert_eq!(
            kind_of(&canonical, None, &[Some(name), None]),
            expected,
            "unexpected kind for {name}"
        );
        // The original export path is the second-choice hint.
        assert_eq!(
            kind_of(
                &canonical,
                Some("  "),
                &[None, Some(&format!("media/{name}"))]
            ),
            expected,
            "unexpected kind for path hint media/{name}"
        );
    }
}

#[test]
fn kind_of_never_lets_a_name_hint_override_the_declared_media_type() {
    let canonical = PathBuf::from(format!("ab/{SHA}"));
    // A declared MIME is authoritative, including the deliberate GIF skip.
    assert_eq!(
        kind_of(&canonical, Some("image/gif"), &[Some("clip.mp4")]),
        None
    );
    assert_eq!(
        kind_of(&canonical, Some("application/pdf"), &[Some("clip.mp4")]),
        None
    );
    assert_eq!(
        kind_of(Path::new("ab/photo.gif"), None, &[Some("clip.mp4")]),
        None
    );
    // A GIF hint is passed over for the next hint, not treated as an image.
    assert_eq!(
        kind_of(&canonical, None, &[Some("still.gif"), Some("clip.mp4")]),
        Some(Kind::Video)
    );
}

#[test]
fn convert_never_overwrites_a_file_that_already_has_the_target_name() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    // a.png beside a.jpg, c.png beside c.jpg and c_1.jpg, and a plain b.png.
    for name in ["a.png", "b.png", "c.png"] {
        fs::write(attachments.join(name), PNG_1X1_RGB).unwrap();
    }
    for name in ["a.jpg", "c.jpg", "c_1.jpg"] {
        fs::write(attachments.join(name), format!("the user's own {name}")).unwrap();
    }
    let files: Vec<PathBuf> = ["a.png", "b.png", "c.png"]
        .iter()
        .map(|name| attachments.join(name))
        .collect();
    let tools = FakeTools::new().writing("a converted jpeg");

    let (report, remap) = process_attachment_files(
        &tools,
        dir.path(),
        &files,
        MediaMode::Convert,
        &CompressOptions::default(),
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.errors, Vec::<String>::new());
    assert_eq!(report.processed, 3);
    assert_eq!(report.skipped, 0);
    let expected: HashMap<String, String> = [
        ("attachments/a.png", "attachments/a_1.jpg"),
        ("attachments/b.png", "attachments/b.jpg"),
        ("attachments/c.png", "attachments/c_2.jpg"),
    ]
    .into_iter()
    .map(|(from, to)| (from.to_string(), to.to_string()))
    .collect();
    assert_eq!(remap, expected);
    for name in ["a.jpg", "c.jpg", "c_1.jpg"] {
        assert_eq!(
            fs::read_to_string(attachments.join(name)).unwrap(),
            format!("the user's own {name}"),
            "{name} was overwritten"
        );
    }
    for name in ["a_1.jpg", "b.jpg", "c_2.jpg"] {
        assert_eq!(
            fs::read_to_string(attachments.join(name)).unwrap(),
            "a converted jpeg",
            "{name} is not the conversion"
        );
    }
    for name in ["a.png", "b.png", "c.png"] {
        assert!(!attachments.join(name).exists(), "{name} was left behind");
    }
}

/// The fake accepts any arguments, so this runs the image compress
/// arguments through the real ffmpeg to show it accepts them.
#[test]
fn compress_turns_a_png_into_a_jpeg_with_the_real_ffmpeg() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("still.png");
    fs::write(&src, PNG_1X1_RGB).unwrap();
    let dest = dir.path().join("still.jpg.in_progress");

    let outcome = transcode_file(
        &tools,
        &src,
        &dest,
        MediaMode::Compress,
        &CompressOptions::default(),
        None,
    )
    .unwrap();

    assert_eq!(outcome, TranscodeOutcome::Produced { hevc_failure: None });
    assert!(
        fs::read(&dest).unwrap().starts_with(&[0xff, 0xd8]),
        "the derivative is not a JPEG"
    );
}

#[test]
fn compress_shrinks_large_jpegs_converts_pngs_and_leaves_gifs_and_small_jpegs() {
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let large = attachments.join("large.jpg");
    fs::write(&large, vec![0xa5; JPEG_COMPRESS_FLOOR as usize + 1]).unwrap();
    let png = attachments.join("still.png");
    fs::write(&png, PNG_1X1_RGB).unwrap();
    // Neither of these reaches ffmpeg.
    let gif = attachments.join("loop.gif");
    fs::write(&gif, b"GIF89a animated").unwrap();
    let small = attachments.join("small.jpg");
    fs::write(&small, b"\xff\xd8\xff small jpeg").unwrap();
    let files = vec![large.clone(), png, gif.clone(), small.clone()];
    let tools = FakeTools::new().writing("a smaller jpeg");

    let (report, remap) = process_attachment_files(
        &tools,
        dir.path(),
        &files,
        MediaMode::Compress,
        &CompressOptions::default(),
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.errors, Vec::<String>::new());
    assert_eq!((report.processed, report.skipped), (2, 2));
    assert_eq!(
        tools.runs().len(),
        2,
        "ffmpeg ran for a GIF or a small JPEG"
    );
    assert_eq!(
        fs::read(&large).unwrap(),
        b"a smaller jpeg",
        "the large JPEG was not replaced by the smaller re-encode"
    );
    assert_eq!(
        remap.get("attachments/large.jpg").map(String::as_str),
        Some("attachments/large.jpg")
    );
    assert_eq!(
        remap.get("attachments/still.png").map(String::as_str),
        Some("attachments/still.jpg")
    );
    assert_eq!(fs::read(&gif).unwrap(), b"GIF89a animated");
    assert_eq!(fs::read(&small).unwrap(), b"\xff\xd8\xff small jpeg");
    assert_eq!(remap.len(), 2);
}

/// Staging forecasts output names from `derivative_name`, and recovers after
/// a crash by looking for them. It must answer as the pass itself decides.
#[test]
fn derivative_name_follows_the_media_step_for_every_mode_and_extension() {
    let dir = tempfile::tempdir().unwrap();
    let sized = |name: &str, len: u64| {
        let path = dir.path().join(name);
        fs::File::create(&path).unwrap().set_len(len).unwrap();
        path
    };
    let small = 10;
    let jpeg_floor = JPEG_COMPRESS_FLOOR;
    let mp3_floor = MP3_COMPRESS_FLOOR;
    let cases: &[(&str, u64, MediaMode, Option<&str>)] = &[
        ("x.png", small, MediaMode::Clone, None),
        ("x.png", small, MediaMode::Disabled, None),
        ("x.png", small, MediaMode::Convert, Some("x.jpg")),
        ("x.heic", small, MediaMode::Convert, Some("x.jpg")),
        ("x.gif", small, MediaMode::Convert, None),
        ("x.jpg", small, MediaMode::Convert, None),
        ("x.JPEG", small, MediaMode::Convert, None),
        ("x.m4a", small, MediaMode::Convert, Some("x.mp3")),
        ("x.mp3", small, MediaMode::Convert, None),
        ("x.mov", small, MediaMode::Convert, Some("x.mp4")),
        ("x.pdf", small, MediaMode::Convert, None),
        ("x.png", small, MediaMode::Compress, Some("x.jpg")),
        ("x.gif", jpeg_floor + 1, MediaMode::Compress, None),
        ("x.jpg", small, MediaMode::Compress, None),
        ("x.jpg", jpeg_floor, MediaMode::Compress, None),
        ("x.jpg", jpeg_floor + 1, MediaMode::Compress, Some("x.jpg")),
        ("x.jpeg", jpeg_floor + 1, MediaMode::Compress, Some("x.jpg")),
        ("x.m4a", small, MediaMode::Compress, Some("x.mp3")),
        ("x.mp3", mp3_floor, MediaMode::Compress, None),
        ("x.mp3", mp3_floor + 1, MediaMode::Compress, Some("x.mp3")),
        ("x.mp4", small, MediaMode::Compress, Some("x.mp4")),
    ];
    for (name, len, mode, expected) in cases {
        let path = sized(name, *len);
        assert_eq!(
            derivative_name(&path, *mode).as_deref(),
            *expected,
            "{name} ({len} bytes) in {mode:?}"
        );
    }
}

#[test]
fn derivative_name_for_missing_ignores_the_size_floors() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("gone.jpg");
    let mp3 = dir.path().join("gone.mp3");
    assert_eq!(
        derivative_name_for_missing(&jpg, MediaMode::Compress).as_deref(),
        Some("gone.jpg")
    );
    assert_eq!(
        derivative_name_for_missing(&mp3, MediaMode::Compress).as_deref(),
        Some("gone.mp3")
    );
    assert_eq!(derivative_name_for_missing(&jpg, MediaMode::Convert), None);
    // The stat-reading variant sees a missing file as size 0, under the floor.
    assert_eq!(derivative_name(&jpg, MediaMode::Compress), None);
}

#[test]
fn audio_becomes_mp3_and_a_small_mp3_is_left_alone() {
    for mode in [MediaMode::Convert, MediaMode::Compress] {
        let dir = tempfile::tempdir().unwrap();
        let attachments = dir.path().join("attachments");
        fs::create_dir_all(&attachments).unwrap();
        let m4a = attachments.join("voice.m4a");
        fs::write(&m4a, b"an m4a voice note").unwrap();
        // Under the MP3 floor: ffmpeg must never see it.
        let mp3 = attachments.join("small.mp3");
        fs::write(&mp3, b"ID3 small mp3").unwrap();
        let tools = FakeTools::new().writing("the mp3");

        let (report, remap) = process_attachment_files(
            &tools,
            dir.path(),
            &[m4a.clone(), mp3.clone()],
            mode,
            &CompressOptions::default(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(report.errors, Vec::<String>::new(), "{mode:?}");
        assert_eq!((report.processed, report.skipped), (1, 1), "{mode:?}");
        assert_eq!(
            remap.get("attachments/voice.m4a").map(String::as_str),
            Some("attachments/voice.mp3"),
            "{mode:?}"
        );
        assert!(has(&only_run(&tools), "libmp3lame"), "{mode:?}");
        assert!(!m4a.exists(), "{mode:?}");
        assert_eq!(fs::read(&mp3).unwrap(), b"ID3 small mp3", "{mode:?}");
    }
}

#[test]
fn convert_turns_an_m4a_into_an_mp3() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let m4a = attachments.join("voice.m4a");
    tools
        .run_ffmpeg(&[
            "-y".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "sine=frequency=440:duration=1".into(),
            "-c:a".into(),
            "aac".into(),
            path_str(&m4a),
        ])
        .expect("generate m4a fixture");

    let new_rel = process_single(
        &tools,
        dir.path(),
        &m4a,
        MediaMode::Convert,
        &CompressOptions::default(),
    );

    assert_eq!(new_rel.as_deref(), Some("attachments/voice.mp3"));
    assert!(!m4a.exists());
    assert!(fs::metadata(attachments.join("voice.mp3")).unwrap().len() > 0);
}

#[test]
fn collect_media_files_keeps_media_and_leaves_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("attachments").join("a1");
    fs::create_dir_all(&nested).unwrap();
    fs::write(dir.path().join("photo.png"), b"png").unwrap();
    fs::write(dir.path().join("receipt.pdf"), b"pdf").unwrap();
    fs::write(nested.join("clip.mp4"), b"mp4").unwrap();
    fs::write(nested.join("clip.msgmedia.tmp.mp4"), b"partial").unwrap();

    let files = collect_media_files(dir.path()).unwrap();

    assert_eq!(
        files,
        vec![nested.join("clip.mp4"), dir.path().join("photo.png")],
        "only media, found in subdirectories too, sorted; never a PDF or an \
         ffmpeg temp file"
    );
}

/// A symlink under the attachments directory is never followed: a linked
/// directory elsewhere is not listed for conversion, not swept of temp files,
/// and not counted, and a link back to a parent does not loop (#2262).
#[cfg(unix)]
#[test]
fn the_attachment_walks_never_follow_a_symlink() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    fs::write(att.join("photo.png"), vec![0u8; 100]).unwrap();

    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("clip.mp4"), vec![0u8; 5000]).unwrap();
    let foreign_temp = elsewhere.join("clip.msgmedia.tmp.mp4");
    fs::write(&foreign_temp, b"not ours").unwrap();

    symlink(&elsewhere, att.join("linked")).unwrap();
    symlink(elsewhere.join("clip.mp4"), att.join("linked-clip.mp4")).unwrap();
    symlink(&att, att.join("loop")).unwrap();

    assert_eq!(
        collect_media_files(&att).unwrap(),
        vec![att.join("photo.png")],
        "only the real file is listed; nothing through a link"
    );

    remove_msgmedia_temps(&att).unwrap();
    assert!(
        foreign_temp.exists(),
        "a temp-named file in a linked directory is not ours to delete"
    );

    assert_eq!(attachments_dir_bytes(&att).unwrap(), 100);
}

/// A socket named like media is not listed for conversion: only regular
/// files are, because a conversion that opened a socket or pipe could block
/// on it.
#[cfg(unix)]
#[test]
fn the_attachment_walks_list_only_regular_files() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    fs::write(att.join("photo.png"), b"png").unwrap();
    let _socket = std::os::unix::net::UnixListener::bind(att.join("socket.mp4")).unwrap();

    assert_eq!(
        collect_media_files(&att).unwrap(),
        vec![att.join("photo.png")]
    );
}

/// The line that starts a pass words its file count singular for one, so a
/// single attachment reads `1 file` (#1815).
#[test]
fn the_starting_line_counts_one_file_and_many_files() {
    assert_eq!(
        starting_line("Converting", 1, 500),
        "Converting attachments (1 file, 500 B)…"
    );
    assert_eq!(
        starting_line("Compressing", 3, 2_000),
        "Compressing attachments (3 files, 2.0 KB)…"
    );
}

/// The line that ends a pass words each count singular for one, and names
/// the files that failed only when one did (#1825).
#[test]
fn the_done_line_counts_one_file_and_many_files() {
    let one = MediaReport {
        processed: 1,
        skipped: 1,
        bytes_before: 2_000,
        bytes_after: 500,
        errors: vec!["a.heic: ffmpeg failed".into()],
        ..MediaReport::default()
    };
    assert_eq!(
        done_line_with_failures(MediaMode::Convert, &one),
        "Attachment convert done: processed 1 file, skipped 1 file, size 2.0 KB → 500 B, \
         1 file failed"
    );
    let many = MediaReport {
        processed: 3,
        skipped: 0,
        bytes_before: 2_000,
        bytes_after: 2_000,
        errors: Vec::new(),
        ..MediaReport::default()
    };
    assert_eq!(
        done_line_with_failures(MediaMode::Compress, &many),
        "Attachment compress done: processed 3 files, skipped 0 files, size 2.0 KB → 2.0 KB"
    );
}

/// The no-progress wait the fallback tests give.
const WAIT: std::time::Duration = std::time::Duration::from_secs(120);

/// ffmpeg as a mock program whose HEVC run follows a row's script and whose
/// H.264 run writes its output and succeeds, watched as the real one is.
/// Every run appends its arguments to `runs`. The HEVC run's clock starts
/// once the script marks itself `started` ([`clock_from`]) when `stalls`,
/// and never moves otherwise; the H.264 run's clock never moves.
#[derive(Debug)]
struct ScriptedTools {
    dir: tempfile::TempDir,
    stalls: bool,
    tools: InstalledTools,
}

/// How long the scripted source plays.
const SCRIPTED_LENGTH: std::time::Duration = std::time::Duration::from_secs(5);

impl ScriptedTools {
    /// Tools whose HEVC run runs `hevc`, a shell script body.
    fn new(hevc: &str, stalls: bool) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        fs::write(
            &ffmpeg,
            format!(
                "#!/bin/sh\n[ \"$1\" = -version ] && exit 0\n\
                 dir=\"$(dirname \"$0\")\"\necho \"$*\" >> \"$dir/runs\"\n\
                 case \"$*\" in *libx265*)\n{hevc}\n;; esac\n\
                 for a; do last=$a; done\necho h264 > \"$last\"\nexit 0\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&ffmpeg, fs::Permissions::from_mode(0o755)).unwrap();
        fs::copy(&ffmpeg, dir.path().join("ffprobe")).unwrap();
        let tools = InstalledTools::new(Some(dir.path().to_path_buf())).searching("");
        Self { dir, stalls, tools }
    }

    /// The arguments of every ffmpeg run so far, one line each.
    fn runs(&self) -> Vec<String> {
        fs::read_to_string(self.dir.path().join("runs"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Whether a run went on to H.264 after HEVC.
    fn tried_h264(&self) -> bool {
        let runs = self.runs();
        assert!(runs[0].contains("libx265"), "HEVC runs first: {runs:?}");
        runs.iter().skip(1).any(|run| run.contains("libx264"))
    }
}

impl MediaTools for ScriptedTools {
    fn find(&self) -> Result<crate::FfmpegTools> {
        self.tools.find()
    }

    fn require(&self) -> Result<()> {
        Ok(())
    }

    fn can_probe(&self) -> bool {
        true
    }

    fn probe(&self, _path: &Path) -> Result<MediaProbe> {
        Ok(video_probe("h264", 30.0))
    }

    fn transcode(
        &self,
        args: &[String],
        dest: &Path,
        reads: Reads<'_>,
        stop: Option<&AtomicBool>,
    ) -> Result<()> {
        let limit = match reads {
            Reads::OneFrame => Limit::ShortStep,
            Reads::All(_) => Limit::Progress(StallWatch::new(WAIT, Some(SCRIPTED_LENGTH))),
        };
        let line = ffmpeg_command_line(args, dest);
        let started = self.dir.path().join("started");
        if self.stalls && has(args, "libx265") {
            self.tools
                .run_ffmpeg_watched(&line, stop, limit, &clock_from(&started))
        } else {
            self.tools
                .run_ffmpeg_watched(&line, stop, limit, &|| std::time::Duration::ZERO)
        }
    }
}

/// A HEVC script that marks itself started and never exits.
const HANG: &str = "touch \"$dir/started\"\nexec sleep 600";

/// Compress the first of [`two_large_videos`] with `tools`.
fn compress_scripted(tools: &ScriptedTools) -> MediaReport {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    process_attachment_files(
        tools,
        dir.path(),
        &files[..1],
        MediaMode::Compress,
        &opts,
        None,
        None,
    )
    .unwrap()
    .0
}

/// A HEVC run that ffmpeg ended with an error, as when libx265 is missing
/// or refused the file, is followed by H.264 (#2898).
#[cfg(unix)]
#[test]
fn compress_tries_h264_after_hevc_failed() {
    let tools = ScriptedTools::new("echo 'Unknown encoder libx265' >&2\nexit 1", false);

    let report = compress_scripted(&tools);

    assert!(tools.tried_h264(), "{:?}", tools.runs());
    assert_eq!(report.processed, 1, "{:?}", report.errors);
    let line = report.hevc_fallback.line().expect("a fallback reason");
    assert!(line.contains("Unknown encoder libx265"), "{line}");
}

/// A HEVC run that writes no progress and never exits most likely hung in
/// the encoder, as a hardware encoder can: it is stopped once the wait has
/// passed, and H.264 is tried, with the fallback naming the stall (#2898).
#[cfg(unix)]
#[test]
fn compress_tries_h264_after_hevc_stalled_before_any_output() {
    let tools = ScriptedTools::new(HANG, true);

    let report = compress_scripted(&tools);

    assert!(tools.tried_h264(), "{:?}", tools.runs());
    assert_eq!(report.processed, 1, "{:?}", report.errors);
    let line = report.hevc_fallback.line().expect("a fallback reason");
    assert!(
        line.contains("made no progress for 2 minutes before converting any of the file"),
        "{line}"
    );
}

/// A HEVC run whose position moves and then stops most likely reached a
/// damaged part of the file, where H.264 would stall too: the file fails,
/// naming the stall, and H.264 is not tried (#2898).
#[cfg(unix)]
#[test]
fn compress_does_not_try_h264_after_hevc_stalled_partway() {
    let tools = ScriptedTools::new(
        &progress_then_hang("out_time=00:00:02.000000\\nprogress=continue\\n"),
        true,
    );

    let report = compress_scripted(&tools);

    assert!(!tools.tried_h264(), "{:?}", tools.runs());
    assert_eq!(report.processed, 0);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(
        report.errors[0].contains("made no progress for 2 minutes after converting 0:02"),
        "{:?}",
        report.errors
    );
    assert_eq!(report.hevc_fallback.line(), None);
}

/// A HEVC run whose position passes twice the file's length is a damaged
/// file looping, which H.264 would loop on too: the file fails, naming why,
/// and H.264 is not tried. The clock never moves, so only the position ends
/// the run (#2898).
#[cfg(unix)]
#[test]
fn compress_does_not_try_h264_after_hevc_ran_past_its_length() {
    let tools = ScriptedTools::new(
        "printf 'out_time=00:00:11.000000\\nprogress=continue\\n'\nexec sleep 600",
        false,
    );

    let report = compress_scripted(&tools);

    assert!(!tools.tried_h264(), "{:?}", tools.runs());
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(
        report.errors[0].contains("ran past twice the file's length of 0:05"),
        "{:?}",
        report.errors
    );
}

/// A HEVC run the run's stop ends while it runs is killed, is not followed
/// by H.264, and fails as stopped, not as a failed file (#2898).
#[cfg(unix)]
#[test]
fn compress_does_not_try_h264_after_hevc_was_stopped() {
    let tools = ScriptedTools::new(HANG, false);
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let dest = dir.path().join("out.mp4");
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stopper = {
        let stop = std::sync::Arc::clone(&stop);
        let started = tools.dir.path().join("started");
        std::thread::spawn(move || {
            while !started.exists() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            stop.store(true, Ordering::Relaxed);
        })
    };

    let err = transcode_file(
        &tools,
        &files[0],
        &dest,
        MediaMode::Compress,
        &opts,
        Some(&stop),
    )
    .expect_err("a stopped compression fails");
    stopper.join().unwrap();

    assert_eq!(
        err.downcast_ref::<Killed>().map(Killed::reason),
        Some(KillReason::Stopped),
        "{err:#}"
    );
    assert!(!tools.tried_h264(), "{:?}", tools.runs());
}

/// The run's stop reaches the HEVC run: a compression asked for once it is
/// set starts no ffmpeg, HEVC or H.264, and fails as stopped (#2898).
#[test]
fn a_stopped_compression_starts_neither_hevc_nor_h264() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let dest = dir.path().join("out.mp4");
    let tools = FakeTools::new().probing(video_probe("h264", 30.0));
    let stop = AtomicBool::new(true);

    let err = transcode_file(
        &tools,
        &files[0],
        &dest,
        MediaMode::Compress,
        &opts,
        Some(&stop),
    )
    .expect_err("a stopped compression fails");

    assert!(
        err.downcast_ref::<Killed>().map(Killed::reason) == Some(KillReason::StoppedBeforeStart),
        "{err:#}"
    );
    assert_eq!(tools.runs(), Vec::<Vec<String>>::new());
}

/// A pass whose stop is set fails as stopped rather than recording the file
/// as one it could not convert, and converts no other file.
#[test]
fn a_stopped_pass_fails_as_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let tools = FakeTools::new().probing(video_probe("h264", 30.0));
    let stop = AtomicBool::new(true);

    let err = process_attachment_files(
        &tools,
        dir.path(),
        &files,
        MediaMode::Compress,
        &opts,
        None,
        Some(&stop),
    )
    .expect_err("a stopped pass fails");

    assert!(
        err.downcast_ref::<Killed>().map(Killed::reason) == Some(KillReason::Stopped),
        "{err:#}"
    );
    assert_eq!(tools.runs(), Vec::<Vec<String>>::new());
}

/// Whether H.264 follows a failed HEVC run, for each way the run can end
/// (#2898): an error and a stall before any output, yes; a stall partway,
/// a run past twice the length, a stop and a short step over its limit, no.
#[test]
fn h264_follows_hevc_only_after_a_failure_or_a_stall_before_output() {
    let killed = |why| anyhow::Error::from(Killed::ffmpeg(why)).context("compress video");
    assert!(h264_follows(&anyhow::anyhow!(
        "ffmpeg failed (exit status: 1)"
    )));
    assert!(h264_follows(&killed(KillReason::StalledBeforeOutput {
        wait: WAIT
    })));
    assert!(!h264_follows(&killed(KillReason::StalledPartway {
        wait: WAIT,
        at: std::time::Duration::from_secs(1),
    })));
    assert!(!h264_follows(&killed(KillReason::RanPastLength {
        length: std::time::Duration::from_secs(1),
    })));
    assert!(!h264_follows(&killed(KillReason::Stopped)));
    assert!(!h264_follows(&killed(KillReason::StoppedBeforeStart)));
    assert!(!h264_follows(&killed(KillReason::ShortStepTooLong {
        limit: std::time::Duration::from_secs(15),
    })));
}
