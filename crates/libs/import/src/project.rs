//! Turn conversation documents into JSON Lines for the Message Crate import API.
//!
//! JSON Lines means one JSON object per line. The first line is the conversation
//! header. Each later line is one message.

use anyhow::{Context, Result, bail};
use ir::{ConversationDocument, ConversationHeader, Message, check_schema_version};

/// Return the backup source name from a conversation header, or an error if the
/// schema version or source field is unusable.
///
/// # Errors
///
/// Returns an error when `schema_version` is not the current version, or when
/// `export.source` is empty.
pub fn validate_header(header: &ConversationHeader) -> Result<String> {
    check_schema_version(header.schema_version)?;
    let source = header.export.source.trim();
    if source.is_empty() {
        bail!("export.source is empty");
    }
    Ok(source.to_string())
}

/// First JSON Lines row for a conversation: the header object plus a newline.
///
/// # Errors
///
/// Returns an error when the header fails [`validate_header`] or cannot be
/// serialized.
pub fn document_header_line(document: &ConversationDocument) -> Result<Vec<u8>> {
    let header = ConversationHeader::from_document(document);
    validate_header(&header)?;
    let mut out = serde_json::to_vec(&header).context("serialize the conversation header")?;
    out.push(b'\n');
    Ok(out)
}

/// How to rewrite one attachment when building an import message line.
#[derive(Debug, Clone)]
pub enum AttachmentProjection {
    /// Bytes were (or will be) uploaded under this digest.
    Digested {
        index: usize,
        digest: String,
        size: u64,
    },
    /// Bytes are absent; keep metadata and set `missing_reason`.
    Missing {
        index: usize,
        reason: String,
        size: Option<u64>,
    },
}

/// One JSON Lines message row, with attachment fingerprints or missing placeholders.
///
/// # Errors
///
/// Returns an error when the message cannot be serialized.
pub fn message_line(
    message: &Message,
    projections: &[AttachmentProjection],
) -> Result<(Vec<u8>, String)> {
    let mut message = message.clone();
    for proj in projections {
        match proj {
            AttachmentProjection::Digested {
                index,
                digest,
                size,
            } => {
                if let Some(att) = message.attachments.get_mut(*index) {
                    att.digest_sha256 = Some(digest.clone());
                    att.size_bytes = Some(*size);
                    att.missing_reason = None;
                }
            }
            AttachmentProjection::Missing {
                index,
                reason,
                size,
            } => {
                if let Some(att) = message.attachments.get_mut(*index) {
                    att.digest_sha256 = None;
                    att.missing_reason = Some(reason.clone());
                    if let Some(size) = size {
                        att.size_bytes = Some(*size);
                    }
                }
            }
        }
    }
    serialize_message(&message)
}

/// Serialize one message and return `(line_bytes, guid)`.
///
/// The guid is the message's own. The server refuses a message without one,
/// so it is not checked here.
///
/// # Errors
///
/// Returns an error when JSON serialization fails.
fn serialize_message(message: &Message) -> Result<(Vec<u8>, String)> {
    let mut out = serde_json::to_vec(message).context("serialize a message")?;
    out.push(b'\n');
    Ok((out, message.guid.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::Attachment;
    use ir::testutil::{sample_attachment, sample_document};

    #[test]
    fn serializes_ir_sms() {
        let mut document = sample_document("hello");
        let message = document.messages.remove(0);
        let header = String::from_utf8(document_header_line(&document).unwrap()).unwrap();
        assert!(header.contains(&format!(r#""schema_version":{}"#, ir::SCHEMA_VERSION)));
        assert!(header.contains(r#""sms-backup-restore""#));
        assert!(!header.contains(r#""record":"conversation""#));

        let (line, guid) = message_line(&message, &[]).unwrap();
        assert_eq!(guid, message.guid);
        let s = String::from_utf8(line).unwrap();
        assert!(s.contains(r#""direction":"incoming""#));
        assert!(!s.contains(r#""record":"message""#));
    }

    #[test]
    fn projects_missing_reason_and_clears_digest() {
        let mut message = sample_document("with attachment").messages.remove(0);
        message.attachments = vec![Attachment {
            original_name: Some("big.jpg".into()),
            digest_sha256: Some("deadbeef".into()),
            size_bytes: Some(99),
            ..sample_attachment()
        }];
        let (line, _) = message_line(
            &message,
            &[AttachmentProjection::Missing {
                index: 0,
                reason: "too_large".into(),
                size: Some(5_000_000),
            }],
        )
        .unwrap();
        let parsed: Message = serde_json::from_slice(&line).unwrap();
        assert!(parsed.attachments[0].digest_sha256.is_none());
        assert_eq!(
            parsed.attachments[0].missing_reason.as_deref(),
            Some("too_large")
        );
        assert_eq!(parsed.attachments[0].size_bytes, Some(5_000_000));
        assert_eq!(
            parsed.attachments[0].original_name.as_deref(),
            Some("big.jpg")
        );
    }
}
