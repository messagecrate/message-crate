use super::*;

/// The first bytes of an ISO base media file of major brand `brand`.
fn iso(brand: &[u8; 4]) -> Vec<u8> {
    let mut head = b"\0\0\0\x20ftyp".to_vec();
    head.extend_from_slice(brand);
    head
}

#[test]
fn each_signature_names_its_type_whatever_was_declared() {
    let cases: &[(&[u8], &str)] = &[
        (b"\xFF\xD8\xFF\xE0rest", "image/jpeg"),
        (b"\x89PNG\r\n\x1A\nrest", "image/png"),
        (b"GIF89arest", "image/gif"),
        (b"RIFF\x24\0\0\0WEBPVP8 ", "image/webp"),
        (b"BM\x3A\0\0\0\0\0\0\0\x36\0", "image/bmp"),
        (b"II*\0rest", "image/tiff"),
        (&iso(b"heic"), "image/heic"),
        (&iso(b"mif1"), "image/heic"),
        (&iso(b"qt  "), "video/quicktime"),
        (&iso(b"3gp5"), "video/3gpp"),
        (&iso(b"M4A "), "audio/mp4"),
        (&iso(b"isom"), "video/mp4"),
        (&iso(b"mp42"), "video/mp4"),
        (b"\x1A\x45\xDF\xA3rest", "video/x-matroska"),
        (b"RIFF\x24\0\0\0AVI LIST", "video/x-msvideo"),
        (b"\0\0\x01\xBArest", "video/mpeg"),
        (b"ID3\x04rest", "audio/mpeg"),
        (b"\xFF\xFBrest", "audio/mpeg"),
        (b"OggSrest", "audio/ogg"),
        (b"#!AMR\nrest", "audio/amr"),
        (b"caff\0\x01rest", "audio/x-caf"),
        (b"RIFF\x24\0\0\0WAVEfmt ", "audio/wav"),
        (b"%PDF-1.7rest", "application/pdf"),
    ];
    for (head, expected) in cases {
        for declared in [None, Some("application/octet-stream"), Some("image/jpeg")] {
            assert_eq!(
                media_type_from_bytes(head, declared),
                Some(*expected),
                "{head:?} declared {declared:?}"
            );
        }
    }
}

/// An ISO file with a generic brand may be an `.m4a` or a `.mov` as well as
/// an MP4, and a Matroska file a WebM, so a declared type the signature
/// carries is kept.
#[test]
fn a_declared_type_the_signature_carries_is_kept() {
    assert_eq!(
        media_type_from_bytes(&iso(b"isom"), Some("audio/mp4")),
        Some("audio/mp4")
    );
    assert_eq!(
        media_type_from_bytes(&iso(b"isom"), Some("video/quicktime")),
        Some("video/quicktime")
    );
    assert_eq!(
        media_type_from_bytes(b"\x1A\x45\xDF\xA3rest", Some("video/webm")),
        Some("video/webm")
    );
    assert_eq!(
        media_type_from_bytes(b"OggSrest", Some("audio/opus")),
        Some("audio/opus")
    );
    // An alias is read in its usual spelling.
    assert_eq!(
        media_type_from_bytes(&iso(b"isom"), Some("audio/aac")),
        Some("audio/mp4")
    );
    // A declared audio type keeps an audio file audio, in a container that
    // also carries video.
    assert_eq!(
        media_type_from_bytes(&iso(b"3gp4"), Some("audio/3gpp")),
        Some("audio/3gpp")
    );
    assert_eq!(
        media_type_from_bytes(&iso(b"mp42"), Some("audio/x-m4a")),
        Some("audio/mp4")
    );
    assert_eq!(
        media_type_from_bytes(&iso(b"3gp4"), Some("audio/amr")),
        Some("audio/3gpp")
    );
    // A brand that names one type is that type, whatever was declared.
    assert_eq!(
        media_type_from_bytes(&iso(b"qt  "), Some("video/mp4")),
        Some("video/quicktime")
    );
}

#[test]
fn bytes_that_match_no_signature_say_nothing() {
    for head in [
        &b""[..],
        b"\xFF\xD8",
        b"just some words",
        b"BMW is a car maker",
        b"\xFF\xF1\x50\x80rest",
        // UTF-16LE text: its byte order mark is a frame sync.
        b"\xFF\xFEB\0E\0G\0I\0N\0",
        b"\xFF\xFF\xFF\xFF",
        // A frame header whose bitrate is the reserved value.
        b"\xFF\xFB\xF0\x00",
        b"\0\0\0\x20ftypavif",
        b"BEGIN:VCARD",
    ] {
        assert_eq!(
            media_type_from_bytes(head, Some("video/mp4")),
            None,
            "{head:?}"
        );
    }
}

#[test]
fn a_file_is_read_for_its_signature_and_a_missing_one_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip");
    let mut bytes = iso(b"isom");
    bytes.extend_from_slice(&[0; 64]);
    std::fs::write(&clip, bytes).unwrap();
    assert_eq!(
        media_type_in_file(&clip, Some("application/octet-stream")),
        Some("video/mp4")
    );
    assert_eq!(media_type_in_file(&dir.path().join("gone"), None), None);
}
