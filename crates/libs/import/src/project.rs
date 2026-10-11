//! Turn conversation documents into JSON Lines for the Message Crate import API.
//!
//! JSON Lines means one JSON object per line. The first line is the conversation
//! header. Each later line is one message.

use anyhow::{Context, Result, bail};
use message_ir::{ConversationDocument, ConversationHeader, IrMessage, check_schema_version};

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
pub fn document_header_line(doc: &ConversationDocument) -> Result<Vec<u8>> {
    let header = ConversationHeader::from_document(doc);
    validate_header(&header)?;
    let mut out =
        serde_json::to_vec(&header).context("serialize message-ir conversation header")?;
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
    msg: &IrMessage,
    projections: &[AttachmentProjection],
) -> Result<(Vec<u8>, String)> {
    let mut msg = msg.clone();
    for proj in projections {
        match proj {
            AttachmentProjection::Digested {
                index,
                digest,
                size,
            } => {
                if let Some(att) = msg.attachments.get_mut(*index) {
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
                if let Some(att) = msg.attachments.get_mut(*index) {
                    att.digest_sha256 = None;
                    att.missing_reason = Some(reason.clone());
                    if let Some(size) = size {
                        att.size_bytes = Some(*size);
                    }
                }
            }
        }
    }
    serialize_message(&msg)
}

/// Serialize one message and return `(line_bytes, guid)`.
///
/// The guid is the message's own. The server refuses a message without one,
/// so it is not checked here.
///
/// # Errors
///
/// Returns an error when JSON serialization fails.
fn serialize_message(msg: &IrMessage) -> Result<(Vec<u8>, String)> {
    let mut out = serde_json::to_vec(msg).context("serialize message-ir message")?;
    out.push(b'\n');
    Ok((out, msg.guid.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_ir::IrAttachment;
    use message_ir::testutil::{sample_attachment, sample_document};

    #[test]
    fn serializes_ir_sms() {
        let mut doc = sample_document("hello");
        let msg = doc.messages.remove(0);
        let header = String::from_utf8(document_header_line(&doc).unwrap()).unwrap();
        assert!(header.contains(r#""schema_version":14"#));
        assert!(header.contains(r#""sms-backup-restore""#));
        assert!(!header.contains(r#""record":"conversation""#));

        let (line, guid) = message_line(&msg, &[]).unwrap();
        assert_eq!(guid, msg.guid);
        let s = String::from_utf8(line).unwrap();
        assert!(s.contains(r#""direction":"incoming""#));
        assert!(!s.contains(r#""record":"message""#));
    }

    #[test]
    fn projects_missing_reason_and_clears_digest() {
        let mut msg = sample_document("with attachment").messages.remove(0);
        msg.attachments = vec![IrAttachment {
            original_name: Some("big.jpg".into()),
            digest_sha256: Some("deadbeef".into()),
            size_bytes: Some(99),
            ..sample_attachment()
        }];
        let (line, _) = message_line(
            &msg,
            &[AttachmentProjection::Missing {
                index: 0,
                reason: "too_large".into(),
                size: Some(5_000_000),
            }],
        )
        .unwrap();
        let parsed: IrMessage = serde_json::from_slice(&line).unwrap();
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
