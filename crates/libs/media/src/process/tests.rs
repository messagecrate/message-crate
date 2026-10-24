use super::*;
use std::sync::atomic::Ordering;

use crate::MediaProbe;
use crate::fake_tools::{FakeTools, video_probe};
use crate::kind_for_mime;
use crate::testutil::PNG_1X1_RGB;
use crate::testutil::{clock_from, progress_then_hang};
use crate::tools::{FfmpegFailed, InstalledTools, Limit, MediaTools, Reads, ffmpeg_command_line};

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
    // Over the floor, so the size gate does not skip it before ffmpeg runs.
    let jpeg = dir.path().join("already-tight.jpg");
    let before = vec![0xa5; JPEG_COMPRESS_FLOOR as usize + 1];
    fs::write(&jpeg, &before).unwrap();
    // The re-encode comes out the same size: not smaller, so not kept.
    let tools = FakeTools::new().writing(vec![0; before.len()]);

    let produced = transcode_single(
        &tools,
        &jpeg,
        MediaMode::Compress,
        &CompressOptions::default(),
    );

    assert_eq!(
        tools.runs().len(),
        1,
        "the JPEG must reach ffmpeg: otherwise every assertion below holds \
         whether or not the keep-smaller guard exists"
    );
    assert_eq!(
        produced, None,
        "a re-encode that is not smaller is not kept"
    );
    assert_eq!(fs::read(&jpeg).unwrap(), before, "original bytes replaced");
    assert!(
        !dir.path().join("already-tight.msgmedia.tmp.jpg").exists(),
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

    let document = dir.path().join("notes.pdf");
    fs::write(&document, b"%PDF").unwrap();
    assert_eq!(derivative_name(&document, MediaMode::Convert), None);
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
    let err = commit_produced(&original, &original, &produced).unwrap_err();
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
fn clone_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.png");
    fs::write(&src, PNG_1X1_RGB).unwrap();
    let tools = FakeTools::new();

    assert_eq!(
        transcode_single(&tools, &src, MediaMode::Clone, &CompressOptions::default()),
        None
    );
    assert_eq!(tools.runs(), Vec::<Vec<String>>::new());
}

#[test]
fn format_bytes_scales() {
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(12_500), "12.5 KB");
    assert_eq!(format_bytes(1_500_000), "1.5 MB");
    assert_eq!(format_bytes(2_500_000_000), "2.5 GB");
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
    assert_eq!(frame_rate_cap(None, Some(30)), Some(30));
    assert_eq!(frame_rate_cap(Some(60.0), Some(30)), Some(30));
    assert_eq!(frame_rate_cap(Some(30.0), Some(30)), None);
    assert_eq!(frame_rate_cap(Some(24.0), Some(30)), None);
}

/// #3167: an empty Max FPS keeps each video's frame rate, so with no
/// maximum no video is capped, even one whose rate ffprobe could not read.
#[test]
fn frame_rate_cap_leaves_the_frame_rate_alone_with_no_maximum() {
    assert_eq!(frame_rate_cap(Some(60.0), None), None);
    assert_eq!(frame_rate_cap(Some(240.0), None), None);
    assert_eq!(frame_rate_cap(None, None), None);
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

/// One file through [`transcode_file`] with `tools`, written beside it as
/// `<stem>.in_progress`: the derivative's path, or `None` when skipped.
fn transcode_single(
    tools: &dyn MediaTools,
    file: &Path,
    mode: MediaMode,
    opts: &CompressOptions,
) -> Option<PathBuf> {
    let dest = file.with_extension("in_progress");
    match transcode_file(tools, file, &dest, mode, opts, None).unwrap() {
        TranscodeOutcome::Produced { .. } => {
            assert!(dest.is_file(), "a produced derivative is at {dest:?}");
            Some(dest)
        }
        TranscodeOutcome::Skipped => {
            assert!(!dest.exists(), "a skipped file wrote {dest:?}");
            None
        }
    }
}

#[test]
fn convert_remuxes_a_mov_into_an_mp4() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mov = dir.path().join("clip.mov");
    write_test_video(&mov, &["-c:v", "libx264"]);

    let produced = transcode_single(
        &tools,
        &mov,
        MediaMode::Convert,
        &CompressOptions::default(),
    )
    .expect("a mov is converted");

    let probe = tools.probe(&produced).unwrap();
    assert_eq!(probe.codec, "h264", "a remux keeps the stream");
}

#[test]
fn compress_only_remuxes_a_video_under_the_minimum_size() {
    let dir = tempfile::tempdir().unwrap();
    let mov = dir.path().join("small.mov");
    fs::write(&mov, b"a small mov").unwrap();
    let mp4 = dir.path().join("other.mp4");
    fs::write(&mp4, b"a small mp4").unwrap();
    let opts = CompressOptions {
        min_size_bytes: Some(100 * 1024 * 1024),
        ..CompressOptions::default()
    };
    let tools = FakeTools::new().writing("the remuxed mp4");

    let produced =
        transcode_single(&tools, &mov, MediaMode::Compress, &opts).expect("a small mov is remuxed");
    assert_eq!(fs::read(&produced).unwrap(), b"the remuxed mp4");
    let remux = only_run(&tools);
    assert!(
        has(&remux, "copy") && !has(&remux, "libx265") && !has(&remux, "libx264"),
        "a small video is remuxed, not re-encoded: {remux:?}"
    );

    // A small MP4 is already in the target container: nothing to do.
    assert_eq!(
        transcode_single(&tools, &mp4, MediaMode::Compress, &opts),
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
    let avc = dir.path().join("avc.mp4");
    write_test_video(&avc, &["-c:v", "libx264"]);
    let opts = CompressOptions {
        min_size_bytes: None,
        max_fps: Some(5),
        ..CompressOptions::default()
    };

    let produced = transcode_single(&tools, &avc, MediaMode::Compress, &opts)
        .expect("a large H.264 video is re-encoded");

    let probe = tools.probe(&produced).unwrap();
    assert_eq!(probe.codec, "hevc", "a large H.264 video is re-encoded");
    assert_eq!(probe.fps, Some(5.0), "the frame rate is capped at max_fps");
}

/// What `compress_video` reads from the probe: an efficient HEVC video is
/// left alone unless the skip is turned off, and a file of exactly the
/// minimum size is large enough to re-encode.
#[test]
fn compress_skips_an_efficient_video_unless_told_not_to() {
    let dir = tempfile::tempdir().unwrap();
    let hevc = dir.path().join("hevc.mp4");
    fs::write(&hevc, b"a small, low-bitrate hevc video").unwrap();
    let efficient = video_probe("hevc", 10.0);
    let opts = CompressOptions {
        min_size_bytes: Some(fs::metadata(&hevc).unwrap().len()),
        ..CompressOptions::default()
    };
    let tools = FakeTools::new()
        .probing(efficient.clone())
        .writing("the re-encode");

    // Already HEVC, small and low bitrate: re-encoding would buy nothing.
    assert_eq!(
        transcode_single(&tools, &hevc, MediaMode::Compress, &opts),
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
    let produced = transcode_single(&tools, &hevc, MediaMode::Compress, &opts)
        .expect("a video over the resolution cap is re-encoded");
    fs::remove_file(produced).unwrap();

    // With the skip turned off, the efficient file is re-encoded too. With
    // no max fps the video keeps its frame rate, so the filter adds no fps
    // step.
    let tools = FakeTools::new().probing(efficient).writing("the re-encode");
    let opts = CompressOptions {
        skip_efficient: false,
        max_fps: None,
        ..opts
    };
    let produced = transcode_single(&tools, &hevc, MediaMode::Compress, &opts)
        .expect("with the skip off, an efficient video is re-encoded");
    assert_eq!(fs::read(&produced).unwrap(), b"the re-encode");
    let run = only_run(&tools);
    assert!(has(&run, "libx265"), "{run:?}");
    assert!(!filter(&run).contains("fps="), "{run:?}");
}

/// A video over the maximum frame rate is brought down to it, and libx264
/// stands in when libx265 fails.
#[test]
fn compress_caps_the_frame_rate_and_falls_back_to_h264() {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip.mov");
    fs::write(&clip, b"a 60 fps h264 video").unwrap();
    let tools = FakeTools::new()
        .probing(video_probe("h264", 60.0))
        .failing_on("libx265")
        .writing("the h264 re-encode");
    let opts = CompressOptions {
        min_size_bytes: None,
        max_fps: Some(30),
        ..CompressOptions::default()
    };

    let produced = transcode_single(&tools, &clip, MediaMode::Compress, &opts)
        .expect("H.264 stands in for HEVC");

    assert_eq!(fs::read(&produced).unwrap(), b"the h264 re-encode");
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
        min_size_bytes: None,
        ..CompressOptions::default()
    };
    (files, opts)
}

/// Why HEVC failed names no file: ffmpeg's error names the video it read
/// and the file it wrote by their full paths, and the server logs the
/// reason, which may hold no file name (#2802).
#[test]
fn why_hevc_failed_names_no_file() {
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
    let hevc_run = &tools.runs()[0];
    assert!(
        hevc_run.iter().any(|arg| arg.ends_with("one.mov")),
        "{hevc_run:?}"
    );
    for name in ["one.mov", "one.msgmedia", dir.path().to_str().unwrap()] {
        assert!(!reason.contains(name), "{name:?} survives in {reason:?}");
    }
    // All of what ffmpeg wrote, not only its last line, because the cause
    // can come before it.
    assert!(
        reason.starts_with("ffmpeg failed (")
            && reason.ends_with(
                "): Input #0, from '<input>':\nOutput #0, to '<output>':\n\
                 libx265: fake failure on '<input>'"
            ),
        "{reason:?}"
    );
}

/// A file ffmpeg could not convert is recorded with the line ffmpeg wrote
/// that names the cause, which here is its last line, with no path in it,
/// while the whole error keeps both failures for the log (#2801).
#[test]
fn a_file_that_fails_records_ffmpegs_last_line_without_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let dest = dir.path().join("out.mp4");
    let tools = FakeTools::new()
        .probing(video_probe("h264", 30.0))
        .failing_on("libx265")
        .failing_on("libx264");

    let err = transcode_file(&tools, &files[0], &dest, MediaMode::Compress, &opts, None)
        .expect_err("both encoders fail");

    assert_eq!(
        why_not_converted(&err, &files[0]),
        "libx264: fake failure on '<input>'"
    );
    let logged = format!("{err:#}");
    assert!(
        logged.contains(&files[0].display().to_string()) && logged.contains("HEVC failed"),
        "the log keeps the whole error: {logged}"
    );
}

/// What a stopped run says is its reason as it stands, and neither an
/// ffmpeg that wrote nothing nor an error from outside ffmpeg lets the
/// contexts around it, which name the file by its path, onto the attachment.
#[test]
fn why_not_converted_names_no_path_however_the_conversion_failed() {
    let file = Path::new("/home/someone/attachments/IMG_0001.heic");
    let killed = crate::testutil::ffmpeg_killed(KillReason::RanPastLength {
        length: std::time::Duration::from_secs(5),
    });
    assert_eq!(why_not_converted(&killed, file), killed.to_string());

    // Killed by a signal, say, with nothing on stderr.
    let silent = anyhow::Error::from(FfmpegFailed::new(
        std::process::ExitStatus::default(),
        b"",
        &[
            "-i".to_string(),
            file.display().to_string(),
            "/tmp/out.jpg".to_string(),
        ],
    ))
    .context(format!("convert image {}", file.display()));
    let reason = why_not_converted(&silent, file);
    assert!(reason.starts_with("ffmpeg failed ("), "{reason:?}");
    assert!(!reason.contains("IMG_0001"), "{reason:?}");

    // A step that names the temporary file beside it is left out; the
    // steps that name no other file stay, so the reason still says what
    // went wrong where.
    let io = anyhow::Error::from(std::io::Error::other("no room left\nsecond line"))
        .context("/home/someone/attachments/IMG_0001.msgmedia.tmp.jpg: write the copy")
        .context("start ffmpeg")
        .context(format!("convert image {}", file.display()));
    assert_eq!(
        why_not_converted(&io, file),
        "convert image <input>: start ffmpeg: no room left"
    );

    let named = anyhow::anyhow!("ffprobe reported no stream for {}", file.display())
        .context(format!("compress video {}", file.display()));
    assert_eq!(
        why_not_converted(&named, file),
        "compress video <input>: ffprobe reported no stream for <input>"
    );
}

/// What ffmpeg wrote when it failed, run with `-i <file> /tmp/out.mp4`.
fn ffmpeg_failed_saying(file: &Path, said: &str) -> anyhow::Error {
    anyhow::Error::from(FfmpegFailed::new(
        std::process::ExitStatus::default(),
        said.as_bytes(),
        &[
            "-i".to_string(),
            file.display().to_string(),
            "/tmp/out.mp4".to_string(),
        ],
    ))
}

/// ffmpeg 8 writes why it failed and then what followed from it, ending
/// with lines such as the output muxer's that nothing was written. The
/// reason is the line that names the cause (#2959). Each stderr is what
/// ffmpeg 8.0.1 wrote at `-loglevel error`.
#[test]
fn why_not_converted_names_the_cause_not_the_last_line() {
    let file = Path::new("/home/someone/attachments/clip.mov");
    let reason = |said: &str| why_not_converted(&ffmpeg_failed_saying(file, said), file);

    // An encoder that would not open.
    assert_eq!(
        reason(
            "[libx265 @ 0x5e6836f4af00] Error setting preset/tune nonsense/(null).\n\
             [vost#0:0/libx265 @ 0x5e6836f4a8c0] [enc:libx265 @ 0x5e6836f4ae80] Error while \
             opening encoder - maybe incorrect parameters such as bit_rate, rate, width or \
             height.\n\
             [vf#0:0 @ 0x5e6836f4b500] Error sending frames to consumers: Invalid argument\n\
             [vf#0:0 @ 0x5e6836f4b500] Task finished with error code: -22 (Invalid argument)\n\
             [vf#0:0 @ 0x5e6836f4b500] Terminating thread with return code -22 (Invalid \
             argument)\n\
             [vost#0:0/libx265 @ 0x5e6836f4a8c0] [enc:libx265 @ 0x5e6836f4ae80] Could not open \
             encoder before EOF\n\
             [vost#0:0/libx265 @ 0x5e6836f4a8c0] Task finished with error code: -22 (Invalid \
             argument)\n\
             [vost#0:0/libx265 @ 0x5e6836f4a8c0] Terminating thread with return code -22 \
             (Invalid argument)\n\
             [out#0/mp4 @ 0x5e6836f4a340] Nothing was written into output file, because at \
             least one of its streams received no packets.\n"
        ),
        "[libx265] Error setting preset/tune nonsense/(null)."
    );

    // An input it cannot read: the demuxer's line says why, and the lines
    // after it repeat that the input did not open.
    assert_eq!(
        reason(
            "[mov,mp4,m4a,3gp,3g2,mj2 @ 0x583b3eba9f00] moov atom not found\n\
             [in#0 @ 0x583b3eba9c40] Error opening input: Invalid data found when processing \
             input\n\
             Error opening input file /home/someone/attachments/clip.mov.\n\
             Error opening input files: Invalid data found when processing input\n"
        ),
        "[mov,mp4,m4a,3gp,3g2,mj2] moov atom not found"
    );

    // An encoder ffmpeg does not have.
    assert_eq!(
        reason(
            "[vost#0:0 @ 0x57532dc21880] Unknown encoder 'nosuchcodec'\n\
             [vost#0:0 @ 0x57532dc21880] Error selecting an encoder\n\
             Error opening output file /tmp/out.mp4.\n\
             Error opening output files: Encoder not found\n"
        ),
        "[vost#0:0] Unknown encoder 'nosuchcodec'"
    );

    // A damaged frame ffmpeg carried on past is not the cause: a full disk
    // that stopped it is.
    assert_eq!(
        reason(
            "[mp3float @ 0x5cf2699f10c0] Header missing\n\
             [aist#0:0/mp3 @ 0x5cf2699e0d80] [dec:mp3float @ 0x5cf2699f0a40] Error submitting \
             packet to decoder: Invalid data found when processing input\n\
             [out#0/adts @ 0x5cf2699e1a40] Error writing trailer: No space left on device\n\
             [out#0/adts @ 0x5cf2699e1a40] Error closing file: No space left on device\n"
        ),
        "[out#0/adts] Error closing file: No space left on device"
    );

    // libx265's report after a full disk stopped it is not the cause.
    assert_eq!(
        reason(
            "[vf#0:0 @ 0x59b3ce069500] Error sending frames to consumers: No space left on device\n\
             [vf#0:0 @ 0x59b3ce069500] Task finished with error code: -28 (No space left on \
             device)\n\
             [out#0/mp4 @ 0x59b3ce068380] Nothing was written into output file, because at \
             least one of its streams received no packets.\n\
             x265 [info]: Weighted P-Frames: Y:0.0% UV:0.0%\n\
             \n\
             encoded 0 frames\n"
        ),
        "[vf#0:0] Error sending frames to consumers: No space left on device"
    );

    // When every line only follows from a failure, the first is the
    // reason, as the one nearest the cause.
    assert_eq!(
        reason(
            "[in#0 @ 0x57c830ccfc40] Error opening input: No such file or directory\n\
             Error opening input file /home/someone/attachments/clip.mov.\n\
             Error opening input files: No such file or directory\n"
        ),
        "[in#0] Error opening input: No such file or directory"
    );

    // A line that is not a consequence is the reason, with the file
    // written as <input>.
    assert_eq!(
        reason(
            "[out#0/mp4 @ 0x5e6836f4a340] Nothing was written into output file\n\
             /home/someone/attachments/clip.mov: Invalid data found when processing input\n"
        ),
        "<input>: Invalid data found when processing input"
    );

    // A C runtime that prints a pointer without `0x`, as one on Windows
    // can, gets the same reason.
    assert_eq!(
        reason("[libx265 @ 000001C8A4F2E0C0] Error setting preset/tune nonsense/(null).\n"),
        "[libx265] Error setting preset/tune nonsense/(null)."
    );

    // A tag whose text after ` @ ` is not an address keeps it.
    assert_eq!(
        reason("[filter @ graph 0] Invalid argument\n"),
        "[filter @ graph 0] Invalid argument"
    );

    // Each tag on the line loses its address and keeps its component.
    assert_eq!(
        reason(
            "[aist#0:0/mp3 @ 0x5cf2699e0d80] [dec:mp3float @ 0x5cf2699f0a40] Error submitting \
             packet to decoder: Invalid data found when processing input\n"
        ),
        "[aist#0:0/mp3] [dec:mp3float] Error submitting packet to decoder: Invalid data found \
         when processing input"
    );
}

/// A file whose path starts with another file's path is replaced whole.
#[test]
fn without_files_replaces_the_longest_path_first() {
    let failed = FfmpegFailed::new(
        std::process::ExitStatus::default(),
        b"/d/a.mov.mp4 and /d/a.mov",
        &[
            "-i".to_string(),
            "/d/a.mov".to_string(),
            "/d/a.mov.mp4".to_string(),
        ],
    );
    assert_eq!(failed.without_files(failed.said()), "<output> and <input>");
}

/// A compression where HEVC works reports no fallback.
#[test]
fn compress_reports_no_fallback_when_hevc_works() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let dest = dir.path().join("out.mp4");
    let tools = FakeTools::new().probing(video_probe("h264", 30.0));

    let outcome =
        transcode_file(&tools, &files[0], &dest, MediaMode::Compress, &opts, None).unwrap();

    assert_eq!(outcome, TranscodeOutcome::Produced { hevc_failure: None });
}

/// When H.264 fails after HEVC, the error names both failures.
#[test]
fn compress_reports_both_errors_when_h264_fails_after_hevc() {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let dest = dir.path().join("out.mp4");
    let tools = FakeTools::new()
        .probing(video_probe("h264", 30.0))
        .failing_on("libx265")
        .failing_on("libx264");

    let err = transcode_file(&tools, &files[0], &dest, MediaMode::Compress, &opts, None)
        .expect_err("both encoders fail");

    let error = format!("{err:#}");
    assert!(error.contains("HEVC failed"), "{error}");
    assert!(error.contains("libx265"), "HEVC's reason is kept: {error}");
    assert!(error.contains("libx264"), "H.264's reason is kept: {error}");
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
    let opts = CompressOptions {
        min_size_bytes: None,
        max_fps: Some(30),
        ..CompressOptions::default()
    };

    let slow = dir.path().join("slow.mp4");
    write_test_video_at(&slow, &["-c:v", "libx264"], 24);
    let produced =
        transcode_single(&tools, &slow, MediaMode::Compress, &opts).expect("slow is re-encoded");
    let probe = tools.probe(&produced).unwrap();
    assert_eq!(probe.codec, "hevc", "the video was re-encoded");
    assert_eq!(probe.fps, Some(24.0), "a 24 fps video stays at 24");

    let fast = dir.path().join("fast.mp4");
    write_test_video_at(&fast, &["-c:v", "libx264"], 60);
    let produced =
        transcode_single(&tools, &fast, MediaMode::Compress, &opts).expect("fast is re-encoded");
    let probe = tools.probe(&produced).unwrap();
    assert_eq!(probe.codec, "hevc", "the video was re-encoded");
    assert_eq!(probe.fps, Some(30.0), "a 60 fps video comes down to 30");
}

#[test]
fn compress_re_encodes_a_large_mp3_when_the_result_is_smaller() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mp3 = dir.path().join("song.mp3");
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

    let produced = transcode_single(
        &tools,
        &mp3,
        MediaMode::Compress,
        &CompressOptions::default(),
    )
    .expect("a large mp3 is re-encoded");

    assert!(fs::metadata(&produced).unwrap().len() < before);
    fs::remove_file(produced).unwrap();

    // Convert leaves an MP3 alone, whatever its size.
    assert_eq!(
        transcode_single(
            &tools,
            &mp3,
            MediaMode::Convert,
            &CompressOptions::default()
        ),
        None
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
    let large = dir.path().join("large.jpg");
    fs::write(&large, vec![0xa5; JPEG_COMPRESS_FLOOR as usize + 1]).unwrap();
    let png = dir.path().join("still.png");
    fs::write(&png, PNG_1X1_RGB).unwrap();
    // Neither of these reaches ffmpeg.
    let gif = dir.path().join("loop.gif");
    fs::write(&gif, b"GIF89a animated").unwrap();
    let small = dir.path().join("small.jpg");
    fs::write(&small, b"\xff\xd8\xff small jpeg").unwrap();
    let tools = FakeTools::new().writing("a smaller jpeg");
    let compress = |file: &Path| {
        transcode_single(
            &tools,
            file,
            MediaMode::Compress,
            &CompressOptions::default(),
        )
    };

    let shrunk = compress(&large).expect("the large JPEG is re-encoded");
    assert_eq!(
        fs::read(&shrunk).unwrap(),
        b"a smaller jpeg",
        "the smaller re-encode is kept"
    );
    assert!(compress(&png).is_some(), "the PNG is converted");
    assert_eq!(compress(&gif), None);
    assert_eq!(compress(&small), None);
    assert_eq!(
        tools.runs().len(),
        2,
        "ffmpeg ran for a GIF or a small JPEG"
    );
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
        let m4a = dir.path().join("voice.m4a");
        fs::write(&m4a, b"an m4a voice note").unwrap();
        // Under the MP3 floor: ffmpeg must never see it.
        let mp3 = dir.path().join("small.mp3");
        fs::write(&mp3, b"ID3 small mp3").unwrap();
        let tools = FakeTools::new().writing("the mp3");

        let produced = transcode_single(&tools, &m4a, mode, &CompressOptions::default());
        assert!(produced.is_some(), "{mode:?}");
        assert_eq!(
            transcode_single(&tools, &mp3, mode, &CompressOptions::default()),
            None,
            "{mode:?}"
        );
        assert!(has(&only_run(&tools), "libmp3lame"), "{mode:?}");
        assert_eq!(fs::read(&mp3).unwrap(), b"ID3 small mp3", "{mode:?}");
    }
}

#[test]
fn convert_turns_an_m4a_into_an_mp3() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let m4a = dir.path().join("voice.m4a");
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

    let produced = transcode_single(
        &tools,
        &m4a,
        MediaMode::Convert,
        &CompressOptions::default(),
    )
    .expect("an m4a is converted");

    assert!(fs::metadata(&produced).unwrap().len() > 0);
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
        let limit = Limit::for_reads(reads, WAIT, |_| Some(SCRIPTED_LENGTH));
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
fn compress_scripted(tools: &ScriptedTools) -> Result<TranscodeOutcome> {
    let dir = tempfile::tempdir().unwrap();
    let (files, opts) = two_large_videos(dir.path());
    let dest = dir.path().join("out.mp4");
    transcode_file(tools, &files[0], &dest, MediaMode::Compress, &opts, None)
}

/// Why HEVC failed, from a compression where H.264 stood in.
fn hevc_failure(outcome: Result<TranscodeOutcome>) -> String {
    match outcome {
        Ok(TranscodeOutcome::Produced {
            hevc_failure: Some(reason),
        }) => reason,
        other => panic!("expected a produced file with HEVC's reason, got {other:?}"),
    }
}

/// A HEVC run that ffmpeg ended with an error, as when libx265 is missing
/// or refused the file, is followed by H.264 (#2898).
#[cfg(unix)]
#[test]
fn compress_tries_h264_after_hevc_failed() {
    let tools = ScriptedTools::new("echo 'Unknown encoder libx265' >&2\nexit 1", false);

    let reason = hevc_failure(compress_scripted(&tools));

    assert!(tools.tried_h264(), "{:?}", tools.runs());
    assert!(reason.contains("Unknown encoder libx265"), "{reason}");
}

/// A HEVC run that writes no progress and never exits most likely hung in
/// the encoder, as a hardware encoder can: it is stopped once the wait has
/// passed, and H.264 is tried, with the fallback naming the stall (#2898).
#[cfg(unix)]
#[test]
fn compress_tries_h264_after_hevc_stalled_before_any_output() {
    let tools = ScriptedTools::new(HANG, true);

    let reason = hevc_failure(compress_scripted(&tools));

    assert!(tools.tried_h264(), "{:?}", tools.runs());
    assert!(
        reason.contains("made no progress for 2 minutes before converting any of the file"),
        "{reason}"
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

    let err = compress_scripted(&tools).expect_err("the file fails");

    assert!(!tools.tried_h264(), "{:?}", tools.runs());
    assert!(
        format!("{err:#}").contains("made no progress for 2 minutes after converting 0:02"),
        "{err:#}"
    );
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

    let err = compress_scripted(&tools).expect_err("the file fails");

    assert!(!tools.tried_h264(), "{:?}", tools.runs());
    assert!(
        format!("{err:#}").contains("ran past twice the file's length of 0:05"),
        "{err:#}"
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
    assert!(!h264_follows(&killed(KillReason::TookTooLong {
        limit: std::time::Duration::from_secs(15),
    })));
}

/// An MP3 made with the real ffmpeg by joining recordings back to back: a
/// one-second part whose Xing header counts only its own frames, then
/// twenty minutes more, long enough that ffmpeg reports its position before
/// it finishes. The header's byte count is set to the whole file's
/// size, as a joining tool that rewrites it leaves it, so ffprobe reads one
/// second without a warning that it estimated it.
fn joined_mp3(tools: &InstalledTools, dir: &Path) -> PathBuf {
    let part = |name: &str, seconds: u32| {
        let path = dir.join(name);
        tools
            .run_ffmpeg(&[
                "-y".into(),
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                format!("sine=duration={seconds}"),
                "-c:a".into(),
                "libmp3lame".into(),
                "-q:a".into(),
                "4".into(),
                path_str(&path),
            ])
            .unwrap();
        fs::read(path).unwrap()
    };
    let mut joined = part("first.mp3", 1);
    let rest = part("rest.mp3", 10);
    for _ in 0..120 {
        joined.extend_from_slice(&rest);
    }
    let xing = joined
        .windows(4)
        .position(|window| window == b"Xing")
        .expect("the first part has a Xing header");
    let flags = u32::from_be_bytes(joined[xing + 4..xing + 8].try_into().unwrap());
    assert_eq!(flags & 0b11, 0b11, "the header counts frames and bytes");
    let size = u32::try_from(joined.len()).unwrap().to_be_bytes();
    joined[xing + 12..xing + 16].copy_from_slice(&size);
    let path = dir.join("joined");
    fs::write(&path, joined).unwrap();
    path
}

/// A joined MP3 whose header states under half its real length converts:
/// the twice-the-length stop is for video only, and an audio conversion
/// is held to a time limit instead (#2909).
#[test]
fn a_joined_mp3_whose_header_states_under_half_its_length_converts() {
    let Some(tools) = crate::testutil::real_ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = joined_mp3(&tools, dir.path());
    // About a second: ffmpeg versions count the encoder's padding apart.
    let stated = crate::probe::played_for(&tools, &src, None);
    assert!(
        stated.is_some_and(|length| length < std::time::Duration::from_secs(2)),
        "ffprobe trusts the first part's header: {stated:?}"
    );
    let dest = dir.path().join("joined.mp3.in_progress");

    let outcome = transcode_file_as(
        &tools,
        &src,
        Kind::Audio,
        &dest,
        MediaMode::Convert,
        &CompressOptions::default(),
        None,
    )
    .unwrap_or_else(|err| panic!("{err:#}"));

    assert_eq!(outcome, TranscodeOutcome::Produced { hevc_failure: None });
    assert!(
        crate::probe::played_for(&tools, &dest, None)
            .is_some_and(|length| length > std::time::Duration::from_secs(20 * 60)),
        "the whole file was converted"
    );
}
