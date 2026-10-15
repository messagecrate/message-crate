//! Shared projection from [`PendingConversation`] to [`ConversationDocument`].
//!
//! Every exporter but Apple Messages builds its documents here (the
//! [`ProjectionHooks`] documentation says why Apple Messages does not). The
//! skeleton lives here once: participants with the single-peer fallback,
//! owner sender handling, sent/received tallying, the dedupe step, GUID
//! derivation, linking each reply to the message it quotes, and document
//! assembly. The genuine per-exporter deltas (vendor `source` fields, service
//! selection, attachment digests and vendor keys, reply keys, reactions,
//! attachment mapping) are supplied through [`ProjectionHooks`].

use crate::{
    Attachment, ConversationDocument, ConversationMeta, ConversationStats, ConversationType,
    Deletion, Direction, EarlierVersion, ExportMeta, Message, MessageCopy, MessageGuid,
    MessageIdentity, MessageKind, Participant, PendingAttachment, PendingConversation,
    PendingMessage, Reaction, ReplyTo, SCHEMA_VERSION, TimePrecision, Transport, VendorFields,
    format_local_ts, one_copy_per_message, owner_sender,
};
use std::collections::{BTreeMap, HashMap};

/// How the projection classifies one pending message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectedRole {
    /// Sent by the owner; counted as sent and given the owner sender identity.
    Outgoing,
    /// Received from a peer; counted as received.
    Incoming,
    /// System / notification row (e.g. an iMazing notification); counted
    /// separately and written as incoming with its own sender fields.
    Notification,
}

/// Unit of [`PendingMessage::sort_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKeyUnit {
    /// `sort_key` is Unix seconds (most exporters).
    Seconds,
    /// `sort_key` is Unix milliseconds (WhatsApp).
    Milliseconds,
}

impl SortKeyUnit {
    /// Convert a `sort_key` in this unit to Unix seconds.
    pub fn to_secs(self, key: i64) -> i64 {
        match self {
            Self::Seconds => key,
            Self::Milliseconds => key / 1000,
        }
    }
}

/// Counters from one projection, for the caller to fold into its report.
///
/// The projection cannot bump an exporter report directly (that type lives in
/// `message-crate-core`, which depends on this crate), so it returns plain
/// counts and each exporter folds the ones it tracks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionTally {
    /// Messages projected (every role).
    pub messages: u64,
    /// Messages with [`ProjectedRole::Outgoing`].
    pub sent: u64,
    /// Messages with [`ProjectedRole::Incoming`].
    pub received: u64,
    /// Messages with [`ProjectedRole::Notification`].
    pub notifications: u64,
    /// Copies dropped because they repeat a message kept (see [`one_copy_per_message`]).
    pub duplicates: u64,
}

/// What a staged message says about the message it quotes, for a source that
/// records replies by the quoted message's own key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingReply {
    /// The quoted message's key, compared with each message's
    /// [`ProjectionHooks::reply_key`] in the same conversation; `None` when
    /// the source names no key.
    pub quoted_key: Option<String>,
}

/// Per-exporter deltas of the shared [`pending_to_document`] projection.
///
/// Only [`export`](Self::export), [`service`](Self::service),
/// [`vendor_fields`](Self::vendor_fields), and
/// [`backup_taken_at_unix_ms`](Self::backup_taken_at_unix_ms) are required;
/// every other method has a default
/// matching the common exporter behavior.
///
/// Every exporter projects through these hooks except Apple Messages
/// (`imessage-ir-exporter`), which builds its documents from the messages
/// the Apple Messages Reader sends, already in [`Message`] form, for two
/// reasons. Each message keeps `chat.db`'s own `guid` as its guid, because
/// that is the name the database's replies (`thread_originator_guid`, read
/// into [`Message::reply_to`]) and tapbacks (`associated_message_guid`,
/// read into [`ImessageExtensions::associated_guid`](crate::ImessageExtensions::associated_guid)) give the
/// message they point at; a [`MessageGuid`] made here would leave every one
/// of them pointing at nothing. And the dedupe step has nothing to do there:
/// `chat.db` holds each message once, as one row with a `guid` no other
/// row has, so no copy repeats another.
pub trait ProjectionHooks {
    /// Export metadata (source / tool / version / owner) stamped on the document.
    fn export(&self) -> ExportMeta;

    /// Transport of one message (e.g. a constant `Transport::Sms`).
    fn service(&self, message: &PendingMessage) -> Transport;

    /// Vendor leftovers for one message. The projection applies
    /// [`VendorFields::into_option`], so an empty bag becomes `None`.
    fn vendor_fields(
        &self,
        conversation: &PendingConversation,
        message: &PendingMessage,
    ) -> VendorFields;

    /// The Backup Date of every message projected
    /// ([`Message::backup_taken_at_unix_ms`]): when the one backup the
    /// exporter read was made, or `None` when nothing says. Required, so no
    /// exporter leaves its messages undated by omission.
    fn backup_taken_at_unix_ms(&self) -> Option<i64>;

    /// Normalize a raw non-owner sender handle (e.g. guard phone digits).
    /// The default keeps the handle as-is.
    fn normalize_handle(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// What tells the message's attachments apart, for its [`MessageGuid`]
    /// and for the dedupe step ([`one_copy_per_message`]). The default uses
    /// each attachment's digest; an unknown digest counts as none.
    fn attachment_digests(&self, message: &PendingMessage) -> Vec<String> {
        message
            .attachments
            .iter()
            .filter_map(|a| a.digest_sha256.clone())
            .collect()
    }

    /// The source's own id for the message, where it has one. It goes into
    /// the [`MessageGuid`], and two copies with different ones are two
    /// messages. The default has none.
    fn vendor_key(&self, _message: &PendingMessage) -> Option<String> {
        None
    }

    /// The key a reply names to quote this message. The default has none.
    fn reply_key(&self, _message: &PendingMessage) -> Option<String> {
        None
    }

    /// `Some` when the message is a reply, naming the message it quotes. The
    /// projection links it to the one message of the conversation whose
    /// [`reply_key`](Self::reply_key) equals [`PendingReply::quoted_key`];
    /// with no such message, or more than one, the reply stays a reply with
    /// no link. The default records no replies.
    fn reply(&self, _message: &PendingMessage) -> Option<PendingReply> {
        None
    }

    /// The reactions that stand on the message, each naming the person who
    /// reacted, in the order the source gives them. The default records
    /// none.
    fn reactions(&self, _message: &PendingMessage) -> Vec<Reaction> {
        Vec::new()
    }

    /// Whether the message was deleted in the source app or unsent by its
    /// sender. The default marks none.
    fn deletion(&self, _message: &PendingMessage) -> Option<Deletion> {
        None
    }

    /// The earlier versions of an edited message, oldest first within each
    /// part. The default records none.
    fn edits(&self, _message: &PendingMessage) -> Vec<EarlierVersion> {
        Vec::new()
    }

    /// Map one queued attachment onto the shared [`Attachment`] shape.
    /// The default is [`PendingAttachment::to_ir`]: metadata only (no path,
    /// no bytes), with the size when the exporter recorded one.
    fn attachment_to_ir(&self, att: &PendingAttachment, _message: &PendingMessage) -> Attachment {
        att.to_ir()
    }

    /// Classify one message. The default maps `is_from_me` to
    /// [`ProjectedRole::Outgoing`] / [`ProjectedRole::Incoming`].
    fn role(&self, message: &PendingMessage) -> ProjectedRole {
        if message.is_from_me {
            ProjectedRole::Outgoing
        } else {
            ProjectedRole::Incoming
        }
    }

    /// Subject line of one message; the default has none.
    fn subject(&self, _message: &PendingMessage) -> Option<String> {
        None
    }

    /// Unit of [`PendingMessage::sort_key`]; the default is seconds.
    fn sort_key_unit(&self) -> SortKeyUnit {
        SortKeyUnit::Seconds
    }

    /// Order of messages inside one conversation before projection. The
    /// default sorts by `sort_key` alone; an exporter whose rows can share a
    /// timestamp adds its own tie-break so output stays deterministic.
    fn message_order(&self, a: &PendingMessage, b: &PendingMessage) -> std::cmp::Ordering {
        a.sort_key.cmp(&b.sort_key)
    }

    /// Row shape of one message. The default maps attachment-less rows to
    /// [`MessageKind::Sms`] and the rest to [`MessageKind::Mms`].
    fn message_kind(&self, message: &PendingMessage) -> MessageKind {
        if message.attachments.is_empty() {
            MessageKind::Sms
        } else {
            MessageKind::Mms
        }
    }

    /// Roster written on the document. The default is
    /// [`default_participants`]: `participant_e164s` with display names
    /// gathered from the messages, plus the single-peer chat-id fallback.
    fn participants(&self, chat_id: &str, conversation: &PendingConversation) -> Vec<Participant> {
        default_participants(chat_id, conversation, &|raw| self.normalize_handle(raw))
    }

    /// Group display title written on the document; the default writes none
    /// (Android group titles are stored as data, not used for filenames).
    fn group_title(&self, _conversation: &PendingConversation) -> Option<String> {
        None
    }

    /// On-disk stem suffix (e.g. `__whatsapp`); the default has none.
    fn packaging_stem_suffix(&self, _conversation: &PendingConversation) -> Option<String> {
        None
    }
}

/// The UTC instant of one pending message in milliseconds, and how finely
/// the source recorded it.
///
/// A sort key in milliseconds is the time. Otherwise a `date_ms` extra that
/// holds a number is the time to the millisecond, and without one the time
/// is the sort key's whole second.
pub fn message_time(message: &PendingMessage, unit: SortKeyUnit) -> (i64, TimePrecision) {
    match unit {
        SortKeyUnit::Milliseconds => (message.sort_key, TimePrecision::Milliseconds),
        SortKeyUnit::Seconds => match message.extra_str("date_ms").trim().parse::<i64>() {
            Ok(ms) => (ms, TimePrecision::Milliseconds),
            Err(_) => (
                message.sort_key.saturating_mul(1000),
                TimePrecision::Seconds,
            ),
        },
    }
}

/// What the projection reads from one pending message before it decides
/// which copies to keep.
struct Prepared {
    sender: Option<String>,
    timestamp_unix_ms: i64,
    precision: TimePrecision,
    attachment_digests: Vec<String>,
    vendor_key: Option<String>,
}

/// Build a [`ConversationDocument`] from one pending conversation.
///
/// The skeleton shared by every exporter: participants (with the single-peer
/// chat-id fallback), the owner sender for outgoing rows, one copy kept per
/// message ([`one_copy_per_message`]), a [`MessageGuid`] per message, and
/// document assembly. Timestamps must already be representable. Run
/// [`prepare_conversation`] (or an equivalent prune) first.
///
/// Returns the document and a [`ProjectionTally`] for the caller to fold into
/// its report. [`project_pending`] also says which pending message each of
/// the document's messages was made from.
pub fn pending_to_document<H: ProjectionHooks + ?Sized>(
    chat_id: &str,
    conversation: &PendingConversation,
    hooks: &H,
) -> (ConversationDocument, ProjectionTally) {
    let projection = project_pending(chat_id, conversation, hooks);
    (projection.document, projection.tally)
}

/// What [`project_pending`] made of one pending conversation.
#[derive(Debug, Clone)]
pub struct Projection {
    /// The conversation's document, as [`pending_to_document`] returns it.
    pub document: ConversationDocument,
    /// The counts, as [`pending_to_document`] returns them.
    pub tally: ProjectionTally,
    /// For each message of [`Self::document`], in order, the index in the
    /// conversation's `messages` of the pending message it was made from. A
    /// copy the dedupe step dropped has no entry.
    pub kept: Vec<usize>,
}

/// [`pending_to_document`], also naming the pending message each message of
/// the document was made from, for an exporter that reports something about
/// each message it kept (SMS Backup & Restore names each one with a part
/// left out, at the time the dedupe step kept).
pub fn project_pending<H: ProjectionHooks + ?Sized>(
    chat_id: &str,
    conversation: &PendingConversation,
    hooks: &H,
) -> Projection {
    let export = hooks.export();
    let (owner_sender_identity, owner_sender_display) = owner_sender(&export);
    let unit = hooks.sort_key_unit();

    let prepared: Vec<Prepared> = conversation
        .messages
        .iter()
        .map(|message| {
            let (timestamp_unix_ms, precision) = message_time(message, unit);
            Prepared {
                sender: (!message.sender_identity.is_empty())
                    .then(|| hooks.normalize_handle(&message.sender_identity)),
                timestamp_unix_ms,
                precision,
                attachment_digests: hooks.attachment_digests(message),
                vendor_key: hooks.vendor_key(message),
            }
        })
        .collect();
    let copies: Vec<MessageCopy<'_>> = conversation
        .messages
        .iter()
        .zip(&prepared)
        .map(|(message, p)| MessageCopy {
            is_from_me: message.is_from_me,
            sender: p.sender.as_deref(),
            timestamp_unix_ms: p.timestamp_unix_ms,
            precision: p.precision,
            text: &message.text,
            attachment_digests: &p.attachment_digests,
            vendor_key: p.vendor_key.as_deref(),
        })
        .collect();
    let kept = one_copy_per_message(&copies);

    let mut tally = ProjectionTally::default();
    let mut messages = Vec::with_capacity(conversation.messages.len());
    let mut kept_indices = Vec::with_capacity(conversation.messages.len());
    let mut replies: Vec<(usize, PendingReply)> = Vec::new();
    let mut guid_by_reply_key: HashMap<String, Option<String>> = HashMap::new();
    for (index, ((message, p), kept)) in conversation
        .messages
        .iter()
        .zip(&prepared)
        .zip(kept)
        .enumerate()
    {
        let Some((timestamp_unix_ms, time_precision)) = kept else {
            tally.duplicates += 1;
            continue;
        };
        kept_indices.push(index);
        let role = hooks.role(message);
        tally.messages += 1;
        match role {
            ProjectedRole::Outgoing => tally.sent += 1,
            ProjectedRole::Incoming => tally.received += 1,
            ProjectedRole::Notification => tally.notifications += 1,
        }

        let guid = MessageGuid::new(&MessageIdentity {
            chat: chat_id,
            is_from_me: message.is_from_me,
            sender: p.sender.as_deref(),
            timestamp_unix_ms,
            text: &message.text,
            attachment_digests: &p.attachment_digests,
            vendor_key: p.vendor_key.as_deref(),
        })
        .into_string();
        if let Some(key) = hooks.reply_key(message) {
            // A key two kept messages share names neither of them.
            guid_by_reply_key
                .entry(key)
                .and_modify(|guid| *guid = None)
                .or_insert_with(|| Some(guid.clone()));
        }
        if let Some(reply) = hooks.reply(message) {
            replies.push((messages.len(), reply));
        }

        let outgoing = role == ProjectedRole::Outgoing;
        let (sender_identity, sender_display_name) = if outgoing {
            (owner_sender_identity.clone(), owner_sender_display.clone())
        } else {
            (p.sender.clone(), message.sender_display_name.clone())
        };
        let attachments: Vec<Attachment> = message
            .attachments
            .iter()
            .map(|a| hooks.attachment_to_ir(a, message))
            .collect();

        messages.push(Message {
            guid,
            timestamp_unix_ms,
            time_precision,
            direction: if outgoing {
                Direction::Outgoing
            } else {
                Direction::Incoming
            },
            service: hooks.service(message),
            message_kind: hooks.message_kind(message),
            sender_identity,
            sender_display_name,
            owner_identity: None,
            subject: hooks.subject(message),
            text: message.text.clone(),
            attachments,
            reactions: hooks.reactions(message),
            deletion: hooks.deletion(message),
            edits: hooks.edits(message),
            // Linked below, once every guid is known.
            reply_to: None,
            imessage: None,
            source: hooks.vendor_fields(conversation, message).into_option(),
            sender_country: None,
            backup_taken_at_unix_ms: hooks.backup_taken_at_unix_ms(),
        });
    }
    for (index, reply) in replies {
        // A key that names the reply itself (a source that keys by the
        // second, when the quoted message is gone and the reply shares its
        // second) links to nothing: a message never quotes itself.
        let guid = reply
            .quoted_key
            .as_ref()
            .and_then(|key| guid_by_reply_key.get(key).cloned().flatten())
            .filter(|guid| *guid != messages[index].guid);
        // No source that stages its rows here records the part a reply
        // answers.
        messages[index].reply_to = Some(ReplyTo {
            guid,
            part_index: None,
        });
    }

    let document = ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export,
        conversation: ConversationMeta {
            chat_identifier: chat_id.to_string(),
            conversation_type: if conversation.is_group {
                ConversationType::Group
            } else {
                ConversationType::OneToOne
            },
            group_title: hooks.group_title(conversation),
            participants: hooks.participants(chat_id, conversation),
            stats: ConversationStats::default(),
        },
        messages,
        packaging_stem_suffix: hooks.packaging_stem_suffix(conversation),
    };
    Projection {
        document,
        tally,
        kept: kept_indices,
    }
}

/// Map of handle → display name from message extras and sender fields.
///
/// `normalize_handle` maps a raw sender handle onto the same form the
/// participant roster uses (e.g. guarded phone normalization).
pub fn display_names_for_handles(
    conversation: &PendingConversation,
    normalize_handle: &dyn Fn(&str) -> String,
) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for message in &conversation.messages {
        if !message.sender_identity.is_empty() {
            let handle = normalize_handle(&message.sender_identity);
            if let Some(name) = message
                .sender_display_name
                .as_deref()
                .and_then(crate::trimmed)
            {
                names.entry(handle).or_insert_with(|| name.to_string());
            }
        }
        if !conversation.is_group {
            let name = message.extra_str("contact_name").trim();
            if !name.is_empty() {
                for peer in &conversation.participant_e164s {
                    names
                        .entry(peer.clone())
                        .or_insert_with(|| name.to_string());
                }
            }
        }
    }
    names
}

/// The default roster: `participant_e164s` (with display names gathered from
/// the messages) plus the single-peer chat-id fallback when the roster is
/// empty for a one-to-one chat. The conversation that names nobody
/// ([`crate::NAMELESS_CHAT_ID`]) has no fallback: it is with nobody, and a
/// participant made from its key would make it a person.
pub fn default_participants(
    chat_id: &str,
    conversation: &PendingConversation,
    normalize_handle: &dyn Fn(&str) -> String,
) -> Vec<Participant> {
    let name_by_handle = display_names_for_handles(conversation, normalize_handle);
    let mut participants: Vec<Participant> = conversation
        .participant_e164s
        .iter()
        .filter(|h| !h.is_empty())
        .map(|h| Participant {
            identity: Some(h.clone()),
            display_name: name_by_handle.get(h).cloned(),
            country: None,
        })
        .collect();
    if participants.is_empty()
        && !conversation.is_group
        && !chat_id.is_empty()
        && chat_id != crate::NAMELESS_CHAT_ID
    {
        if crate::name_of_chat_id(chat_id).is_some() {
            // The source named this person and recorded no address for them,
            // so the chat id is made from the name. That chat id is not
            // something to store as an identity.
            participants.push(Participant {
                identity: None,
                display_name: conversation.first_contact_name(),
                country: None,
            });
        } else {
            participants.push(Participant {
                identity: Some(chat_id.to_string()),
                display_name: name_by_handle
                    .get(chat_id)
                    .cloned()
                    .or_else(|| conversation.first_contact_name()),
                country: None,
            });
        }
    }
    participants
}

/// Get or create the pending conversation for `chat_id`.
///
/// A conversation already in `map` is returned as-is; `is_group`,
/// `display_name`, and `participant_e164s` seed a new entry only.
pub fn ensure_conversation<'a>(
    map: &'a mut BTreeMap<String, PendingConversation>,
    chat_id: &str,
    is_group: bool,
    display_name: Option<String>,
    participant_e164s: Vec<String>,
) -> &'a mut PendingConversation {
    map.entry(chat_id.to_string()).or_insert_with(|| {
        PendingConversation::new(chat_id, is_group, display_name, participant_e164s)
    })
}

/// Sort messages with `cmp`, drop messages whose timestamp cannot be
/// represented, and set `has_attachments`.
///
/// `to_secs` converts a message sort key to Unix seconds (exporters that
/// store milliseconds pass `|k| k / 1000`).
///
/// Returns `(any_messages_remain, skipped_invalid_date_count)`; the caller
/// folds the skipped count into its report.
pub fn prepare_conversation(
    conversation: &mut PendingConversation,
    cmp: impl FnMut(&PendingMessage, &PendingMessage) -> std::cmp::Ordering,
    to_secs: impl Fn(i64) -> i64,
) -> (bool, u64) {
    conversation.messages.sort_by(cmp);
    let mut skipped = 0u64;
    conversation.messages.retain(|m| {
        if format_local_ts(to_secs(m.sort_key)).is_some() {
            true
        } else {
            skipped += 1;
            false
        }
    });
    conversation.has_attachments = conversation
        .messages
        .iter()
        .any(|m| !m.attachments.is_empty());
    (!conversation.messages.is_empty(), skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHooks;

    impl ProjectionHooks for TestHooks {
        fn export(&self) -> ExportMeta {
            ExportMeta {
                source: "test".into(),
                tool: "Test".into(),
                tool_version: "0".into(),
                owner_identity: Some("+15555550100".into()),
                owner_display_name: None,
                exported_at_unix_ms: None,
            }
        }

        fn backup_taken_at_unix_ms(&self) -> Option<i64> {
            None
        }

        fn service(&self, _message: &PendingMessage) -> Transport {
            Transport::Sms
        }

        fn vendor_fields(
            &self,
            _conversation: &PendingConversation,
            _message: &PendingMessage,
        ) -> VendorFields {
            VendorFields::default()
        }
    }

    fn pending_message(secs: i64, from_me: bool, text: &str) -> PendingMessage {
        PendingMessage {
            sort_key: secs,
            is_from_me: from_me,
            sender_identity: if from_me {
                String::new()
            } else {
                "+15555550122".into()
            },
            sender_display_name: (!from_me).then(|| "Peer".to_string()),
            text: text.into(),
            attachments: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

    /// Hooks that read a message's key from its `key` extra and a reply's
    /// quoted key from its `quotes` extra, as a source keyed like WhatsApp
    /// would.
    struct KeyedHooks;

    impl ProjectionHooks for KeyedHooks {
        fn export(&self) -> ExportMeta {
            TestHooks.export()
        }

        fn backup_taken_at_unix_ms(&self) -> Option<i64> {
            None
        }

        fn service(&self, _message: &PendingMessage) -> Transport {
            Transport::Whatsapp
        }

        fn vendor_fields(
            &self,
            _conversation: &PendingConversation,
            _message: &PendingMessage,
        ) -> VendorFields {
            VendorFields::default()
        }

        fn reply_key(&self, message: &PendingMessage) -> Option<String> {
            crate::trimmed(message.extra_str("key")).map(str::to_string)
        }

        fn reply(&self, message: &PendingMessage) -> Option<PendingReply> {
            message.extra.get("quotes").map(|quoted| PendingReply {
                quoted_key: crate::trimmed(quoted).map(str::to_string),
            })
        }
    }

    fn keyed(secs: i64, text: &str, key: &str, quotes: Option<&str>) -> PendingMessage {
        let mut m = pending_message(secs, false, text);
        m.extra.insert("key".into(), key.into());
        if let Some(quoted) = quotes {
            m.extra.insert("quotes".into(), quoted.into());
        }
        m
    }

    /// A reply links to the one message whose key it names. A key no
    /// message has, a key two messages share, a reply that names no key,
    /// and a reply that names its own key leave a reply with no link; a
    /// message that is not a reply has none.
    #[test]
    fn a_reply_links_only_to_the_one_message_with_its_key() {
        let mut conversation =
            PendingConversation::new("+15555550122", false, None, vec!["+15555550122".into()]);
        conversation.messages = vec![
            keyed(1_609_459_200, "quoted", "K1", None),
            keyed(1_609_459_201, "twin a", "K2", None),
            keyed(1_609_459_202, "twin b", "K2", None),
            keyed(1_609_459_203, "links", "K3", Some("K1")),
            keyed(1_609_459_204, "absent", "K4", Some("K9")),
            keyed(1_609_459_205, "ambiguous", "K5", Some("K2")),
            keyed(1_609_459_206, "no key", "K6", Some("")),
            keyed(1_609_459_207, "itself", "K7", Some("K7")),
        ];

        let (document, _) = pending_to_document("+15555550122", &conversation, &KeyedHooks);
        let reply_of = |text: &str| {
            document
                .messages
                .iter()
                .find(|m| m.text == text)
                .unwrap()
                .reply_to
                .clone()
        };
        let unlinked = Some(ReplyTo::default());
        assert_eq!(reply_of("quoted"), None);
        assert_eq!(
            reply_of("links"),
            Some(ReplyTo {
                guid: Some(document.messages[0].guid.clone()),
                part_index: None,
            })
        );
        assert_eq!(reply_of("absent"), unlinked);
        assert_eq!(reply_of("ambiguous"), unlinked);
        assert_eq!(reply_of("no key"), unlinked);
        assert_eq!(reply_of("itself"), unlinked);
    }

    #[test]
    fn a_projection_names_the_pending_message_each_kept_message_was_made_from() {
        let mut conversation =
            PendingConversation::new("+15555550122", false, None, vec!["+15555550122".into()]);
        conversation.messages = vec![
            pending_message(1_609_459_200, false, "hi"),
            pending_message(1_609_459_200, false, "hi"),
            pending_message(1_609_459_260, true, "yo"),
        ];

        let projection = project_pending("+15555550122", &conversation, &TestHooks);
        assert_eq!(projection.tally.duplicates, 1);
        let texts: Vec<_> = projection
            .document
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(texts, ["hi", "yo"]);
        assert_eq!(projection.kept, [0, 2], "the repeated copy has no entry");
    }

    #[test]
    fn projects_owner_sender_and_tallies_directions() {
        let mut conversation =
            PendingConversation::new("+15555550122", false, None, vec!["+15555550122".into()]);
        conversation.messages = vec![
            pending_message(1_609_459_200, false, "hi"),
            pending_message(1_609_459_260, true, "yo"),
        ];

        let (document, tally) = pending_to_document("+15555550122", &conversation, &TestHooks);
        assert_eq!(tally.messages, 2);
        assert_eq!(tally.sent, 1);
        assert_eq!(tally.received, 1);
        assert_eq!(document.messages.len(), 2);
        assert_eq!(document.messages[0].direction, Direction::Incoming);
        assert_eq!(
            document.messages[0].sender_identity.as_deref(),
            Some("+15555550122")
        );
        assert_eq!(document.messages[1].direction, Direction::Outgoing);
        assert_eq!(
            document.messages[1].sender_identity.as_deref(),
            Some("+15555550100")
        );
        assert_eq!(
            document.messages[1].sender_display_name.as_deref(),
            Some("Me")
        );
        assert_eq!(
            document.messages[0].timestamp_unix_ms, 1_609_459_200_000,
            "seconds sort keys fall back to secs * 1000"
        );
        assert_eq!(document.conversation.participants.len(), 1);
        assert_eq!(
            document.conversation.participants[0]
                .display_name
                .as_deref(),
            Some("Peer")
        );
    }

    #[test]
    fn single_peer_fallback_uses_chat_id_and_contact_name() {
        let mut conversation = PendingConversation::new("+15555550122", false, None, Vec::new());
        let mut m = pending_message(1_609_459_200, false, "hi");
        m.sender_identity = String::new();
        m.sender_display_name = None;
        m.extra.insert("contact_name".into(), "Bob".into());
        conversation.messages = vec![m];

        let (document, _) = pending_to_document("+15555550122", &conversation, &TestHooks);
        assert_eq!(document.conversation.participants.len(), 1);
        assert_eq!(
            document.conversation.participants[0].identity.as_deref(),
            Some("+15555550122")
        );
        assert_eq!(
            document.conversation.participants[0]
                .display_name
                .as_deref(),
            Some("Bob")
        );
    }

    /// The conversation that names nobody is with nobody: its key never
    /// becomes a participant, whom the server would make a contact (#1591).
    #[test]
    fn the_conversation_that_names_nobody_has_no_fallback_participant() {
        let mut conversation =
            PendingConversation::new(crate::NAMELESS_CHAT_ID, false, None, Vec::new());
        let mut m = pending_message(1_609_459_200, true, "hi");
        m.sender_identity = String::new();
        conversation.messages = vec![m];

        let (document, _) = pending_to_document(crate::NAMELESS_CHAT_ID, &conversation, &TestHooks);
        assert!(
            document.conversation.participants.is_empty(),
            "{:?}",
            document.conversation.participants
        );
    }

    /// GO SMS Pro and SMS Backup+ put the contact's name on every message of
    /// a one-to-one conversation, including the ones the owner sent. When the
    /// peer sent nothing, that is the only place the name is.
    #[test]
    fn a_contact_name_on_a_sent_message_names_the_peer_of_a_one_to_one_conversation() {
        let named = |name: &str| {
            let mut m = pending_message(1_609_459_200, true, "hi");
            m.extra.insert("contact_name".into(), name.into());
            m
        };
        let same = |raw: &str| raw.to_string();
        let peer = "+15555550122".to_string();

        let mut conversation = PendingConversation::new(&peer, false, None, vec![peer.clone()]);
        conversation.messages = vec![named(" Bob ")];
        assert_eq!(
            display_names_for_handles(&conversation, &same),
            HashMap::from([(peer.clone(), "Bob".to_string())])
        );

        conversation.messages = vec![named("  ")];
        assert!(
            display_names_for_handles(&conversation, &same).is_empty(),
            "a blank name names nobody"
        );

        let mut group = PendingConversation::new(
            "group-1",
            true,
            None,
            vec![peer, "+15555550133".to_string()],
        );
        group.messages = vec![named("Bob")];
        assert!(
            display_names_for_handles(&group, &same).is_empty(),
            "one name cannot belong to every member of a group"
        );
    }

    #[test]
    fn prepare_conversation_sorts_prunes_and_counts() {
        let mut conversation = PendingConversation::new("x", false, None, Vec::new());
        conversation.messages = vec![
            pending_message(1_609_459_260, true, "later"),
            pending_message(i64::MAX, false, "unrepresentable"),
            pending_message(1_609_459_200, false, "earlier"),
        ];
        let (keep, skipped) =
            prepare_conversation(&mut conversation, |a, b| a.sort_key.cmp(&b.sort_key), |k| k);
        assert!(keep);
        assert_eq!(skipped, 1);
        assert_eq!(conversation.messages.len(), 2);
        assert_eq!(conversation.messages[0].text, "earlier");
    }

    /// Hooks whose sort keys are milliseconds, as WhatsApp's are.
    struct MillisecondHooks;

    impl ProjectionHooks for MillisecondHooks {
        fn export(&self) -> ExportMeta {
            TestHooks.export()
        }

        fn backup_taken_at_unix_ms(&self) -> Option<i64> {
            None
        }

        fn service(&self, message: &PendingMessage) -> Transport {
            TestHooks.service(message)
        }

        fn vendor_fields(
            &self,
            conversation: &PendingConversation,
            message: &PendingMessage,
        ) -> VendorFields {
            TestHooks.vendor_fields(conversation, message)
        }

        fn sort_key_unit(&self) -> SortKeyUnit {
            SortKeyUnit::Milliseconds
        }
    }

    #[test]
    fn sort_key_unit_converts_to_seconds() {
        assert_eq!(SortKeyUnit::Seconds.to_secs(1_609_459_200), 1_609_459_200);
        assert_eq!(
            SortKeyUnit::Milliseconds.to_secs(1_609_459_200_999),
            1_609_459_200
        );
    }

    #[test]
    fn millisecond_sort_keys_keep_their_milliseconds() {
        let mut conversation = PendingConversation::new("+15555550122", false, None, Vec::new());
        conversation.messages = vec![pending_message(1_609_459_200_123, false, "hi")];

        let (document, _) = pending_to_document("+15555550122", &conversation, &MillisecondHooks);
        assert_eq!(document.messages[0].timestamp_unix_ms, 1_609_459_200_123);
        // The GUID is built from the message's millisecond.
        let guid = MessageGuid::new(&MessageIdentity {
            chat: "+15555550122",
            is_from_me: false,
            sender: Some("+15555550122"),
            timestamp_unix_ms: 1_609_459_200_123,
            text: "hi",
            attachment_digests: &[],
            vendor_key: None,
        });
        assert_eq!(document.messages[0].guid, guid.as_str());
    }

    /// GO SMS Pro writes an MMS once to its XML backup, with milliseconds and
    /// no attachments, and once as a PDU file, with whole seconds and the
    /// attachments. One message comes out, with both.
    #[test]
    fn an_xml_row_and_its_pdu_file_are_one_message_with_the_attachments_and_the_milliseconds() {
        let mut xml = pending_message(1_609_459_200, false, "look");
        xml.extra.insert("date_ms".into(), "1609459200250".into());
        let mut pdu = pending_message(1_609_459_200, false, "look");
        pdu.attachments = vec![PendingAttachment {
            rel_path: String::new(),
            content_type: "image/jpeg".into(),
            digest_sha256: Some("a".repeat(64)),
            name_hint: None,
            size_bytes: None,
        }];
        for order in [vec![xml.clone(), pdu.clone()], vec![pdu, xml]] {
            let mut conversation =
                PendingConversation::new("+15555550122", false, None, Vec::new());
            conversation.messages = order;
            let (document, tally) = pending_to_document("+15555550122", &conversation, &TestHooks);
            assert_eq!(document.messages.len(), 1);
            assert_eq!(tally.duplicates, 1);
            assert_eq!(tally.messages, 1);
            assert_eq!(document.messages[0].attachments.len(), 1);
            assert_eq!(document.messages[0].timestamp_unix_ms, 1_609_459_200_250);
        }
    }

    #[test]
    fn photo_only_messages_in_the_same_second_get_their_own_guids() {
        let photo = |digest: &str| {
            let mut m = pending_message(1_609_459_200, true, "");
            m.attachments = vec![PendingAttachment {
                rel_path: format!("attachments/{digest}.jpg"),
                content_type: "image/jpeg".into(),
                digest_sha256: Some(digest.to_string()),
                name_hint: None,
                size_bytes: None,
            }];
            m
        };
        let mut conversation = PendingConversation::new("+15555550122", false, None, Vec::new());
        conversation.messages = vec![photo(&"a".repeat(64)), photo(&"b".repeat(64))];

        let (document, _) = pending_to_document("+15555550122", &conversation, &TestHooks);
        assert_ne!(document.messages[0].guid, document.messages[1].guid);
    }

    #[test]
    fn p1_1_two_senders_same_second_same_text_get_their_own_guids() {
        let mut conversation = PendingConversation::new("group-1", true, None, Vec::new());
        let mut alice = pending_message(1_609_459_200, false, "lol");
        alice.sender_identity = "+15555550122".into();
        let mut bob = pending_message(1_609_459_200, false, "lol");
        bob.sender_identity = "+15555550133".into();
        conversation.messages = vec![alice, bob];
        let (document, _) = pending_to_document("group-1", &conversation, &TestHooks);
        assert_eq!(document.messages.len(), 2);
        assert_ne!(
            document.messages[0].guid, document.messages[1].guid,
            "Alice's and Bob's 'lol' in the same second share one GUID"
        );
    }

    /// Child half of the P1-2 test: prints the GUID under the process's TZ.
    #[test]
    #[ignore = "run by p1_2_guid_does_not_depend_on_the_computer_time_zone"]
    fn p1_2_child_print_guid() {
        let mut conversation = PendingConversation::new("+15555550122", false, None, Vec::new());
        conversation.messages = vec![pending_message(1_609_459_200, false, "hi")];
        let (document, _) = pending_to_document("+15555550122", &conversation, &TestHooks);
        println!("GUID={}", document.messages[0].guid);
    }

    #[test]
    fn p1_2_guid_does_not_depend_on_the_computer_time_zone() {
        let guid_in = |tz: &str| {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "projection::tests::p1_2_child_print_guid",
                    "--nocapture",
                ])
                .env("TZ", tz)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            stdout
                .lines()
                .find_map(|l| l.strip_prefix("GUID=").map(str::to_string))
                .expect("child printed a GUID")
        };
        let new_york = guid_in("America/New_York");
        let london = guid_in("Europe/London");
        assert_eq!(
            new_york, london,
            "the same message gets a different GUID in another time zone"
        );
    }
}
