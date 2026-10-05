//! The wire protocol between the desktop app and the `imessage-reader` helper.
//!
//! The helper is a separate program because it links `imessage-database` and
//! `crabapple`, which are GPL-3.0-or-later, while the app is under the Fair
//! Core License. The two talk over pipes: the app writes one [`Request`] per
//! line on the helper's stdin and reads one [`Event`] per line from its
//! stdout. Every line is a JSON object, and every enum is tagged (`op` for
//! requests, `event` for events, `kind` for attachment sources), so a reader
//! that meets an unknown tag can say so instead of guessing.
//!
//! A session runs in this order:
//!
//! 1. The app sends [`Request::Export`] or [`Request::Identities`].
//! 2. The helper answers [`Event::Source`] once, then streams [`Event::Log`],
//!    [`Event::Progress`], [`Event::Conversation`] and [`Event::Message`]
//!    lines in any order, then
//!    [`Event::ExportDone`] (or [`Event::Identities`] for the identities
//!    request). [`Event::Error`] ends the request instead when it fails.
//! 3. For an encrypted backup the app may then send any number of
//!    [`Request::Attachment`] lines; the helper answers each with one
//!    [`Event::Attachment`].
//! 4. The app closes the helper's stdin, and the helper exits.
//!
//! [`Request::BackupDomain`] is a session of its own: the helper answers
//! [`Event::Source`], streams [`Event::Log`] and [`Event::Progress`] lines
//! while it decrypts, then sends [`Event::BackupDomainDone`].
//!
//! This crate carries the type definitions, their serde shapes, and one rule:
//! [`bare_address`], which says what an owner address looks like on the wire.
//! It is MIT OR Apache-2.0 so that both sides can link it.
//!
//! [`Reaction`], [`Deletion`] and [`EarlierVersion`] are defined here and
//! nowhere else. Each is the shape the conversation file carries too
//! (`message_ir` re-exports them), for every source, and this is the one
//! crate both the GPL reader and the app may link.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped when a change to these types or to the order of a session would
/// make an older helper and a newer app (or the reverse) misread each other.
/// The helper sends it in [`Event::Source`] and the app refuses any other
/// number, and any answer that comes before it.
///
/// 2: the identities request answers [`Event::Source`] first, as an export
/// does.
/// 3: [`Event::Identities`] values and [`Message::owner_identity`] are bare
/// addresses ([`bare_address`]); the app no longer strips prefixes itself.
/// 4: [`Request::BackupDomain`] and [`Event::BackupDomainDone`].
/// 5: [`Request::Identities`] carries a scratch folder
/// ([`IdentitiesRequest`]), and [`ExportRequest::scratch_dir`] is required.
/// 6: [`Event::Attachment`] carries an [`AttachmentFile`], which tells a
/// file the backup does not hold from one that failed to decrypt.
/// 7: every address is named an identity: [`Participant::identity`],
/// [`Message::sender_identity`], [`Message::owner_identity`] and the
/// tapbacks' `reactor_identity` (they were `handle`, `sender_handle`,
/// `owner_handle` and `reactor_handle`).
/// 8: a message's reactions are [`Message::reactions`], a list of
/// [`Reaction`], and no longer a JSON value in `Imessage::tapbacks`.
/// 9: a message deleted in Messages or unsent by its sender carries
/// [`Message::deletion`], and `Imessage::is_deleted` is gone.
/// 10: an edited message carries its earlier versions in
/// [`Message::edits`], a list of [`EarlierVersion`], and `Imessage::edits`
/// is gone.
/// 11: a message in no chat is in the [`ORPHANED_CONVERSATION_TYPE`]
/// conversation of its sender, keyed by [`orphaned_chat_id`], where every
/// such message was in one `individual` conversation named `orphaned`.
pub const PROTOCOL_VERSION: u32 = 11;

/// The [`Conversation::conversation_type`] of a conversation that holds
/// orphaned messages: messages the backup holds without recording which
/// conversation they were said in. Such a conversation is neither one-to-one
/// nor a group.
pub const ORPHANED_CONVERSATION_TYPE: &str = "orphaned";

/// What every orphaned conversation's chat id starts with, so it never
/// equals the sender's address, which keys their one-to-one conversation.
pub const ORPHANED_CHAT_ID_PREFIX: &str = "orphaned:";

/// The chat id of the orphaned conversation of `sender`: the orphaned
/// messages that person sent. `None` for the account holder's, whose
/// recipient is not recorded: they all sit in one conversation, keyed by the
/// prefix alone.
#[must_use]
pub fn orphaned_chat_id(sender: Option<&str>) -> String {
    format!("{ORPHANED_CHAT_ID_PREFIX}{}", sender.unwrap_or_default())
}

/// The owner address behind a raw `chat.account_login` or
/// `message.destination_caller_id` value, or `None` when nothing is left.
///
/// Apple stores the owner's addresses three ways: `P:+15555550110` and
/// `E:owner@example.com` in `chat.account_login`, and bare or as
/// `tel:+15555550110` in `message.destination_caller_id`. The helper puts
/// every one through this function before it crosses the pipe, so an
/// identity in [`Event::Identities`] and the owner of a [`Message`] are
/// spelled the same way and the app can compare them as text. Real backups
/// hold `account_login` rows that are the bare prefix `E:` with nothing after
/// it, so the emptiness test runs on the remainder.
#[must_use]
pub fn bare_address(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("P:")
        .or_else(|| trimmed.strip_prefix("E:"))
        .or_else(|| trimmed.strip_prefix("tel:"))
        .unwrap_or(trimmed)
        .trim();
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

/// The file name of the helper executable, without the `.exe` Windows adds.
pub const HELPER_NAME: &str = "imessage-reader";

/// Which Messages layout the source has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// A `chat.db` file, as on a Mac or copied off a jailbroken iPhone.
    MacOs,
    /// An iPhone backup folder holding `Manifest.plist`.
    Ios,
}

/// Where the Messages data is and how to open it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// The `chat.db` file (macOS) or the backup folder (iOS).
    pub db_path: PathBuf,
    /// Which layout `db_path` has.
    pub platform: Platform,
    /// The backup password, for an encrypted iOS backup.
    pub backup_password: Option<String>,
}

/// One request line on the helper's stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Read every message and stream it back as [`Event`] lines.
    Export(ExportRequest),
    /// Report the bare addresses the backup's device sent from.
    Identities(IdentitiesRequest),
    /// Decrypt one attachment of the export just streamed into a file the
    /// app can read. Only meaningful after [`Event::Source`] reported
    /// `encrypted: true`; a plain file needs no help.
    Attachment {
        /// The `path` an [`AttachmentSource::Path`] reported.
        path: PathBuf,
    },
    /// Decrypt every file of one domain of an encrypted iPhone backup into a
    /// folder. This is how another app's data (WhatsApp) comes out of an
    /// encrypted backup: the program that reads it cannot take the password.
    BackupDomain(BackupDomainRequest),
}

/// What reading a backup's identities needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitiesRequest {
    /// The Messages data to read.
    pub source: Source,
    /// A folder the helper writes decrypted files into, as
    /// [`ExportRequest::scratch_dir`].
    pub scratch_dir: PathBuf,
}

/// What decrypting one domain of an encrypted iPhone backup needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupDomainRequest {
    /// The backup folder holding `Manifest.plist`.
    pub backup_path: PathBuf,
    /// The backup password.
    pub backup_password: String,
    /// The domain as `Manifest.db` spells it, such as
    /// `AppDomainGroup-group.net.whatsapp.WhatsApp.shared`.
    pub domain: String,
    /// A folder the app owns. Each file is written to
    /// `<out_dir>/<domain>/<its path inside the domain>`.
    pub out_dir: PathBuf,
}

/// What an export run needs beyond the source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    /// The Messages data to read.
    pub source: Source,
    /// A folder attachments live under, when the database's own paths do
    /// not apply (a jailbroken phone's `sms.db` copied beside its files).
    pub attachment_root: Option<String>,
    /// An Apple Contacts database to take names from. macOS only.
    pub contacts_path: Option<PathBuf>,
    /// Name the owner by the destination caller id instead of `Me`.
    pub use_caller_id: bool,
    /// A folder the helper writes decrypted files into: an encrypted
    /// backup's message and contacts databases, and each attachment the app
    /// asks for. The app owns it and deletes it when the request ends, so a
    /// killed helper leaves nothing behind. A request without one is refused,
    /// because the system's temporary folder is no place for a decrypted
    /// database that a killed helper never deletes.
    pub scratch_dir: PathBuf,
}

/// One event line on the helper's stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// The source opened. Sent once, before any message.
    Source {
        /// The helper's [`PROTOCOL_VERSION`].
        protocol_version: u32,
        /// Whether attachment paths need [`Request::Attachment`] to read.
        encrypted: bool,
    },
    /// A progress or warning line for the app's log.
    Log {
        /// The line, without a trailing newline.
        line: String,
    },
    /// A count for the app's progress bar. Log lines are for people; nothing
    /// reads numbers back out of them.
    Progress(Progress),
    /// A conversation seen for the first time. Always precedes the first
    /// [`Event::Message`] that names its `chat_identifier`.
    Conversation(Conversation),
    /// One message. Boxed because it is several times the size of any other
    /// event, and events are passed around by value.
    Message(Box<Message>),
    /// The export stream ended.
    ExportDone {
        /// Rows read from the database, repeats included.
        messages_seen: u64,
        /// Rows that could not be converted and were skipped.
        failures: u64,
    },
    /// The answer to [`Request::Identities`].
    Identities {
        /// The distinct `chat.account_login` and
        /// `message.destination_caller_id` values, each through
        /// [`bare_address`]: prefixes stripped, blanks dropped. One address
        /// can still appear more than once when the columns spell it
        /// differently; the app deduplicates.
        values: Vec<String>,
    },
    /// The answer to [`Request::Attachment`].
    Attachment(AttachmentFile),
    /// The answer to [`Request::BackupDomain`].
    BackupDomainDone {
        /// Files written. Zero means the backup does not hold the domain.
        files: u64,
        /// Files the manifest lists that could not be decrypted. Each one
        /// has a line on the log.
        failures: u64,
    },
    /// The request failed. The helper writes nothing after this line.
    Error {
        /// A sentence for the person, in the app's own words where the
        /// helper knows them.
        message: String,
    },
}

/// What became of one attachment the app asked for ([`Request::Attachment`]).
///
/// A file the backup does not hold and a file the helper could not write
/// out are different outcomes: the first is a gap in the backup, the second
/// is a fault on this computer (a full disk, a damaged entry) that the
/// person should hear about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AttachmentFile {
    /// The file to read: the decrypted copy in the scratch folder, or the
    /// path itself for a source that is not encrypted.
    Ready {
        /// Where the file is.
        path: PathBuf,
    },
    /// The backup does not hold the file.
    Missing,
    /// The backup holds the file, and decrypting or writing it failed.
    Failed {
        /// Why, as the error said it.
        reason: String,
    },
}

/// Where the reader is, as counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Progress {
    /// A numbered setup step before any message is read: deriving backup
    /// keys, decrypting a database, caching a table.
    Setup {
        /// What the step does, for the bar's caption.
        label: String,
        /// This step's number, from 1.
        step: u64,
        /// How many setup steps there are.
        total: u64,
    },
    /// Rows read so far, out of the rows the database holds.
    Parse {
        /// Rows read.
        done: u64,
        /// Rows in total.
        total: u64,
    },
}

/// A conversation's roster and shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    /// Apple's `chat.chat_identifier`, or [`orphaned_chat_id`] for messages
    /// in no chat.
    pub chat_identifier: String,
    /// `individual`, `group`, or [`ORPHANED_CONVERSATION_TYPE`].
    pub conversation_type: String,
    /// The name a person gave the group, if any.
    pub group_title: Option<String>,
    /// Everyone in the chat other than the owner.
    pub participants: Vec<Participant>,
}

/// One person in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    /// Phone number or email address as Messages stores it.
    pub identity: String,
    /// The contact name, when the address book knows one.
    pub display_name: Option<String>,
}

/// One message row, already classified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The conversation this row belongs to.
    pub chat_identifier: String,
    /// Apple's message GUID.
    pub guid: String,
    /// Sent time as milliseconds since 1970-01-01 UTC.
    pub timestamp_unix_ms: i64,
    /// `true` for a message the owner sent.
    pub outgoing: bool,
    /// `iMessage`, `SMS`, `RCS`, or empty when unknown.
    pub service: String,
    /// `imessage`, `sms`, `mms`, `tapback`, `sticker_tapback`,
    /// `announcement`, `location_share`, or `balloon`.
    pub message_kind: String,
    /// The sender's address, for an incoming message.
    pub sender_identity: Option<String>,
    /// The sender's contact name, for an incoming message.
    pub sender_display_name: Option<String>,
    /// The subject line, when the message has one.
    pub subject: Option<String>,
    /// The body, or the sentence that stands in for a tapback or
    /// announcement.
    pub text: String,
    /// The reactions on this message that stand, in part, time and row
    /// order. A reaction that was later removed is not in the list, and a
    /// tapback row has none of its own.
    pub reactions: Vec<Reaction>,
    /// Whether the message was deleted in Messages or unsent by its sender;
    /// `None` for neither.
    pub deletion: Option<Deletion>,
    /// The earlier versions of an edited message, oldest first within each
    /// part; empty for a message never edited. `text` is the final version.
    pub edits: Vec<EarlierVersion>,
    /// The owner's address on this row (`destination_caller_id` through
    /// [`bare_address`]), or empty when the row carries none.
    pub owner_identity: String,
    /// The owner's display name, when `use_caller_id` asked for one.
    pub owner_display_name: Option<String>,
    /// Apple-specific fields; `None` when every field is empty.
    pub imessage: Option<Imessage>,
    /// Attachments the body references, in body order.
    pub attachments: Vec<Attachment>,
}

/// Everything Apple-specific the core message fields do not carry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Imessage {
    /// A reply inside a thread.
    pub is_reply: bool,
    /// The message replied to, or reacted to.
    pub in_reply_to_guid: Option<String>,
    /// Part index within the thread originator.
    pub thread_originator_part: Option<u32>,
    /// How many replies this message has.
    pub num_replies: Option<u32>,
    /// A send effect's label.
    pub send_effect: Option<String>,
    /// A shared-location label.
    pub shared_location: Option<String>,
    /// An announcement's text.
    pub announcement: Option<String>,
    /// When the message was read, RFC 3339.
    pub read_receipt_rfc3339: Option<String>,
    /// Body parts as a JSON array.
    pub parts: Option<Value>,
    /// An app balloon's payload.
    pub app: Option<Value>,
    /// The balloon's bundle id.
    pub balloon_bundle_id: Option<String>,
    /// The balloon's kind label.
    pub balloon_kind: Option<String>,
    /// For a tapback, the GUID it reacts to.
    pub associated_guid: Option<String>,
    /// For a tapback, the part index it reacts to.
    pub associated_part: Option<u32>,
    /// For a tapback, `loved`, `liked`, `emoji`, and so on.
    pub tapback_kind: Option<String>,
    /// For an emoji tapback, the emoji.
    pub tapback_emoji: Option<String>,
    /// For a tapback, `add` or `remove`.
    pub tapback_action: Option<String>,
}

/// One reaction on a message: who reacted, to which part, and with what.
///
/// Adds and removes are resolved before a list of these is written, so a
/// reaction has no action: it is one that stands. Each names its own reactor,
/// who is rarely the author of the message reacted to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaction {
    /// The part of the message reacted to; 0 for the first or only part.
    pub part_index: u32,
    /// What the reaction is: `loved`, `liked`, `disliked`, `laughed`,
    /// `emphasized`, `questioned`, `sticker`, or `emoji`.
    pub kind: String,
    /// For an `emoji` reaction, the emoji.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    /// `true` when the owner reacted.
    pub is_from_me: bool,
    /// The identity of the person who reacted, when someone other than the
    /// owner did and the source names them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reactor_identity: Option<String>,
    /// The name of the person who reacted, when the source knows one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reactor_display_name: Option<String>,
}

/// Why a message's content is gone, or marked as going, in the app it came
/// from.
///
/// Every source writes the same two marks, so the type is defined here, the
/// one crate the Apple Messages Reader and the rest of Message Crate may both
/// link, and `message-ir` re-exports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Deletion {
    /// The person deleted the message in the source app before the backup
    /// was made; the backup still holds it, with its text where it kept it.
    DeletedInSourceApp,
    /// The sender took the message back for everyone: every part of it was
    /// unsent, or some part was and no part has any content left. A message
    /// only partly unsent is not marked.
    Unsent,
}

impl Deletion {
    /// Both marks, in the order the docs list them.
    pub const ALL: [Self; 2] = [Self::DeletedInSourceApp, Self::Unsent];

    /// The mark as every format writes it: `deleted_in_source_app` or
    /// `unsent`, the same text serde writes.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeletedInSourceApp => "deleted_in_source_app",
            Self::Unsent => "unsent",
        }
    }
}

/// One earlier version of one part of an edited message: the text that part
/// held before an edit replaced it.
///
/// A message's own text is its final version, so a list of these holds only
/// the versions before it, oldest first within each part. Every source that
/// records edits writes the same shape, so the type is defined here beside
/// [`Reaction`] and `message-ir` re-exports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarlierVersion {
    /// The part of the message this version belongs to; 0 for the first or
    /// only part.
    pub part_index: u32,
    /// The part's text in this version.
    pub text: String,
    /// When this version was written, as milliseconds since 1970-01-01 UTC:
    /// the send time for the original, the time of the edit that wrote it for
    /// a later one. `None` when the source does not record it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at_unix_ms: Option<i64>,
}

/// One attachment's metadata and where its bytes are.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// The file name Messages transferred.
    pub original_name: Option<String>,
    /// The MIME type Messages recorded.
    pub mime_type: Option<String>,
    /// A sticker rather than a file.
    pub is_sticker: bool,
    /// Transcription of an audio message.
    pub transcription: Option<String>,
    /// The sticker's effect name.
    pub sticker_effect: Option<String>,
    /// Where the bytes are.
    pub source: AttachmentSource,
}

/// Where an attachment's bytes are.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachmentSource {
    /// A path the helper resolved. Readable as-is for a plain source; for an
    /// encrypted backup it names the entry to pass to [`Request::Attachment`].
    Path {
        /// The resolved path.
        path: PathBuf,
        /// The size Messages recorded, when it is known.
        size_hint: Option<u64>,
    },
    /// Text the helper rendered itself: a handwriting message as SVG.
    Inline {
        /// The rendered document.
        text: String,
    },
    /// No file to read.
    Missing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_events_round_trip_with_their_tags() {
        let request = Request::Export(ExportRequest {
            source: Source {
                db_path: "/tmp/chat.db".into(),
                platform: Platform::MacOs,
                backup_password: None,
            },
            attachment_root: None,
            contacts_path: None,
            use_caller_id: true,
            scratch_dir: "/tmp/scratch".into(),
        });
        let line = serde_json::to_string(&request).unwrap();
        assert!(line.starts_with(r#"{"op":"export""#), "{line}");
        let back: Request = serde_json::from_str(&line).unwrap();
        assert!(matches!(back, Request::Export(_)));

        for (answer, wire) in [
            (
                AttachmentFile::Ready {
                    path: "/scratch/a.mov".into(),
                },
                r#"{"event":"attachment","outcome":"ready","path":"/scratch/a.mov"}"#,
            ),
            (
                AttachmentFile::Missing,
                r#"{"event":"attachment","outcome":"missing"}"#,
            ),
            (
                AttachmentFile::Failed {
                    reason: "No space left on device".into(),
                },
                r#"{"event":"attachment","outcome":"failed","reason":"No space left on device"}"#,
            ),
        ] {
            let line = serde_json::to_string(&Event::Attachment(answer.clone())).unwrap();
            assert_eq!(line, wire);
            let back: Event = serde_json::from_str(&line).unwrap();
            assert!(
                matches!(back, Event::Attachment(a) if a == answer),
                "{line}"
            );
        }

        let request = Request::BackupDomain(BackupDomainRequest {
            backup_path: "/tmp/backup".into(),
            backup_password: "secret".into(),
            domain: "AppDomainGroup-group.net.whatsapp.WhatsApp.shared".into(),
            out_dir: "/tmp/out".into(),
        });
        let line = serde_json::to_string(&request).unwrap();
        assert!(line.starts_with(r#"{"op":"backup_domain""#), "{line}");
        let back: Request = serde_json::from_str(&line).unwrap();
        assert!(matches!(back, Request::BackupDomain(_)));

        let source = AttachmentSource::Inline {
            text: "<svg/>".into(),
        };
        let line = serde_json::to_string(&source).unwrap();
        assert_eq!(line, r#"{"kind":"inline","text":"<svg/>"}"#);
    }

    #[test]
    fn a_deletion_is_written_as_serde_writes_it() {
        for deletion in Deletion::ALL {
            assert_eq!(
                serde_json::to_string(&deletion).unwrap(),
                format!("\"{}\"", deletion.as_str())
            );
        }
    }

    #[test]
    fn bare_address_strips_each_prefix_and_drops_what_is_then_empty() {
        assert_eq!(
            bare_address("P:+15555550110").as_deref(),
            Some("+15555550110")
        );
        assert_eq!(
            bare_address("E:owner@example.com").as_deref(),
            Some("owner@example.com")
        );
        assert_eq!(
            bare_address("tel:+15555550110").as_deref(),
            Some("+15555550110")
        );
        assert_eq!(
            bare_address(" +15555550110 ").as_deref(),
            Some("+15555550110")
        );
        assert_eq!(bare_address("E:"), None);
        assert_eq!(bare_address("tel: "), None);
        assert_eq!(bare_address("  "), None);
    }

    /// The reader decrypts an encrypted backup's databases into the
    /// request's scratch folder, so a request that names none is refused
    /// rather than sent to the system's temporary folder, where a killed
    /// reader left the decrypted message database behind (#1135).
    #[test]
    fn a_request_without_a_scratch_folder_is_refused() {
        let source = r#""source":{"db_path":"/backup","platform":"ios","backup_password":"pw"}"#;
        for line in [
            r#"{"op":"identities","db_path":"/backup","platform":"ios","backup_password":"pw"}"#
                .to_string(),
            format!(r#"{{"op":"identities",{source}}}"#),
            format!(
                r#"{{"op":"export",{source},"attachment_root":null,"contacts_path":null,"use_caller_id":true}}"#
            ),
            format!(
                r#"{{"op":"export",{source},"attachment_root":null,"contacts_path":null,"use_caller_id":true,"scratch_dir":null}}"#
            ),
        ] {
            assert!(
                serde_json::from_str::<Request>(&line).is_err(),
                "accepted without a scratch folder: {line}"
            );
        }

        let line = format!(r#"{{"op":"identities",{source},"scratch_dir":"/app/scratch/one"}}"#);
        let Ok(Request::Identities(request)) = serde_json::from_str::<Request>(&line) else {
            panic!("an identities request with a scratch folder is read: {line}");
        };
        assert_eq!(request.scratch_dir, PathBuf::from("/app/scratch/one"));
    }
}
