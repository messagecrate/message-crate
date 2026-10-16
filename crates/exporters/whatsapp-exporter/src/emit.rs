//! Convert wtsexporter JSON into the shared conversation structure, then write
//! the chosen output format through [`convert_backup`].

use crate::jid::{
    PHONE_PUNCTUATION, chat_id_from_jid, is_channel_jid, is_group_jid, is_status_jid, jid_to_e164,
    phone_id_to_e164,
};
use crate::read::{
    ChatJson, MemberJson, MessageJson, ReactionJson, group_action, is_reply, key_string,
    load_chat_store, media_path, message_text, timestamp_ms, timestamp_secs,
};
use anyhow::{Context, Result};
use ir::{
    Attachment, ConversationDocument, ExportMeta, IdentityType, MessageKind, Participant,
    PendingAttachment, PendingConversation, PendingMessage, PendingReply, ProjectionHooks,
    Reaction, SortKeyUnit, Transport, VendorFields,
};
use message_crate_core::{ConvertRun, Counter, ExportReport, project_conversation};
use message_crate_csv::format_local_ts;
use serde_json::Map;
use staging::{
    AttachmentPaths, AttachmentSource, Project, ReadBackup, WriterSetup, convert_backup,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = message_crate_core::Exporter::Whatsapp.source_id();
const EXPORT_TOOL: &str = "WhatsApp Chat Exporter";
/// Pinned documented upstream version (JSON convert path; shell-out may differ).
pub(crate) const EXPORT_TOOL_VERSION: &str = "0.13.0";

/// The extra flag `ingest_chat` sets on a group event the fork gave a text,
/// which [`WhatsappProjection`] writes as an announcement.
const IS_ANNOUNCEMENT: &str = "is_announcement";
/// The extra, and the `source.fields` key, holding the fork's
/// `group_action_jid`: the id of the member a group event's text names.
const GROUP_ACTION_JID: &str = "group_action_jid";

/// Inputs for [`convert_export`]: the wtsexporter `result.json` to read,
/// where to write it, and how.
pub(crate) struct ConvertExportArgs<'a> {
    pub json_path: &'a Path,
    pub output: &'a Path,
    /// Directories tried when resolving relative media paths. For a ready-made
    /// `result.json`: the backup input, when given, and the JSON's directory.
    /// Otherwise: the wtsexporter working directory and the backup input, which
    /// is the process cwd when the config names no input.
    pub media_search_roots: &'a [PathBuf],
    /// The account holder's number in E.164, stamped on the export header and
    /// so on every message as the address it was held at. `None` records no
    /// owner, which leaves the conversations counted toward no identity.
    pub owner_identity: Option<String>,
    /// When the backup was made, in Unix milliseconds: every message's
    /// Backup Date ([`backup_taken_at_unix_ms`](crate::run::backup_taken_at_unix_ms)).
    pub backup_taken_at_unix_ms: Option<i64>,
    pub convert_run: ConvertRun,
}

/// Convert a wtsexporter `result.json` into the shared conversation structure,
/// then write the chosen output format.
///
/// Cancellation is checked between chats as they are read and between
/// conversations.
///
/// # Errors
///
/// Returns an error when the JSON cannot be read, a conversation cannot be
/// written, or the user cancels.
pub(crate) fn convert_export(args: ConvertExportArgs<'_>) -> Result<ExportReport> {
    let ConvertExportArgs {
        json_path,
        output,
        media_search_roots,
        owner_identity,
        backup_taken_at_unix_ms,
        convert_run,
    } = args;
    fs::create_dir_all(output).with_context(|| format!("create {}", output.display()))?;
    // Load the chat store BEFORE the writer cleans the output directory. The
    // JSON may live inside the output dir (e.g. wtsexporter_result.json) and
    // cleaning deletes all *.json files.
    let store = load_chat_store(json_path)?;
    convert_backup(
        output,
        convert_run,
        WriterSetup::default(),
        |writer, cancel, mut report| {
            let copy_attachments = writer.files().copies();
            let mut conversations: BTreeMap<String, PendingConversation> = BTreeMap::new();
            let mut rosters: BTreeMap<String, Vec<Participant>> = BTreeMap::new();

            for (jid, chat) in store {
                message_crate_core::check_cancel(cancel)?;
                if jid.starts_with('_') {
                    // Reserved / system keys if any.
                    continue;
                }
                if let Some(counter) = skipped_chat_counter(&jid) {
                    report.bump(counter, chat.messages.len() as u64);
                    continue;
                }
                if let Some((chat_id, conversation, roster)) = ingest_chat(
                    &jid,
                    &chat,
                    owner_identity.as_deref(),
                    copy_attachments,
                    media_search_roots,
                    &mut report,
                ) {
                    rosters.insert(chat_id.clone(), roster);
                    conversations.insert(chat_id, conversation);
                }
            }

            Ok(ReadBackup {
                conversations: conversations.into_iter().collect(),
                project: WhatsappDocuments {
                    hooks: WhatsappProjection {
                        rosters,
                        export: message_crate_core::export_meta(
                            EXPORT_SOURCE,
                            EXPORT_TOOL,
                            EXPORT_TOOL_VERSION,
                            owner_identity,
                            None,
                        ),
                        backup_taken_at_unix_ms,
                    },
                    media_sources: AttachmentPaths::default(),
                },
                report,
            })
        },
    )
}

/// The projection of each conversation read, for [`convert_backup`], and
/// the media file of each attachment it keeps.
struct WhatsappDocuments {
    hooks: WhatsappProjection,
    /// The media file of each attachment of the documents projected so
    /// far, in the order the writer asks for them.
    media_sources: AttachmentPaths,
}

impl Project for WhatsappDocuments {
    type Conversation = (String, PendingConversation);

    fn project(
        &mut self,
        (chat_id, mut conversation): (String, PendingConversation),
        report: &mut ExportReport,
    ) -> Option<ConversationDocument> {
        let document = project_conversation(&chat_id, &mut conversation, &self.hooks, report)?;
        collect_media_sources(&conversation, &mut self.media_sources);
        Some(document)
    }

    fn attachment_source(
        &mut self,
        attachment: &mut Attachment,
    ) -> (AttachmentSource, Option<u64>) {
        self.media_sources.next_source(attachment)
    }
}

/// The run-summary counter for a chat that is not a conversation, or `None`
/// for one that is.
///
/// Status updates (`status@broadcast`) are posts to many people that expire
/// after 24 hours, and a Channel (`@newsletter`) is a one-way feed from a
/// publisher. Neither has anyone in it to make a contact for, so neither is
/// written, and each of its messages is counted so none is dropped silently.
/// A reply to someone's Status is a message in the one-to-one conversation with that
/// person and is written as usual.
fn skipped_chat_counter(jid: &str) -> Option<Counter> {
    if is_status_jid(jid) {
        Some(SKIPPED_STATUS_UPDATES)
    } else if is_channel_jid(jid) {
        Some(SKIPPED_CHANNEL_POSTS)
    } else {
        None
    }
}

/// Status updates skipped, counted by the messages in the status chat.
pub(crate) const SKIPPED_STATUS_UPDATES: Counter = Counter::new(
    "skipped_status_updates",
    "Skipped 1 status update",
    "Skipped {n} status updates",
);

/// Channel posts skipped.
pub(crate) const SKIPPED_CHANNEL_POSTS: Counter = Counter::new(
    "skipped_channel_posts",
    "Skipped 1 channel post",
    "Skipped {n} channel posts",
);

/// Ingest one WhatsApp chat JSON into a pending conversation (messages +
/// media), with the conversation's participants.
fn ingest_chat(
    jid: &str,
    chat: &ChatJson,
    owner: Option<&str>,
    copy_attachments: bool,
    media_search_roots: &[PathBuf],
    report: &mut ExportReport,
) -> Option<(String, PendingConversation, Vec<Participant>)> {
    let group = is_group_jid(jid);
    let chat_id = chat_id_from_jid(jid);
    let chat_name = chat.name.as_deref().and_then(ir::nonempty);
    let group_title = if group { chat_name.clone() } else { None };

    let mut roster = Roster::new(owner);
    if group {
        // `null` means the backup's member table was not read: the group's
        // senders below are then all that is known of its people.
        for member in chat.members.get().into_iter().flatten() {
            roster.add(Person::member(member));
        }
    } else {
        roster.add_peer(jid, &chat_id, chat_name.clone());
    }

    let mut pending = PendingConversation::new(chat_id.clone(), group, group_title, Vec::new());
    pending.extra.insert("whatsapp_jid".into(), jid.to_string());

    for message in chat.messages.values() {
        let Some(ts_raw) = message.timestamp else {
            report.skipped_invalid_date += 1;
            continue;
        };
        let secs = timestamp_secs(ts_raw);
        if format_local_ts(secs).is_none() {
            report.skipped_invalid_date += 1;
            continue;
        }

        let is_from_me = message.from_me;
        let (sender_identity, sender_display_name) = if is_from_me {
            (String::new(), None)
        } else if group {
            let sender = Person::sender(message);
            let identity = sender
                .identity
                .as_ref()
                .map(|(identity, _)| identity.clone())
                .unwrap_or_default();
            let name = sender.name.clone();
            roster.add(sender);
            (identity, name)
        } else {
            (chat_id.clone(), chat_name.clone())
        };

        // A group event the fork gives a text is an announcement with that
        // text; its `data` is the backup's raw row (#2038).
        let announcement = group_action(message);
        let text = announcement.map_or_else(|| message_text(message), str::to_string);
        let (attachments, media_source) = match media_path(message) {
            Some(src) => queue_media(
                src,
                chat.media_base.as_deref(),
                media_search_roots,
                copy_attachments,
                message,
                report,
            ),
            None => (Vec::new(), None),
        };

        pending.messages.push(PendingMessage {
            sort_key: timestamp_ms(ts_raw),
            is_from_me,
            sender_identity,
            sender_display_name,
            text,
            attachments,
            extra: {
                let mut e = BTreeMap::new();
                e.insert("key_id".into(), key_id_string(message));
                e.insert(
                    "full_key_id".into(),
                    key_string(message.full_key_id.as_ref()).unwrap_or_default(),
                );
                if announcement.is_some() {
                    e.insert(IS_ANNOUNCEMENT.into(), "true".into());
                }
                if let Some(named_member_jid) =
                    message.group_action_jid.as_deref().and_then(ir::trimmed)
                {
                    e.insert(GROUP_ACTION_JID.into(), named_member_jid.to_string());
                }
                if is_reply(message) {
                    e.insert("is_reply".into(), "true".into());
                }
                if let Some(quoted) = key_string(message.reply_key_id.as_ref()) {
                    e.insert("reply_key_id".into(), quoted);
                }
                if let Some(reactions) = reactions_json(message) {
                    e.insert("reactions".into(), reactions);
                }
                e.insert(
                    "is_sticker".into(),
                    if message.sticker { "true" } else { "false" }.into(),
                );
                if let Some(path) = media_source {
                    e.insert("media_source".into(), path.to_string_lossy().into_owned());
                }
                e
            },
        });
    }

    if pending.messages.is_empty() {
        return None;
    }

    // The phone numbers name an untitled group's file.
    pending.participant_e164s = roster.numbers;
    Some((chat_id, pending, roster.participants))
}

/// One person of a group as the fork's JSON gives them: a member entry or a
/// message's sender.
struct Person {
    /// Their WhatsApp id: the phone id, or an `@lid` id the backup could not
    /// map to one. `None` when the backup names no sender.
    jid: Option<String>,
    /// What they are written as: the phone number of a phone id, or else
    /// the raw id, which the server types `other` because WhatsApp carries
    /// no email address. The one-to-one conversation with the same person writes the
    /// same id ([`Roster::add_peer`]), so both are one identity.
    identity: Option<(String, IdentityType)>,
    name: Option<String>,
}

impl Person {
    fn new(jid: Option<&str>, name: Option<String>) -> Self {
        let jid = jid.and_then(ir::nonempty);
        let identity = jid.as_deref().map(|jid| match phone_id_to_e164(jid) {
            Some(number) => (number, IdentityType::Phone),
            None => (jid.to_string(), IdentityType::Other),
        });
        Self {
            jid,
            identity,
            name,
        }
    }

    /// A member entry, named by the owner's address book first and the
    /// member's own profile second.
    fn member(member: &MemberJson) -> Self {
        Self::new(
            member.jid.as_deref(),
            first_name([member.contact_name.as_deref(), member.push_name.as_deref()]),
        )
    }

    /// A received group message's sender. The number comes only from
    /// `sender_jid`, and only when it is a phone id: `sender` is never read
    /// as a number, because its digits may be an `@lid` id's. The name is
    /// the first of `sender_contact_name`, `sender` when it is a name, and
    /// `sender_push_name`.
    fn sender(message: &MessageJson) -> Self {
        Self::new(
            message.sender_jid.get().map(String::as_str),
            first_name([
                message.sender_contact_name.get().map(String::as_str),
                message.sender.as_deref().filter(|sender| is_a_name(sender)),
                message.sender_push_name.get().map(String::as_str),
            ]),
        )
    }
}

/// The first of `names` that is not blank, trimmed.
fn first_name<const N: usize>(names: [Option<&str>; N]) -> Option<String> {
    names.into_iter().flatten().find_map(ir::nonempty)
}

/// True when wtsexporter's `sender` holds a name rather than an id or the
/// digits of one.
fn is_a_name(sender: &str) -> bool {
    let sender = sender.trim();
    !sender.is_empty()
        && !sender.contains('@')
        && !sender
            .chars()
            .all(|c| c.is_ascii_digit() || PHONE_PUNCTUATION.contains(&c))
}

/// The participants of one conversation, in the order they are met: a
/// group's members first, then each sender who is not one of them. The
/// owner of the phone is never a participant of their own conversation, so
/// a member or sender with the owner's number is left out.
struct Roster<'a> {
    owner: Option<&'a str>,
    participants: Vec<Participant>,
    /// The phone numbers among `participants`, in the same order.
    numbers: Vec<String>,
    /// Index into `participants` of each person already added.
    seen: BTreeMap<PersonKey, usize>,
}

/// What tells two people of one conversation apart: their WhatsApp id, or
/// their name when the JSON gives no id for them.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum PersonKey {
    Jid(String),
    Name(String),
}

impl<'a> Roster<'a> {
    fn new(owner: Option<&'a str>) -> Self {
        Self {
            owner,
            participants: Vec::new(),
            numbers: Vec::new(),
            seen: BTreeMap::new(),
        }
    }

    /// Add `person`, or give the participant already there their name when
    /// it has none. A person with neither an id nor a name has nothing to
    /// write.
    fn add(&mut self, person: Person) {
        let Person {
            jid,
            identity,
            name,
        } = person;
        if let Some((number, IdentityType::Phone)) = &identity
            && self.is_owner(number)
        {
            return;
        }
        let key = match (jid, &name) {
            (Some(jid), _) => PersonKey::Jid(jid),
            (None, Some(name)) => PersonKey::Name(name.clone()),
            (None, None) => return,
        };
        if let Some(&at) = self.seen.get(&key) {
            let known = &mut self.participants[at];
            if known.display_name.is_none() {
                known.display_name = name;
            }
            return;
        }
        self.seen.insert(key, self.participants.len());
        let (identity, identity_type) = identity.unzip();
        if identity_type == Some(IdentityType::Phone)
            && let Some(number) = &identity
        {
            self.numbers.push(number.clone());
        }
        self.participants.push(Participant {
            identity,
            display_name: name,
            country: None,
        });
    }

    /// A one-to-one conversation's one participant, named by its WhatsApp chat. A chat whose
    /// id is not a phone id, such as an internal `@lid` id, has its raw id,
    /// which the server types `other` because WhatsApp carries no email
    /// address (#1671).
    fn add_peer(&mut self, jid: &str, chat_id: &str, name: Option<String>) {
        if jid_to_e164(jid).is_some() {
            self.numbers.push(chat_id.to_string());
        }
        self.participants.push(Participant {
            identity: Some(chat_id.to_string()),
            display_name: name,
            country: None,
        });
    }

    /// True when `number` is the owner's, compared by digits, so a number
    /// typed on the form with spaces still matches.
    fn is_owner(&self, number: &str) -> bool {
        let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
        self.owner
            .is_some_and(|owner| digits(owner) == digits(number))
    }
}

/// Resolve a media path during parse. Do not copy; the runner writes later.
///
/// A file that is not found keeps its attachment with no source, and the
/// writer marks it `file_missing`, so a message that is only a photo still
/// says a photo was there.
fn queue_media(
    src: &str,
    media_base: Option<&str>,
    media_search_roots: &[PathBuf],
    copy_attachments: bool,
    message: &MessageJson,
    report: &mut ExportReport,
) -> (Vec<PendingAttachment>, Option<PathBuf>) {
    let name = Path::new(src)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    let pending = PendingAttachment {
        rel_path: String::new(),
        content_type: message.mime.clone().unwrap_or_default(),
        digest_sha256: None,
        name_hint: name,
        size_bytes: None,
    };
    if !copy_attachments {
        return (vec![pending], None);
    }
    let src_path = resolve_media_file(src, media_base, media_search_roots);
    if src_path.is_none() {
        report.caveat(
            message_crate_core::ATTACHMENTS_MISSING,
            1,
            src,
            "This attachment's file is not in the backup, so its message is kept without it.",
        );
    }
    (vec![pending], src_path)
}

/// Collect source paths in the same order attachments will appear on documents.
fn collect_media_sources(conversation: &PendingConversation, out: &mut AttachmentPaths) {
    for message in &conversation.messages {
        if message.attachments.is_empty() {
            continue;
        }
        let source = message.extra_str("media_source").to_string();
        for _ in &message.attachments {
            out.push((!source.is_empty()).then(|| PathBuf::from(&source)));
        }
    }
}

/// Resolve a wtsexporter media path against `media_base` and search roots.
///
/// Only paths that resolve inside an allowed root (search roots, or an
/// absolute `media_base`) are accepted. Absolute hints and `..` segments
/// cannot escape those roots.
fn resolve_media_file(
    src: &str,
    media_base: Option<&str>,
    media_search_roots: &[PathBuf],
) -> Option<PathBuf> {
    let allowed = allowed_media_roots(media_base, media_search_roots);
    if allowed.is_empty() {
        return None;
    }

    let hint = Path::new(src);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if hint.is_absolute() {
        candidates.push(hint.to_path_buf());
    } else if let Some(base) = media_base.and_then(ir::trimmed) {
        let base_path = Path::new(base);
        if base_path.is_absolute() {
            candidates.push(base_path.join(hint));
        }
        for root in media_search_roots {
            candidates.push(root.join(base_path).join(hint));
        }
        for root in media_search_roots {
            candidates.push(root.join(hint));
        }
    } else {
        for root in media_search_roots {
            candidates.push(root.join(hint));
        }
    }

    candidates
        .into_iter()
        .find(|p| p.is_file() && path_within_any(p, &allowed))
}

/// Allowed roots for media path checks (search roots plus an absolute `media_base`).
fn allowed_media_roots(media_base: Option<&str>, media_search_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = media_search_roots.to_vec();
    if let Some(base) = media_base.and_then(ir::trimmed) {
        let base_path = Path::new(base);
        if base_path.is_absolute() {
            roots.push(base_path.to_path_buf());
        }
    }
    roots
}

/// True when `path` resolves to a location under any of `roots`.
fn path_within_any(path: &Path, roots: &[PathBuf]) -> bool {
    let Ok(canon) = fs::canonicalize(path) else {
        return false;
    };
    roots.iter().any(|root| {
        fs::canonicalize(root)
            .ok()
            .is_some_and(|root_canon| canon.starts_with(root_canon))
    })
}

/// WhatsApp `key_id` as a string (empty when missing).
fn key_id_string(message: &MessageJson) -> String {
    match &message.key_id {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

/// The message's reactions as the conversation model carries them, serialized
/// for the `reactions` extra the projection reads back; `None` when the
/// message has none, or when they are not known.
fn reactions_json(message: &MessageJson) -> Option<String> {
    let reactions: Vec<Reaction> = message
        .reaction_details
        .as_deref()?
        .iter()
        .filter_map(reaction_from_json)
        .collect();
    (!reactions.is_empty()).then(|| serde_json::to_string(&reactions).expect("serialize reactions"))
}

/// One reaction under the reactor's WhatsApp id (#1646), the id the same
/// person is written under as a sender or a member ([`Person::new`]): the
/// phone number when the fork's `jid` is a phone id, else the raw id, an
/// `@lid` id the backup maps to no phone. The owner's own reactions carry
/// `is_from_me` and no id, as the fork writes none for them. No display
/// name: the fork's `reactions` map names a reactor by display name, and a
/// name is not an identity. The fork leaves a withdrawn reaction out, so an
/// entry with no emoji is malformed and is dropped.
fn reaction_from_json(reaction: &ReactionJson) -> Option<Reaction> {
    let emoji = reaction.emoji.as_deref().and_then(ir::trimmed)?;
    let reactor_identity = if reaction.from_me {
        None
    } else {
        Person::new(reaction.jid.as_deref(), None)
            .identity
            .map(|(identity, _)| identity)
    };
    Some(Reaction {
        part_index: 0,
        kind: "emoji".into(),
        emoji: Some(emoji.to_string()),
        is_from_me: reaction.from_me,
        reactor_identity,
        reactor_display_name: None,
    })
}

/// WhatsApp deltas of the shared [`ir::pending_to_document`] projection.
struct WhatsappProjection {
    export: ExportMeta,
    /// Every message's Backup Date.
    backup_taken_at_unix_ms: Option<i64>,
    /// Each conversation's participants, by chat id.
    rosters: BTreeMap<String, Vec<Participant>>,
}

impl ProjectionHooks for WhatsappProjection {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn backup_taken_at_unix_ms(&self) -> Option<i64> {
        self.backup_taken_at_unix_ms
    }

    fn service(&self, _message: &PendingMessage) -> Transport {
        Transport::Whatsapp
    }

    fn sort_key_unit(&self) -> SortKeyUnit {
        SortKeyUnit::Milliseconds
    }

    /// [`MessageKind::Announcement`] for a group event with the fork's
    /// text, which the server marks as an announcement; otherwise the
    /// default.
    fn message_kind(&self, message: &PendingMessage) -> MessageKind {
        if message.extra_flag(IS_ANNOUNCEMENT) {
            MessageKind::Announcement
        } else {
            ir::default_message_kind(message)
        }
    }

    /// Same-millisecond rows fall back to `key_id` so output order is stable.
    fn message_order(&self, a: &PendingMessage, b: &PendingMessage) -> std::cmp::Ordering {
        a.sort_key
            .cmp(&b.sort_key)
            .then_with(|| a.extra_str("key_id").cmp(b.extra_str("key_id")))
    }

    /// None: whether a media file was found must leave the GUID unchanged,
    /// and `key_id` already tells WhatsApp messages apart.
    fn attachment_digests(&self, _message: &PendingMessage) -> Vec<String> {
        Vec::new()
    }

    /// `key_id`: it tells WhatsApp messages apart, including same-millisecond
    /// messages with identical text.
    fn vendor_key(&self, message: &PendingMessage) -> Option<String> {
        ir::trimmed(message.extra_str("key_id")).map(str::to_string)
    }

    /// `full_key_id`, the whole id a reply names in its `reply_key_id`.
    /// `key_id` is not used: on an iPhone it is a 17-character prefix that
    /// two messages can share.
    fn reply_key(&self, message: &PendingMessage) -> Option<String> {
        ir::trimmed(message.extra_str("full_key_id")).map(str::to_string)
    }

    /// A message that quotes another, linked by `reply_key_id` to the message
    /// of the same chat whose `full_key_id` it equals. A reply from a JSON
    /// with no `reply_key_id` is still a reply, with no link.
    fn reply(&self, message: &PendingMessage) -> Option<PendingReply> {
        (message.extra_str("is_reply") == "true").then(|| PendingReply {
            quoted_key: ir::trimmed(message.extra_str("reply_key_id")).map(str::to_string),
        })
    }

    /// The reactions `ingest_chat` read from the fork's `reaction_details`,
    /// each under the reactor's WhatsApp id ([`reaction_from_json`]).
    fn reactions(&self, message: &PendingMessage) -> Vec<Reaction> {
        match message.extra.get("reactions") {
            Some(json) => serde_json::from_str(json).expect("reactions written by ingest_chat"),
            None => Vec::new(),
        }
    }

    fn attachment_to_ir(
        &self,
        attachment: &PendingAttachment,
        message: &PendingMessage,
    ) -> Attachment {
        Attachment {
            // No path yet: the writer sets it when it stages the file.
            path: None,
            original_name: attachment.name_hint.clone(),
            mime_type: attachment.mime_type(),
            digest_sha256: attachment.digest_sha256.clone(),
            is_sticker: message.extra_flag("is_sticker"),
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        }
    }

    /// The participants [`ingest_chat`] gathered for the chat ([`Roster`]).
    fn participants(&self, chat_id: &str, _conversation: &PendingConversation) -> Vec<Participant> {
        self.rosters.get(chat_id).cloned().unwrap_or_default()
    }

    fn group_title(&self, conversation: &PendingConversation) -> Option<String> {
        conversation.display_name.clone()
    }

    fn packaging_stem_suffix(&self, _conversation: &PendingConversation) -> Option<String> {
        Some("__whatsapp".into())
    }

    /// The chat's `jid`, the message's `key_id` and, on a group event, the
    /// `group_action_jid` of the member its text names: raw values the
    /// conversation model has no place for. A reply is not copied here,
    /// because `reply_to` records it on the message. A reply whose quoted
    /// message is not in the export keeps no quoted id: `reply_to` names
    /// only a message of the same export, and neither the raw `reply` value
    /// nor `reply_key_id` is stored beside it. Nor is a reaction copied
    /// here: `reactions` records each on the message, and the fork's
    /// name-to-emoji `reactions` map is not read at all.
    fn vendor_fields(
        &self,
        conversation: &PendingConversation,
        message: &PendingMessage,
    ) -> VendorFields {
        let mut fields = Map::new();
        let whatsapp_jid = conversation.extra_str("whatsapp_jid");
        if !whatsapp_jid.is_empty() {
            fields.insert(
                "jid".into(),
                serde_json::Value::String(whatsapp_jid.to_string()),
            );
        }
        let key_id = message.extra_str("key_id");
        if !key_id.is_empty() {
            fields.insert(
                "key_id".into(),
                serde_json::Value::String(key_id.to_string()),
            );
        }
        let named_member_jid = message.extra_str(GROUP_ACTION_JID);
        if !named_member_jid.is_empty() {
            fields.insert(
                GROUP_ACTION_JID.into(),
                serde_json::Value::String(named_member_jid.to_string()),
            );
        }
        VendorFields {
            android_type: None,
            fields,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_media_rejects_absolute_paths_outside_roots() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.bin");
        fs::write(&secret, b"secret").unwrap();
        assert!(
            resolve_media_file(secret.to_str().unwrap(), None, &[root.path().to_path_buf()],)
                .is_none()
        );
    }

    #[test]
    fn resolve_media_rejects_file_only_under_cwd_like_path() {
        // Media roots passed to convert must be explicit (input / JSON parent /
        // work directory). A path that only exists under a separate "CWD-like" tree
        // must not resolve when that tree is omitted from the allowlist.
        let allowed = tempfile::tempdir().unwrap();
        let cwd_like = tempfile::tempdir().unwrap();
        let secret = cwd_like.path().join("media.jpg");
        fs::write(&secret, b"jpeg").unwrap();
        let roots = [allowed.path().to_path_buf()];
        assert!(!path_within_any(&secret, &roots));
        assert!(
            resolve_media_file(secret.to_str().unwrap(), None, &roots).is_none(),
            "file under a non-allowed tree must be rejected"
        );
    }

    #[test]
    fn resolve_media_rejects_dotdot_escape() {
        let root = tempfile::tempdir().unwrap();
        let sibling = root.path().parent().unwrap().join("escape_probe.bin");
        fs::write(&sibling, b"x").unwrap();
        let hint = "../escape_probe.bin";
        assert!(
            resolve_media_file(hint, None, &[root.path().to_path_buf()]).is_none(),
            "relative .. must not escape search roots"
        );
        let _ = fs::remove_file(&sibling);
    }

    #[test]
    fn resolve_media_accepts_path_under_search_root() {
        let root = tempfile::tempdir().unwrap();
        let media_base = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";
        let rel = "Message/Media/chat/a/b/photo.jpg";
        let src = root.path().join(media_base).join(rel);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, b"jpeg").unwrap();
        let found = resolve_media_file(rel, Some(media_base), &[root.path().to_path_buf()]);
        assert_eq!(found.as_deref(), Some(src.as_path()));
    }

    /// Ingest leaves every attachment's size empty, so the writer's byte
    /// totals and headroom check see a media file only through its size on
    /// disk (#3057).
    #[test]
    fn a_media_file_with_no_size_reports_its_size_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("photo.jpg");
        fs::write(&media, vec![0u8; 1234]).unwrap();
        let mut documents = WhatsappDocuments {
            hooks: WhatsappProjection {
                export: message_crate_core::export_meta(
                    EXPORT_SOURCE,
                    EXPORT_TOOL,
                    EXPORT_TOOL_VERSION,
                    None,
                    None,
                ),
                backup_taken_at_unix_ms: None,
                rosters: BTreeMap::new(),
            },
            media_sources: AttachmentPaths::default(),
        };
        documents.media_sources.push(Some(media.clone()));
        let mut attachment = Attachment {
            path: None,
            original_name: Some("photo.jpg".to_string()),
            mime_type: Some("image/jpeg".to_string()),
            digest_sha256: None,
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        };
        let (source, size) = documents.attachment_source(&mut attachment);
        assert!(matches!(source, AttachmentSource::Path(path) if path == media));
        assert_eq!(size, Some(1234));
    }
}
