//! Import-side records mapped from message-ir JSONL, and the one text form
//! of a stored message time (`utc_timestamp_text`), which search day bounds
//! use too.

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use message_ir::{
    ConversationHeader, Deletion, EarlierVersion, HandleService, HandleType, IrAttachment,
    IrDirection, IrMessage, IrMessageKind, IrParticipant, Reaction, ReplyTo,
    check_schema_version_in_json, nonempty, trimmed,
};
use phone::Handle;
use serde_json::Value;

use crate::config::validate_source_id;
use crate::imports_api::ImportFailure;

/// One JSONL conversation after IR → database-row mapping.
#[derive(Debug, Clone)]
pub enum ExportRecord {
    /// A conversation header record.
    Conversation(ConversationRecord),
    /// One message record.
    Message(MessageRecord),
}

/// The conversation header of one JSONL conversation.
#[derive(Debug, Clone)]
pub struct ConversationRecord {
    /// The line the header is on in its file or batch, counted from 1 with
    /// blank lines included, so a refusal of the header names it.
    pub line: usize,
    /// The conversation's identifier as the export wrote it.
    pub chat_identifier: String,
    /// Platform service, e.g. `imessage`.
    pub service: Option<String>,
    /// `individual` or `group`.
    pub conversation_type: String,
    /// Group label, when set.
    pub group_title: Option<String>,
    /// Participants of the conversation.
    pub participants: Vec<ParticipantRecord>,
    /// IR `export.source` — used as `messages.source` for directory import.
    pub export_source: Option<String>,
}

impl ConversationRecord {
    /// The source id a directory import files this conversation under: its
    /// header's `export.source`, trimmed.
    ///
    /// # Errors
    ///
    /// Refuses the header on its line when `export.source` is missing or
    /// blank, or is not a valid source id: the sender's to fix in the file.
    pub fn directory_source(&self) -> Result<&str, ImportFailure> {
        let refuse = |detail: String| ImportFailure::Invalid {
            line: self.line,
            detail,
        };
        let Some(source) = self.export_source.as_deref().and_then(trimmed) else {
            return Err(refuse(format!(
                "conversation '{}' has no export.source, which a directory import needs \
                 unless --source names one",
                self.chat_identifier
            )));
        };
        validate_source_id(source)
            .map_err(|err| refuse(format!("export.source '{source}' is not valid: {err:#}")))?;
        Ok(source)
    }
}

/// One participant of an imported conversation.
#[derive(Debug, Clone)]
pub struct ParticipantRecord {
    /// Raw identity value. For a person the source named with no address it
    /// is the name, and `handle_type` is `Other`.
    pub handle: String,
    /// Display-name alias, when the export supplied one.
    pub name_alias: Option<String>,
    /// Handle type (phone, email, username, or other).
    pub handle_type: Option<HandleType>,
}

/// One message of an imported conversation.
#[derive(Debug, Clone)]
pub struct MessageRecord {
    /// The message's id from the export, never empty: Apple's own for Apple
    /// Messages, otherwise the exporter's `MessageGuid`. Production skips a
    /// guid it already holds, which is what lets a batch be sent again.
    pub guid: String,
    /// The line the message is on in its file or batch, counted from 1 with
    /// blank lines included, so a refusal of one of its attachments names it.
    pub line: usize,
    /// The instant the message was sent, to the millisecond: RFC 3339 in UTC
    /// with three fractional digits and a `Z` suffix
    /// (`2015-03-12T18:04:22.250Z`).
    pub timestamp: String,
    /// True for messages sent by the account owner.
    pub is_from_me: bool,
    /// Sender handle for incoming messages: the address, or the name when the
    /// source named the sender with no address (`sender_handle_type` is then
    /// `Other`).
    pub sender: Option<String>,
    /// The sender's identity type read from the address alone (phone, email
    /// or other). Staging prefers the type the header gives a participant
    /// with the same address.
    pub sender_handle_type: Option<HandleType>,
    /// The account holder's own address on this message, sent from or
    /// received at: the message's owner handle, else the header's.
    pub owner: Option<String>,
    /// Per-message transport (`sms` / `imessage` / `rcs` / `whatsapp` / …).
    pub service: Option<String>,
    /// Subject line, when set.
    pub subject: Option<String>,
    /// Body text, when present.
    pub text: Option<String>,
    /// True for group announcements.
    pub is_announcement: bool,
    /// Announcement text when `is_announcement`.
    pub announcement: Option<String>,
    /// Attachments on this message.
    pub attachments: Vec<AttachmentRecord>,
    /// Reactions on this message.
    pub tapbacks: Vec<TapbackRecord>,
    /// The message a reply quotes; `None` for a message that is not a reply.
    /// An empty guid is read as none.
    pub reply_to: Option<ReplyTo>,
    /// Deleted in the source app or Unsent; `None` for neither.
    pub deletion: Option<Deletion>,
    /// The earlier versions of an edited message, in the order the file
    /// lists them; `text` is the final version.
    pub earlier_versions: Vec<EarlierVersionRecord>,
}

/// One earlier version of one part of an edited message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarlierVersionRecord {
    /// The part of the message this version belongs to.
    pub part_index: i64,
    /// The part's text in this version.
    pub text: String,
    /// When this version was written, to the millisecond in the form
    /// `MessageRecord::timestamp` takes; `None` when the source does not
    /// record it.
    pub edited_at: Option<String>,
}

/// One attachment of an imported message.
#[derive(Debug, Clone)]
pub struct AttachmentRecord {
    /// Path inside the export.
    pub path: Option<String>,
    /// File name from the export.
    pub original_name: Option<String>,
    /// MIME type, when known.
    pub mime_type: Option<String>,
    /// Content fingerprint, when the exporter computed one.
    pub sha256: Option<String>,
    /// True for sticker files.
    pub is_sticker: bool,
    /// OCR/ASR transcription, when the exporter produced one.
    pub transcription: Option<String>,
    /// File size in bytes, when known.
    pub size_bytes: Option<u64>,
    /// Why the file is missing, when it is.
    pub missing_reason: Option<String>,
}

/// One tapback reaction on an imported message.
#[derive(Debug, Clone)]
pub struct TapbackRecord {
    /// Attachment part the reaction applies to.
    pub part_index: i64,
    /// Reaction type, e.g. `love`.
    pub kind: String,
    /// Emoji form of the reaction, when one exists.
    pub emoji: Option<String>,
    /// True when the account owner reacted.
    pub is_from_me: bool,
    /// Reactor handle for incoming reactions.
    pub sender: Option<String>,
}

/// Strip Apple's attachment object-replacement character (U+FFFC) from body text.
pub fn clean_body(text: Option<&str>) -> Option<String> {
    text.map(|s| s.replace('\u{FFFC}', "").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Parse message-ir JSONL lines into import records.
///
/// Accepts one or more concatenated conversations (each: header line, then
/// message lines). Upload clients batch multiple conversations this way.
///
/// # Errors
///
/// Returns the [`ImportFailure`] the first broken line gives, or one naming
/// every message without a guid. Parsing reads nothing but `lines`, so every
/// way it can stop is the sender's to fix, and the error type has no other
/// kind.
pub fn parse_ir_lines(
    lines: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<ExportRecord>, ImportFailure> {
    use crate::imports_api::MISSING_GUID_LINES_NAMED;

    let mut out = Vec::new();
    let mut saw_header = false;
    // The owner the current conversation's header names, for messages that
    // do not name their own.
    let mut header_owner: Option<String> = None;
    // Messages without a guid, counted and named up to a limit, so one
    // refusal names every line to fix rather than the first.
    let mut missing_guid_lines = Vec::new();
    let mut missing_guid_total = 0usize;
    for (i, line) in lines.into_iter().enumerate() {
        let line = line.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        let line_no = i + 1;
        let value: Value = serde_json::from_str(line).map_err(|e| ImportFailure::NotJson {
            line: line_no,
            detail: e.to_string(),
        })?;
        if is_ir_header(&value) {
            // The version is checked before the header is deserialized, so a
            // file from another schema version is refused by its version and
            // not by whichever current field it lacks.
            check_schema_version_in_json(line).map_err(|refusal| ImportFailure::SchemaVersion {
                refusal,
                line: line_no,
            })?;
            let header: ConversationHeader =
                serde_json::from_value(value).map_err(|e| ImportFailure::Invalid {
                    line: line_no,
                    detail: format!("the conversation header is not valid: {e}"),
                })?;
            out.push(ExportRecord::Conversation(conversation_from_ir(
                &header, line_no,
            )));
            header_owner = header.export.owner_identity.as_deref().and_then(nonempty);
            saw_header = true;
        } else {
            if !saw_header {
                return Err(ImportFailure::Invalid {
                    line: line_no,
                    detail: "a message appears before the conversation header".into(),
                });
            }
            let msg: IrMessage =
                serde_json::from_value(value).map_err(|e| ImportFailure::Invalid {
                    line: line_no,
                    detail: format!("the message is not valid: {e}"),
                })?;
            // A reaction row is not a message. The reaction reaches the
            // message it reacts to through that message's `reactions`, which
            // already leave removed reactions out.
            if matches!(
                msg.message_kind,
                IrMessageKind::Tapback | IrMessageKind::StickerTapback
            ) {
                continue;
            }
            if msg.guid.trim().is_empty() {
                missing_guid_total += 1;
                if missing_guid_lines.len() < MISSING_GUID_LINES_NAMED {
                    missing_guid_lines.push(line_no);
                }
                continue;
            }
            let record = message_from_ir(&msg, header_owner.as_deref(), line_no).map_err(|e| {
                ImportFailure::Invalid {
                    line: line_no,
                    detail: format!("{e:#}"),
                }
            })?;
            out.push(ExportRecord::Message(record));
        }
    }
    if missing_guid_total > 0 {
        return Err(ImportFailure::MissingGuid {
            lines: missing_guid_lines,
            total: missing_guid_total,
        });
    }
    if out.is_empty() {
        return Err(ImportFailure::Invalid {
            line: 1,
            detail: "the file has no conversation header".into(),
        });
    }
    Ok(out)
}

/// True when the JSON object is a conversation header rather than a message.
fn is_ir_header(value: &Value) -> bool {
    value.get("schema_version").is_some() && value.get("conversation").is_some()
}

/// Map a JSON Lines header onto the server's conversation record.
fn conversation_from_ir(header: &ConversationHeader, line: usize) -> ConversationRecord {
    let export_source = {
        let s = header.export.source.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    ConversationRecord {
        line,
        chat_identifier: header.conversation.chat_identifier.clone(),
        // Platform identity for handles (phone | whatsapp), not SMS/iMessage/RCS.
        service: Some(
            if export_source
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("whatsapp"))
            {
                HandleService::Whatsapp.as_str().to_string()
            } else {
                HandleService::Phone.as_str().to_string()
            },
        ),
        conversation_type: header.conversation.conversation_type.as_str().to_string(),
        group_title: header.conversation.group_title.clone(),
        participants: header
            .conversation
            .participants
            .iter()
            .filter_map(participant_from_ir)
            .collect(),
        export_source,
    }
}

/// Map one IR message onto the server's message record. `header_owner` is the
/// owner the conversation header names, used when the message names none.
fn message_from_ir(
    msg: &IrMessage,
    header_owner: Option<&str>,
    line: usize,
) -> Result<MessageRecord> {
    let timestamp = format_utc_timestamp(msg.timestamp_unix_ms).with_context(|| {
        format!(
            "unrepresentable timestamp_unix_ms {}",
            msg.timestamp_unix_ms
        )
    })?;
    let is_from_me = msg.direction == IrDirection::Outgoing;
    let im = msg.imessage.as_ref();
    let text = {
        let t = msg.text.trim();
        if t.is_empty() {
            None
        } else {
            Some(msg.text.clone())
        }
    };
    let tapbacks = msg.reactions.iter().map(tapback_from_reaction).collect();
    let earlier_versions = msg
        .edits
        .iter()
        .map(earlier_version_from_ir)
        .collect::<Result<Vec<_>>>()?;
    let sender = if is_from_me {
        None
    } else {
        sender_identity(
            msg.sender_identity.as_deref(),
            msg.sender_display_name.as_deref(),
        )
    };

    Ok(MessageRecord {
        guid: msg.guid.clone(),
        line,
        timestamp,
        is_from_me,
        sender: sender.as_ref().map(|(value, _)| value.clone()),
        sender_handle_type: sender.and_then(|(_, kind)| kind),
        owner: msg
            .owner_identity
            .as_deref()
            .and_then(nonempty)
            .or_else(|| header_owner.map(str::to_string)),
        service: Some(msg.service.as_str().to_string()),
        subject: msg.subject.clone().filter(|s| !s.is_empty()),
        text,
        is_announcement: im.is_some_and(|i| i.announcement.is_some())
            || matches!(msg.message_kind, IrMessageKind::Announcement),
        announcement: im.and_then(|i| i.announcement.clone()),
        attachments: msg.attachments.iter().map(attachment_from_ir).collect(),
        tapbacks,
        reply_to: msg.reply_to.as_ref().map(|r| ReplyTo {
            guid: r.guid.as_deref().and_then(nonempty),
            part_index: r.part_index,
        }),
        deletion: msg.deletion,
        earlier_versions,
    })
}

/// One earlier version as the server stores it, its time in the form a
/// message's timestamp takes.
fn earlier_version_from_ir(version: &EarlierVersion) -> Result<EarlierVersionRecord> {
    let edited_at = version
        .edited_at_unix_ms
        .map(|ms| {
            format_utc_timestamp(ms)
                .with_context(|| format!("unrepresentable edited_at_unix_ms {ms}"))
        })
        .transpose()?;
    Ok(EarlierVersionRecord {
        part_index: i64::from(version.part_index),
        text: version.text.clone(),
        edited_at,
    })
}

/// One header participant as a record, or `None` for one that gives neither
/// an address nor a name and so says nothing.
///
/// Every participant is an identity. A person the source names with no
/// address gets an identity of type `other` whose value is the name, so the
/// same name on one service is one identity and one contact on every import
/// (`docs/architecture/contacts-identities-and-messages.md`).
fn participant_from_ir(p: &IrParticipant) -> Option<ParticipantRecord> {
    let name_alias = p.display_name.clone();
    if let Some(handle) = p.identity.as_deref().and_then(nonempty) {
        return Some(ParticipantRecord {
            handle,
            name_alias,
            handle_type: p.identity_type,
        });
    }
    let name = p.display_name.as_deref().and_then(nonempty)?;
    Some(ParticipantRecord {
        handle: name,
        name_alias,
        handle_type: Some(HandleType::Other),
    })
}

/// An incoming message's sender as an identity value and, when known, its
/// type: the address, else the name the source gave with no address as an
/// identity of type `other`, the rule [`participant_from_ir`] applies.
/// `None` when the message names neither.
fn sender_identity(
    address: Option<&str>,
    name: Option<&str>,
) -> Option<(String, Option<HandleType>)> {
    if let Some(address) = address.and_then(nonempty) {
        let kind = sender_handle_type(Some(address.as_str()));
        return Some((address, kind));
    }
    let name = name.and_then(nonempty)?;
    Some((name, Some(HandleType::Other)))
}

/// The shape of a sender's address, read from the address alone.
///
/// A message carries only the sender's address, never its type. Staging uses
/// the type the participant with the same address has, and this one only
/// when the header lists no such participant, and then only within what the
/// message's service carries (`db::handles::handle_type_on`): an address
/// with an `@` on WhatsApp is `other`. It is
/// [`Handle::parse`], the one rule for what an address looks like: a
/// contact's number is a phone number on a service the model does not know
/// too, such as a message Apple Messages sent by satellite (#1144).
fn sender_handle_type(sender_identity: Option<&str>) -> Option<HandleType> {
    sender_identity.and_then(Handle::parse).map(|h| h.kind())
}

/// Map one IR attachment onto the server's attachment record.
fn attachment_from_ir(a: &IrAttachment) -> AttachmentRecord {
    AttachmentRecord {
        path: a.path.clone(),
        original_name: a.original_name.clone(),
        mime_type: a.mime_type.clone(),
        sha256: a.digest_sha256.clone(),
        is_sticker: a.is_sticker,
        transcription: a.transcription.clone(),
        size_bytes: a.size_bytes,
        missing_reason: a.missing_reason.clone(),
    }
}

/// One of a message's reactions as the row the server stores, under the
/// person who reacted: the owner when `is_from_me`, else `reactor_identity`.
/// A reaction is never given the author or the direction of the message it
/// reacts to, because the reactor is rarely the author.
fn tapback_from_reaction(reaction: &Reaction) -> TapbackRecord {
    TapbackRecord {
        part_index: i64::from(reaction.part_index),
        kind: reaction.kind.clone(),
        emoji: reaction.emoji.clone(),
        is_from_me: reaction.is_from_me,
        sender: if reaction.is_from_me {
            None
        } else {
            reaction.reactor_identity.clone()
        },
    }
}

/// The UTC RFC 3339 string for a Unix time in milliseconds, or `None` when it
/// cannot be represented, in the form `utc_timestamp_text` writes. The server
/// stores the instant and nothing about where the phone was; the account's
/// time zone turns it into a clock reading.
fn format_utc_timestamp(ms: i64) -> Option<String> {
    Some(utc_timestamp_text(Utc.timestamp_millis_opt(ms).single()?))
}

/// The one text form of a stored message time: UTC RFC 3339 with three
/// fractional digits and a `Z` suffix (`2015-03-12T18:04:22.000Z` for a whole
/// second). Every stored time and every string compared with one, such as a
/// search day bound, is written here, so they all sort as text in time order.
pub(crate) fn utc_timestamp_text(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_ir::{IrImessage, UnsupportedSchemaVersion};

    use crate::test_support::{MessageLine, conversation_header, message_line};

    /// An incoming SMS from Sam, "hello", sent at 1400773261000.
    fn from_sam(guid: &str) -> MessageLine {
        message_line(guid, "hello")
            .at(1_400_773_261_000)
            .sms()
            .sender("+15555550101")
            .sender_display_name("Sam")
    }

    #[test]
    fn parses_ir_sms_without_imessage_bag() {
        let header = conversation_header("sms-backup-restore", "+15555550101")
            .participant("+15555550101", Some("Sam"))
            .to_string();
        let lines = [header, from_sam("g1").to_string()];
        let records = parse_ir_lines(lines).unwrap();
        assert_eq!(records.len(), 2);
        match &records[1] {
            ExportRecord::Message(m) => {
                assert_eq!(m.guid, "g1");
                assert!(!m.is_from_me);
                assert_eq!(m.text.as_deref(), Some("hello"));
                assert_eq!(m.service.as_deref(), Some("sms"));
                assert_eq!(m.sender_handle_type, Some(HandleType::Phone));
                assert!(m.tapbacks.is_empty());
                assert!(m.reply_to.is_none());
            }
            _ => panic!("expected message"),
        }
    }

    /// The header and one incoming iMessage whose `subject` is `subject`,
    /// or `null` for `None`.
    fn message_with(subject: Option<&str>) -> MessageRecord {
        let header = conversation_header("imessage", "+15555550101")
            .participant("+15555550101", Some("Sam"))
            .to_string();
        let mut message = message_line("g1", "hello")
            .at(1_400_773_261_000)
            .sender("+15555550101")
            .sender_display_name("Sam");
        if let Some(subject) = subject {
            message = message.subject(subject);
        }
        let mut records = parse_ir_lines([header, message.to_string()]).unwrap();
        match records.pop() {
            Some(ExportRecord::Message(m)) => m,
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn a_subject_is_kept_and_an_empty_one_is_none() {
        assert_eq!(
            message_with(Some("Dinner on Friday")).subject.as_deref(),
            Some("Dinner on Friday")
        );
        assert_eq!(message_with(Some("")).subject, None);
        assert_eq!(message_with(None).subject, None);
    }

    /// A reaction names its reactor in `reactor_identity` and says in
    /// `is_from_me` whether the owner reacted. Neither is taken from the
    /// message reacted to (#1213).
    #[test]
    fn a_reaction_keeps_the_reactor_it_names() {
        let header = conversation_header("imessage", "+15555550101")
            .participant("+15555550101", Some("Ada"))
            .to_string();
        let reacted_to = |message: MessageLine| {
            message
                .at(1_400_773_261_000)
                .reaction(Reaction {
                    part_index: 0,
                    kind: "loved".into(),
                    emoji: None,
                    is_from_me: false,
                    reactor_identity: Some("+15555550110".into()),
                    reactor_display_name: Some("Sam".into()),
                })
                .reaction(Reaction {
                    part_index: 2,
                    kind: "emoji".into(),
                    emoji: Some("🔥".into()),
                    is_from_me: true,
                    reactor_identity: None,
                    reactor_display_name: Some("Me".into()),
                })
                .to_string()
        };
        // The owner's own message and Ada's, each reacted to by Sam and then
        // by the owner.
        let records = parse_ir_lines([
            header,
            reacted_to(message_line("g-mine", "hello").outgoing()),
            reacted_to(message_line("g-adas", "hello").sender("+15555550101")),
        ])
        .unwrap();
        let messages: Vec<_> = records
            .iter()
            .filter_map(|r| match r {
                ExportRecord::Message(m) => Some(m),
                ExportRecord::Conversation(_) => None,
            })
            .collect();
        assert_eq!(messages.len(), 2);
        for m in messages {
            let rows: Vec<_> = m
                .tapbacks
                .iter()
                .map(|t| {
                    (
                        t.part_index,
                        t.kind.as_str(),
                        t.emoji.as_deref(),
                        t.is_from_me,
                        t.sender.as_deref(),
                    )
                })
                .collect();
            assert_eq!(
                rows,
                [
                    (0, "loved", None, false, Some("+15555550110")),
                    (2, "emoji", Some("🔥"), true, None),
                ],
                "{}: Sam's reaction is Sam's and the owner's is the owner's",
                m.guid
            );
        }
    }

    /// The Apple Messages reader writes each reaction as a row of its own as
    /// well as in the `reactions` of the message reacted to. The row is
    /// not a message, and it carries no reaction of its own (#1213).
    #[test]
    fn a_reaction_row_is_not_a_message() {
        let header = conversation_header("imessage", "+15555550101")
            .participant("+15555550101", Some("Sam"))
            .to_string();
        let target = message_line("g-hi", "hi")
            .at(1_400_773_261_000)
            .outgoing()
            .to_string();
        let row = |guid: &str, kind: IrMessageKind, text: &str, action: &str| {
            message_line(guid, text)
                .at(1_400_773_262_000)
                .kind(kind)
                .sender("+15555550101")
                .sender_display_name("Sam")
                .imessage(IrImessage {
                    associated_guid: Some("g-hi".into()),
                    associated_part: Some(0),
                    tapback_kind: Some("loved".into()),
                    tapback_action: Some(action.into()),
                    ..IrImessage::default()
                })
                .to_string()
        };
        let records = parse_ir_lines([
            header,
            target,
            row("g-love", IrMessageKind::Tapback, "Loved a message", "add"),
            row(
                "g-unlove",
                IrMessageKind::Tapback,
                "Removed Heart",
                "remove",
            ),
            row(
                "g-sticker",
                IrMessageKind::StickerTapback,
                "Reacted with a sticker",
                "add",
            ),
        ])
        .unwrap();
        let messages: Vec<_> = records
            .iter()
            .filter_map(|r| match r {
                ExportRecord::Message(m) => Some(m),
                ExportRecord::Conversation(_) => None,
            })
            .collect();
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert_eq!(messages[0].guid, "g-hi");
        assert!(messages[0].tapbacks.is_empty());
    }

    #[test]
    fn parses_concatenated_ir_conversations() {
        let header = |chat: &str| {
            conversation_header("sms-backup-restore", chat)
                .participant(chat, None)
                .to_string()
        };
        let msg = |guid: &str, handle: &str| {
            message_line(guid, "hi")
                .at(1_400_773_261_000)
                .sms()
                .sender(handle)
                .to_string()
        };
        let records = parse_ir_lines([
            header("+15555550101"),
            msg("g1", "+15555550101"),
            header("+15555550102"),
            msg("g2", "+15555550102"),
        ])
        .unwrap();
        assert_eq!(records.len(), 4);
        match &records[0] {
            ExportRecord::Conversation(c) => assert_eq!(c.chat_identifier, "+15555550101"),
            _ => panic!("expected conversation"),
        }
        match &records[2] {
            ExportRecord::Conversation(c) => assert_eq!(c.chat_identifier, "+15555550102"),
            _ => panic!("expected conversation"),
        }
    }

    #[test]
    fn types_a_sender_by_the_address_alone() {
        assert_eq!(
            sender_handle_type(Some("alice@example.com")),
            Some(HandleType::Email)
        );
        assert_eq!(
            sender_handle_type(Some("+1 (555) 555-0101")),
            Some(HandleType::Phone)
        );
        assert_eq!(sender_handle_type(Some("AMAZON")), Some(HandleType::Other));
        assert_eq!(sender_handle_type(Some("  ")), None);
        assert_eq!(sender_handle_type(None), None);
    }

    #[test]
    fn parse_ir_lines_refuses_schema_3_as_a_failure() {
        let header = r#"{"schema_version":3,"export":{"source":"whatsapp","tool":"t","owner_identity":"+1","owner_display_name":"Me"},"conversation":{"chat_identifier":"+2","conversation_type":"individual","participants":[]}}"#;
        let failure = parse_ir_lines([header]).unwrap_err();
        assert_eq!(
            failure,
            ImportFailure::SchemaVersion {
                refusal: UnsupportedSchemaVersion { found: 3 },
                line: 1
            }
        );
    }

    #[test]
    fn parse_ir_lines_reports_a_non_json_line_as_a_failure() {
        let failure = parse_ir_lines(["this is not json"]).unwrap_err();
        match failure {
            ImportFailure::NotJson { line, .. } => assert_eq!(line, 1),
            other => panic!("expected NotJson, got {other:?}"),
        }
    }

    #[test]
    fn parse_ir_lines_reports_a_message_before_any_header_as_a_failure() {
        let failure = parse_ir_lines([r#"{"guid":"m1"}"#]).unwrap_err();
        match failure {
            ImportFailure::Invalid { line, detail } => {
                assert_eq!(line, 1);
                assert!(
                    detail.contains("before the conversation header"),
                    "{detail}"
                );
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn parse_ir_lines_reports_a_message_with_an_impossible_timestamp_as_a_failure() {
        let header = conversation_header("sms-backup-restore", "+15555550101")
            .participant("+15555550101", Some("Sam"))
            .to_string();
        // The timestamp is the input under test, so the line stays written out.
        let msg = r#"{"guid":"g1","timestamp_unix_ms":9223372036854775807,"direction":"incoming","service":"sms","message_kind":"sms","sender_identity":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}"#;
        let failure = parse_ir_lines([header, msg.to_string()]).unwrap_err();
        match failure {
            ImportFailure::Invalid { line, .. } => assert_eq!(line, 2),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    /// A message the guid index cannot see would be stored again by every
    /// retried batch (#1162), so the whole file is refused, naming every
    /// line that has no guid, before anything is staged.
    #[test]
    fn parse_ir_lines_refuses_messages_without_a_guid_naming_every_line() {
        let header = conversation_header("sms-backup-restore", "+15555550101")
            .participant("+15555550101", Some("Sam"))
            .to_string();
        // The guids are the input under test, so the lines stay written out.
        let msg = |guid: &str| {
            format!(
                r#"{{"guid":"{guid}","timestamp_unix_ms":1400773261000,"direction":"incoming","service":"sms","message_kind":"sms","sender_identity":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}}"#
            )
        };
        let lines = [header.to_string(), msg("g1"), msg(""), msg("   ")];
        let failure = parse_ir_lines(lines).unwrap_err();
        assert_eq!(
            failure,
            ImportFailure::MissingGuid {
                lines: vec![3, 4],
                total: 2
            }
        );
    }
}
