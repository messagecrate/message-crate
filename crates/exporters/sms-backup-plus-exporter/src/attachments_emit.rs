//! Attachment helpers: queue decoded attachments as [`PendingAttachment`]
//! metadata during parse.

use crate::types::AttachmentBytes;
use anyhow::Result;
use message_ir::PendingAttachment;
use message_staging::AttachmentSpool;

/// Queue decoded attachments as metadata, each with its payload's size whether
/// or not the run copies it. With a `spool`, each payload is written to it
/// here, so no attachment's bytes stay in memory until the shared runner
/// writes them.
///
/// # Errors
///
/// Returns an error when a payload cannot be written to the spool.
pub(super) fn queue_attachments(
    attachments: &[AttachmentBytes],
    spool: Option<&AttachmentSpool>,
) -> Result<Vec<PendingAttachment>> {
    attachments
        .iter()
        .map(|attachment| {
            let digest = match spool {
                Some(spool) if !attachment.data.is_empty() => spool.put(&attachment.data)?,
                _ => attachment.digest_hex.clone(),
            };
            Ok(PendingAttachment {
                rel_path: String::new(),
                content_type: attachment.mime_type.clone().unwrap_or_default(),
                digest_sha256: Some(digest),
                name_hint: attachment
                    .original_name
                    .clone()
                    .or_else(|| Some(attachment.filename.clone())),
                size_bytes: Some(attachment.data.len() as u64),
            })
        })
        .collect()
}
