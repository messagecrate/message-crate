//! Write [`ConversationDocument`] as JSON, JSON Lines, CSV, or mail.

use crate::util;
use anyhow::{Context, Result, bail};
use ir::{
    ConversationDocument, ConversationHeader, Deletion, IdentityService, ImessageExtensions,
    Message, MessageKind, Transport,
};
use mail::{MailAttachment, MailMessage, MailPackage, Participant, write_mail_package};
use message_crate_core::OutputFormat;
use message_crate_csv::{
    AttachmentCell, ParticipantCell, format_local_ts, json_cell, write_guarded_record,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Unified CSV columns for every exporter (IR v4 projection).
///
/// Apple-only cells are empty for non-iMessage sources. Legacy names
/// (`date_ms`, `contact_name`, `xml_fields_json`) are gone. Use
/// `timestamp_unix_ms`, `sender_display_name`, and `source_fields_json`.
pub const CSV_HEADERS: &[&str] = &[
    "chat_identifier",
    "conversation_type",
    "group_title",
    "participants_json",
    "guid",
    "timestamp",
    "timestamp_utc",
    "timestamp_display",
    "timestamp_unix_ms",
    "time_precision",
    "direction",
    "service",
    "sender_identity",
    "sender_display_name",
    "identity_type",
    "subject",
    "text",
    "attachments_json",
    "reactions_json",
    "deletion",
    "earlier_versions_json",
    "is_reply",
    "reply_to_guid",
    "reply_to_part",
    "message_kind",
    "export_source",
    "export_tool",
    "export_tool_version",
    "owner_identity",
    "owner_display_name",
    "backup_taken_at_unix_ms",
    "exported_at_unix_ms",
    "message_owner_identity",
    "android_type",
    "source_fields_json",
    "read_receipt",
    "send_effect",
    "shared_location",
    "is_announcement",
    "announcement",
    "parts_json",
    "app_json",
    "balloon_bundle_id",
    "balloon_kind",
    "associated_guid",
    "associated_part",
    "tapback_kind",
    "tapback_emoji",
    "tapback_action",
];

/// Write one conversation in a per-chat format.
///
/// A vendor format is written by the crate that owns it, so
/// [`OutputFormat::Xml`] and [`OutputFormat::SmsBackupPlus`] are refused
/// here and go through [`FormatSink::with_archive`](crate::FormatSink::with_archive).
///
/// # Errors
///
/// Returns an error when the directory cannot be created, a file cannot be
/// written, or `format` is a vendor format.
pub fn write_format(
    output_dir: &Path,
    format: OutputFormat,
    mut document: ConversationDocument,
) -> Result<PathBuf> {
    document.finalize_stats();
    match format {
        OutputFormat::Csv => write_conversation_csv(output_dir, &document),
        OutputFormat::Json => write_conversation_json(output_dir, &document),
        OutputFormat::Jsonl => write_conversation_jsonl(output_dir, &document),
        OutputFormat::Eml => {
            write_conversation_mail(output_dir, &document, MailPackage::EmlDirectories)
        }
        OutputFormat::Mbox => write_conversation_mail(output_dir, &document, MailPackage::Mbox),
        OutputFormat::Xml | OutputFormat::SmsBackupPlus => anyhow::bail!(
            "{} is written by the crate that owns it: supply its merged archive to \
             FormatSink instead of writing one conversation",
            format.as_str()
        ),
    }
}

/// Per-conversation JSON artifact (`<stem>.json`).
fn write_conversation_json(output_dir: &Path, document: &ConversationDocument) -> Result<PathBuf> {
    let path = output_dir.join(format!("{}.json", crate::filename_stem(document)));
    let json = serde_json::to_vec_pretty(document).context("serialize ConversationDocument")?;
    file_io::write_atomic(&path, |out| {
        out.write_all(&json)?;
        out.write_all(b"\n")?;
        Ok(())
    })?;
    Ok(path)
}

/// Write `document` as JSON Lines to exactly `path`, atomically.
///
/// Unlike the export writers this does not derive the file name from the
/// document: a caller patching a file it already read must write back to the
/// same path. The write goes through a `.tmp` sibling and a rename, so a
/// reader never sees a half-written conversation.
///
/// # Errors
///
/// Returns an error when the file cannot be created, serialized, or renamed.
pub fn write_conversation_jsonl_to(path: &Path, document: &ConversationDocument) -> Result<()> {
    file_io::write_atomic(path, |out| {
        let header = ConversationHeader::from_document(document);
        serde_json::to_writer(&mut *out, &header).context("serialize JSONL header")?;
        out.write_all(b"\n")?;
        for message in &document.messages {
            serde_json::to_writer(&mut *out, message).context("serialize JSONL message")?;
            out.write_all(b"\n")?;
        }
        Ok(())
    })
}

/// Write `document` as `<stem>.jsonl` under `output_dir` and return that path.
///
/// # Errors
///
/// Returns an error when the file cannot be created or written.
pub fn write_conversation_jsonl(
    output_dir: &Path,
    document: &ConversationDocument,
) -> Result<PathBuf> {
    let path = output_dir.join(format!("{}.jsonl", crate::filename_stem(document)));
    write_conversation_jsonl_to(&path, document)?;
    Ok(path)
}

/// Compact JSON for nested bags; empty string when absent (never the literal `null`).
fn value_cell(v: Option<&Value>) -> String {
    v.filter(|v| !v.is_null())
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

/// CSV `parts_json` cell: only from the iMessage bag, and omit a single plain
/// text/run part that merely duplicates [`Message::text`].
fn parts_cell_for_csv(text: &str, parts: Option<&Value>) -> String {
    if parts_are_trivial_text_duplicate(text, parts) {
        return String::new();
    }
    value_cell(parts)
}

/// True when `parts` is a one-element array whose text equals `message_text`
/// and kind is absent, `run`, or `text`.
pub(crate) fn parts_are_trivial_text_duplicate(message_text: &str, parts: Option<&Value>) -> bool {
    let Some(Value::Array(items)) = parts else {
        return false;
    };
    if items.len() != 1 {
        return false;
    }
    let Some(obj) = items[0].as_object() else {
        return false;
    };
    let Some(part_text) = obj.get("text").and_then(|v| v.as_str()) else {
        return false;
    };
    if part_text != message_text {
        return false;
    }
    matches!(
        obj.get("kind").and_then(|v| v.as_str()),
        None | Some("run" | "text")
    )
}

/// CSV `identity_type` cell: the sender's identity type as the server works
/// it out, from the message's service and the shape
/// [`phone::Handle::parse`] gives the sender identity, so a WhatsApp
/// `123456@lid` is `other`. Empty when the message has no sender identity,
/// or when the sender identity is blank, because a blank one is not an
/// address.
fn sender_identity_type_cell(sender_identity: Option<&str>, service: Transport) -> &'static str {
    sender_identity
        .and_then(phone::Handle::parse)
        .map_or("", |handle| {
            IdentityService::from_transport(service)
                .type_on(handle.kind())
                .as_str()
        })
}

/// Per-conversation CSV using the unified [`CSV_HEADERS`] contract.
///
/// # Errors
///
/// Returns an error when a message's timestamp cannot be formatted or the
/// file cannot be written.
pub(crate) fn write_conversation_csv(
    output_dir: &Path,
    document: &ConversationDocument,
) -> Result<PathBuf> {
    let path = output_dir.join(format!("{}.csv", crate::filename_stem(document)));
    let participants_json = json_cell(
        &document
            .conversation
            .participants
            .iter()
            .map(|p| ParticipantCell {
                identity: p.identity.clone().unwrap_or_default(),
                display_name: p.display_name.clone().unwrap_or_default(),
            })
            .collect::<Vec<_>>(),
    );
    let exported_at = document
        .export
        .exported_at_unix_ms
        .map(|ms| ms.to_string())
        .unwrap_or_default();

    file_io::write_atomic(&path, |out| {
        let mut wtr = csv::Writer::from_writer(out);
        wtr.write_record(CSV_HEADERS)
            .with_context(|| format!("write header {}", path.display()))?;
        for message in &document.messages {
            let cells = MessageCells::new(message)?;
            // Every cell goes through `written_cell`, so a spreadsheet shows a
            // message that starts with `=` as text rather than running it.
            write_guarded_record(
                &mut wtr,
                csv_record(document, &participants_json, &exported_at, message, &cells),
            )
            .with_context(|| format!("write row {}", path.display()))?;
        }
        wtr.flush()?;
        Ok(())
    })?;

    Ok(path)
}

/// The cells of one message that have to be built rather than borrowed:
/// formatted times, JSON columns, and the `imessage` bag's columns.
struct MessageCells {
    ts_local: String,
    ts_utc: String,
    ts_display: String,
    timestamp_unix_ms: String,
    /// The message's Backup Date, blank when it has none.
    backup_taken_at: String,
    attachments_json: String,
    reactions_json: String,
    earlier_versions_json: String,
    reply_to_part: String,
    android_type: String,
    source_fields_json: String,
    imessage: ImessageCells,
}

impl MessageCells {
    /// # Errors
    ///
    /// Returns an error when the timestamp is outside the representable range.
    fn new(message: &Message) -> Result<Self> {
        let secs = message.timestamp_unix_ms.div_euclid(1000);
        let (ts_local, ts_utc, ts_display) = format_local_ts(secs).ok_or_else(|| {
            anyhow::anyhow!("invalid timestamp_unix_ms {}", message.timestamp_unix_ms)
        })?;
        let attachment_cells: Vec<AttachmentCell> = message
            .attachments
            .iter()
            .map(|a| AttachmentCell {
                meta: a.into(),
                is_sticker: a.is_sticker,
                transcription: a.transcription.clone(),
                sticker_effect: a.sticker_effect.clone(),
            })
            .collect();
        Ok(Self {
            ts_local,
            ts_utc,
            ts_display,
            timestamp_unix_ms: message.timestamp_unix_ms.to_string(),
            backup_taken_at: message
                .backup_taken_at_unix_ms
                .map(|ms| ms.to_string())
                .unwrap_or_default(),
            attachments_json: json_cell(&attachment_cells),
            reactions_json: if message.reactions.is_empty() {
                String::new()
            } else {
                json_cell(&message.reactions)
            },
            earlier_versions_json: if message.edits.is_empty() {
                String::new()
            } else {
                json_cell(&message.edits)
            },
            reply_to_part: number_cell(message.reply_to.as_ref().and_then(|r| r.part_index)),
            android_type: message
                .source
                .as_ref()
                .and_then(|s| s.android_type)
                .map(|n| n.to_string())
                .unwrap_or_default(),
            source_fields_json: message
                .source
                .as_ref()
                .filter(|s| !s.fields.is_empty())
                .map(|s| serde_json::to_string(&s.fields).unwrap_or_default())
                .unwrap_or_default(),
            imessage: ImessageCells::new(&message.text, message.imessage.as_ref()),
        })
    }
}

/// The `imessage` bag's columns as cell text; every cell is blank when the
/// message has no bag.
#[derive(Default)]
struct ImessageCells {
    read_receipt: String,
    send_effect: String,
    shared_location: String,
    announcement: String,
    parts_json: String,
    app_json: String,
    balloon_bundle_id: String,
    balloon_kind: String,
    associated_guid: String,
    associated_part: String,
    tapback_kind: String,
    tapback_emoji: String,
    tapback_action: String,
}

impl ImessageCells {
    /// `text` is the message text, which decides whether the parts column
    /// repeats it and is therefore left blank.
    fn new(text: &str, im: Option<&ImessageExtensions>) -> Self {
        let Some(im) = im else {
            return Self {
                parts_json: parts_cell_for_csv(text, None),
                ..Self::default()
            };
        };
        Self {
            read_receipt: text_cell(im.read_receipt_rfc3339.as_deref()),
            send_effect: text_cell(im.send_effect.as_deref()),
            shared_location: text_cell(im.shared_location.as_deref()),
            announcement: text_cell(im.announcement.as_deref()),
            parts_json: parts_cell_for_csv(text, im.parts.as_ref()),
            app_json: value_cell(im.app.as_ref()),
            balloon_bundle_id: text_cell(im.balloon_bundle_id.as_deref()),
            balloon_kind: text_cell(im.balloon_kind.as_deref()),
            associated_guid: text_cell(im.associated_guid.as_deref()),
            associated_part: number_cell(im.associated_part),
            tapback_kind: text_cell(im.tapback_kind.as_deref()),
            tapback_emoji: text_cell(im.tapback_emoji.as_deref()),
            tapback_action: text_cell(im.tapback_action.as_deref()),
        }
    }
}

/// An optional string as a cell: the string, or blank.
fn text_cell(value: Option<&str>) -> String {
    value.unwrap_or_default().to_string()
}

/// An optional number as a cell: the number, or blank.
fn number_cell(value: Option<u32>) -> String {
    value.map(|n| n.to_string()).unwrap_or_default()
}

/// `true` or `false`.
fn bool_cell(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// One CSV row in [`CSV_HEADERS`] order.
fn csv_record<'a>(
    document: &'a ConversationDocument,
    participants_json: &'a str,
    exported_at: &'a str,
    message: &'a Message,
    cells: &'a MessageCells,
) -> [&'a str; 49] {
    let im = &cells.imessage;
    [
        document.conversation.chat_identifier.as_str(),
        document.conversation.conversation_type.as_str(),
        document.conversation.group_title.as_deref().unwrap_or(""),
        participants_json,
        message.guid.as_str(),
        cells.ts_local.as_str(),
        cells.ts_utc.as_str(),
        cells.ts_display.as_str(),
        cells.timestamp_unix_ms.as_str(),
        message.time_precision.as_str(),
        message.direction.as_str(),
        message.service.as_str(),
        message.sender_identity.as_deref().unwrap_or(""),
        message.sender_display_name.as_deref().unwrap_or(""),
        sender_identity_type_cell(message.sender_identity.as_deref(), message.service),
        message.subject.as_deref().unwrap_or(""),
        message.text.as_str(),
        cells.attachments_json.as_str(),
        cells.reactions_json.as_str(),
        message.deletion.map_or("", Deletion::as_str),
        cells.earlier_versions_json.as_str(),
        bool_cell(message.reply_to.is_some()),
        message
            .reply_to
            .as_ref()
            .and_then(|r| r.guid.as_deref())
            .unwrap_or(""),
        cells.reply_to_part.as_str(),
        message.message_kind.as_str(),
        document.export.source.as_str(),
        document.export.tool.as_str(),
        document.export.tool_version.as_str(),
        document.export.owner_identity.as_deref().unwrap_or(""),
        document.export.owner_display_name.as_deref().unwrap_or(""),
        cells.backup_taken_at.as_str(),
        exported_at,
        message.owner_identity.as_deref().unwrap_or(""),
        cells.android_type.as_str(),
        cells.source_fields_json.as_str(),
        im.read_receipt.as_str(),
        im.send_effect.as_str(),
        im.shared_location.as_str(),
        bool_cell(message.message_kind == MessageKind::Announcement),
        im.announcement.as_str(),
        im.parts_json.as_str(),
        im.app_json.as_str(),
        im.balloon_bundle_id.as_str(),
        im.balloon_kind.as_str(),
        im.associated_guid.as_str(),
        im.associated_part.as_str(),
        im.tapback_kind.as_str(),
        im.tapback_emoji.as_str(),
        im.tapback_action.as_str(),
    ]
}

/// Write a conversation as EML files or one mbox, by `package`.
fn write_conversation_mail(
    output_dir: &Path,
    document: &ConversationDocument,
    package: MailPackage,
) -> Result<PathBuf> {
    let messages = document_to_mail_messages(document, output_dir)?;
    if messages.is_empty() {
        bail!("conversation has no messages");
    }
    write_mail_package(
        output_dir,
        &crate::filename_stem(document),
        package,
        &messages,
    )
}

/// Build [`MailMessage`] list from IR (reads attachment bytes from disk when missing).
///
/// # Errors
///
/// Returns an error when an attachment file cannot be read from disk.
pub fn document_to_mail_messages(
    document: &ConversationDocument,
    output_dir: &Path,
) -> Result<Vec<MailMessage>> {
    let participants: Vec<Participant> = document
        .conversation
        .participants
        .iter()
        .map(|p| Participant {
            identity: p.identity.clone().unwrap_or_default(),
            display_name: p.display_name.clone(),
        })
        .collect();

    let mut out = Vec::with_capacity(document.messages.len());
    for message in &document.messages {
        let mut attachments = Vec::with_capacity(message.attachments.len());
        for a in &message.attachments {
            let bytes = util::load_attachment_bytes(a, output_dir)?;
            attachments.push(MailAttachment {
                bytes,
                meta: a.into(),
                is_sticker: a.is_sticker,
                transcription: a.transcription.clone(),
                sticker_effect: a.sticker_effect.clone(),
            });
        }

        out.push(MailMessage {
            chat_identifier: document.conversation.chat_identifier.clone(),
            conversation_type: document.conversation.conversation_type.as_str().to_string(),
            group_title: document.conversation.group_title.clone(),
            participants: participants.clone(),
            owner_identity: document.export.owner_identity.clone().unwrap_or_default(),
            owner_display_name: document.export.owner_display_name.clone(),
            export_source: document.export.source.clone(),
            export_tool: document.export.tool.clone(),
            export_tool_version: document.export.tool_version.clone(),
            exported_at_unix_ms: document.export.exported_at_unix_ms,
            message: message.clone(),
            attachments,
        });
    }
    Ok(out)
}
