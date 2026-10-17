//! Attachment helpers for the emitter.

use anyhow::Result;
use go_sms_mms::ParsedPdu;
use ir::PendingAttachment;
use message_crate_core::digest_prefix;
use staging::AttachmentSpool;

/// Queue PDU attachment parts as metadata, each with its payload's size
/// whether or not the run copies it. With a `spool`, each payload is
/// written to it here, so no attachment's bytes stay in memory until the
/// shared runner writes them. The extension comes from the part's content
/// type; a type the media table does not know gets `.bin` so the bytes are
/// still kept.
///
/// # Errors
///
/// Returns an error when a payload cannot be written to the spool.
pub(super) fn queue_pdu_attachments(
    parsed: &ParsedPdu,
    spool: Option<&AttachmentSpool>,
) -> Result<Vec<PendingAttachment>> {
    let mut out = Vec::new();
    for (idx, attachment) in parsed.attachments.iter().enumerate() {
        let digest_hex = match spool {
            Some(spool) if !attachment.data.is_empty() => spool.put(&attachment.data)?,
            _ => file_io::sha256_hex(&attachment.data),
        };
        let ext = media::ext_for_mime(&attachment.content_type).unwrap_or(".bin");
        let name = format!(
            "I_{}_{}_{}{}",
            parsed.timestamp,
            digest_prefix(&digest_hex),
            idx + 1,
            ext
        );
        out.push(PendingAttachment {
            rel_path: String::new(),
            content_type: attachment.content_type.clone(),
            digest_sha256: Some(digest_hex),
            name_hint: attachment.name.clone().or(Some(name)),
            size_bytes: Some(attachment.data.len() as u64),
        });
    }
    Ok(out)
}
