//! Stream every row of `chat.db` to the app as protocol events.
//!
//! Each row becomes one [`MessageRecord`], already classified (tapback,
//! announcement, balloon, plain text) with its Apple-specific fields filled
//! in. The first row of each conversation is preceded by a
//! [`ConversationRecord`] carrying the roster. Nothing is written to disk
//! here; the app owns the output.

use std::collections::HashSet;

use imessage_database::{
    message_types::{
        edited::EditStatus,
        variants::{Announcement, Tapback, TapbackAction, Variant},
    },
    tables::{
        chat::Chat,
        messages::{
            Message,
            models::{GroupAction, Service},
        },
        table::{ME, Table, YOU},
    },
    util::dates::TIMESTAMP_FACTOR,
};
use imessage_reader_protocol::{
    Conversation as ConversationRecord, Deletion, Event, Imessage as ImessageRecord,
    Message as MessageRecord, ORPHANED_CONVERSATION_TYPE, Participant, Progress, Reaction, ReplyTo,
    TimePrecision, bare_address, orphaned_chat_id,
};
use serde_json::Value;

use crate::{
    attachments_emit::collect_parts_and_attachments,
    body::apply_body,
    error::RuntimeError,
    fields::{
        balloon_kind_label, balloon_summary, build_balloon_value, build_earlier_versions,
        expressive_label, parse_thread_part, shared_location_label,
    },
    log::emit,
    session::MailSession,
};

/// Report often enough that the message stream does not look frozen on large
/// backups.
const MESSAGE_PROGRESS_EVERY: u64 = 1_000;

/// Poll votes and updates: noise that CSV and HTML export skip too.
fn is_poll_noise(message: &Message) -> bool {
    message.is_poll_vote() || message.is_poll_update()
}

/// Stream every row of `chat.db` as events. A row that fails to convert is
/// logged and counted, not fatal.
///
/// # Errors
///
/// Returns an error when the database cannot be read.
pub(crate) fn stream_export(session: &MailSession) -> Result<(), RuntimeError> {
    let mut announced: HashSet<String> = HashSet::new();
    let mut last_row = -1;
    let mut seen = 0u64;
    let mut failures = 0u64;
    let total = Message::get_count(session.data_source.db(), &session.options.query_context)?;
    let mut statement =
        Message::stream_rows(session.data_source.db(), &session.options.query_context)?;

    for message in Message::rows(&mut statement, [])? {
        let mut msg = message?;
        seen += 1;
        // The stream repeats a row once per attachment join; keep the first.
        if msg.rowid == last_row {
            continue;
        }
        last_row = msg.rowid;
        if !msg.is_edited() && is_poll_noise(&msg) {
            continue;
        }
        apply_body(&mut msg, session.data_source.db());
        if is_poll_noise(&msg) {
            continue;
        }

        match build_record(session, &msg) {
            Ok((conversation, record)) => {
                if announced.insert(conversation.chat_identifier.clone()) {
                    emit(&Event::Conversation(conversation));
                }
                emit(&Event::Message(Box::new(record)));
            }
            Err(why) => {
                failures += 1;
                session.options.emit_log(format!(
                    "Message {} (row {} of the messages database) could not be read, so it is left out: {}",
                    msg.guid, msg.rowid, why
                ));
            }
        }
        if seen.is_multiple_of(MESSAGE_PROGRESS_EVERY) {
            session.options.emit_log(format!("  …{seen}/{total}"));
            emit(&Event::Progress(Progress::Parse {
                done: seen,
                total: u64::try_from(total).unwrap_or(u64::MAX),
            }));
        }
    }

    // The count above only reports every MESSAGE_PROGRESS_EVERY rows; say
    // the reading is finished, or the line stops short of its total.
    let read = seen.max(u64::try_from(total).unwrap_or(u64::MAX));
    emit(&Event::Progress(Progress::Parse {
        done: read,
        total: read,
    }));
    if failures > 0 {
        session.options.emit_log(if failures == 1 {
            "1 message could not be read and was left out".to_string()
        } else {
            format!("{failures} messages could not be read and were left out")
        });
    }
    emit(&Event::ExportDone {
        messages_seen: seen,
        failures,
    });
    Ok(())
}

/// Message time as milliseconds since 1970-01-01 UTC, and how finely
/// `chat.db` recorded it. A stamp of at least 10^12 is in nanoseconds (macOS
/// 10.13 and iOS 11 on), and its milliseconds are read from the stamp itself,
/// because `Message::date` cuts the time to the second. A smaller stamp is in
/// seconds, from an older database. A time read from the raw stamp, because
/// the library could not read the date, is whole seconds either way.
fn timestamp_unix_ms(message: &Message, offset: i64) -> (i64, TimePrecision) {
    let stamp = message.date;
    let nanoseconds = stamp >= 1_000_000_000_000;
    if let Ok(dt) = message.date(offset) {
        if nanoseconds {
            let millis = (stamp / 1_000_000).saturating_add(offset.saturating_mul(1000));
            return (millis, TimePrecision::Milliseconds);
        }
        return (dt.timestamp_millis(), TimePrecision::Seconds);
    }
    let seconds_since_2001 = if nanoseconds {
        stamp / TIMESTAMP_FACTOR
    } else {
        stamp
    };
    (
        (seconds_since_2001 + offset).saturating_mul(1000),
        TimePrecision::Seconds,
    )
}

/// When the message was read, as RFC 3339, or `None` for a message never
/// read. `chat.db` stores `date_read` as NULL for an unread row and the
/// library reads that back as `0`; `Message::date_read` does not treat `0`
/// as "no date", it returns Apple's epoch, so the raw stamp is checked first.
fn read_receipt_rfc3339(message: &Message, offset: i64) -> Option<String> {
    if message.date_read == 0 {
        return None;
    }
    message.date_read(offset).ok().map(|d| d.to_rfc3339())
}

/// The address of a Messages `handle_id`: that one handle row's `id`. Handle
/// 0 is no handle, so a received row with handle 0 has no sender.
fn raw_handle(session: &MailSession, handle_id: i32) -> Option<String> {
    session.handle_address(handle_id).map(str::to_string)
}

/// Contact display name for a Messages `handle_id`, falling back to the handle.
fn display_name_for(session: &MailSession, handle_id: i32) -> Option<String> {
    session
        .handle_name(handle_id)
        .or_else(|| session.handle_address(handle_id))
        .map(str::to_string)
}

/// Participants and conversation type (`individual` / `group`) for one chat room.
///
/// The members are the chat's `chat_handle_join` rows, each one handle's
/// own address, less any address of the owner's: Apple lists the owner's own
/// handle among a group's members, and the account holder is never a
/// participant. A one-to-one chat with no handle rows takes its identifier's
/// address as its member; a group with none lists nobody.
fn participants_for(session: &MailSession, chatroom: &Chat) -> (Vec<Participant>, &'static str) {
    let group = session.is_group(chatroom);
    let mut records: Vec<Participant> = Vec::new();
    let mut add = |address: &str, display_name: Option<String>| {
        let address = address.trim();
        if address.is_empty()
            || session.is_owner(address)
            || records.iter().any(|p| p.identity == address)
        {
            return;
        }
        records.push(Participant {
            identity: address.to_string(),
            display_name,
        });
    };
    match session.chat_members.get(&chatroom.rowid) {
        Some(handles) => {
            for &handle_id in handles {
                if let Some(address) = session.handle_address(handle_id) {
                    add(address, session.handle_name(handle_id).map(str::to_string));
                }
            }
        }
        None if !group => {
            let address = chatroom.chat_identifier.as_str();
            add(address, session.address_name(address));
        }
        None => {}
    }
    let conversation_type = if group { "group" } else { "individual" };
    (records, conversation_type)
}

/// The conversation an orphaned message sits in: the one of the person it
/// names, with that person as its only participant, kept apart from their
/// one-to-one conversation because the backup does not say the message was
/// said there. A received row names its sender, and a sent row names its
/// recipient in the same `handle_id`, as Apple writes a one-to-one chat's
/// sent rows, so both directions of one person's orphaned rows sit together.
/// A row that names nobody, sent with `handle_id` 0 as a group's rows are,
/// received with no sender, or received from one of the holder's own
/// addresses, sits in the one conversation with no participants (#1778).
fn orphaned_conversation(session: &MailSession, message: &Message) -> ConversationRecord {
    let person = message.handle_id.and_then(|handle_id| {
        let address = session.handle_address(handle_id)?.trim();
        (!address.is_empty() && !session.is_owner(address)).then(|| Participant {
            identity: address.to_string(),
            display_name: session.handle_name(handle_id).map(str::to_string),
        })
    });
    ConversationRecord {
        chat_identifier: orphaned_chat_id(person.as_ref().map(|p| p.identity.as_str())),
        conversation_type: ORPHANED_CONVERSATION_TYPE.to_string(),
        group_title: None,
        participants: person.into_iter().collect(),
    }
}

/// Human-readable text for a group announcement (rename, add, leave, and similar).
fn announcement_text(session: &MailSession, msg: &Message) -> Option<String> {
    let announcement = msg.get_announcement()?;
    let mut who = session.who(
        msg.handle_id,
        msg.is_from_me(),
        msg.destination_caller_id.as_deref(),
    );
    if who == ME {
        who = YOU;
    }
    let participant_name = match &announcement {
        Announcement::GroupAction(
            GroupAction::ParticipantAdded(handle) | GroupAction::ParticipantRemoved(handle),
        ) => session.who(Some(*handle), false, msg.destination_caller_id.as_deref()),
        _ => "someone",
    };

    let body = match &announcement {
        Announcement::AudioMessageKept => "kept an audio message.".to_string(),
        Announcement::FullyUnsent => "unsent a message!".to_string(),
        Announcement::Unknown(num) => format!("performed unknown action {num}."),
        Announcement::GroupAction(group) => match group {
            GroupAction::ParticipantAdded(_) => {
                format!("added {participant_name} to the conversation.")
            }
            GroupAction::ParticipantRemoved(_) => {
                format!("removed {participant_name} from the conversation.")
            }
            GroupAction::NameChange(name) => format!("named the conversation {name}"),
            GroupAction::ParticipantLeft => "left the conversation.".to_string(),
            GroupAction::GroupIconChanged => "changed the group photo.".to_string(),
            GroupAction::GroupIconRemoved => "removed the group photo.".to_string(),
            GroupAction::ChatBackgroundChanged => "changed the chat background.".to_string(),
            GroupAction::ChatBackgroundRemoved => "removed the chat background.".to_string(),
            GroupAction::PhoneNumberChanged(_) => "changed their phone number.".to_string(),
        },
    };
    Some(format!("{who} {body}"))
}

/// The owner's address on this row, bare, or `None` when Apple wrote NULL.
///
/// `destination_caller_id` is the same address `chat.account_login` carries
/// with a `P:` or `E:` prefix, and some rows carry it as `tel:+1…`. One rule,
/// [`bare_address`], strips all three so the address on a message equals the
/// same address in the identities answer.
fn owner_address(message: &Message) -> Option<String> {
    message
        .destination_caller_id
        .as_deref()
        .and_then(bare_address)
}

/// Owner display name from the destination caller id, or `Me`, when that option is on.
fn owner_display_name(session: &MailSession, message: &Message) -> Option<String> {
    if session.options.use_caller_id {
        owner_address(message).or_else(|| Some(ME.to_string()))
    } else {
        None
    }
}

/// One-line description of a tapback (Loved, Liked, Removed Heart, and similar).
fn tapback_human_line(kind: &str, emoji: Option<&str>, action: &str) -> String {
    if action == "remove" {
        return match kind {
            "loved" => "Removed Heart".into(),
            "liked" => "Removed Like".into(),
            "disliked" => "Removed Dislike".into(),
            "laughed" => "Removed Laugh".into(),
            "emphasized" => "Removed Exclamation".into(),
            "questioned" => "Removed Question Mark".into(),
            "emoji" => format!("Removed {}", emoji.unwrap_or("emoji")),
            "sticker" => "Removed Sticker".into(),
            other => format!("Removed {other}"),
        };
    }
    match kind {
        "loved" => "Loved a message".into(),
        "liked" => "Liked a message".into(),
        "disliked" => "Disliked a message".into(),
        "laughed" => "Laughed at a message".into(),
        "emphasized" => "Emphasized a message".into(),
        "questioned" => "Questioned a message".into(),
        "emoji" => format!("{} reacted", emoji.unwrap_or("Emoji")),
        "sticker" => "Reacted with a sticker".into(),
        other => format!("{other} reaction"),
    }
}

/// The name of a reaction and, for an emoji reaction, the emoji itself.
fn tapback_kind(kind: Tapback<'_>) -> (&'static str, Option<String>) {
    match kind {
        Tapback::Loved => ("loved", None),
        Tapback::Liked => ("liked", None),
        Tapback::Disliked => ("disliked", None),
        Tapback::Laughed => ("laughed", None),
        Tapback::Emphasized => ("emphasized", None),
        Tapback::Questioned => ("questioned", None),
        Tapback::Emoji(e) => ("emoji", e.map(str::to_string)),
        Tapback::Sticker => ("sticker", None),
    }
}

/// The reactions that stand on this message, in part, date and row order.
/// A removed reaction is left out, so the list needs no action.
fn build_reactions(session: &MailSession, message: &Message) -> Vec<Reaction> {
    let Some(parts) = session.tapbacks.get(&message.guid) else {
        return Vec::new();
    };
    let mut sortable: Vec<(usize, i64, i32, Reaction)> = Vec::new();
    for (&part_index, tapbacks) in parts {
        for tapback in tapbacks {
            let Variant::Tapback(_, action, kind) = tapback.variant() else {
                continue;
            };
            if matches!(action, TapbackAction::Removed) {
                continue;
            }
            let Ok(part) = u32::try_from(part_index) else {
                continue;
            };
            let (kind, emoji) = tapback_kind(kind);
            let (reactor_identity, reactor_display_name) = if tapback.is_from_me() {
                (
                    None,
                    Some(owner_display_name(session, tapback).unwrap_or_else(|| ME.to_string())),
                )
            } else if let Some(handle_id) = tapback.handle_id {
                (
                    raw_handle(session, handle_id),
                    display_name_for(session, handle_id),
                )
            } else {
                (None, None)
            };
            sortable.push((
                part_index,
                tapback.date,
                tapback.rowid,
                Reaction {
                    part_index: part,
                    kind: kind.to_string(),
                    emoji: trimmed(emoji),
                    is_from_me: tapback.is_from_me(),
                    reactor_identity,
                    reactor_display_name,
                },
            ));
        }
    }
    sortable.sort_by_key(|(part, date, rowid, _)| (*part, *date, *rowid));
    sortable.into_iter().map(|(_, _, _, r)| r).collect()
}

/// Chat id, roster and sender fields for one row.
struct RowContext {
    conversation: ConversationRecord,
    is_from_me: bool,
    sender_identity: Option<String>,
    sender_display_name: Option<String>,
    service: String,
}

/// Resolve the conversation and sender a row belongs to. A row in no chat,
/// or in a chat with no identifier to key it by, is an orphaned message and
/// lands in the orphaned conversation of the person it names
/// ([`orphaned_conversation`]).
fn resolve_context(session: &MailSession, message: &Message) -> RowContext {
    let conversation = match session.conversation(message) {
        Some(chatroom) if !chatroom.chat_identifier.is_empty() => {
            let (participants, conversation_type) = participants_for(session, chatroom);
            ConversationRecord {
                chat_identifier: chatroom.chat_identifier.clone(),
                conversation_type: conversation_type.to_string(),
                group_title: chatroom
                    .display_name()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_string),
                participants,
            }
        }
        _ => orphaned_conversation(session, message),
    };

    let is_from_me = message.is_from_me();
    let (sender_identity, sender_display_name) = if is_from_me {
        (None, None)
    } else if let Some(handle_id) = message.handle_id {
        (
            raw_handle(session, handle_id),
            display_name_for(session, handle_id),
        )
    } else {
        (None, None)
    };

    let service = match message.service() {
        Service::Unknown => String::new(),
        other => other.to_string(),
    };

    RowContext {
        conversation,
        is_from_me,
        sender_identity,
        sender_display_name,
        service,
    }
}

/// Build the conversation and message records for one row.
///
/// # Errors
///
/// Returns an error when body parts or attachments cannot be loaded.
fn build_record(
    session: &MailSession,
    message: &Message,
) -> Result<(ConversationRecord, MessageRecord), RuntimeError> {
    let context = resolve_context(session, message);
    let (parts, attachments) = collect_parts_and_attachments(session, message)?;
    let deletion = deletion(message, !attachments.is_empty());
    let mut row = classify_row(
        session,
        message,
        &context.service,
        !attachments.is_empty(),
        deletion,
    );
    let kind = row.kind;
    let text = std::mem::take(&mut row.text);
    // A tapback has no reactions of its own.
    let reactions = if row.tapback.is_some() {
        Vec::new()
    } else {
        build_reactions(session, message)
    };
    let edits = message
        .edited_parts
        .as_ref()
        .map(|edited| build_earlier_versions(edited, session.offset))
        .unwrap_or_default();
    let reply_to = reply_to(message, row.tapback.as_ref());
    let imessage = imessage_fields(session, message, row, &parts);

    let (timestamp_unix_ms, time_precision) = timestamp_unix_ms(message, session.offset);
    let record = MessageRecord {
        chat_identifier: context.conversation.chat_identifier.clone(),
        guid: message.guid.clone(),
        timestamp_unix_ms,
        time_precision,
        outgoing: context.is_from_me,
        service: context.service,
        message_kind: kind.to_string(),
        sender_identity: context.sender_identity,
        sender_display_name: context.sender_display_name,
        subject: message.subject.clone().filter(|s| !s.is_empty()),
        text,
        reactions,
        deletion,
        edits,
        reply_to,
        owner_identity: owner_address(message).unwrap_or_default(),
        owner_display_name: owner_display_name(session, message),
        imessage: (!is_empty(&imessage)).then_some(imessage),
        attachments,
    };
    Ok((context.conversation, record))
}

/// Deleted in the source app, Unsent, or neither.
///
/// A row in a chat's recently deleted list (`chat_recoverable_message_join`)
/// was deleted in Messages, and keeps whatever text the database still
/// holds. A row is Unsent when every part of it was unsent, or when some
/// part was and nothing is left in any part: no text and no attachment. A
/// row whose every part was unsent is Unsent even when attachment rows are
/// still joined to it, because those files were unsent with their parts. A
/// row only partly unsent keeps what is left and is not marked. A row both
/// deleted and unsent is Unsent, which is why nothing of it is left.
fn deletion(message: &Message, has_attachments: bool) -> Option<Deletion> {
    let some_part_unsent = message.edited_parts.as_ref().is_some_and(|edited| {
        edited
            .parts
            .iter()
            .any(|part| matches!(part.status, EditStatus::Unsent))
    });
    // U+FFFC stands in for an attachment in the text, so it is not text.
    let text_left = message
        .text
        .as_deref()
        .is_some_and(|text| text.chars().any(|c| !c.is_whitespace() && c != '\u{FFFC}'));
    if message.is_fully_unsent() || (some_part_unsent && !text_left && !has_attachments) {
        Some(Deletion::Unsent)
    } else if message.is_deleted() {
        Some(Deletion::DeletedInSourceApp)
    } else {
        None
    }
}

/// Which of the message kinds a row is, the text that stands for it, and the
/// values that decided it (which the Apple-specific fields report too).
struct RowKind {
    kind: &'static str,
    text: String,
    /// The announcement's text, for announcement rows.
    announcement: Option<String>,
    /// The shared-location label, for location rows.
    shared_location: Option<String>,
    /// The send effect's label, already appended to `text`.
    send_effect: Option<String>,
    /// The app balloon's payload, for balloon rows.
    app: Option<Value>,
    tapback: Option<TapbackFields>,
}

/// A tapback row: which reaction, added or removed, on which message part.
struct TapbackFields {
    kind: &'static str,
    /// The emoji, for the `emoji` kind.
    emoji: Option<String>,
    action: &'static str,
    /// Guid of the message reacted to.
    associated_guid: Option<String>,
    /// Part index within that message.
    associated_part: Option<u32>,
}

impl TapbackFields {
    /// Read the reaction out of a row's [`Variant::Tapback`].
    fn from_variant(message: &Message, action: TapbackAction, kind: Tapback<'_>) -> Self {
        let (kind, emoji) = tapback_kind(kind);
        let action = match action {
            TapbackAction::Added => "add",
            TapbackAction::Removed => "remove",
        };
        let target = message.clean_associated_guid();
        Self {
            kind,
            emoji,
            action,
            associated_guid: target.map(|(_, guid)| guid.to_string()),
            associated_part: target.map(|(part, _)| part as u32),
        }
    }

    /// The message kind: stickers sent as reactions are their own kind.
    fn message_kind(&self) -> &'static str {
        if self.kind == "sticker" {
            "sticker_tapback"
        } else {
            "tapback"
        }
    }

    /// The human-readable line that stands in for the reaction's text.
    fn text(&self) -> String {
        tapback_human_line(self.kind, self.emoji.as_deref(), self.action)
    }
}

/// Decide a row's kind and text. The order matters: a tapback, SharePlay
/// end, or announcement wins over its (usually empty) text; a shared
/// location or app balloon over a plain text; and a plain text is iMessage,
/// MMS, or SMS by its service and whether it carries attachments.
fn classify_row(
    session: &MailSession,
    message: &Message,
    service: &str,
    has_attachments: bool,
    deletion: Option<Deletion>,
) -> RowKind {
    let shared_location = message
        .shared_location_kind()
        .map(shared_location_label)
        .map(str::to_string);
    let app = build_balloon_value(session.data_source.db(), message);
    let plain = || message.text.clone().unwrap_or_default();

    let (kind, text, announcement, tapback) =
        if let Variant::Tapback(_, action, kind) = message.variant() {
            let tapback = TapbackFields::from_variant(message, action, kind);
            (tapback.message_kind(), tapback.text(), None, Some(tapback))
        } else if message.is_shareplay() {
            let text = "SharePlay Message Ended".to_string();
            ("announcement", text.clone(), Some(text), None)
        } else if message.is_announcement() && deletion != Some(Deletion::Unsent) {
            // A message marked Unsent is the message itself (see
            // `deletion`), not a line saying someone unsent it. The one
            // decision makes both, so the mark and the kind never disagree.
            let text = announcement_text(session, message).unwrap_or_default();
            ("announcement", text.clone(), Some(text), None)
        } else if let Some(location) = shared_location.as_deref() {
            let text = message
                .text
                .clone()
                .unwrap_or_else(|| format!("Shared location {location}"));
            ("location_share", text, None, None)
        } else if let Some(app) = &app {
            (
                "balloon",
                balloon_summary(app, message.text.as_deref()),
                None,
                None,
            )
        } else if service.eq_ignore_ascii_case("imessage") {
            ("imessage", plain(), None, None)
        } else if has_attachments {
            ("mms", plain(), None, None)
        } else {
            ("sms", plain(), None, None)
        };

    let send_effect = expressive_label(message.get_expressive());
    RowKind {
        kind,
        text: with_send_effect(text, send_effect.as_deref()),
        announcement,
        shared_location,
        send_effect,
        app,
        tapback,
    }
}

/// The text with the send effect's label appended, unless it already names it.
fn with_send_effect(text: String, effect: Option<&str>) -> String {
    match effect {
        None => text,
        Some(effect) if text.is_empty() => effect.to_string(),
        Some(effect) if text.contains(effect) => text,
        Some(effect) => format!("{text}\n\n{effect}"),
    }
}

/// The message a row replies to: its thread originator and the part, for a
/// reply in a thread. A tapback is never a reply; the message it reacts to
/// is in its own fields.
fn reply_to(message: &Message, tapback: Option<&TapbackFields>) -> Option<ReplyTo> {
    if tapback.is_some() || !message.is_reply() {
        return None;
    }
    Some(ReplyTo {
        guid: trimmed(message.thread_originator_guid.clone()),
        part_index: message
            .thread_originator_part
            .as_deref()
            .and_then(parse_thread_part),
    })
}

/// Everything Apple-specific the core message fields do not carry. Blank
/// strings become `None` so the record stays small.
fn imessage_fields(
    session: &MailSession,
    message: &Message,
    row: RowKind,
    parts: &[crate::fields::PartRecord],
) -> ImessageRecord {
    let read_receipt = read_receipt_rfc3339(message, session.offset);
    let tapback = row.tapback.as_ref();
    ImessageRecord {
        send_effect: trimmed(row.send_effect),
        shared_location: trimmed(row.shared_location),
        announcement: trimmed(row.announcement),
        read_receipt_rfc3339: trimmed(read_receipt),
        parts: json_if_any(parts),
        balloon_kind: trimmed(row.app.as_ref().and_then(balloon_kind_label)),
        balloon_bundle_id: trimmed(message.balloon_bundle_id.clone()),
        associated_guid: trimmed(tapback.and_then(|t| t.associated_guid.clone())),
        associated_part: tapback.and_then(|t| t.associated_part),
        tapback_kind: tapback.map(|t| t.kind.to_string()),
        tapback_emoji: trimmed(tapback.and_then(|t| t.emoji.clone())),
        tapback_action: tapback.map(|t| t.action.to_string()),
        app: row.app,
    }
}

/// Whether every Apple-specific field is empty, so the record can be dropped.
fn is_empty(fields: &ImessageRecord) -> bool {
    fields.send_effect.is_none()
        && fields.shared_location.is_none()
        && fields.announcement.is_none()
        && fields.read_receipt_rfc3339.is_none()
        && fields.parts.is_none()
        && fields.app.is_none()
        && fields.balloon_bundle_id.is_none()
        && fields.balloon_kind.is_none()
        && fields.associated_guid.is_none()
        && fields.associated_part.is_none()
        && fields.tapback_kind.is_none()
        && fields.tapback_emoji.is_none()
        && fields.tapback_action.is_none()
}

/// The string trimmed, or `None` when blank.
fn trimmed(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The items as a JSON array, or `None` when there are none.
fn json_if_any<T: serde::Serialize>(items: &[T]) -> Option<Value> {
    if items.is_empty() {
        return None;
    }
    serde_json::to_value(items).ok()
}

#[cfg(test)]
mod tests;
