//! Map export API messages into conversation documents.
//!
//! The export API is the Message Crate HTTP server's read path. Each document
//! is later written as JSON Lines (one JSON object per line).

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDateTime};
// `message_crate_api_types` and `message_ir` both name a `Message` and an
// `Attachment`; the conversation file's are imported here as `IrMessage` and
// `IrAttachment`.
use message_ir::{
    Attachment as IrAttachment, ConversationDocument, ConversationMeta, ConversationStats,
    ConversationType, Deletion, Direction, EarlierVersion, ExportMeta, ImessageExtensions,
    Message as IrMessage, MessageKind, Participant, Reaction, ReplyTo, SCHEMA_VERSION,
    TimePrecision, Transport, VendorFields,
};
use serde_json::json;

use message_crate_api_types::{Attachment, Message, Tapback};

/// Grouping key so messages from the same chat and backup source stay together.
pub fn conversation_key(msg: &Message) -> String {
    format!("{}::{}", msg.source, msg.conversation.chat_identifier)
}

/// Build one conversation document from a seed message and the mapped rows,
/// written at `exported_at_unix_ms`, the Export Date the run's caller gave.
/// Each message carries its own stored Backup Date ([`to_ir_message`]), so
/// a conversation stored from several backups keeps every message's date.
pub fn build_document(
    source: &str,
    seed: &Message,
    messages: Vec<IrMessage>,
    exported_at_unix_ms: i64,
) -> ConversationDocument {
    // The server says whether the conversation is a group; the Export Run does
    // not read `conversation_type` to decide it again. It reads it only to
    // tell a conversation of orphaned messages from a one-to-one
    // conversation, as the API type says: written back as one-to-one, its
    // `orphaned:` key would come back as a person (#1095).
    let conversation_type = if seed.conversation.is_group {
        ConversationType::Group
    } else if ConversationType::parse(&seed.conversation.conversation_type)
        == ConversationType::Orphaned
    {
        ConversationType::Orphaned
    } else {
        ConversationType::OneToOne
    };
    let participants = participants_from_seed(seed);
    let mut attachment_count = 0u64;
    let mut first_ts = None;
    let mut last_ts = None;
    for m in &messages {
        attachment_count += m.attachments.len() as u64;
        first_ts = Some(first_ts.map_or(m.timestamp_unix_ms, |t: i64| t.min(m.timestamp_unix_ms)));
        last_ts = Some(last_ts.map_or(m.timestamp_unix_ms, |t: i64| t.max(m.timestamp_unix_ms)));
    }

    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: source.to_string(),
            tool: "message-crate".into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
            owner_identity: shared_owner(&messages),
            owner_display_name: Some("Me".into()),
            exported_at_unix_ms: Some(exported_at_unix_ms),
        },
        conversation: ConversationMeta {
            chat_identifier: seed.conversation.chat_identifier.clone(),
            conversation_type,
            group_title: seed.conversation.group_title.clone(),
            participants,
            stats: ConversationStats {
                message_count: messages.len() as u64,
                attachment_count,
                first_timestamp_unix_ms: first_ts,
                last_timestamp_unix_ms: last_ts,
            },
        },
        messages,
        packaging_stem_suffix: None,
    }
}

/// Map one exported message into the shared conversation message type,
/// with the Backup Date the server stored for it, or none where it stored
/// none.
///
/// # Errors
///
/// Returns an error when the timestamp or the backup date cannot be parsed.
pub fn to_ir_message(msg: &Message, skip_attachments: bool) -> Result<IrMessage> {
    let timestamp_unix_ms = parse_timestamp_unix_ms(msg.timestamp.as_str())
        .with_context(|| format!("message {} timestamp", msg.id))?;
    let backup_taken_at_unix_ms = msg
        .backup_taken_at
        .as_deref()
        .map(parse_timestamp_unix_ms)
        .transpose()
        .with_context(|| format!("message {} backup date", msg.id))?;

    let service = Transport::parse(msg.service.as_deref().unwrap_or(""));
    let direction = if msg.is_from_me {
        Direction::Outgoing
    } else {
        Direction::Incoming
    };
    let message_kind = infer_kind(msg, service);

    let attachments = if skip_attachments {
        Vec::new()
    } else {
        msg.attachments
            .iter()
            .map(to_ir_attachment)
            .collect::<Vec<_>>()
    };

    let reply_to = msg.reply_to.as_ref().map(|reply_to| ReplyTo {
        guid: reply_to.guid.clone(),
        part_index: reply_to.part_index.and_then(|n| u32::try_from(n).ok()),
    });
    let imessage = ImessageExtensions {
        // An announcement with no text carries no information, so drop it.
        announcement: msg
            .is_announcement
            .then(|| msg.text.clone().unwrap_or_default())
            .filter(|text| !text.is_empty()),
        ..Default::default()
    };

    // Keep the server's row id and source name so a later import can trace
    // each message back to the server it came from.
    let mut source_fields = serde_json::Map::new();
    source_fields.insert("server_message_id".into(), json!(msg.id));
    source_fields.insert("server_source".into(), json!(msg.source));

    Ok(IrMessage {
        guid: msg.guid.clone(),
        timestamp_unix_ms,
        time_precision: time_precision_from_api(msg.time_precision),
        direction,
        service,
        message_kind,
        sender_identity: msg.sender.clone(),
        sender_display_name: None,
        sender_country: None,
        owner_identity: msg.owner.clone().filter(|o| !o.trim().is_empty()),
        subject: msg.subject.clone(),
        text: msg.text.clone().unwrap_or_default(),
        attachments,
        reactions: msg.tapbacks.iter().filter_map(reaction_from_row).collect(),
        deletion: msg.deletion.map(deletion_from_api),
        edits: msg
            .earlier_versions
            .iter()
            .map(earlier_version_from_api)
            .collect::<Result<_>>()
            .with_context(|| format!("message {} earlier versions", msg.id))?,
        reply_to,
        imessage: imessage.into_option(),
        source: VendorFields {
            android_type: None,
            fields: source_fields,
        }
        .into_option(),
        backup_taken_at_unix_ms,
    })
}

/// One earlier version the server stores, as the conversation file writes
/// it: its time back in milliseconds.
fn earlier_version_from_api(
    version: &message_crate_api_types::EarlierVersion,
) -> Result<EarlierVersion> {
    Ok(EarlierVersion {
        part_index: u32::try_from(version.part_index)
            .with_context(|| format!("part index {}", version.part_index))?,
        text: version.text.clone(),
        edited_at_unix_ms: version
            .edited_at
            .as_deref()
            .map(parse_timestamp_unix_ms)
            .transpose()?,
    })
}

/// The precision the server stored, as the conversation file writes it.
fn time_precision_from_api(precision: message_crate_api_types::TimePrecision) -> TimePrecision {
    match precision {
        message_crate_api_types::TimePrecision::Seconds => TimePrecision::Seconds,
        message_crate_api_types::TimePrecision::Milliseconds => TimePrecision::Milliseconds,
    }
}

/// The mark a server message carries, as the conversation file writes it.
fn deletion_from_api(deletion: message_crate_api_types::Deletion) -> Deletion {
    match deletion {
        message_crate_api_types::Deletion::DeletedInSourceApp => Deletion::DeletedInSourceApp,
        message_crate_api_types::Deletion::Unsent => Deletion::Unsent,
    }
}

/// The owner address every message of a conversation carries, when they all
/// carry the same one; `None` when one has none or two differ. Each message
/// keeps its own address either way, so a conversation held at two of the
/// holder's addresses keeps the split.
fn shared_owner(messages: &[IrMessage]) -> Option<String> {
    let first = messages.first()?.owner_identity.as_deref()?;
    messages
        .iter()
        .all(|m| m.owner_identity.as_deref() == Some(first))
        .then(|| first.to_string())
}

/// Copy participant handles and display names from the seed export message.
fn participants_from_seed(seed: &Message) -> Vec<Participant> {
    let mut participants = Vec::with_capacity(seed.conversation.participants.len());
    for p in &seed.conversation.participants {
        participants.push(Participant {
            identity: p.identity.clone(),
            // `name` falls back to the raw handle when nothing names the
            // person (ADR-0006). Carrying a bare handle through as a display
            // name would let a later import write it onto a Contact as that
            // person's name, turning a correctly nameless Contact into a
            // wrongly-named one. So only a name distinct from the handle
            // counts as a display name here. A participant with no handle at
            // all has nothing to be identical to, so their name always counts.
            display_name: (p.identity.as_deref() != Some(p.name.as_str())).then(|| p.name.clone()),
            country: None,
        });
    }
    participants
}

/// Where an export writes one attachment, relative to the output directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPath {
    /// The path the file is written at and the conversation file names:
    /// the server's path when the check accepts it, else
    /// `attachments/{sha256}`. `None` when the attachment has neither a usable
    /// path nor a fingerprint.
    pub rel: Option<String>,
    /// The server's path, as sent, when the check refused it.
    pub refused: Option<String>,
}

/// Choose where an attachment is written under the output directory.
///
/// The server's path is input: an import that reuses a stored fingerprint
/// never read the file, so the server can hold `../x` or an absolute path.
/// Joined onto the output directory, such a path writes outside it, so it goes
/// through [`message_ir::safe_attachment_path`] like every other reader of an
/// attachment path. A refused path is not used: the attachment falls back to
/// `attachments/{sha256}`, the name an attachment with no path gets.
pub fn export_path(att: &Attachment) -> ExportPath {
    let by_fingerprint = att
        .sha256
        .as_deref()
        .and_then(message_ir::trimmed)
        .map(|sha| format!("attachments/{sha}"));
    let Some(path) = att.path.as_deref().and_then(message_ir::trimmed) else {
        return ExportPath {
            rel: by_fingerprint,
            refused: None,
        };
    };
    // The base is empty because only the verdict matters here: the caller
    // joins the accepted path onto the output directory itself.
    if message_ir::safe_attachment_path(Path::new(""), path).is_ok() {
        ExportPath {
            rel: Some(path.to_string()),
            refused: None,
        }
    } else {
        ExportPath {
            rel: by_fingerprint,
            refused: att.path.clone(),
        }
    }
}

/// Map one server attachment record onto the shared attachment type.
fn to_ir_attachment(att: &Attachment) -> IrAttachment {
    IrAttachment {
        path: export_path(att).rel,
        original_name: att.original_name.clone(),
        mime_type: att.mime_type.clone(),
        digest_sha256: att.sha256.clone(),
        is_sticker: att.is_sticker,
        transcription: att.transcription.clone(),
        sticker_effect: None,
        // The server's attachment shape carries no byte length.
        size_bytes: None,
        missing_reason: att.missing_reason.clone(),
        bytes: None,
    }
}

/// One stored reaction as the conversation file's [`Reaction`], with the
/// reactor's name when the server holds one in place of an address. A part
/// index no message can
/// have (below zero, or past `u32`) leaves the reaction out rather than
/// moving it onto another part.
fn reaction_from_row(row: &Tapback) -> Option<Reaction> {
    Some(Reaction {
        part_index: u32::try_from(row.part_index).ok()?,
        kind: row.kind.clone(),
        emoji: row.emoji.clone(),
        is_from_me: row.is_from_me,
        reactor_identity: row.sender.clone(),
        reactor_display_name: row.sender_display_name.clone(),
    })
}

/// Choose SMS, MMS, iMessage, or announcement from service and attachments.
fn infer_kind(msg: &Message, service: Transport) -> MessageKind {
    if msg.is_announcement {
        return MessageKind::Announcement;
    }
    if !msg.attachments.is_empty() && matches!(service, Transport::Sms | Transport::Rcs) {
        return MessageKind::Mms;
    }
    match service {
        Transport::IMessage => MessageKind::IMessage,
        Transport::Sms | Transport::Rcs => MessageKind::Sms,
        Transport::Whatsapp
        | Transport::Discord
        | Transport::Signal
        | Transport::Telegram
        | Transport::Slack => MessageKind::Unknown,
        Transport::Unknown => {
            if msg.attachments.is_empty() {
                MessageKind::Sms
            } else {
                MessageKind::Mms
            }
        }
    }
}

/// Parse a server timestamp into milliseconds since Unix epoch.
///
/// Accepts a millisecond or second integer, RFC 3339, or a few common server
/// date strings without a timezone (treated as UTC).
///
/// # Errors
///
/// Returns an error when the string is empty or none of the formats match.
fn parse_timestamp_unix_ms(raw: &str) -> Result<i64> {
    let t = raw.trim();
    if t.is_empty() {
        bail!("empty timestamp");
    }
    if let Ok(ms) = t.parse::<i64>() {
        // Heuristic: seconds vs millis.
        // Any numeric value below 10^10 is a seconds timestamp (year 2286 in
        // seconds, well beyond any real SMS data). Values at or above 10^10
        // are millisecond timestamps (10^10 ms is 1970-04-26, long before any
        // SMS, so real data won't hit the ambiguity).
        return Ok(if ms.abs() < 10_000_000_000 {
            ms.saturating_mul(1000)
        } else {
            ms
        });
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Ok(dt.timestamp_millis());
    }
    // Common server form without offset: treat as UTC.
    if let Ok(ndt) = NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(ndt.and_utc().timestamp_millis());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S") {
        return Ok(ndt.and_utc().timestamp_millis());
    }
    // Last resort: chrono's RFC3339-ish with space
    if let Ok(dt) = DateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S %z") {
        return Ok(dt.timestamp_millis());
    }
    bail!("unrecognized timestamp: {t}");
}

#[cfg(test)]
mod tests;
