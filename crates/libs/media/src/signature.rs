//! What a file's first bytes say it is.
//!
//! A declared media type is only a claim: an export or an upload can call a
//! video `application/octet-stream`, or a PNG `image/jpeg`. Most media
//! formats open with a fixed signature, so the first [`SIGNATURE_LEN`] bytes
//! of a file name its type, or the few types one container carries. The
//! server stores and serves an Asset under the type its bytes show, and the
//! media pass plans its Thumbnail and Preview from it
//! (`docs/architecture/media.md`, rule 2).

use std::io::Read;
use std::path::Path;

/// How many bytes from the start of a file a signature is read from.
pub const SIGNATURE_LEN: usize = 12;

/// One signature: the test a file's first bytes pass, and the media types a
/// file that passes it can be. The first type is the one assumed when the
/// declared type is none of them; a later one is kept only when declared,
/// because the bytes alone cannot tell it from the first.
struct Signature {
    starts_as: fn(&[u8]) -> bool,
    types: &'static [&'static str],
}

/// The brands of an ISO base media file (`ftyp`) that name a HEIF image.
const HEIF_BRANDS: &[&[u8; 4]] = &[
    b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"mif1", b"msf1",
];

/// The brands of an ISO base media file that name no one type: an MP4 most
/// often, but an `.m4a`, a `.mov` or a `.3gp` can carry them too.
const GENERIC_ISO_BRANDS: &[&[u8; 4]] = &[
    b"isom", b"iso2", b"iso3", b"iso4", b"iso5", b"iso6", b"mp41", b"mp42", b"mp71", b"avc1",
    b"dash", b"M4V ", b"M4VH", b"M4VP", b"MSNV", b"mmp4", b"f4v ",
];

/// The major brand of an ISO base media file, the four bytes after `ftyp`.
fn iso_brand(head: &[u8]) -> Option<&[u8]> {
    (head.get(4..8) == Some(b"ftyp")).then(|| head.get(8..12))?
}

fn iso_brand_in(head: &[u8], brands: &[&[u8; 4]]) -> bool {
    iso_brand(head).is_some_and(|brand| brands.iter().any(|b| &b[..] == brand))
}

/// Whether `head` starts a RIFF file of the four-byte `form`.
fn riff(head: &[u8], form: &[u8; 4]) -> bool {
    head.starts_with(b"RIFF") && head.get(8..12) == Some(&form[..])
}

/// Whether `head` starts an MP3: an `ID3` tag, or an MPEG audio frame sync
/// of layer I, II or III. An AAC ADTS frame, whose layer bits are `00`, is
/// not an MP3.
fn is_mp3(head: &[u8]) -> bool {
    head.starts_with(b"ID3")
        || matches!(head, [0xFF, second, ..] if second & 0xE0 == 0xE0 && second & 0x06 != 0)
}

/// Every signature known, each of a type in the extension table
/// (`crate::mime`) or a PDF.
const SIGNATURES: &[Signature] = &[
    Signature {
        starts_as: |head| head.starts_with(&[0xFF, 0xD8, 0xFF]),
        types: &["image/jpeg"],
    },
    Signature {
        starts_as: |head| head.starts_with(b"\x89PNG\r\n\x1A\n"),
        types: &["image/png"],
    },
    Signature {
        starts_as: |head| head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a"),
        types: &["image/gif"],
    },
    Signature {
        starts_as: |head| riff(head, b"WEBP"),
        types: &["image/webp"],
    },
    Signature {
        // `BM` alone starts plenty of text; the four reserved bytes after
        // the file size are zero in every BMP.
        starts_as: |head| head.starts_with(b"BM") && head.get(6..10) == Some(&[0, 0, 0, 0]),
        types: &["image/bmp"],
    },
    Signature {
        starts_as: |head| head.starts_with(b"II*\0") || head.starts_with(b"MM\0*"),
        types: &["image/tiff"],
    },
    Signature {
        starts_as: |head| iso_brand_in(head, HEIF_BRANDS),
        types: &["image/heic"],
    },
    Signature {
        starts_as: |head| iso_brand(head) == Some(b"qt  "),
        types: &["video/quicktime"],
    },
    Signature {
        starts_as: |head| {
            iso_brand(head).is_some_and(|b| b.starts_with(b"3gp") || b.starts_with(b"3g2"))
        },
        types: &["video/3gpp"],
    },
    Signature {
        starts_as: |head| matches!(iso_brand(head), Some(b"M4A " | b"M4B " | b"M4P ")),
        types: &["audio/mp4"],
    },
    Signature {
        starts_as: |head| iso_brand_in(head, GENERIC_ISO_BRANDS),
        types: &["video/mp4", "audio/mp4", "video/quicktime", "video/3gpp"],
    },
    Signature {
        // EBML: Matroska, and WebM, which is Matroska under another name.
        starts_as: |head| head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]),
        types: &["video/x-matroska", "video/webm", "audio/webm"],
    },
    Signature {
        starts_as: |head| riff(head, b"AVI "),
        types: &["video/x-msvideo"],
    },
    Signature {
        // An MPEG program stream pack, or an MPEG video sequence header.
        starts_as: |head| head.starts_with(&[0, 0, 1, 0xBA]) || head.starts_with(&[0, 0, 1, 0xB3]),
        types: &["video/mpeg"],
    },
    Signature {
        starts_as: is_mp3,
        types: &["audio/mpeg"],
    },
    Signature {
        starts_as: |head| head.starts_with(b"OggS"),
        types: &["audio/ogg", "audio/opus"],
    },
    Signature {
        starts_as: |head| head.starts_with(b"#!AMR"),
        types: &["audio/amr"],
    },
    Signature {
        starts_as: |head| head.starts_with(b"caff"),
        types: &["audio/x-caf"],
    },
    Signature {
        starts_as: |head| riff(head, b"WAVE"),
        types: &["audio/wav"],
    },
    Signature {
        starts_as: |head| head.starts_with(b"%PDF-"),
        types: &["application/pdf"],
    },
];

/// The media type the first bytes of a file, `head`, show it to be, or
/// `None` when they match no signature known here.
///
/// Where a signature fits several types (an ISO file with a generic brand
/// is an MP4 or an `.m4a`), `declared` is kept when it is one of them;
/// otherwise the signature's usual type is the answer. `declared` is
/// compared as it is, so a caller passes it lowercased and without
/// parameters, as [`media_type_of`](crate::media_type_of) gives it.
#[must_use]
pub fn media_type_from_bytes(head: &[u8], declared: Option<&str>) -> Option<&'static str> {
    let signature = SIGNATURES.iter().find(|s| (s.starts_as)(head))?;
    let kept = declared.and_then(|declared| signature.types.iter().find(|t| **t == declared));
    Some(kept.copied().unwrap_or(signature.types[0]))
}

/// [`media_type_from_bytes`] over the first bytes of the file at `src`.
/// `None` when the file cannot be read, a symlink included, as well as when
/// its bytes match no signature: either way, its bytes say nothing.
#[must_use]
pub fn media_type_in_file(src: &Path, declared: Option<&str>) -> Option<&'static str> {
    let head = read_head(src).ok()?;
    media_type_from_bytes(&head, declared)
}

/// The first [`SIGNATURE_LEN`] bytes of `src`, or all of it when it is
/// shorter. A symlink is refused rather than followed.
pub(crate) fn read_head(src: &Path) -> anyhow::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(SIGNATURE_LEN);
    file_io::open_nofollow(src)?
        .take(SIGNATURE_LEN as u64)
        .read_to_end(&mut head)?;
    Ok(head)
}

#[cfg(test)]
mod tests;
