//! The conversation header line and message lines of a test's JSON Lines
//! batch, built from `ir`'s own types.
//!
//! The server's unit tests reach these through `test_support`. The
//! integration tests under `tests/` cannot reach a `cfg(test)` module of the
//! library, so `tests/common/mod.rs` compiles this file into each test binary
//! with `#[path]` (#1898). It uses nothing from the server crate for that
//! reason: only `ir`, `serde` and `serde_json`, which both kinds of
//! test can name.

/// The conversation header line that opens a test's JSON Lines batch, built
/// from `ir`'s own header type at [`ir::SCHEMA_VERSION`]. A
/// schema version bump or a renamed field reaches every test through this one
/// builder, rather than through a hand-written literal in each file (#1732).
///
/// It starts as a one-to-one conversation with no owner, no title and no
/// participants, and each method sets one more field. `Display` writes the
/// line as JSON without a trailing newline. The stats are left at zero,
/// because the import reads none of them.
#[derive(Debug, Clone)]
pub struct ConversationHeaderLine(ir::ConversationHeader);

/// A header line for a conversation from `source` keyed by `chat_identifier`.
pub fn conversation_header(source: &str, chat_identifier: &str) -> ConversationHeaderLine {
    ConversationHeaderLine(ir::ConversationHeader {
        schema_version: ir::SCHEMA_VERSION,
        export: ir::ExportMeta {
            source: source.to_string(),
            tool: "test".to_string(),
            tool_version: "0".to_string(),
            owner_identity: None,
            owner_display_name: None,
            exported_at_unix_ms: None,
        },
        conversation: ir::ConversationMeta {
            chat_identifier: chat_identifier.to_string(),
            conversation_type: ir::ConversationType::OneToOne,
            group_title: None,
            participants: Vec::new(),
            stats: ir::ConversationStats::default(),
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
        self.0.conversation.conversation_type = ir::ConversationType::Group;
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
        self.0.conversation.conversation_type = ir::ConversationType::Orphaned;
        self
    }

    /// One more participant, reached at `identity`.
    pub fn participant(self, identity: &str, display_name: Option<&str>) -> Self {
        self.with_participant(Some(identity), display_name)
    }

    /// The country the source read the last participant's identity in, as
    /// an ISO 3166-1 alpha-2 code.
    pub fn in_country(mut self, country: &str) -> Self {
        self.0
            .conversation
            .participants
            .last_mut()
            .expect("a participant to give a country")
            .country = Some(country.to_string());
        self
    }

    /// One more participant the source recorded no address for, named
    /// `display_name` when it named them at all.
    pub fn participant_without_identity(self, display_name: Option<&str>) -> Self {
        self.with_participant(None, display_name)
    }

    fn with_participant(mut self, identity: Option<&str>, display_name: Option<&str>) -> Self {
        self.0.conversation.participants.push(ir::Participant {
            identity: identity.map(str::to_string),
            display_name: display_name.map(str::to_string),
            country: None,
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

/// A message line in a test's JSON Lines batch, built from `ir`'s own
/// message type. A renamed or newly required message field reaches every test
/// through this one builder, as a header field reaches them through
/// [`conversation_header`], rather than through a hand-written literal in
/// each file (#1885).
///
/// It starts as an incoming iMessage sent at 1426183462000 (2015-03-12
/// 18:04:22 UTC), with no sender, subject, attachments, reactions, deletion,
/// edits, reply, `imessage` extensions or vendor leftovers, and each method
/// sets one more field. `Display` writes the line as JSON without a trailing newline.
///
/// A line that is not a valid conversation file line, such as one missing a required field
/// or holding the wrong type, cannot be built here. A test of how the server
/// reads a malformed or unusual line, or refuses one, writes that line out,
/// because the line is the input under test.
#[derive(Debug, Clone)]
pub struct MessageLine(ir::Message);

/// A message line with the guid `guid` and the body `text`.
pub fn message_line(guid: &str, text: &str) -> MessageLine {
    MessageLine(ir::Message {
        guid: guid.to_string(),
        timestamp_unix_ms: 1_426_183_462_000,
        time_precision: ir::TimePrecision::Milliseconds,
        direction: ir::Direction::Incoming,
        service: ir::Transport::IMessage,
        message_kind: ir::MessageKind::IMessage,
        sender: ir::Sender::default(),
        owner_identity: None,
        subject: None,
        text: text.to_string(),
        attachments: Vec::new(),
        reactions: Vec::new(),
        deletion: None,
        edits: Vec::new(),
        reply_to: None,
        imessage: None,
        source: None,
        backup_taken_at_unix_ms: None,
    })
}

impl MessageLine {
    /// Sent at `timestamp_unix_ms`, in milliseconds since 1970-01-01 UTC.
    pub fn at(mut self, timestamp_unix_ms: i64) -> Self {
        self.0.timestamp_unix_ms = timestamp_unix_ms;
        self
    }

    /// Read from a backup made at `unix_ms`, in Unix milliseconds: the
    /// message's Backup Date. `None` says nothing, as a file without one.
    pub fn backup_taken_at(mut self, unix_ms: Option<i64>) -> Self {
        self.0.backup_taken_at_unix_ms = unix_ms;
        self
    }

    /// From a source that records whole seconds: `time_precision` is
    /// `seconds`. The time stays as it was.
    pub fn whole_seconds(mut self) -> Self {
        self.0.time_precision = ir::TimePrecision::Seconds;
        self
    }

    /// Sent by the account holder.
    pub fn outgoing(mut self) -> Self {
        self.0.direction = ir::Direction::Outgoing;
        self
    }

    /// An SMS: the `sms` service and the `sms` kind.
    pub fn sms(self) -> Self {
        self.service(ir::Transport::Sms).kind(ir::MessageKind::Sms)
    }

    /// An MMS: the `sms` service and the `mms` kind.
    pub fn mms(self) -> Self {
        self.service(ir::Transport::Sms).kind(ir::MessageKind::Mms)
    }

    /// Carried by `service`. The kind stays as it was.
    pub fn service(mut self, service: ir::Transport) -> Self {
        self.0.service = service;
        self
    }

    /// Of the kind `kind`. The service stays as it was.
    pub fn kind(mut self, kind: ir::MessageKind) -> Self {
        self.0.message_kind = kind;
        self
    }

    /// Sent by the person reached at `identity`.
    pub fn sender(mut self, identity: &str) -> Self {
        self.0.sender.identity = Some(identity.to_string());
        self
    }

    /// The country the source read the sender's identity in, as an ISO
    /// 3166-1 alpha-2 code.
    pub fn sender_country(mut self, country: &str) -> Self {
        self.0.sender.country = Some(country.to_string());
        self
    }

    /// The name the source gave the sender.
    pub fn sender_display_name(mut self, display_name: &str) -> Self {
        self.0.sender.display_name = Some(display_name.to_string());
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
    pub fn attachment(mut self, attachment: ir::Attachment) -> Self {
        self.0.attachments.push(attachment);
        self
    }

    /// One more reaction that stands on the message.
    pub fn reaction(mut self, reaction: ir::Reaction) -> Self {
        self.0.reactions.push(reaction);
        self
    }

    /// Deleted in the source app, or Unsent, as `deletion` says.
    pub fn deletion(mut self, deletion: ir::Deletion) -> Self {
        self.0.deletion = Some(deletion);
        self
    }

    /// One more earlier version of the message, after the ones already set.
    pub fn edit(mut self, earlier: ir::EarlierVersion) -> Self {
        self.0.edits.push(earlier);
        self
    }

    /// A reply that quotes the message `guid`, or a reply whose quoted
    /// message is not named when `guid` is `None`.
    pub fn reply_to(mut self, guid: Option<&str>) -> Self {
        self.0.reply_to = Some(ir::ReplyTo {
            guid: guid.map(str::to_string),
            part_index: None,
        });
        self
    }

    /// The Apple extensions.
    pub fn imessage(mut self, imessage: ir::ImessageExtensions) -> Self {
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
/// struct update syntax, `Attachment { digest_sha256: …, ..attachment(…) }`.
pub fn attachment(path: &str, original_name: &str, mime_type: &str) -> ir::Attachment {
    ir::Attachment {
        path: Some(path.to_string()),
        original_name: Some(original_name.to_string()),
        mime_type: Some(mime_type.to_string()),
        ..ir::testutil::sample_attachment()
    }
}

/// Write `value` as one line of JSON, without a trailing newline: how both
/// line types display.
fn write_json(value: &impl serde::Serialize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&serde_json::to_string(value).map_err(|_| std::fmt::Error)?)
}
