//! Shared conversation structure every exporter writes.
//!
//! A [`ConversationDocument`] is the in-memory form of one conversation:
//! export metadata, participants, and messages. Backup converters parse
//! vendor formats into this type. Writing files (JSON, CSV, EML, and so on)
//! lives in `ir-format`. The atomic, synced write those files go
//! through lives in `file-io`. Converting an existing export
//! directory lives in `reexport`. See the [common message](https://messagecrate.app/docs/developer/architecture/common-message/) page.
//!
//! Converters stage parsed rows in [`PendingMessage`] and
//! [`PendingConversation`] (with per-converter metadata in their `extra`
//! maps) before building a [`ConversationDocument`].
//!
//! Types are named for what they are, with no prefix: the crate path tells
//! `ir::Message` from `api_types::Message`, and a file
//! that needs both imports one of them with `as`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

mod attachment_path;
mod conversation_key;
mod identity;
pub mod missing_reason;
mod projection;
mod schema_version;
#[cfg(feature = "testutil")]
pub mod testutil;

pub use attachment_path::{UNSAFE_ATTACHMENT_PATH, UnsafeAttachmentPath, safe_attachment_path};
pub use conversation_key::{
    ConversationKey, GROUP_CHAT_ID_PREFIX, NAME_CHAT_ID_PREFIX, NAMELESS_CHAT_ID, name_of_chat_id,
};
pub use identity::{
    MessageCopy, MessageGuid, MessageIdentity, collapse_whitespace, one_copy_per_message,
};
pub use projection::{
    PendingReply, ProjectedRole, Projection, ProjectionHooks, ProjectionTally, SortKeyUnit,
    default_message_kind, default_participants, display_names_for_handles, ensure_conversation,
    message_time, prepare_conversation, project_pending,
};
pub use schema_version::{
    UnsupportedSchemaVersion, check_schema_version, check_schema_version_in_json,
};

/// Kind of a participant identity. Defined in `phone`, which classifies an
/// address, so that crate does not depend on the conversation model.
pub use phone::IdentityType;

/// One reaction on a message, the same shape for every source.
///
/// It is defined in `imessage-reader-protocol`, the one crate the GPL Apple
/// Messages Reader and the rest of Message Crate may both link, so the reader
/// writes reactions in the shape the conversation file carries
/// (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).
pub use imessage_reader_protocol::Reaction;

/// The storage ids of [`ConversationType::OneToOne`] and
/// [`ConversationType::Orphaned`], what every chat id of an orphaned
/// conversation starts with, and that chat id. Defined in
/// `imessage-reader-protocol` beside [`Reaction`], for the same reason: the
/// Apple Messages Reader writes conversations in the shape the conversation
/// file carries.
pub use imessage_reader_protocol::{
    ONE_TO_ONE_CONVERSATION_TYPE, ORPHANED_CHAT_ID_PREFIX, ORPHANED_CONVERSATION_TYPE,
    orphaned_chat_id,
};

/// Why a message's content is gone in the app it came from: Deleted in the
/// source app, or Unsent. Serialized `deleted_in_source_app` / `unsent`.
///
/// Defined in `imessage-reader-protocol` beside [`Reaction`], for the same
/// reason: the Apple Messages Reader writes it in the shape the conversation
/// file carries.
pub use imessage_reader_protocol::Deletion;

/// How finely a source recorded a message's time: a message's
/// `time_precision` in the conversation file, `seconds` or `milliseconds`.
///
/// Defined in `imessage-reader-protocol` beside [`Reaction`], for the same
/// reason: the Apple Messages Reader sends each message's precision in the
/// shape the conversation file carries
/// (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).
pub use imessage_reader_protocol::TimePrecision;

/// One earlier version of one part of an edited message: its part, its text
/// when the source kept it, and when it was written. A message's `text` is
/// its final version, so [`Message::edits`] holds only the versions before
/// it.
///
/// Defined in `imessage-reader-protocol` beside [`Reaction`], for the same
/// reason: the Apple Messages Reader writes it in the shape the conversation
/// file carries.
pub use imessage_reader_protocol::EarlierVersion;

/// The message a reply quotes: its guid when the source names it, and the
/// part replied to when the source records one. A message
/// with one is a reply even when the quoted message cannot be named.
///
/// Defined in `imessage-reader-protocol` beside [`Reaction`], for the same
/// reason: the Apple Messages Reader writes it in the shape the conversation
/// file carries.
pub use imessage_reader_protocol::ReplyTo;

/// The reply a flat record describes, from its reply mark and the quoted
/// message's guid and part, as the CSV `is_reply`, `reply_to_guid` and
/// `reply_to_part` cells and the `X-ME-Is-Reply` and `X-ME-Reply-To-*` mail
/// headers carry it. A record that names a quoted guid is a reply even
/// without its mark; one with neither is not a reply.
#[must_use]
pub fn reply_to_from_mark(
    is_reply: bool,
    guid: Option<String>,
    part_index: Option<u32>,
) -> Option<ReplyTo> {
    (is_reply || guid.is_some()).then_some(ReplyTo { guid, part_index })
}

/// The mark a text field holds, as the CSV `deletion` cell and the
/// `X-ME-Deletion` mail header write it: blank for no mark, else
/// [`Deletion::as_str`]'s text.
///
/// # Errors
///
/// Returns [`UnknownDeletion`] for any other text, so a reader refuses it
/// rather than read it as no mark.
pub fn parse_deletion(text: &str) -> Result<Option<Deletion>, UnknownDeletion> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    Deletion::ALL
        .into_iter()
        .find(|d| d.as_str() == text)
        .map(Some)
        .ok_or_else(|| UnknownDeletion(text.to_string()))
}

/// Text that names neither mark of [`Deletion`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownDeletion(pub String);

impl std::fmt::Display for UnknownDeletion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} is neither deleted_in_source_app nor unsent",
            self.0
        )
    }
}

impl std::error::Error for UnknownDeletion {}

/// Schema version written into every [`ConversationDocument`].
///
/// Version 15 adds the country an identity was read in (`country`, and
/// the sender's); version 14 had none. Version 16 names a one-to-one
/// conversation `one-to-one` in
/// [`ConversationMeta::conversation_type`], where version 15 said
/// `individual`. Version 17 dates each message by the backup it came from
/// ([`Message::backup_taken_at_unix_ms`]) and says when the file was written
/// ([`ExportMeta::exported_at_unix_ms`]), where version 16 gave the whole
/// file one backup date. Version 18 carries a message's sender as one
/// [`Message::sender`] object, where version 17 wrote `sender_identity`,
/// `sender_display_name` and `sender_country` beside each other.
pub const SCHEMA_VERSION: u32 = 18;

/// One exported conversation: export metadata, conversation roster and
/// stats, and messages.
///
/// This is the common-message schema every exporter writes and every reader
/// parses. See the [common message](https://messagecrate.app/docs/developer/architecture/common-message/) page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationDocument {
    /// Schema version written into this document ([`SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Where and how this export was produced.
    pub export: ExportMeta,
    /// Roster and computed stats for this conversation.
    pub conversation: ConversationMeta,
    /// Messages in timestamp order.
    pub messages: Vec<Message>,
    /// On-disk stem suffix (e.g. `__whatsapp`). Never written into JSON or JSON Lines files.
    #[serde(skip)]
    pub packaging_stem_suffix: Option<String>,
}

/// Provenance of an export: which backup tool and account it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportMeta {
    /// Backup source id (e.g. `sms-backup-restore`).
    pub source: String,
    /// Human tool name (e.g. `SMS Backup & Restore`).
    pub tool: String,
    /// Version string of the tool.
    pub tool_version: String,
    /// The owner's identity used for outgoing rows; `None` when the backup has no owner identity.
    pub owner_identity: Option<String>,
    /// Outgoing display name. Set when known (iMessage caller-id or `"Me"`).
    pub owner_display_name: Option<String>,
    /// The Export Date: when the run that wrote this file wrote it, in Unix
    /// milliseconds, as its caller gave the instant. Carried for whoever
    /// opens the file; it decides nothing on import, and the server does not
    /// store it. `None` when the file was read from a format that does not
    /// say.
    pub exported_at_unix_ms: Option<i64>,
}

/// The shape of a conversation: one-to-one, group, or orphaned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversationType {
    /// A one-to-one conversation: one with a single other person that the
    /// source does not mark as a group. Serialized `one-to-one`.
    #[serde(rename = "one-to-one")]
    OneToOne,
    /// A group conversation. Where the source marks its groups (a WhatsApp
    /// `@g.us` id, Apple's chat style), the mark decides, whatever the number
    /// of other people in it. Where the source has no mark, two or more
    /// people other than the account holder make a group: an SMS or MMS
    /// backup counts the addresses a message names, and an iMazing or
    /// OpenExtract export counts who wrote, not who was in the group. iMazing
    /// also takes a Messages session named as a roster ("A & B") as a group.
    Group,
    /// Orphaned messages: ones the backup holds without recording which
    /// conversation they were said in. The ones that name one person, sent
    /// by them or to them, sit in a conversation with that person as its only
    /// participant, keyed by [`orphaned_chat_id`]; the ones that name nobody
    /// sit in one with no participants. Neither one-to-one nor a group.
    Orphaned,
}

impl ConversationType {
    /// Lowercase storage id (`one-to-one` / `group` / `orphaned`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneToOne => ONE_TO_ONE_CONVERSATION_TYPE,
            Self::Group => "group",
            Self::Orphaned => ORPHANED_CONVERSATION_TYPE,
        }
    }

    /// Parse a storage id, ignoring case; anything but `group` or `orphaned`
    /// is `OneToOne`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "group" => Self::Group,
            ORPHANED_CONVERSATION_TYPE => Self::Orphaned,
            _ => Self::OneToOne,
        }
    }
}

/// Roster and computed stats for one conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMeta {
    /// Stable chat id from the source (E.164, group key, or app thread id).
    pub chat_identifier: String,
    /// One-to-one, group, or orphaned.
    pub conversation_type: ConversationType,
    /// Group display title; `None` for one-to-one conversations and untitled
    /// groups.
    pub group_title: Option<String>,
    /// Roster of identities and display names.
    pub participants: Vec<Participant>,
    /// Computed counts and first/last timestamps.
    pub stats: ConversationStats,
}

/// Message and attachment counts plus first and last message timestamps,
/// computed from `messages` at write time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ConversationStats {
    /// Number of messages in the conversation.
    pub message_count: u64,
    /// Total attachments across all messages.
    pub attachment_count: u64,
    /// Earliest message timestamp. `None` when the conversation has no messages.
    pub first_timestamp_unix_ms: Option<i64>,
    /// Latest message timestamp. `None` when the conversation has no messages.
    pub last_timestamp_unix_ms: Option<i64>,
}

/// One conversation member: an identity, a display name, or both.
///
/// `identity` is `None` when the source named a person without recording any
/// address for them. The rescue exporters (iMazing, OpenExtract, SMS
/// Backup+) read formats that identify the other party by name alone. Such a
/// participant always carries a `display_name`; the server reconciles it
/// against contacts on import rather than the exporter inventing an address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    /// Phone, email, or username string; `None` when the source recorded no
    /// address for this person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Display name shown in UIs; `None` when the source has none.
    pub display_name: Option<String>,
    /// The country the source read this identity in, as an upper-case ISO
    /// 3166-1 alpha-2 code (`GB`): a phone number written without its `+`
    /// code is a number in this country. `None`, and left out of the file,
    /// when the source names none; Apple Messages names one from `chat.db`'s
    /// `handle.country`, and no other source does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

/// The transport a message took: SMS, MMS (recorded as `sms`), RCS, iMessage,
/// or another service's own. Written to the file under the key `service`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// SMS text.
    Sms,
    /// Apple iMessage (serialized as `imessage`).
    #[serde(rename = "imessage")]
    IMessage,
    /// WhatsApp.
    Whatsapp,
    /// RCS (Android).
    Rcs,
    /// Discord.
    Discord,
    /// Signal.
    Signal,
    /// Telegram.
    Telegram,
    /// Slack.
    Slack,
    /// Unrecognized or unset transport.
    Unknown,
}

impl Transport {
    /// Lowercase storage id (`sms` / `imessage` / `whatsapp` / …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sms => "sms",
            Self::IMessage => "imessage",
            Self::Whatsapp => "whatsapp",
            Self::Rcs => "rcs",
            Self::Discord => "discord",
            Self::Signal => "signal",
            Self::Telegram => "telegram",
            Self::Slack => "slack",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a storage id; unknown values map to `Unknown`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "sms" => Self::Sms,
            "imessage" => Self::IMessage,
            "whatsapp" => Self::Whatsapp,
            "rcs" => Self::Rcs,
            "discord" => Self::Discord,
            "signal" => Self::Signal,
            "telegram" => Self::Telegram,
            "slack" => Self::Slack,
            _ => Self::Unknown,
        }
    }
}

/// Platform identity stored on `handles.service` (not per-message SMS/iMessage/RCS).
///
/// [`Self::label`] gives the name the app shows for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityService {
    /// Phone platform (SMS/iMessage/RCS are transports, not platforms).
    Phone,
    /// WhatsApp platform.
    Whatsapp,
}

impl IdentityService {
    /// Lowercase storage id (`phone` / `whatsapp`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Whatsapp => "whatsapp",
        }
    }

    /// The service's name as the app shows it: "Text Message" or "WhatsApp".
    pub fn label(self) -> &'static str {
        match self {
            Self::Phone => "Text Message",
            Self::Whatsapp => "WhatsApp",
        }
    }

    /// Parse storage ids and common aliases from documents, UI, and import.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "whatsapp" | "wa" => Self::Whatsapp,
            // Everything else, including the phone-platform aliases (`phone`,
            // `sms`, `mms`, `imessage`, `ios`, `rcs`, `text message`), is the
            // phone platform: SMS/iMessage/RCS are transports, not platforms.
            _ => Self::Phone,
        }
    }

    /// Whether this service carries identities of type `kind`. Text Message
    /// carries phone numbers and email addresses, because iMessage reaches an
    /// email address. WhatsApp carries phone numbers and its own ids
    /// (`…@lid`, `…@g.us`), which are `Other`. Every service carries `Other`,
    /// the type of a name that is neither a number nor an address, such as
    /// the sender `AMAZON`. No service carries a username.
    ///
    /// Every pair is listed, so a new service or type does not compile until
    /// someone decides whether it is carried.
    pub fn carries(self, kind: IdentityType) -> bool {
        match (self, kind) {
            (Self::Phone, IdentityType::Phone | IdentityType::Email | IdentityType::Other)
            | (Self::Whatsapp, IdentityType::Phone | IdentityType::Other) => true,
            (Self::Phone, IdentityType::Username)
            | (Self::Whatsapp, IdentityType::Email | IdentityType::Username) => false,
        }
    }

    /// The type an address of shape `kind` takes on this service: the shape's
    /// type when the service carries it ([`Self::carries`]), and `Other`
    /// otherwise. WhatsApp carries no email address, so an `@` address on
    /// WhatsApp, such as `123456@lid`, is `Other` (#1671).
    pub fn type_on(self, kind: IdentityType) -> IdentityType {
        if self.carries(kind) {
            kind
        } else {
            IdentityType::Other
        }
    }

    /// Map a per-message transport onto a handle platform bucket.
    pub fn from_transport(transport: Transport) -> Self {
        match transport {
            Transport::Whatsapp => Self::Whatsapp,
            Transport::Sms
            | Transport::IMessage
            | Transport::Rcs
            | Transport::Discord
            | Transport::Signal
            | Transport::Telegram
            | Transport::Slack
            | Transport::Unknown => Self::Phone,
        }
    }
}

/// Shape of one message row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Plain SMS text.
    Sms,
    /// Multimedia message.
    Mms,
    /// iMessage (serialized as `imessage`).
    #[serde(rename = "imessage")]
    IMessage,
    /// iMessage tapback reaction.
    Tapback,
    /// iMessage sticker tapback.
    StickerTapback,
    /// iMessage announcement (e.g. group rename).
    Announcement,
    /// iMessage shared location.
    LocationShare,
    /// iMessage Digital Touch balloon.
    Balloon,
    /// Unrecognized or unset kind.
    Unknown,
}

impl MessageKind {
    /// Lowercase storage id (`sms` / `mms` / `imessage` / `tapback` / …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sms => "sms",
            Self::Mms => "mms",
            Self::IMessage => "imessage",
            Self::Tapback => "tapback",
            Self::StickerTapback => "sticker_tapback",
            Self::Announcement => "announcement",
            Self::LocationShare => "location_share",
            Self::Balloon => "balloon",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a storage id; unknown values map to `Unknown`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "sms" => Self::Sms,
            "mms" => Self::Mms,
            "imessage" => Self::IMessage,
            "tapback" => Self::Tapback,
            "sticker_tapback" => Self::StickerTapback,
            "announcement" => Self::Announcement,
            "location_share" => Self::LocationShare,
            "balloon" => Self::Balloon,
            _ => Self::Unknown,
        }
    }
}

/// The person who sent a message. Every fact about a sender travels in this
/// one value, so a new fact is added here and nowhere else in the file.
/// Each fact is optional: a source can name a sender by address alone, by
/// name alone, or not at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sender {
    /// The sender's address: a phone number, an email address, or the
    /// source's own handle. The owner's identity for an outgoing message.
    pub identity: Option<String>,
    /// The sender's display name.
    pub display_name: Option<String>,
    /// The country the source read [`Self::identity`] in, as
    /// [`Participant::country`] says. `None`, and left out of the file,
    /// when the source names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

impl Sender {
    /// The sender a source names by identity and display name, with no
    /// country. An empty identity or name is none.
    pub fn new(identity: Option<String>, display_name: Option<String>) -> Self {
        let given = |value: Option<String>| value.filter(|value| !value.is_empty());
        Self {
            identity: given(identity),
            display_name: given(display_name),
            country: None,
        }
    }
}

/// One message in a conversation: sender, body text, and attachments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Stable message id: Apple's own for Apple Messages, otherwise a
    /// [`MessageGuid`] made from the message's identity.
    pub guid: String,
    /// Unix milliseconds; the chronological sort key.
    pub timestamp_unix_ms: i64,
    /// Whether the source recorded [`Self::timestamp_unix_ms`] to the
    /// millisecond or in whole seconds. Required: the flag, never the
    /// value, says whether a time has milliseconds, since a millisecond
    /// time can end in `.000`. The import shows a whole-second message once
    /// when its source also holds it with milliseconds in the same second.
    pub time_precision: TimePrecision,
    /// Incoming or outgoing.
    pub direction: Direction,
    /// Transport the message arrived on.
    pub service: Transport,
    /// Row shape.
    pub message_kind: MessageKind,
    /// Who sent the message: the owner for an outgoing one.
    pub sender: Sender,
    /// The owner's own address on this message: the one it was sent from
    /// (outgoing) or received at (incoming). `None` when the source knows
    /// only one owner address, which is then `export.owner_identity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_identity: Option<String>,
    /// Message subject line (rare).
    pub subject: Option<String>,
    /// Plain-text body; never includes attachment data.
    pub text: String,
    /// Attachment metadata in order; bytes live on disk or in `bytes`.
    pub attachments: Vec<Attachment>,
    /// The reactions that stand on this message, each naming the person who
    /// reacted. Removed reactions are already left out. Empty, and left out
    /// of the file, when the message has none or the source records none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<Reaction>,
    /// Deleted in the source app, or Unsent; `None`, and left out of the
    /// file, for a message that is neither or a source that records neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion: Option<Deletion>,
    /// The earlier versions of an edited message, oldest first within each
    /// part; [`Self::text`] is the final version. Empty, and left out of the
    /// file, for a message never edited or a source that records no edits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<EarlierVersion>,
    /// The message this one replies to; `None`, and left out of the file,
    /// for a message that is not a reply or a source that records no
    /// replies. [`ReplyTo::guid`] is the quoted message's GUID when the
    /// source names it, which does not promise the message is in the export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyTo>,
    /// Apple extensions; `None` for non-iMessage messages.
    pub imessage: Option<ImessageExtensions>,
    /// Vendor leftovers (Android type code and raw fields).
    pub source: Option<VendorFields>,
    /// The Backup Date: when the backup this message was read from was
    /// made, in Unix milliseconds. An iPhone backup's `Manifest.plist` date,
    /// an SMS Backup & Restore file's `backup_date`, or the backup file's
    /// modification time where the source records nothing better; a file
    /// an Export Run wrote carries each message's stored date, so one file
    /// can hold messages from several backups. `None`, and left out of the
    /// file, when nothing says. Between two copies of one message from one
    /// source, the import lets the copy from the later backup decide its
    /// deletion mark and text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_taken_at_unix_ms: Option<i64>,
}

/// Whether the owner sent or received the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Received from a peer.
    Incoming,
    /// Sent by the owner.
    Outgoing,
}

impl Message {
    /// Whether this is an SMS or an MMS, from any source, iMessage's SMS
    /// fallback included. An MMS carries the `Sms` service with the `Mms`
    /// kind. A message whose service is unknown (a Mac `chat.db` row with no
    /// service, or one pulled back from the server as `unknown`) is one when
    /// its kind says SMS or MMS, as every other layer reads it. RCS and every
    /// other service are not. The SMS Backup & Restore and SMS Backup+
    /// exports hold these and leave every other message out.
    pub fn is_sms_or_mms(&self) -> bool {
        match self.service {
            Transport::Sms => true,
            Transport::Unknown => {
                matches!(self.message_kind, MessageKind::Sms | MessageKind::Mms)
            }
            _ => false,
        }
    }
}

impl Direction {
    /// Lowercase storage id (`incoming` / `outgoing`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
        }
    }
}

/// Core attachment metadata shared by the IR attachment, the CSV cell, and the
/// mail MIME layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    /// Relative path under `attachments/` to the staged file.
    pub path: Option<String>,
    /// Filename the sender's device had for the file.
    pub original_name: Option<String>,
    /// Detected or declared MIME type.
    pub mime_type: Option<String>,
    /// 64-hex SHA-256 of the file contents (content addressing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest_sha256: Option<String>,
    /// Stored file length in bytes, as [`Attachment::size_bytes`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Why the file is not there, as [`Attachment::missing_reason`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_reason: Option<String>,
}

impl From<&Attachment> for AttachmentMeta {
    fn from(a: &Attachment) -> Self {
        Self {
            path: a.path.clone(),
            original_name: a.original_name.clone(),
            mime_type: a.mime_type.clone(),
            digest_sha256: a.digest_sha256.clone(),
            size_bytes: a.size_bytes,
            missing_reason: a.missing_reason.clone(),
        }
    }
}

/// Metadata for one attachment.
///
/// Bytes are never serialized: JSON, JSONL, and CSV carry only this metadata,
/// and the bytes live in a sidecar file under `attachments/` (or in `bytes`
/// for in-memory EML/MBOX/XML embedding).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Relative path under `attachments/` to the staged file.
    pub path: Option<String>,
    /// Filename the sender's device had for the file.
    pub original_name: Option<String>,
    /// Detected or declared MIME type.
    pub mime_type: Option<String>,
    /// 64-hex SHA-256 of the file contents (content addressing).
    pub digest_sha256: Option<String>,
    /// Sticker flag.
    pub is_sticker: bool,
    /// Transcribed text of the attachment (e.g., OCR of an image or a
    /// voice-note transcript).
    pub transcription: Option<String>,
    /// iMessage sticker effect name.
    pub sticker_effect: Option<String>,
    /// On-disk / stored asset length in bytes (not file contents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// None when the attachment was imported; set only when bytes were
    /// skipped, to one of the closed set in [`missing_reason`]:
    /// `file_missing`, `too_large`, `not_copied`, `not_fetched`, `no_path`,
    /// or `convert_failed: <detail>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_reason: Option<String>,
    /// In-memory bytes for EML embedding; never written to JSON.
    #[serde(skip)]
    pub bytes: Option<Vec<u8>>,
}

/// Vendor leftovers. Display names live on [`Sender::display_name`], not here.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VendorFields {
    /// Android `type` attribute from the source (e.g. 1 = received, 2 = sent).
    pub android_type: Option<i32>,
    /// Raw vendor attributes; display names never live here.
    #[serde(default)]
    pub fields: Map<String, Value>,
}

impl VendorFields {
    /// True when no vendor leftovers were recorded.
    pub fn is_empty(&self) -> bool {
        self.android_type.is_none() && self.fields.is_empty()
    }

    /// `None` when [`Self::is_empty`], else `Some(self)`.
    pub fn into_option(self) -> Option<Self> {
        if self.is_empty() { None } else { Some(self) }
    }
}

/// iMessage extensions. Nested Apple blobs remain JSON values (not strings).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImessageExtensions {
    /// iMessage send effect (e.g. `slam`).
    pub send_effect: Option<String>,
    /// Shared-location payload.
    pub shared_location: Option<String>,
    /// Announcement payload (e.g. group rename).
    pub announcement: Option<String>,
    /// RFC 3339 timestamp of the read receipt.
    pub read_receipt_rfc3339: Option<String>,
    /// Apple `parts` blob as a JSON value.
    pub parts: Option<Value>,
    /// Apple `app` blob as a JSON value.
    pub app: Option<Value>,
    /// Digital Touch balloon bundle id.
    pub balloon_bundle_id: Option<String>,
    /// Digital Touch balloon kind.
    pub balloon_kind: Option<String>,
    /// Tapback target message GUID.
    pub associated_guid: Option<String>,
    /// Tapback target part index.
    pub associated_part: Option<u32>,
    /// Tapback kind string.
    pub tapback_kind: Option<String>,
    /// Tapback emoji.
    pub tapback_emoji: Option<String>,
    /// Tapback action string.
    pub tapback_action: Option<String>,
}

impl ImessageExtensions {
    /// True when every field is unset (`None`).
    pub fn is_empty(&self) -> bool {
        self.send_effect.is_none()
            && self.shared_location.is_none()
            && self.announcement.is_none()
            && self.read_receipt_rfc3339.is_none()
            && self.parts.is_none()
            && self.app.is_none()
            && self.balloon_bundle_id.is_none()
            && self.balloon_kind.is_none()
            && self.associated_guid.is_none()
            && self.associated_part.is_none()
            && self.tapback_kind.is_none()
            && self.tapback_emoji.is_none()
            && self.tapback_action.is_none()
    }

    /// `None` when [`Self::is_empty`], else `Some(self)`.
    pub fn into_option(self) -> Option<Self> {
        if self.is_empty() { None } else { Some(self) }
    }
}

impl ConversationDocument {
    /// Recompute [`ConversationMeta::stats`] from `messages`.
    pub fn finalize_stats(&mut self) {
        self.conversation.stats = compute_stats(&self.messages);
    }
}

/// Count messages and attachments and find first/last timestamps.
fn compute_stats(messages: &[Message]) -> ConversationStats {
    let message_count = messages.len() as u64;
    let attachment_count = messages.iter().map(|m| m.attachments.len() as u64).sum();
    let mut first = None;
    let mut last = None;
    for message in messages {
        first = Some(first.map_or(message.timestamp_unix_ms, |f: i64| {
            f.min(message.timestamp_unix_ms)
        }));
        last = Some(last.map_or(message.timestamp_unix_ms, |l: i64| {
            l.max(message.timestamp_unix_ms)
        }));
    }
    ConversationStats {
        message_count,
        attachment_count,
        first_timestamp_unix_ms: first,
        last_timestamp_unix_ms: last,
    }
}

/// Format a Unix second as local / UTC / display strings.
///
/// Returns `None` when the timestamp cannot be represented in local or UTC.
pub fn format_local_ts(secs: i64) -> Option<(String, String, String)> {
    use chrono::{Local, TimeZone, Utc};
    let local = Local.timestamp_opt(secs, 0).single().or_else(|| {
        Utc.timestamp_opt(secs, 0)
            .single()
            .map(|utc| Local.from_utc_datetime(&utc.naive_utc()))
    })?;
    let utc = local.with_timezone(&Utc);
    let display = local.format("%b %e, %Y %I:%M:%S %p").to_string();
    Some((
        local.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        display,
    ))
}

/// `s` trimmed, or `None` when blank. The one place "blank means absent"
/// is spelled out; pass it to `Option::and_then` for optional fields.
pub fn trimmed(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t) }
}

/// Trimmed owned copy of `s`, or `None` when blank.
pub fn nonempty(s: &str) -> Option<String> {
    trimmed(s).map(str::to_string)
}

/// One mebibyte, for byte counts shown or compared in MiB.
pub const MIB: u64 = 1024 * 1024;

/// The sender of an outgoing row: the owner's identity and display name
/// (`"Me"` when the identity is set and the name is missing).
pub fn owner_sender(export: &ExportMeta) -> Sender {
    let identity = export.owner_identity.as_deref().and_then(nonempty);
    let display_name = export
        .owner_display_name
        .as_deref()
        .and_then(nonempty)
        .or_else(|| identity.as_ref().map(|_| "Me".into()));
    Sender {
        identity,
        display_name,
        country: None,
    }
}

/// Export and conversation metadata without messages (JSONL header line
/// and CSV header row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationHeader {
    /// Schema version written into this header ([`SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Where and how this export was produced.
    pub export: ExportMeta,
    /// Roster and computed stats for this conversation.
    pub conversation: ConversationMeta,
}

impl ConversationHeader {
    /// Copy export and conversation metadata from a full document.
    pub fn from_document(document: &ConversationDocument) -> Self {
        Self {
            schema_version: document.schema_version,
            export: document.export.clone(),
            conversation: document.conversation.clone(),
        }
    }

    /// The document a reader builds from a header and the messages that
    /// followed it, at the current schema version, with its stats computed.
    /// It has no stem suffix: a reader recovers that from the file name.
    pub fn into_document(self, messages: Vec<Message>) -> ConversationDocument {
        let mut document = ConversationDocument {
            schema_version: SCHEMA_VERSION,
            export: self.export,
            conversation: self.conversation,
            messages,
            packaging_stem_suffix: None,
        };
        document.finalize_stats();
        document
    }
}

/// Intermediate message before conversion to [`Message`].
///
/// Exporters parse vendor formats into these, then convert to
/// [`ConversationDocument`] through the shared [`project_pending`].
/// Exporter-specific metadata goes in [`Self::extra`].
#[derive(Debug, Clone)]
pub struct PendingMessage {
    /// Unix timestamp for chronological sort (seconds or milliseconds).
    pub sort_key: i64,
    /// Whether the owner sent the message.
    pub is_from_me: bool,
    /// Who sent the message, as the source names them, built with
    /// [`Sender::new`]. The projection gives an outgoing message the owner
    /// as its sender and ignores this for one.
    pub sender: Sender,
    /// Plain-text body; never includes attachment data.
    pub text: String,
    /// Relative paths to staged attachment files.
    pub attachments: Vec<PendingAttachment>,
    /// Per-exporter metadata (e.g., `key_id`, `android_type`, `xml_fields`).
    pub extra: std::collections::BTreeMap<String, String>,
}

impl PendingMessage {
    /// Read an exporter-specific string field from [`Self::extra`].
    pub fn extra_str(&self, key: &str) -> &str {
        self.extra.get(key).map_or("", String::as_str)
    }

    /// Read a boolean stored in [`Self::extra`] (values `"true"` / `"false"`).
    pub fn extra_flag(&self, key: &str) -> bool {
        self.extra.get(key).is_some_and(|v| v == "true")
    }

    /// Read an optional string field from [`Self::extra`] (empty = `None`).
    pub fn extra_opt(&self, key: &str) -> Option<String> {
        let v = self.extra_str(key);
        (!v.is_empty()).then(|| v.to_string())
    }
}

/// Intermediate attachment reference before conversion to [`Attachment`].
#[derive(Debug, Clone)]
pub struct PendingAttachment {
    /// Relative path to the staged file.
    pub rel_path: String,
    /// MIME content type.
    pub content_type: String,
    /// SHA-256 of the file contents; `None` when unknown.
    pub digest_sha256: Option<String>,
    /// Optional SMIL/content-location name.
    pub name_hint: Option<String>,
    /// Length of the decoded payload in bytes; `None` when the exporter
    /// does not hold the payload. Recorded whether or not the run copies
    /// the payload, so a run that copies no attachments still writes it.
    pub size_bytes: Option<u64>,
}

impl PendingAttachment {
    /// MIME type as an option; empty [`Self::content_type`] means `None`.
    pub fn mime_type(&self) -> Option<String> {
        (!self.content_type.is_empty()).then(|| self.content_type.clone())
    }

    /// The IR attachment for a queued one, with neither a path nor bytes,
    /// carrying the size when the exporter recorded one: the runner that
    /// writes the file reads the payload by its digest and fills the path
    /// in, and the size when it is still unknown.
    pub fn to_ir(&self) -> Attachment {
        Attachment {
            path: None,
            original_name: self.name_hint.clone(),
            mime_type: self.mime_type(),
            digest_sha256: self.digest_sha256.clone(),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: self.size_bytes,
            missing_reason: None,
            bytes: None,
        }
    }
}

/// Intermediate conversation before conversion to [`ConversationDocument`].
#[derive(Debug, Clone)]
pub struct PendingConversation {
    /// Stable chat id from the source.
    pub chat_id: String,
    /// Display name for the conversation. `None` when the source has none.
    pub display_name: Option<String>,
    /// Participant handles: a phone number in E.164 form where the source
    /// gives one, else the source's own handle (an email address, a short
    /// code, a name).
    pub participant_handles: Vec<String>,
    /// Messages awaiting conversion to [`Message`].
    pub messages: Vec<PendingMessage>,
    /// Whether this is a group conversation.
    pub is_group: bool,
    /// Whether any message in this conversation has attachments.
    pub has_attachments: bool,
    /// Per-exporter metadata.
    pub extra: std::collections::BTreeMap<String, String>,
}

impl PendingConversation {
    /// Fresh conversation with no messages and an empty `extra` map.
    pub fn new(
        chat_id: impl Into<String>,
        is_group: bool,
        display_name: Option<String>,
        participant_handles: Vec<String>,
    ) -> Self {
        Self {
            chat_id: chat_id.into(),
            display_name,
            participant_handles,
            messages: Vec::new(),
            is_group,
            has_attachments: false,
            extra: std::collections::BTreeMap::new(),
        }
    }

    /// Read an exporter-specific string field from [`Self::extra`].
    pub fn extra_str(&self, key: &str) -> &str {
        self.extra.get(key).map_or("", String::as_str)
    }

    /// First non-empty `contact_name` extra on a message in this conversation.
    pub fn first_contact_name(&self) -> Option<String> {
        self.messages
            .iter()
            .map(|m| m.extra_str("contact_name").trim())
            .find(|n| !n.is_empty())
            .map(str::to_string)
    }
}

#[cfg(test)]
mod identity_service_tests {
    use super::IdentityService;

    #[test]
    fn parse_phone_aliases() {
        for s in [
            "phone",
            "SMS",
            "imessage",
            "rcs",
            "mms",
            "Text message",
            "text_message",
        ] {
            assert_eq!(IdentityService::parse(s), IdentityService::Phone, "{s}");
        }
    }

    #[test]
    fn parse_whatsapp() {
        assert_eq!(
            IdentityService::parse("whatsapp"),
            IdentityService::Whatsapp
        );
        assert_eq!(IdentityService::parse("WA"), IdentityService::Whatsapp);
    }
}

/// Every enum value survives being written and read back.
///
/// Each list names every variant, so a new variant without a `parse` arm
/// fails here instead of turning into the fallback value on read. The storage
/// id must also be the serde name, because JSONL is written by serde and read
/// back through `parse` by the server.
#[cfg(test)]
mod storage_id_round_trip_tests {
    use super::*;

    fn assert_matches_serde<T: Serialize>(value: T, id: &str) {
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::Value::String(id.to_string()),
            "{id}"
        );
    }

    #[test]
    fn conversation_type() {
        for v in [
            ConversationType::OneToOne,
            ConversationType::Group,
            ConversationType::Orphaned,
        ] {
            assert_eq!(ConversationType::parse(v.as_str()), v);
            // `as_str` takes `one-to-one` and `orphaned` from
            // `imessage-reader-protocol`'s constants, so this also pins the
            // serde names to them.
            assert_matches_serde(v, v.as_str());
        }
    }

    #[test]
    fn identity_type() {
        for v in [
            IdentityType::Phone,
            IdentityType::Email,
            IdentityType::Username,
            IdentityType::Other,
        ] {
            assert_eq!(IdentityType::parse(v.as_str()), v);
            assert_matches_serde(v, v.as_str());
        }
    }

    #[test]
    fn service() {
        for v in [
            Transport::Sms,
            Transport::IMessage,
            Transport::Whatsapp,
            Transport::Rcs,
            Transport::Discord,
            Transport::Signal,
            Transport::Telegram,
            Transport::Slack,
            Transport::Unknown,
        ] {
            assert_eq!(Transport::parse(v.as_str()), v);
            assert_matches_serde(v, v.as_str());
        }
    }

    #[test]
    fn identity_service() {
        for v in [IdentityService::Phone, IdentityService::Whatsapp] {
            assert_eq!(IdentityService::parse(v.as_str()), v);
            assert_matches_serde(v, v.as_str());
        }
    }

    #[test]
    fn message_kind() {
        for v in [
            MessageKind::Sms,
            MessageKind::Mms,
            MessageKind::IMessage,
            MessageKind::Tapback,
            MessageKind::StickerTapback,
            MessageKind::Announcement,
            MessageKind::LocationShare,
            MessageKind::Balloon,
            MessageKind::Unknown,
        ] {
            assert_eq!(MessageKind::parse(v.as_str()), v);
            assert_matches_serde(v, v.as_str());
        }
    }

    #[test]
    fn direction() {
        for v in [Direction::Incoming, Direction::Outgoing] {
            assert_matches_serde(v, v.as_str());
        }
    }
}

#[cfg(test)]
mod pending_message_tests {
    use super::PendingMessage;
    use std::collections::BTreeMap;

    #[test]
    fn an_extra_flag_is_set_only_by_the_value_true() {
        let message = PendingMessage {
            sort_key: 0,
            is_from_me: false,
            sender: crate::Sender::default(),
            text: String::new(),
            attachments: Vec::new(),
            extra: BTreeMap::from([
                ("is_sticker".to_string(), "true".to_string()),
                ("is_deleted".to_string(), "false".to_string()),
            ]),
        };
        assert!(message.extra_flag("is_sticker"));
        assert!(!message.extra_flag("is_deleted"));
        assert!(!message.extra_flag("never_set"));
    }
}

/// Which identity types each service carries, and the type an address of
/// each shape takes there
/// (`docs/architecture/rules/contacts-identities-and-messages.md`, CONTACT-9 and CONTACT-10).
#[cfg(test)]
mod service_carries_tests {
    use super::{IdentityService, IdentityType};

    /// Every `(service, type)` pair and whether the service carries it;
    /// [`IdentityService::carries`] says why.
    const TABLE: [(IdentityService, IdentityType, bool); 8] = [
        (IdentityService::Phone, IdentityType::Phone, true),
        (IdentityService::Phone, IdentityType::Email, true),
        (IdentityService::Phone, IdentityType::Username, false),
        (IdentityService::Phone, IdentityType::Other, true),
        (IdentityService::Whatsapp, IdentityType::Phone, true),
        (IdentityService::Whatsapp, IdentityType::Email, false),
        (IdentityService::Whatsapp, IdentityType::Username, false),
        (IdentityService::Whatsapp, IdentityType::Other, true),
    ];

    #[test]
    fn each_service_carries_only_the_types_it_names() {
        for (service, kind, carried) in TABLE {
            assert_eq!(service.carries(kind), carried, "{service:?} {kind:?}");
        }
    }

    /// A shape the service carries keeps its type, and one it cannot carry
    /// is `other`, so an import never stores a pair the service refuses.
    #[test]
    fn a_shape_the_service_cannot_carry_is_other() {
        for (service, kind, carried) in TABLE {
            let expected = if carried { kind } else { IdentityType::Other };
            assert_eq!(service.type_on(kind), expected, "{service:?} {kind:?}");
            assert!(
                service.carries(service.type_on(kind)),
                "{service:?} {kind:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests;
