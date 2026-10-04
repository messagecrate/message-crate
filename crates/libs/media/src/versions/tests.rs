use std::fs;
use std::path::Path;

use super::*;
use crate::tools::run_ffmpeg;

/// A stop that is never set, for a conversion that runs to its end.
static NOT_STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Generate a test file with ffmpeg from a lavfi `source`, encoded by `codec`.
fn generate(path: &Path, source: &str, codec: &[&str]) {
    let mut args: Vec<String> = ["-y", "-f", "lavfi", "-i", source]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    args.extend(codec.iter().map(|s| (*s).to_string()));
    args.push(path_str(path));
    run_ffmpeg(&args).expect("generate a test file");
}

fn picture(path: &Path, width: u32, height: u32) {
    generate(
        path,
        &format!("testsrc=size={width}x{height}"),
        &["-frames:v", "1", "-update", "1"],
    );
}

fn video(path: &Path, codec: &[&str]) {
    let mut args = vec!["-pix_fmt", "yuv420p", "-an"];
    args.extend_from_slice(codec);
    generate(path, "testsrc=size=320x240:rate=10:duration=0.5", &args);
}

/// `codec,width,height` of the first video stream, as ffprobe reads it.
fn shape(path: &Path) -> (String, u32, u32) {
    let probe = probe_video(path).expect("ffprobe reads the file");
    (probe.codec, probe.width, probe.height)
}

#[test]
fn a_media_type_comes_from_the_extension_then_the_declared_type_then_a_name() {
    let none: [Option<&str>; 0] = [];
    assert_eq!(
        media_type_of(Path::new("ab/abc.heic"), Some("image/jpeg"), &none).as_deref(),
        Some("image/heic"),
        "the stored file's extension comes first"
    );
    assert_eq!(
        media_type_of(Path::new("ab/abc"), Some("image/JPG; q=1"), &none).as_deref(),
        Some("image/jpeg"),
        "a declared alias reads as its usual spelling"
    );
    assert_eq!(
        media_type_of(Path::new("ab/abc"), Some("video/x-m4v"), &none).as_deref(),
        Some("video/x-m4v"),
        "a declared type the table does not hold is kept as it is"
    );
    assert_eq!(
        media_type_of(Path::new("ab/abc"), None, &[None, Some("Voice.AMR")]).as_deref(),
        Some("audio/amr"),
        "with nothing else, a name the export gave it"
    );
    assert_eq!(media_type_of(Path::new("ab/abc"), None, &[None]), None);
}

#[test]
fn every_browser_shows_jpeg_png_gif_webp_and_mp3_as_they_are() {
    let nowhere = Path::new("no/such/file");
    for shown in [
        "image/jpeg",
        "image/png",
        "image/gif",
        "image/webp",
        "audio/mpeg",
    ] {
        assert!(browser_shows(nowhere, Some(shown)), "{shown}");
    }
    for preview in [
        "image/heic",
        "image/tiff",
        "image/bmp",
        "video/quicktime",
        "video/3gpp",
        "audio/amr",
        "audio/mp4",
        "audio/x-caf",
    ] {
        assert!(!browser_shows(nowhere, Some(preview)), "{preview}");
    }
    assert!(
        !browser_shows(nowhere, None),
        "a file of no known type gets a Preview"
    );
    assert!(
        !browser_shows(nowhere, Some("video/mp4")),
        "an MP4 ffprobe cannot read gets a Preview"
    );
}

#[test]
fn an_mp4_is_shown_as_it_is_only_with_h264_video() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let h264 = dir.path().join("h264.mp4");
    video(&h264, &["-c:v", "libx264"]);
    let hevc = dir.path().join("hevc.mp4");
    video(&hevc, &["-c:v", "libx265", "-tag:v", "hvc1"]);

    assert!(browser_shows(&h264, Some("video/mp4")));
    assert!(
        !browser_shows(&hevc, Some("video/mp4")),
        "HEVC in an MP4 needs a Preview"
    );
}

#[test]
fn a_thumbnail_is_a_jpeg_scaled_to_560_pixels_on_its_long_side() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let wide = dir.path().join("wide.png");
    picture(&wide, 1200, 900);
    let tall = dir.path().join("tall.png");
    picture(&tall, 600, 1200);
    let small = dir.path().join("small.png");
    picture(&small, 100, 50);

    for (src, want) in [
        (&wide, (560, 420)),
        (&tall, (280, 560)),
        (&small, (100, 50)),
    ] {
        let dest = dir.path().join("thumbnail.jpg");
        make_thumbnail(src, &dest, &NOT_STOPPED).unwrap();
        let (codec, width, height) = shape(&dest);
        assert_eq!(codec, "mjpeg", "{}", src.display());
        assert_eq!((width, height), want, "{}", src.display());
        assert!(fs::metadata(&dest).unwrap().len() < 100 * 1024);
    }
}

#[test]
fn a_video_thumbnail_is_its_first_frame() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mov");
    video(&src, &["-c:v", "libx265", "-tag:v", "hvc1"]);
    let dest = dir.path().join("thumbnail.jpg");

    make_thumbnail(&src, &dest, &NOT_STOPPED).unwrap();

    assert_eq!(shape(&dest), ("mjpeg".to_string(), 320, 240));
}

#[test]
fn a_hevc_video_preview_is_h264_that_every_browser_plays() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mov");
    video(&src, &["-c:v", "libx265", "-tag:v", "hvc1"]);
    let dest = dir.path().join("preview.mp4");

    make_preview(&src, Kind::Video, &dest, &NOT_STOPPED).unwrap();

    assert_eq!(shape(&dest), ("h264".to_string(), 320, 240));
    assert!(browser_shows(&dest, Some("video/mp4")));
}

#[test]
fn an_image_preview_is_a_jpeg_and_an_audio_preview_an_mp3() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("scan.bmp");
    picture(&image, 64, 48);
    let audio = dir.path().join("note.wav");
    generate(&audio, "sine=duration=0.5", &[]);

    let jpeg = dir.path().join("preview.jpg");
    make_preview(&image, Kind::Image, &jpeg, &NOT_STOPPED).unwrap();
    assert_eq!(shape(&jpeg).0, "mjpeg");

    let mp3 = dir.path().join("preview.mp3");
    make_preview(&audio, Kind::Audio, &mp3, &NOT_STOPPED).unwrap();
    assert!(fs::read(&mp3).unwrap().len() > 100, "an MP3 was written");
}

#[test]
fn a_preview_is_written_only_under_its_own_extension() {
    let dir = tempfile::tempdir().unwrap();
    let err = make_preview(
        &dir.path().join("clip.mov"),
        Kind::Video,
        &dir.path().join("preview.mov"),
        &NOT_STOPPED,
    );
    let Err(err) = err else {
        panic!("a video Preview written as .mov");
    };
    // Without ffmpeg the refusal is that ffmpeg is missing, which is as good.
    let said = format!("{err:#}");
    assert!(said.contains(".mp4") || said.contains("ffmpeg"), "{said}");
}
