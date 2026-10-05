//! Attachment helpers for the emitter.

use message_ir::{IrAttachment, PendingAttachment, PendingMessage};

/// The `extra` key of the source file of a message's `index`-th attachment.
pub(super) fn attachment_source_key(index: usize) -> String {
    format!("attachment_source.{index}")
}

/// The `extra` key of the content digest of a message's `index`-th
/// attachment, set only where `Ingest::tell_apart_files_of_one_name` needs
/// it.
pub(super) fn attachment_content_key(index: usize) -> String {
    format!("attachment_content.{index}")
}

/// The `extra` key that marks a message's `index`-th attachment as a row
/// with no file in a group `Ingest::tell_apart_files_of_one_name` hashed:
/// another export of the chat holds the files, so the row matches whichever
/// of them it repeats.
pub(super) fn attachment_matches_any_file_key(index: usize) -> String {
    format!("attachment_matches_any_file.{index}")
}

/// What tells a message's attachments apart, for its id and for the dedupe
/// step: the digest of the file's content where the run hashed it
/// (`Ingest::tell_apart_files_of_one_name`), else the attachment's digest,
/// else its path, which for an iMazing row is its `Attachment` cell. An
/// attachment marked to match any file gives nothing, so the dedupe step
/// treats its message as a copy of any of the hashed ones.
pub(super) fn attachment_digests(msg: &PendingMessage) -> Vec<String> {
    let mut digests: Vec<String> = msg
        .attachments
        .iter()
        .enumerate()
        .filter(|(index, _)| !msg.extra_flag(&attachment_matches_any_file_key(*index)))
        .map(|(index, a)| {
            message_ir::trimmed(msg.extra_str(&attachment_content_key(index)))
                .map(str::to_string)
                .or_else(|| a.digest_sha256.clone())
                .unwrap_or_else(|| a.rel_path.clone())
        })
        .collect();
    digests.sort();
    digests
}

/// Map a staged attachment onto the shared [`IrAttachment`] shape.
pub(super) fn pending_attachment_to_ir(
    a: &PendingAttachment,
    msg: &PendingMessage,
) -> IrAttachment {
    IrAttachment {
        path: a
            .rel_path
            .starts_with("attachments/")
            .then(|| a.rel_path.clone()),
        original_name: a.name_hint.clone(),
        mime_type: a.mime_type(),
        digest_sha256: a.digest_sha256.clone(),
        is_sticker: msg.extra_flag("is_sticker"),
        transcription: msg.extra_opt("transcription"),
        sticker_effect: msg.extra_opt("sticker_effect"),
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    }
}
