//! The `X-ME-*` header names, spelled once for the writer and the reader.
//!
//! `build_eml` (writer) and `parse.rs` (reader) both name headers through
//! these constants, so a typo cannot silently break the EML roundtrip.

/// Conversation id header.
pub(crate) const CHAT_IDENTIFIER: &str = "X-ME-Chat-Identifier";
/// `individual` or `group`.
pub(crate) const CONVERSATION_TYPE: &str = "X-ME-Conversation-Type";
/// `incoming` or `outgoing`.
pub(crate) const DIRECTION: &str = "X-ME-Direction";
/// IR service (`sms`, `imessage`, …).
pub(crate) const SERVICE: &str = "X-ME-Service";
/// IR message kind (`sms`, `mms`, `tapback`, …).
pub(crate) const MESSAGE_KIND: &str = "X-ME-Message-Kind";
/// Message timestamp in Unix milliseconds.
pub(crate) const TIMESTAMP_UNIX_MS: &str = "X-ME-Timestamp-Unix-Ms";
/// Message guid.
pub(crate) const GUID: &str = "X-ME-Guid";
/// Export source id.
pub(crate) const EXPORT_SOURCE: &str = "X-ME-Export-Source";
/// Export tool name.
pub(crate) const EXPORT_TOOL: &str = "X-ME-Export-Tool";
/// Export tool version.
pub(crate) const EXPORT_TOOL_VERSION: &str = "X-ME-Export-Tool-Version";
/// Group chat title.
pub(crate) const GROUP_TITLE: &str = "X-ME-Group-Title";
/// Conversation roster as JSON.
pub(crate) const PARTICIPANTS: &str = "X-ME-Participants";
/// Sender identity.
pub(crate) const SENDER_IDENTITY: &str = "X-ME-Sender-Identity";
/// The message's reactions as JSON, a list of `message_ir::Reaction`.
pub(crate) const REACTIONS: &str = "X-ME-Reactions";
/// Why the message's content is gone in the source app:
/// `deleted_in_source_app` or `unsent`, as `message_ir::Deletion` writes it.
pub(crate) const DELETION: &str = "X-ME-Deletion";
/// The header an earlier Message Crate kept the Apple Messages deleted mark
/// in. The reader refuses a mail that carries it: nothing reads it, so the
/// mail would lose its mark.
pub(crate) const EARLIER_IS_DELETED: &str = "X-ME-Is-Deleted";
/// The headers an earlier Message Crate wrote for the sender and the owner,
/// when it named each address a handle. The reader refuses a mail that
/// carries one: nothing reads them, so the mail would lose its sender.
pub(crate) const EARLIER_HANDLE_HEADERS: &[&str] = &[
    "X-ME-Sender-Handle",
    "X-ME-Owner-Handle",
    "X-ME-Message-Owner-Handle",
];
/// Sender display name.
pub(crate) const SENDER_DISPLAY_NAME: &str = "X-ME-Sender-Display-Name";
/// The conversation's owner identity.
pub(crate) const OWNER_IDENTITY: &str = "X-ME-Owner-Identity";
/// The owner identity this one message was sent from or received at, when the
/// source records one per message (Apple Messages does).
pub(crate) const MESSAGE_OWNER_IDENTITY: &str = "X-ME-Message-Owner-Identity";
/// Owner display name.
pub(crate) const OWNER_DISPLAY_NAME: &str = "X-ME-Owner-Display-Name";
/// SMS/MMS subject.
pub(crate) const SUBJECT: &str = "X-ME-Subject";
/// Android SMS box type.
pub(crate) const ANDROID_TYPE: &str = "X-ME-Android-Type";
/// Vendor source fields as JSON.
pub(crate) const SOURCE_FIELDS: &str = "X-ME-Source-Fields";
/// iMessage reply flag.
pub(crate) const IS_REPLY: &str = "X-ME-Is-Reply";
/// Thread originator guid (reply parent).
pub(crate) const THREAD_ORIGINATOR_GUID: &str = "X-ME-Thread-Originator-Guid";
/// Thread originator part index.
pub(crate) const THREAD_ORIGINATOR_PART: &str = "X-ME-Thread-Originator-Part";
/// Reply count.
pub(crate) const NUM_REPLIES: &str = "X-ME-Num-Replies";
/// Send effect name.
pub(crate) const SEND_EFFECT: &str = "X-ME-Send-Effect";
/// Shared location payload.
pub(crate) const SHARED_LOCATION: &str = "X-ME-Shared-Location";
/// Group announcement text.
pub(crate) const ANNOUNCEMENT: &str = "X-ME-Announcement";
/// Read receipt timestamp (RFC 3339).
pub(crate) const READ_RECEIPT: &str = "X-ME-Read-Receipt";
/// Message parts as JSON.
pub(crate) const PARTS: &str = "X-ME-Parts";
/// An edited message's earlier versions as JSON, a list of
/// `message_ir::EarlierVersion`.
pub(crate) const EARLIER_VERSIONS: &str = "X-ME-Earlier-Versions";
/// The header an earlier Message Crate kept the Apple Messages edit history
/// in. The reader refuses a mail that carries it: nothing reads it, so the
/// mail would lose its earlier versions.
pub(crate) const EARLIER_EDIT_HISTORY: &str = "X-ME-Edits";
/// The header an earlier Message Crate kept Apple Messages reactions in.
/// The reader refuses a mail that carries it: nothing reads it, so the mail
/// would lose its reactions.
pub(crate) const EARLIER_TAPBACKS: &str = "X-ME-Tapbacks";
/// App/balloon payload as JSON.
pub(crate) const APP: &str = "X-ME-App";
/// Balloon bundle id.
pub(crate) const BALLOON_BUNDLE_ID: &str = "X-ME-Balloon-Bundle-Id";
/// Balloon kind.
pub(crate) const BALLOON_KIND: &str = "X-ME-Balloon-Kind";
/// Guid of the message a tapback targets.
pub(crate) const ASSOCIATED_GUID: &str = "X-ME-Associated-Guid";
/// Part index a tapback targets.
pub(crate) const ASSOCIATED_PART: &str = "X-ME-Associated-Part";
/// Tapback kind.
pub(crate) const TAPBACK_KIND: &str = "X-ME-Tapback-Kind";
/// Tapback emoji.
pub(crate) const TAPBACK_EMOJI: &str = "X-ME-Tapback-Emoji";
/// Tapback add/remove action.
pub(crate) const TAPBACK_ACTION: &str = "X-ME-Tapback-Action";
/// Attachment metadata as JSON.
pub(crate) const ATTACHMENT_META: &str = "X-ME-Attachment-Meta";
