//! Map export API messages into conversation documents.
//!
//! The export API is the Message Crate HTTP server's read path. Each document
//! is later written as JSON Lines (one JSON object per line).

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDateTime};
use message_ir::{
    ConversationDocument, ConversationMeta, ConversationStats, Deletion, EarlierVersion,
    ExportMeta, IrAttachment, IrConversationType, IrDirection, IrImessage, IrMessage,
    IrMessageKind, IrParticipant, IrService, IrSource, Reaction, ReplyTo, SCHEMA_VERSION,
    TimePrecision,
};
use serde_json::json;

use message_crate_api_types::{Attachment, Message, Tapback};

/// Grouping key so messages from the same chat and backup source stay together.
pub fn conversation_key(msg: &Message) -> String {
    format!("{}::{}", msg.source, msg.conversation.chat_identifier)
}

/// Build one conversation document from a seed message and the mapped rows.
///
/// The document's backup date is the seed's `backup_taken_at`, which the
/// Export Run keeps only while every message of the conversation has the
/// same one ([`common_backup_taken_at`]), and none otherwise.
pub fn build_document(
    source: &str,
    seed: &Message,
    messages: Vec<IrMessage>,
) -> ConversationDocument {
    // The server says whether the conversation is a group; the Export Run does
    // not read `conversation_type` to decide it again. It reads it only to
    // tell a conversation of orphaned messages from a one-to-one
    // conversation, as the API type says: written back as one-to-one, its
    // `orphaned:` key would come back as a person (#1095).
    let conversation_type = if seed.conversation.is_group {
        IrConversationType::Group
    } else if IrConversationType::parse(&seed.conversation.conversation_type)
        == IrConversationType::Orphaned
    {
        IrConversationType::Orphaned
    } else {
        IrConversationType::Individual
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
            backup_taken_at_unix_ms: seed
                .backup_taken_at
                .as_deref()
                .and_then(|at| parse_timestamp_unix_ms(at).ok()),
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

/// Clear `seed`'s `backup_taken_at` when `msg`'s differs from it, so the
/// conversation file names a backup date only when every message of the
/// conversation came from that one backup. The file carries one date for
/// all its messages, and any one date would be wrong for some of them when
/// they differ: the newest would make a message decided by an older backup
/// win over a backup made between the two, and would date a message that
/// had none. With no date, an import of the file keeps the rules for files
/// without one.
pub fn common_backup_taken_at(seed: &mut Message, msg: &Message) {
    if msg.backup_taken_at != seed.backup_taken_at {
        seed.backup_taken_at = None;
    }
}

/// Map one exported message into the shared conversation message type.
///
/// # Errors
///
/// Returns an error when the timestamp cannot be parsed.
pub fn to_ir_message(msg: &Message, skip_attachments: bool) -> Result<IrMessage> {
    let timestamp_unix_ms = parse_timestamp_unix_ms(msg.timestamp.as_str())
        .with_context(|| format!("message {} timestamp", msg.id))?;

    let service = IrService::parse(msg.service.as_deref().unwrap_or(""));
    let direction = if msg.is_from_me {
        IrDirection::Outgoing
    } else {
        IrDirection::Incoming
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
    let imessage = IrImessage {
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
        source: IrSource {
            android_type: None,
            fields: source_fields,
        }
        .into_option(),
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
fn participants_from_seed(seed: &Message) -> Vec<IrParticipant> {
    let mut participants = Vec::with_capacity(seed.conversation.participants.len());
    for p in &seed.conversation.participants {
        participants.push(IrParticipant {
            identity: p.identity.clone(),
            // `name` falls back to the raw handle when nothing names the
            // person (ADR-0006). Carrying a bare handle through as a display
            // name would let a later import write it onto a Contact as that
            // person's name, turning a correctly nameless Contact into a
            // wrongly-named one — so only a name distinct from the handle
            // counts as a display name here. A participant with no handle at
            // all has nothing to be identical to, so their name always counts.
            display_name: (p.identity.as_deref() != Some(p.name.as_str())).then(|| p.name.clone()),
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

/// One stored reaction as the conversation file's [`Reaction`]. The server
/// keeps no reactor name, so none is written. A part index no message can
/// have (below zero, or past `u32`) leaves the reaction out rather than
/// moving it onto another part.
fn reaction_from_row(row: &Tapback) -> Option<Reaction> {
    Some(Reaction {
        part_index: u32::try_from(row.part_index).ok()?,
        kind: row.kind.clone(),
        emoji: row.emoji.clone(),
        is_from_me: row.is_from_me,
        reactor_identity: row.sender.clone(),
        reactor_display_name: None,
    })
}

/// Choose SMS, MMS, iMessage, or announcement from service and attachments.
fn infer_kind(msg: &Message, service: IrService) -> IrMessageKind {
    if msg.is_announcement {
        return IrMessageKind::Announcement;
    }
    if !msg.attachments.is_empty() && matches!(service, IrService::Sms | IrService::Rcs) {
        return IrMessageKind::Mms;
    }
    match service {
        IrService::IMessage => IrMessageKind::IMessage,
        IrService::Sms | IrService::Rcs => IrMessageKind::Sms,
        IrService::Whatsapp
        | IrService::Discord
        | IrService::Signal
        | IrService::Telegram
        | IrService::Slack => IrMessageKind::Unknown,
        IrService::Unknown => {
            if msg.attachments.is_empty() {
                IrMessageKind::Sms
            } else {
                IrMessageKind::Mms
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
