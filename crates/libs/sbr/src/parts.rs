//! MMS parts: their `data` decoded and hashed, the message text and
//! attachments they make, and their attributes for the vendor bag.

use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use message_ir::valid_filename;

use crate::xml::{btree, decode_body, get};

/// Raw `<part>` element: content-type, name, location, and payload columns
/// plus the full attribute map.
#[derive(Debug, Clone, Default)]
pub struct MmsPart {
    /// MIME type from the `ct` attribute.
    pub ct: String,
    /// Content name from the `name` attribute.
    pub name: String,
    /// Content-Location from the `cl` attribute.
    pub cl: String,
    /// Filename from the XML `fn` attribute (not a function attribute).
    pub filename_attr: String,
    /// Content-ID from the `cid` attribute, angle brackets and all.
    pub cid: String,
    /// Text body (SMIL) when present.
    pub text: String,
    /// Base64 payload when present.
    pub data: String,
    /// All raw attributes.
    pub attrs: BTreeMap<String, String>,
}

/// Decoded MMS attachment with a content-addressed filename.
#[derive(Debug, Clone)]
pub struct AttachmentBlob {
    /// Content-addressed filename (`<sha256><ext>`).
    pub filename: String,
    /// Original part name from the XML, when present.
    pub original_name: Option<String>,
    /// MIME type from the part's `ct`.
    pub mime_type: Option<String>,
    /// Decoded payload bytes shared by reference.
    pub data: Arc<[u8]>,
    /// Lowercase hex SHA-256 of the payload.
    pub digest_hex: String,
}

/// An MMS part from a `<part>` element's attributes.
pub(crate) fn part(attrs: &HashMap<String, String>) -> MmsPart {
    MmsPart {
        ct: get(attrs, "ct").into(),
        name: get(attrs, "name").into(),
        cl: get(attrs, "cl").into(),
        filename_attr: get(attrs, "fn").into(),
        cid: get(attrs, "cid").into(),
        text: get(attrs, "text").into(),
        data: get(attrs, "data").into(),
        attrs: btree(attrs),
    }
}

/// File extension for a part's content type.
fn extension(part: &MmsPart) -> String {
    match part.ct.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => ".jpg".into(),
        "image/png" => ".png".into(),
        "image/gif" => ".gif".into(),
        "image/webp" => ".webp".into(),
        "video/mp4" => ".mp4".into(),
        "video/3gpp" | "video/3gp" => ".3gp".into(),
        "audio/amr" => ".amr".into(),
        "audio/mpeg" => ".mp3".into(),
        "audio/mp4" => ".m4a".into(),
        ct => [&part.name, &part.cl, &part.filename_attr]
            .iter()
            .find_map(|n| {
                valid_filename(n).and_then(|n| {
                    Path::new(&n)
                        .extension()?
                        .to_str()
                        .map(|e| format!(".{}", e.to_ascii_lowercase()))
                })
            })
            .unwrap_or_else(|| {
                if ct.starts_with("image/") {
                    ".jpg".into()
                } else if ct.starts_with("video/") {
                    ".mp4".into()
                } else if ct.starts_with("audio/") {
                    ".amr".into()
                } else {
                    ".bin".into()
                }
            }),
    }
}

/// One base64 decode + SHA-256 of a part's `data` attribute.
///
/// Shared by attachment staging and source-field write-back so each part is
/// decoded once.
pub(crate) enum DecodedPartData {
    /// Empty or `"null"` — no payload.
    Absent,
    /// Successfully decoded payload.
    Ok {
        bytes: Arc<[u8]>,
        digest_hex: String,
    },
    /// Non-empty data that is not valid base64.
    Err { raw_len: usize, raw_sha256: String },
}

/// A part's `data` attribute decoded from base64, or why it could not be.
pub(crate) fn decode_part_data(raw: &str) -> DecodedPartData {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return DecodedPartData::Absent;
    }
    match base64::engine::general_purpose::STANDARD.decode(trimmed) {
        Ok(bytes) => {
            let digest_hex = hex::encode(Sha256::digest(&bytes));
            DecodedPartData::Ok {
                bytes: Arc::from(bytes),
                digest_hex,
            }
        }
        Err(_) => DecodedPartData::Err {
            raw_len: raw.len(),
            raw_sha256: hex::encode(Sha256::digest(raw.as_bytes())),
        },
    }
}

/// What a part holds, for [`mms_parts::body_of`].
///
/// A `text/plain` part's words are in `text`. Any other part's content is
/// in `data`, base64-encoded, and a contact card is written as
/// `ct="text/x-vcard" text="null" data="…"`. A part with no `data` but a
/// `text` holds that text.
fn part_content<'a>(part: &MmsPart, decoded: &'a DecodedPartData) -> mms_parts::Content<'a> {
    use mms_parts::Content;
    let text = decode_body(&part.text);
    let text = if text.eq_ignore_ascii_case("null") {
        String::new()
    } else {
        text
    };
    if mms_parts::is_text(&part.ct) && !text.is_empty() {
        return Content::Text(text);
    }
    match decoded {
        DecodedPartData::Ok { bytes, .. } if !bytes.is_empty() => Content::Bytes(bytes),
        DecodedPartData::Err { .. } => Content::Unreadable,
        DecodedPartData::Ok { .. } | DecodedPartData::Absent if text.is_empty() => Content::None,
        DecodedPartData::Ok { .. } | DecodedPartData::Absent => Content::Text(text),
    }
}

/// The message text and attachment blobs the MMS parts make, by the rules of
/// [`mms_parts`], and how many parts it left out because their `data` is not
/// base64.
pub(crate) fn mms_body(
    parts: &[MmsPart],
    decoded: &[DecodedPartData],
) -> (String, Vec<AttachmentBlob>, u64) {
    let shaped: Vec<mms_parts::Part<'_>> = parts
        .iter()
        .zip(decoded)
        .map(|(part, payload)| mms_parts::Part {
            content_type: &part.ct,
            keys: vec![&part.name, &part.cl, &part.cid, &part.filename_attr],
            content: part_content(part, payload),
        })
        .collect();
    let body = mms_parts::body_of(&shaped);
    let attachments = body
        .attachments
        .iter()
        .filter_map(|&index| {
            let (bytes, digest_hex) = match (&shaped[index].content, &decoded[index]) {
                (mms_parts::Content::Bytes(_), DecodedPartData::Ok { bytes, digest_hex }) => {
                    (Arc::clone(bytes), digest_hex.clone())
                }
                (mms_parts::Content::Text(text), _) => (
                    Arc::from(text.as_bytes()),
                    hex::encode(Sha256::digest(text.as_bytes())),
                ),
                _ => return None,
            };
            Some(attachment_blob(&parts[index], bytes, digest_hex))
        })
        .collect();
    (body.text, attachments, body.unreadable.len() as u64)
}

/// One attachment blob, named by its digest and the extension its type gives.
fn attachment_blob(part: &MmsPart, data: Arc<[u8]>, digest_hex: String) -> AttachmentBlob {
    AttachmentBlob {
        filename: format!("{digest_hex}{}", extension(part)),
        original_name: valid_filename(&part.name)
            .or_else(|| valid_filename(&part.cl))
            .or_else(|| valid_filename(&part.filename_attr)),
        mime_type: Some(if part.ct.trim().is_empty() {
            "application/octet-stream".into()
        } else {
            part.ct.clone()
        }),
        data,
        digest_hex,
    }
}

/// A part's attributes for the vendor bag, with the base64 data replaced by a marker.
pub(crate) fn part_fields(part: &MmsPart, decoded: &DecodedPartData) -> BTreeMap<String, String> {
    let mut attrs = part.attrs.clone();
    if attrs
        .remove("data")
        .is_some_and(|d| !d.trim().is_empty() && !d.eq_ignore_ascii_case("null"))
    {
        match decoded {
            DecodedPartData::Ok { bytes, digest_hex } => {
                attrs.insert("data_len".into(), bytes.len().to_string());
                attrs.insert("data_sha256".into(), digest_hex.clone());
            }
            DecodedPartData::Err {
                raw_len,
                raw_sha256,
            } => {
                attrs.insert("data_len".into(), raw_len.to_string());
                attrs.insert("data_sha256".into(), raw_sha256.clone());
                attrs.insert("data_decode_error".into(), "true".into());
            }
            DecodedPartData::Absent => {}
        }
    }
    attrs
}
