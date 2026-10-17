//! Prepare conversation documents so round-trip tests can compare content.

#[cfg(test)]
use ir::{Attachment, ConversationDocument};
use ir::{ImessageExtensions, VendorFields};

/// Prepare a document so two copies can be compared after a round trip.
///
/// Recomputes conversation stats. Drops empty `source` / `imessage` bags.
/// Clears the packaging stem suffix and attachment bytes, which are not part
/// of the JSON content.
#[cfg(test)]
pub(crate) fn normalize_document_for_compare(document: &mut ConversationDocument) {
    for message in &mut document.messages {
        if let Some(source) = message.source.take() {
            message.source = source.into_option();
        }
        if let Some(mut imessage) = message.imessage.take() {
            // CSV packaging omits a single text/run part that equals `text`.
            if crate::write::parts_are_trivial_text_duplicate(
                &message.text,
                imessage.parts.as_ref(),
            ) {
                imessage.parts = None;
            }
            message.imessage = imessage.into_option();
        }
        for attachment in &mut message.attachments {
            clear_attachment_ephemera(attachment);
        }
        empty_to_none(&mut message.sender.identity);
        empty_to_none(&mut message.sender.display_name);
        empty_to_none(&mut message.subject);
    }
    empty_to_none(&mut document.export.owner_identity);
    empty_to_none(&mut document.export.owner_display_name);
    empty_to_none(&mut document.conversation.group_title);
    for p in &mut document.conversation.participants {
        empty_to_none(&mut p.display_name);
    }
    document.packaging_stem_suffix = None;
    document.schema_version = ir::SCHEMA_VERSION;
    document.finalize_stats();
}

/// Drop fields that exist only while packaging (bytes, empty strings).
#[cfg(test)]
fn clear_attachment_ephemera(attachment: &mut Attachment) {
    attachment.bytes = None;
    empty_to_none(&mut attachment.path);
    empty_to_none(&mut attachment.original_name);
    empty_to_none(&mut attachment.mime_type);
    empty_to_none(&mut attachment.digest_sha256);
    empty_to_none(&mut attachment.transcription);
    empty_to_none(&mut attachment.sticker_effect);
}

/// Replace a blank or whitespace-only string with `None`.
#[cfg(test)]
fn empty_to_none(v: &mut Option<String>) {
    if let Some(s) = v.as_ref()
        && s.trim().is_empty()
    {
        *v = None;
    }
}

/// Return `None` when every iMessage-only field is empty.
pub(crate) fn imessage_from_parts(im: ImessageExtensions) -> Option<ImessageExtensions> {
    im.into_option()
}

/// Build [`VendorFields`] from an Android type and a JSON object of extra fields.
///
/// Returns `None` when both pieces are empty. Invalid JSON becomes an empty
/// field map.
pub(crate) fn vendor_fields_from_parts(
    android_type: Option<i32>,
    fields_json: &str,
) -> Option<VendorFields> {
    let fields = if fields_json.trim().is_empty() {
        Default::default()
    } else {
        serde_json::from_str(fields_json).unwrap_or_default()
    };
    VendorFields {
        android_type,
        fields,
    }
    .into_option()
}
