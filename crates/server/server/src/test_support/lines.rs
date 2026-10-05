//! The conversation header line and message lines of a test's JSON Lines
//! batch, built from `message_ir`'s own types.
//!
//! The server's unit tests reach these through `test_support`. The
//! integration tests under `tests/` cannot reach a `cfg(test)` module of the
//! library, so `tests/common/mod.rs` compiles this file into each test binary
//! with `#[path]` (#1898). It uses nothing from the server crate for that
//! reason: only `message_ir`, `serde` and `serde_json`, which both kinds of
//! test can name.

/// The conversation header line that opens a test's JSON Lines batch, built
/// from `message_ir`'s own header type at [`message_ir::SCHEMA_VERSION`]. A
/// schema version bump or a renamed field reaches every test through this one
/// builder, rather than through a hand-written literal in each file (#1732).
///
/// It starts as a one-to-one conversation with no owner, no title and no
/// participants, and each method sets one more field. `Display` writes the
/// line as JSON without a trailing newline. The stats are left at zero,
/// because the import reads none of them.
#[derive(Debug, Clone)]
pub struct ConversationHeaderLine(message_ir::ConversationHeader);

/// A header line for a conversation from `source` keyed by `chat_identifier`.
pub fn conversation_header(source: &str, chat_identifier: &str) -> ConversationHeaderLine {
    ConversationHeaderLine(message_ir::ConversationHeader {
        schema_version: message_ir::SCHEMA_VERSION,
        export: message_ir::ExportMeta {
            source: source.to_string(),
            tool: "test".to_string(),
            tool_version: "0".to_string(),
            owner_identity: None,
            owner_display_name: None,
        },
        conversation: message_ir::ConversationMeta {
            chat_identifier: chat_identifier.to_string(),
            conversation_type: message_ir::IrConversationType::Individual,
            group_title: None,
            participants: Vec::new(),
            stats: message_ir::ConversationStats::default(),
        },
    })
}

impl ConversationHeaderLine {
    /// The account holder's identity, and the name they appear under.
    pub fn owner(mut self, identity: &str, display_name: Option<&str>) -> Self {
        self.0.export.owner_identity = Some(identity.to_string());
        self.0.export.owner_display_name = display_name.map(str::to_string);
        self
    }

    /// A group conversation.
    pub fn group(mut self) -> Self {
        self.0.conversation.conversation_type = message_ir::IrConversationType::Group;
        self
    }

    /// The title the source gave the conversation.
    pub fn title(mut self, title: &str) -> Self {
        self.0.conversation.group_title = Some(title.to_string());
        self
    }

    /// An orphaned conversation: messages the backup holds without
    /// recording which conversation they were said in.
    pub fn orphaned(mut self) -> Self {
        self.0.conversation.conversation_type = message_ir::IrConversationType::Orphaned;
        self
    }

    /// One more participant, reached at `identity`.
    pub fn participant(self, identity: &str, display_name: Option<&str>) -> Self {
        self.with_participant(Some(identity), display_name, None)
    }

    /// One more participant, reached at `identity` of a known type.
    pub fn typed_participant(
        self,
        identity: &str,
        display_name: Option<&str>,
        identity_type: message_ir::HandleType,
    ) -> Self {
        self.with_participant(Some(identity), display_name, Some(identity_type))
    }

    /// One more participant the source recorded no address for, named
    /// `display_name` when it named them at all.
    pub fn participant_without_identity(self, display_name: Option<&str>) -> Self {
        self.with_participant(None, display_name, None)
    }

    fn with_participant(
        mut self,
        identity: Option<&str>,
        display_name: Option<&str>,
        identity_type: Option<message_ir::HandleType>,
    ) -> Self {
        self.0
            .conversation
            .participants
            .push(message_ir::IrParticipant {
                identity: identity.map(str::to_string),
                display_name: display_name.map(str::to_string),
                identity_type,
            });
        self
    }

    /// The header as the first line of a file, ending in its newline.
    pub fn line(&self) -> String {
        format!("{self}\n")
    }
}

impl std::fmt::Display for ConversationHeaderLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_json(&self.0, f)
    }
}

/// A message line in a test's JSON Lines batch, built from `message_ir`'s own
/// message type. A renamed or newly required message field reaches every test
/// through this one builder, as a header field reaches them through
/// [`conversation_header`], rather than through a hand-written literal in
/// each file (#1885).
///
/// It starts as an incoming iMessage sent at 1426183462000 (2015-03-12
/// 18:04:22 UTC), with no sender, subject, attachments, reactions, deletion,
/// edits, `imessage` extensions or vendor leftovers, and each method sets one
/// more field. `Display` writes the line as JSON without a trailing newline.
///
/// A line that is not valid message-ir, such as one missing a required field
/// or holding the wrong type, cannot be built here. A test of how the server
/// reads a malformed or unusual line, or refuses one, writes that line out,
/// because the line is the input under test.
#[derive(Debug, Clone)]
pub struct MessageLine(message_ir::IrMessage);

/// A message line with the guid `guid` and the body `text`.
pub fn message_line(guid: &str, text: &str) -> MessageLine {
    MessageLine(message_ir::IrMessage {
        guid: guid.to_string(),
        timestamp_unix_ms: 1_426_183_462_000,
        direction: message_ir::IrDirection::Incoming,
        service: message_ir::IrService::IMessage,
        message_kind: message_ir::IrMessageKind::IMessage,
        sender_identity: None,
        sender_display_name: None,
        owner_identity: None,
        subject: None,
        text: text.to_string(),
        attachments: Vec::new(),
        reactions: Vec::new(),
        deletion: None,
        edits: Vec::new(),
        imessage: None,
        source: None,
    })
}

impl MessageLine {
    /// Sent at `timestamp_unix_ms`, in milliseconds since 1970-01-01 UTC.
    pub fn at(mut self, timestamp_unix_ms: i64) -> Self {
        self.0.timestamp_unix_ms = timestamp_unix_ms;
        self
    }

    /// Sent by the account holder.
    pub fn outgoing(mut self) -> Self {
        self.0.direction = message_ir::IrDirection::Outgoing;
        self
    }

    /// An SMS: the `sms` service and the `sms` kind.
    pub fn sms(self) -> Self {
        self.service(message_ir::IrService::Sms)
            .kind(message_ir::IrMessageKind::Sms)
    }

    /// An MMS: the `sms` service and the `mms` kind.
    pub fn mms(self) -> Self {
        self.service(message_ir::IrService::Sms)
            .kind(message_ir::IrMessageKind::Mms)
    }

    /// Carried by `service`. The kind stays as it was.
    pub fn service(mut self, service: message_ir::IrService) -> Self {
        self.0.service = service;
        self
    }

    /// Of the kind `kind`. The service stays as it was.
    pub fn kind(mut self, kind: message_ir::IrMessageKind) -> Self {
        self.0.message_kind = kind;
        self
    }

    /// Sent by the person reached at `identity`.
    pub fn sender(mut self, identity: &str) -> Self {
        self.0.sender_identity = Some(identity.to_string());
        self
    }

    /// The name the source gave the sender.
    pub fn sender_display_name(mut self, display_name: &str) -> Self {
        self.0.sender_display_name = Some(display_name.to_string());
        self
    }

    /// The account holder's own address on this message: the one it was
    /// sent from, or received at.
    pub fn owner_identity(mut self, identity: &str) -> Self {
        self.0.owner_identity = Some(identity.to_string());
        self
    }

    /// The subject line.
    pub fn subject(mut self, subject: &str) -> Self {
        self.0.subject = Some(subject.to_string());
        self
    }

    /// One more attachment, after the ones already set.
    pub fn attachment(mut self, attachment: message_ir::IrAttachment) -> Self {
        self.0.attachments.push(attachment);
        self
    }

    /// One more reaction that stands on the message.
    pub fn reaction(mut self, reaction: message_ir::Reaction) -> Self {
        self.0.reactions.push(reaction);
        self
    }

    /// Deleted in the source app, or Unsent, as `deletion` says.
    pub fn deletion(mut self, deletion: message_ir::Deletion) -> Self {
        self.0.deletion = Some(deletion);
        self
    }

    /// One more earlier version of the message, after the ones already set.
    pub fn edit(mut self, earlier: message_ir::EarlierVersion) -> Self {
        self.0.edits.push(earlier);
        self
    }

    /// The Apple extensions.
    pub fn imessage(mut self, imessage: message_ir::IrImessage) -> Self {
        self.0.imessage = Some(imessage);
        self
    }

    /// The message as one line of a file, ending in its newline.
    pub fn line(&self) -> String {
        format!("{self}\n")
    }
}

impl std::fmt::Display for MessageLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_json(&self.0, f)
    }
}

/// An attachment at `path` that the sender's device named `original_name`,
/// of the MIME type `mime_type`: not a sticker, with no fingerprint, size,
/// transcription or reason it is missing. A test sets any of those with
/// struct update syntax, `IrAttachment { digest_sha256: …, ..attachment(…) }`.
pub fn attachment(path: &str, original_name: &str, mime_type: &str) -> message_ir::IrAttachment {
    message_ir::IrAttachment {
        path: Some(path.to_string()),
        original_name: Some(original_name.to_string()),
        mime_type: Some(mime_type.to_string()),
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    }
}

/// Write `value` as one line of JSON, without a trailing newline: how both
/// line types display.
fn write_json(value: &impl serde::Serialize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&serde_json::to_string(value).map_err(|_| std::fmt::Error)?)
}
