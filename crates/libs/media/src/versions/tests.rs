use std::fs;
use std::path::Path;

use super::*;
use crate::fake_tools::{FakeTools, video_probe};
use crate::probe_media;
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
    let probe = probe_media(path).expect("ffprobe reads the file");
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

/// The first bytes of a file of each type every browser shows as it is,
/// each followed by bytes no signature reads.
const SIGNATURES: [(&str, &[u8]); 7] = [
    ("image/jpeg", b"\xFF\xD8\xFF\xE0rest"),
    ("image/png", b"\x89PNG\r\n\x1A\nrest"),
    ("image/gif", b"GIF89arest"),
    ("image/webp", b"RIFF\x24\x00\x00\x00WEBPVP8 rest"),
    ("audio/mpeg", b"ID3\x04\x00rest"),
    ("audio/mpeg", b"\xFF\xFB\x90\x64rest"),
    ("audio/mpeg", b"\xFF\xF3\x48\xC4rest"),
];

/// Write `bytes` to a file named `name` in `dir`, and answer its path.
fn file(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn every_browser_shows_jpeg_png_gif_webp_and_mp3_as_they_are() {
    let dir = tempfile::tempdir().unwrap();
    for (index, (shown, bytes)) in SIGNATURES.iter().enumerate() {
        let src = file(dir.path(), &index.to_string(), bytes);
        assert_eq!(browser_shows(&src, Some(shown)), Some(true), "{shown}");
    }
    let nowhere = Path::new("no/such/file");
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
        assert_eq!(
            browser_shows(nowhere, Some(preview)),
            Some(false),
            "{preview}"
        );
    }
    assert_eq!(
        browser_shows(nowhere, None),
        Some(false),
        "a file of no known type gets a Preview"
    );
}

/// A declared type is only a claim: a file whose first bytes are not that
/// type's is not shown as it is, so it gets a Preview.
#[test]
fn a_file_whose_bytes_are_not_its_declared_type_is_not_shown_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let png = file(dir.path(), "png", b"\x89PNG\r\n\x1A\nrest");
    let text = file(dir.path(), "text", b"just some words, not a picture");
    let empty = file(dir.path(), "empty", b"");
    let short = file(dir.path(), "short", b"\xFF\xD8");
    let adts = file(dir.path(), "adts", b"\xFF\xF1\x50\x80rest");
    let wave = file(dir.path(), "wave", b"RIFF\x24\x00\x00\x00WAVEfmt rest");

    for (src, declared) in [
        (&png, "image/jpeg"),
        (&text, "image/jpeg"),
        (&text, "image/png"),
        (&text, "image/gif"),
        (&text, "image/webp"),
        (&text, "audio/mpeg"),
        (&empty, "image/jpeg"),
        (&short, "image/jpeg"),
        (&adts, "audio/mpeg"),
        (&wave, "image/webp"),
    ] {
        assert_eq!(
            browser_shows(src, Some(declared)),
            Some(false),
            "{} declared {declared}",
            src.display()
        );
    }
    assert_eq!(
        browser_shows(Path::new("no/such/file"), Some("image/jpeg")),
        Some(false),
        "a file that cannot be read is not shown as it is"
    );
}

#[test]
fn an_mp4_is_shown_as_it_is_only_with_h264_video() {
    let clip = Path::new("clip.mp4");
    let video = |codec: &str| video_probe(codec, 10.0);

    let h264 = FakeTools::new().probing(video("h264"));
    assert_eq!(
        browser_shows_with(&h264, clip, Some("video/mp4")),
        Some(true)
    );
    let hevc = FakeTools::new().probing(video("hevc"));
    assert_eq!(
        browser_shows_with(&hevc, clip, Some("video/mp4")),
        Some(false),
        "HEVC in an MP4 needs a Preview"
    );
    assert_eq!(
        browser_shows_with(&FakeTools::new(), clip, Some("video/mp4")),
        Some(false),
        "an MP4 ffprobe cannot read gets a Preview"
    );
}

/// Without ffprobe nothing is known about an MP4's codec, so the answer is
/// `None` and a caller keeps what an earlier answer said. Every other type
/// is still answered by its type alone.
#[test]
fn without_ffprobe_an_mp4_is_not_known() {
    let tools = FakeTools::missing();
    let nowhere = Path::new("no/such/file");
    let dir = tempfile::tempdir().unwrap();
    let jpeg = file(dir.path(), "photo", SIGNATURES[0].1);

    assert_eq!(browser_shows_with(&tools, nowhere, Some("video/mp4")), None);
    assert_eq!(
        browser_shows_with(&tools, &jpeg, Some("image/jpeg")),
        Some(true)
    );
    assert_eq!(
        browser_shows_with(&tools, nowhere, Some("image/heic")),
        Some(false)
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
    assert_eq!(browser_shows(&dest, Some("video/mp4")), Some(true));
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
    let tools = FakeTools::new();
    let err = make_preview_with(
        &tools,
        &dir.path().join("clip.mov"),
        Kind::Video,
        &dir.path().join("preview.mov"),
        &NOT_STOPPED,
    )
    .expect_err("a video Preview written as .mov");

    assert!(format!("{err:#}").contains(".mp4"), "{err:#}");
    assert_eq!(tools.runs(), Vec::<Vec<String>>::new());
}
