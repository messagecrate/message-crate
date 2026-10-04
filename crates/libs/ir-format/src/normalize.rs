//! Prepare conversation documents so round-trip tests can compare content.

#[cfg(test)]
use message_ir::{ConversationDocument, IrAttachment};
use message_ir::{IrImessage, IrSource};

/// Prepare a document so two copies can be compared after a round trip.
///
/// Recomputes conversation stats. Drops empty `source` / `imessage` bags.
/// Clears the packaging stem suffix and attachment bytes, which are not part
/// of the JSON content.
#[cfg(test)]
pub(crate) fn normalize_document_for_compare(doc: &mut ConversationDocument) {
    for msg in &mut doc.messages {
        if let Some(source) = msg.source.take() {
            msg.source = source.into_option();
        }
        if let Some(mut imessage) = msg.imessage.take() {
            // CSV packaging omits a single text/run part that equals `text`.
            if crate::write::parts_are_trivial_text_duplicate(&msg.text, imessage.parts.as_ref()) {
                imessage.parts = None;
            }
            msg.imessage = imessage.into_option();
        }
        for att in &mut msg.attachments {
            clear_attachment_ephemera(att);
        }
        empty_to_none(&mut msg.sender_identity);
        empty_to_none(&mut msg.sender_display_name);
        empty_to_none(&mut msg.subject);
    }
    empty_to_none(&mut doc.export.owner_identity);
    empty_to_none(&mut doc.export.owner_display_name);
    empty_to_none(&mut doc.conversation.group_title);
    for p in &mut doc.conversation.participants {
        empty_to_none(&mut p.display_name);
    }
    doc.packaging_stem_suffix = None;
    doc.schema_version = message_ir::SCHEMA_VERSION;
    doc.finalize_stats();
}

/// Drop fields that exist only while packaging (bytes, empty strings).
#[cfg(test)]
fn clear_attachment_ephemera(att: &mut IrAttachment) {
    att.bytes = None;
    empty_to_none(&mut att.path);
    empty_to_none(&mut att.original_name);
    empty_to_none(&mut att.mime_type);
    empty_to_none(&mut att.digest_sha256);
    empty_to_none(&mut att.transcription);
    empty_to_none(&mut att.sticker_effect);
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
pub(crate) fn imessage_from_parts(im: IrImessage) -> Option<IrImessage> {
    im.into_option()
}

/// Build [`IrSource`] from an Android type and a JSON object of extra fields.
///
/// Returns `None` when both pieces are empty. Invalid JSON becomes an empty
/// field map.
pub(crate) fn source_from_parts(android_type: Option<i32>, fields_json: &str) -> Option<IrSource> {
    let fields = if fields_json.trim().is_empty() {
        Default::default()
    } else {
        serde_json::from_str(fields_json).unwrap_or_default()
    };
    IrSource {
        android_type,
        fields,
    }
    .into_option()
}
