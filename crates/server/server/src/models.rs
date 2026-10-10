//! Import-side records mapped from message-ir JSONL, and the one text form
//! of a stored message time (`utc_timestamp_text`), which search day bounds
//! use too.

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use message_ir::{
    ConversationHeader, Deletion, EarlierVersion, IdentityService, IdentityType, IrAttachment,
    IrDirection, IrMessage, IrMessageKind, IrParticipant, Reaction, ReplyTo, TimePrecision,
    check_schema_version_in_json, nonempty, trimmed,
};
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
    /// When the backup the file was read from was made, in the form a
    /// message's timestamp takes; `None` when the file does not say.
    pub backup_taken_at: Option<StoredTime>,
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
    /// The participant's address, or the name the source gave in its place.
    pub handle: HandleValue,
    /// Display-name alias, when the export supplied one.
    pub name_alias: Option<String>,
}

/// What a conversation file gave for a person: an address, or a name in
/// place of one when the source recorded no address. The file states no
/// type; [`HandleValue::handle_type_on`] works it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleValue {
    /// A phone number, email address or app id, as the file wrote it.
    Address(String),
    /// A name the source gave with no address, such as `Mom`.
    Name(String),
}

impl HandleValue {
    /// The value as the file wrote it.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Address(value) | Self::Name(value) => value,
        }
    }

    /// The type an import gives this value on `service`: a name is `Other`,
    /// and an address is typed by its service and shape
    /// ([`handle_type_on`](crate::db::handles::handle_type_on)). A
    /// participant, a message's sender and a reaction's sender are all typed
    /// here, so one address is one type wherever it appears in a
    /// conversation (#1959).
    pub fn handle_type_on(&self, service: IdentityService) -> IdentityType {
        match self {
            Self::Address(address) => crate::db::handles::handle_type_on(address, service),
            Self::Name(_) => IdentityType::Other,
        }
    }
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
    pub timestamp: StoredTime,
    /// Whether the source recorded `timestamp` to the millisecond or in
    /// whole seconds, as the conversation file says.
    pub time_precision: TimePrecision,
    /// True for messages sent by the account owner.
    pub is_from_me: bool,
    /// Sender for incoming messages: the address, or the name when the
    /// source named the sender with no address.
    pub sender: Option<HandleValue>,
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
    /// The part's text in this version; `None` when the source recorded the
    /// edit and not the text it replaced, as iMazing does.
    pub text: Option<String>,
    /// When this version was written, to the millisecond in the form
    /// `MessageRecord::timestamp` takes; `None` when the source does not
    /// record it.
    pub edited_at: Option<StoredTime>,
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
            let conversation =
                conversation_from_ir(&header, line_no).map_err(|e| ImportFailure::Invalid {
                    line: line_no,
                    detail: format!("{e:#}"),
                })?;
            out.push(ExportRecord::Conversation(conversation));
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
///
/// # Errors
///
/// Returns an error when the backup date is outside the times a timestamp
/// can hold.
fn conversation_from_ir(header: &ConversationHeader, line: usize) -> Result<ConversationRecord> {
    let export_source = {
        let s = header.export.source.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    let backup_taken_at = header
        .export
        .backup_taken_at_unix_ms
        .map(|ms| {
            format_utc_timestamp(ms)
                .with_context(|| format!("unrepresentable backup_taken_at_unix_ms {ms}"))
        })
        .transpose()?;
    Ok(ConversationRecord {
        line,
        chat_identifier: header.conversation.chat_identifier.clone(),
        // Platform identity for handles (phone | whatsapp), not SMS/iMessage/RCS.
        service: Some(
            if export_source
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("whatsapp"))
            {
                IdentityService::Whatsapp.as_str().to_string()
            } else {
                IdentityService::Phone.as_str().to_string()
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
        backup_taken_at,
    })
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
        handle_value(
            msg.sender_identity.as_deref(),
            msg.sender_display_name.as_deref(),
        )
    };

    Ok(MessageRecord {
        guid: msg.guid.clone(),
        line,
        timestamp,
        time_precision: msg.time_precision,
        is_from_me,
        sender,
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
/// (`docs/architecture/contacts-identities-and-messages.md`). The file
/// states no type: staging types an address by its service and shape
/// (`db::handles::handle_type_on`).
fn participant_from_ir(p: &IrParticipant) -> Option<ParticipantRecord> {
    let name_alias = p.display_name.clone();
    let handle = handle_value(p.identity.as_deref(), p.display_name.as_deref())?;
    Some(ParticipantRecord { handle, name_alias })
}

/// The address, else the name the source gave with no address. `None` when
/// the source gave neither. A message's sender and a participant follow this
/// one rule.
fn handle_value(address: Option<&str>, name: Option<&str>) -> Option<HandleValue> {
    address
        .and_then(nonempty)
        .map(HandleValue::Address)
        .or_else(|| name.and_then(nonempty).map(HandleValue::Name))
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
fn format_utc_timestamp(ms: i64) -> Option<StoredTime> {
    Some(utc_timestamp_text(Utc.timestamp_millis_opt(ms).single()?))
}

/// The one text form of a stored message time: UTC RFC 3339 with three
/// fractional digits and a `Z` suffix (`2015-03-12T18:04:22.000Z` for a whole
/// second). Every stored time and every string compared with one, such as a
/// search day bound, is written here, so they all sort as text in time order.
pub(crate) fn utc_timestamp_text(instant: DateTime<Utc>) -> StoredTime {
    StoredTime(instant.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// A message time in the text form `messages.timestamp` stores and every
/// list order, aggregate and search day bound compares as text. Only
/// [`utc_timestamp_text`] makes one, so a time built another way, such as
/// `2015-03-12T00:00:00Z`, which sorts after `2015-03-12T00:00:00.000Z`,
/// cannot reach a stored time or a comparison with one (#1963, #1965). It
/// binds as text, and decodes only text in its own form, so a stored time
/// read back to be compared again keeps the type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct StoredTime(String);

impl std::fmt::Display for StoredTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl sqlx::Type<sqlx::Sqlite> for StoredTime {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for StoredTime {
    /// Refuses text that is not the stored form, such as `…T00:00:00Z`, so a
    /// column written another way cannot become a `StoredTime` by being read.
    fn decode(value: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let text = <String as sqlx::Decode<'r, sqlx::Sqlite>>::decode(value)?;
        let stored = DateTime::parse_from_rfc3339(&text)
            .ok()
            .map(|instant| utc_timestamp_text(instant.with_timezone(&Utc)));
        match stored {
            Some(stored) if stored.0 == text => Ok(stored),
            _ => Err(format!("{text:?} is not a stored message time").into()),
        }
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for StoredTime {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <String as sqlx::Encode<'q, sqlx::Sqlite>>::encode_by_ref(&self.0, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_ir::{IrImessage, UnsupportedSchemaVersion};

    use crate::test_support::{MessageLine, conversation_header, message_line};

    /// The stored form has three fractional digits and a `Z` for a whole
    /// second and for one with milliseconds, so it sorts as text in time
    /// order; `…T00:00:00Z` would sort after `…T00:00:00.000Z` (#1963).
    #[test]
    fn a_stored_time_has_milliseconds_and_a_z() {
        let at = |ms| utc_timestamp_text(Utc.timestamp_millis_opt(ms).single().unwrap());
        assert_eq!(
            at(1_426_183_462_000).to_string(),
            "2015-03-12T18:04:22.000Z"
        );
        assert_eq!(
            at(1_426_183_462_250).to_string(),
            "2015-03-12T18:04:22.250Z"
        );
    }

    /// A stored time read back decodes, and text in another form is refused,
    /// so reading a column cannot make a `StoredTime` that sorts wrong.
    #[tokio::test]
    async fn only_the_stored_form_decodes_as_a_stored_time() {
        use sqlx::Connection;
        let mut conn = sqlx::SqliteConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        let stored: StoredTime = sqlx::query_scalar("SELECT '2015-03-12T18:04:22.250Z'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(stored.to_string(), "2015-03-12T18:04:22.250Z");
        for other in [
            "2015-03-12T18:04:22Z",
            "2015-03-12T18:04:22.250+00:00",
            "yesterday",
        ] {
            let read = sqlx::query_scalar::<_, StoredTime>("SELECT $1")
                .bind(other)
                .fetch_one(&mut conn)
                .await;
            assert!(read.is_err(), "{other}");
        }
    }

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
        let msg = r#"{"guid":"g1","timestamp_unix_ms":9223372036854775807,"time_precision":"milliseconds","direction":"incoming","service":"sms","message_kind":"sms","sender_identity":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}"#;
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
                r#"{{"guid":"{guid}","timestamp_unix_ms":1400773261000,"time_precision":"milliseconds","direction":"incoming","service":"sms","message_kind":"sms","sender_identity":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}}"#
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
