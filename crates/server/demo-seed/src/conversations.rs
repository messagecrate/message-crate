//! Write conversation files for the demo dataset.
//!
//! Each file is JSON Lines: a header object, then one JSON object per message.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use message_ir::{
    ConversationHeader, ConversationMeta, ConversationStats, Deletion, EarlierVersion, ExportMeta,
    IrAttachment, IrConversationType, IrDirection, IrImessage, IrMessage, IrMessageKind,
    IrParticipant, IrService, Reaction, ReplyTo, SCHEMA_VERSION, TimePrecision, orphaned_chat_id,
};
use rand::Rng;
use rand::RngExt;
use rand::seq::{IndexedRandom, SliceRandom};

use crate::assets::{JPG_PHOTOS, OTHER_ATTACHMENTS};
use crate::config::SeedConfig;
use crate::corpus::Corpus;
use crate::personas::{
    Contact, EMPTY_GROUP_HANDLE, EMPTY_GROUP_MEMBERS, EMPTY_ONE_TO_ONE_HANDLE, ORPHAN_SENDER,
    OWNER_EMAIL, OWNER_PHONE, Roster, Unassigned,
};
use crate::phones;
use crate::stop_if_cancelled;

const IMESSAGE_SOURCE: &str = "imessage";
const SBR_SOURCE: &str = "sms-backup-restore";
const WHATSAPP_SOURCE: &str = "whatsapp";

#[derive(Clone, Copy)]
enum SourceFlavor {
    IMessage,
    SmsBackupRestore,
    Whatsapp,
}

/// Counts of contacts, conversations, messages, and attachments written this run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GenStats {
    /// Contacts invented.
    pub contacts: usize,
    /// Conversation files written across every backup directory.
    pub conversation_files: usize,
    /// Messages written.
    pub messages: usize,
    /// Attachment references written into messages.
    pub attachment_refs: usize,
    /// Group conversations written.
    pub groups: usize,
    /// Messages written to both the iMessage and the Android backup, one copy
    /// each. Every one is counted twice in `messages`, and a database that
    /// imports the bundle hides exactly this many as duplicates.
    pub shared_messages: usize,
}

const TAPBACK_KINDS: &[&str] = &[
    "loved",
    "liked",
    "disliked",
    "laughed",
    "emphasized",
    "questioned",
    "emoji",
];

const PHOTO_CAPTIONS: &[&str] = &[
    "Check this out",
    "Thought you'd like this",
    "From yesterday",
    "Saw this and thought of you",
    "",
];

const EMOJI_ONLY: &[&str] = &["👍", "😂", "❤️", "🎉", "😊"];

/// Export metadata stamped on a conversation header. `owner_identity` is the
/// Demo Account's identity the conversation's messages are held at, and
/// `backup_taken_at_unix_ms` when the backup was made: the settings'
/// reference time, which every generated message is before.
fn export_meta(source: &str, owner_identity: &str, backup_taken_at_unix_ms: i64) -> ExportMeta {
    ExportMeta {
        source: source.into(),
        tool: "demo-seed".into(),
        tool_version: "0.2.0".into(),
        owner_identity: Some(owner_identity.into()),
        owner_display_name: Some("Me".into()),
        backup_taken_at_unix_ms: Some(backup_taken_at_unix_ms),
    }
}

/// The three backup directories a run writes into.
pub struct StagingDirs<'a> {
    /// The directory that looks like an iMessage backup.
    pub imessage: &'a Path,
    /// The directory that looks like an SMS Backup & Restore backup.
    pub sbr: &'a Path,
    /// The directory that looks like a WhatsApp backup.
    pub whatsapp: &'a Path,
}

/// Write every conversation file into the three backup directories and return counts.
///
/// One-to-one contacts are split into iMessage-only, Android-only, and overlap.
/// Unassigned handles, groups, orphaned messages, empty conversations, and WhatsApp
/// copies are written after that, in that order.
///
/// # Errors
///
/// Returns [`crate::Cancelled`] when `cancel` is set, checked before each
/// conversation, or an error if a conversation file cannot be created or
/// written.
pub fn write_all<R: Rng>(
    staging: &StagingDirs<'_>,
    roster: &Roster,
    cfg: &SeedConfig,
    corpus: &Corpus,
    rng: &mut R,
    attachment_digests: &HashMap<String, (String, u64)>,
    cancel: &AtomicBool,
) -> Result<GenStats> {
    clear_jsonl(staging.imessage)?;
    clear_jsonl(staging.sbr)?;
    clear_jsonl(staging.whatsapp)?;
    let mut seeder = Seeder {
        cfg,
        corpus,
        rng,
        attachment_digests,
        cancel,
        stats: GenStats {
            contacts: roster.contacts.len(),
            groups: roster.groups.len(),
            ..Default::default()
        },
    };
    seeder.write_all(staging, roster)?;
    Ok(seeder.stats)
}

/// Everything the writers share: the config, the corpus, the random source,
/// the attachment fingerprints, the cancel flag, and the running counts. One
/// value per run; every writer is a method on it.
struct Seeder<'a, R: Rng> {
    cfg: &'a SeedConfig,
    corpus: &'a Corpus,
    rng: &'a mut R,
    attachment_digests: &'a HashMap<String, (String, u64)>,
    cancel: &'a AtomicBool,
    stats: GenStats,
}

impl<R: Rng> Seeder<'_, R> {
    /// Write the whole dataset in the order the doc on [`write_all`] gives.
    fn write_all(&mut self, staging: &StagingDirs<'_>, roster: &Roster) -> Result<()> {
        let mut one_to_one = contacts_with_one_to_one(roster);
        one_to_one.shuffle(&mut *self.rng);

        let overlap_count = self.cfg.sources.overlap_count.min(one_to_one.len());
        let (overlap, rest) = one_to_one.split_at(overlap_count);
        let android_only_count =
            crate::rounded_fraction(rest.len(), self.cfg.sources.android_only_fraction);
        let (android_only, imessage_only) = rest.split_at(android_only_count);

        for contact in imessage_only {
            stop_if_cancelled(self.cancel)?;
            let spec = Individual::for_contact(contact, SourceFlavor::IMessage);
            self.individual(staging.imessage, &spec)?;
        }
        for contact in android_only {
            stop_if_cancelled(self.cancel)?;
            let spec = Individual::for_contact(contact, SourceFlavor::SmsBackupRestore);
            self.individual(staging.sbr, &spec)?;
        }
        for contact in overlap {
            stop_if_cancelled(self.cancel)?;
            self.overlap_individual(staging, contact)?;
        }
        for ua in &roster.unassigned {
            stop_if_cancelled(self.cancel)?;
            let count = self.rng.random_range(4..16);
            self.unassigned(staging.imessage, ua, count)?;
        }
        for group in &roster.groups {
            stop_if_cancelled(self.cancel)?;
            self.group(staging.imessage, roster, group)?;
        }
        stop_if_cancelled(self.cancel)?;
        self.orphaned(staging.imessage)?;

        if self.cfg.edge_cases.empty_individual {
            self.create_empty_conversation_file(
                staging.imessage,
                EMPTY_ONE_TO_ONE_HANDLE,
                IrConversationType::Individual,
                &[],
            )?;
        }
        if self.cfg.edge_cases.empty_group {
            self.create_empty_conversation_file(
                staging.imessage,
                EMPTY_GROUP_HANDLE,
                IrConversationType::Group,
                &EMPTY_GROUP_MEMBERS,
            )?;
        }

        // WhatsApp conversations reuse the contact's phone number. Import treats them as
        // a separate platform on the same person.
        for contact in roster.contacts.iter().filter(|c| c.has_whatsapp) {
            stop_if_cancelled(self.cancel)?;
            let spec = Individual::for_contact(contact, SourceFlavor::Whatsapp);
            self.individual(staging.whatsapp, &spec)?;
        }
        Ok(())
    }
}

/// Contacts that have a phone number and a one-to-one conversation.
fn contacts_with_one_to_one(roster: &Roster) -> Vec<&Contact> {
    let mut contacts = Vec::new();
    for contact in &roster.contacts {
        if contact.phones.is_empty() {
            continue;
        }
        if !contact.has_one_to_one() {
            continue;
        }
        contacts.push(contact);
    }
    contacts
}

/// Delete existing `.jsonl` and `.json` files in `staging` so a regenerate does not leave leftovers.
///
/// # Errors
///
/// Returns an error if the directory cannot be listed or a file cannot be deleted.
fn clear_jsonl(staging: &Path) -> Result<()> {
    if !staging.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(staging)? {
        let entry = entry?;
        let path = entry.path();
        if is_json_conversation_file(&path) {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// True when `path` is a conversation file (`.jsonl` or `.json`).
fn is_json_conversation_file(path: &Path) -> bool {
    match path.extension() {
        Some(extension) => extension == "jsonl" || extension == "json",
        None => false,
    }
}

/// One one-to-one conversation to write: whose, how long, how many messages,
/// and for which backup source.
struct Individual<'a> {
    chat_id: &'a str,
    span_years: f64,
    msg_count: usize,
    flavor: SourceFlavor,
}

impl<'a> Individual<'a> {
    /// The contact's conversation as `flavor` would hold it. WhatsApp copies
    /// are shorter and more recent than the phone's own history.
    fn for_contact(contact: &'a Contact, flavor: SourceFlavor) -> Self {
        let (span_years, msg_count) = match flavor {
            SourceFlavor::Whatsapp => (
                contact.span_years.min(3.0),
                contact.message_count().clamp(20, 80),
            ),
            SourceFlavor::IMessage | SourceFlavor::SmsBackupRestore => {
                (contact.span_years, contact.message_count())
            }
        };
        Self {
            chat_id: contact.primary_phone(),
            span_years,
            msg_count,
            flavor,
        }
    }
}

/// A message written to both sides of an overlapping conversation with the
/// same text and time, so import can treat the two copies as duplicates.
struct SharedMessage {
    timestamp: i64,
    from_me: bool,
    text: String,
}

impl SharedMessage {
    /// The plain message for one side.
    fn message(
        &self,
        guid: String,
        chat_id: &str,
        service: IrService,
        message_kind: IrMessageKind,
    ) -> IrMessage {
        IrMessage {
            guid,
            timestamp_unix_ms: self.timestamp,
            // Every source the demo imitates records milliseconds.
            time_precision: TimePrecision::Milliseconds,
            direction: if self.from_me {
                IrDirection::Outgoing
            } else {
                IrDirection::Incoming
            },
            service,
            message_kind,
            sender_identity: if self.from_me {
                None
            } else {
                Some(chat_id.into())
            },
            sender_display_name: None,
            owner_identity: None,
            subject: None,
            text: self.text.clone(),
            attachments: vec![],
            reactions: Vec::new(),
            deletion: None,
            edits: Vec::new(),
            reply_to: None,
            imessage: None,
            source: None,
        }
    }
}

/// An overlapping conversation: the shared messages both sides carry, the
/// iMessage side's full timeline, and how many extra Android-only messages
/// follow it.
struct Overlap<'a> {
    chat_id: &'a str,
    display_name: Option<String>,
    msg_count: usize,
    timestamps: &'a [i64],
    shared: &'a [SharedMessage],
    extra_n: usize,
}

impl Overlap<'_> {
    /// Timestamp to start extra Android messages from: the last shared
    /// message, else the last iMessage time, else the reference time.
    fn android_base_timestamp(&self, cfg: &SeedConfig) -> i64 {
        if let Some(shared) = self.shared.last() {
            return shared.timestamp;
        }
        if let Some(&timestamp) = self.timestamps.last() {
            return timestamp;
        }
        cfg.reference_time.timestamp_millis()
    }
}

/// What a conversation file's header says: the conversation, and the backup
/// `owner_identity` exported it from.
struct ConversationFileHeader<'a> {
    chat_id: &'a str,
    conversation_type: IrConversationType,
    group_title: Option<String>,
    participants: Vec<IrParticipant>,
    message_count: usize,
    source: &'a str,
    owner_identity: &'a str,
}

impl<'a> ConversationFileHeader<'a> {
    /// The header of a one-to-one conversation, which has no group title.
    fn one_to_one(
        chat_id: &'a str,
        participants: Vec<IrParticipant>,
        message_count: usize,
        source: &'a str,
        owner_identity: &'a str,
    ) -> Self {
        Self {
            chat_id,
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants,
            message_count,
            source,
            owner_identity,
        }
    }
}

impl<R: Rng> Seeder<'_, R> {
    /// Write one conversation file: create it at `path`, write `header` as
    /// its first line (exported at the seed's reference time), let `body`
    /// write the messages, then flush the file and count it. Every generator
    /// writes its files through here, so none can drop a file unflushed:
    /// dropping a `BufWriter` flushes it but ignores the error, which would
    /// leave a cut-off file behind a seed that reports success.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created, `body` fails, or a
    /// line cannot be written.
    fn write_conversation_file(
        &mut self,
        path: &Path,
        header: ConversationFileHeader<'_>,
        body: impl FnOnce(&mut Self, &mut BufWriter<File>) -> Result<()>,
    ) -> Result<()> {
        let mut file = open_jsonl(path)?;
        let line = ConversationHeader {
            schema_version: SCHEMA_VERSION,
            export: export_meta(
                header.source,
                header.owner_identity,
                self.cfg.reference_time.timestamp_millis(),
            ),
            conversation: ConversationMeta {
                chat_identifier: header.chat_id.into(),
                conversation_type: header.conversation_type,
                group_title: header.group_title,
                participants: header.participants,
                stats: ConversationStats {
                    message_count: header.message_count as u64,
                    attachment_count: 0,
                    first_timestamp_unix_ms: None,
                    last_timestamp_unix_ms: None,
                },
            },
        };
        writeln!(file, "{}", serde_json::to_string(&line)?)?;
        body(self, &mut file)?;
        file.flush()
            .with_context(|| format!("write {}", path.display()))?;
        self.stats.conversation_files += 1;
        Ok(())
    }

    /// Write one one-to-one conversation for a single backup source.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or a line cannot be written.
    fn individual(&mut self, staging: &Path, spec: &Individual<'_>) -> Result<()> {
        let chat_id = spec.chat_id;
        let msg_count = spec.msg_count;
        let flavor = spec.flavor;
        // A roster contact's backups carry no name for them: each arrives as
        // an Unknown, and the demo's address book names them afterwards.
        let participants = individual_participants(chat_id, None);
        let path = staging.join(conversation_file_name(chat_id));
        self.write_conversation_file(
            &path,
            ConversationFileHeader::one_to_one(
                chat_id,
                participants,
                msg_count,
                source_id(flavor),
                OWNER_PHONE,
            ),
            |seeder, file| {
                let timestamps =
                    seeder.timestamps(msg_count, spec.span_years, sample_direct_day_burst);
                let mut origin_guid: Option<String> = None;
                for (i, &timestamp_ms) in timestamps.iter().enumerate() {
                    let from_me = i % 3 != 0;
                    let guid = format!("{}1to1-{chat_id}-{i}", guid_prefix(flavor));
                    let mut msg =
                        seeder.text_message(&guid, timestamp_ms, from_me, chat_id, flavor);
                    match flavor {
                        SourceFlavor::IMessage => {
                            seeder.decorate_message(
                                &mut msg,
                                i,
                                msg_count,
                                chat_id,
                                from_me,
                                &mut origin_guid,
                            );
                            mark_deletion(&mut msg, i, &seeder.cfg.messages);
                            mark_edited(&mut msg, i, &seeder.cfg.messages, seeder.corpus);
                        }
                        SourceFlavor::SmsBackupRestore => {
                            seeder.decorate_android_message(&mut msg, i, msg_count);
                        }
                        SourceFlavor::Whatsapp => {
                            // WhatsApp conversations skip iMessage-only fields such as tapbacks
                            // and replies.
                        }
                    }
                    seeder.emit(file, msg)?;
                }
                Ok(())
            },
        )
    }

    /// Write the same contact into both the iMessage and Android directories.
    ///
    /// Shared messages use the same text and time so import can treat them as
    /// duplicates. The Android copy also gets extra messages that only exist there.
    ///
    /// # Errors
    ///
    /// Returns an error if either conversation file cannot be written.
    fn overlap_individual(&mut self, staging: &StagingDirs<'_>, contact: &Contact) -> Result<()> {
        let chat_id = contact.primary_phone();
        let msg_count = contact.message_count().max(1);
        let shared_raw = (msg_count as f64) * self.cfg.sources.overlap_shared_fraction;
        let shared_n = shared_raw.round().clamp(1.0, msg_count as f64) as usize;
        let extra_lo = self.cfg.sources.overlap_android_extra_min;
        let extra_hi = self.cfg.sources.overlap_android_extra_max.max(extra_lo + 1);
        let extra_n = self.rng.random_range(extra_lo..extra_hi);

        let timestamps = self.timestamps(msg_count, contact.span_years, sample_direct_day_burst);
        let shared: Vec<SharedMessage> = timestamps
            .iter()
            .take(shared_n)
            .enumerate()
            .map(|(i, &timestamp)| SharedMessage {
                timestamp,
                from_me: i % 3 != 0,
                text: format!("Shared demo message {i} with {chat_id}"),
            })
            .collect();
        self.stats.shared_messages += shared.len();
        let overlap = Overlap {
            chat_id,
            display_name: None,
            msg_count,
            timestamps: &timestamps,
            shared: &shared,
            extra_n,
        };
        self.overlap_imessage(staging.imessage, &overlap)?;
        self.overlap_android(staging.sbr, &overlap)
    }

    /// The iMessage side of an overlapping conversation: shared rows, then the rest.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn overlap_imessage(&mut self, staging: &Path, overlap: &Overlap<'_>) -> Result<()> {
        let chat_id = overlap.chat_id;
        let path = staging.join(conversation_file_name(chat_id));
        self.write_conversation_file(
            &path,
            ConversationFileHeader::one_to_one(
                chat_id,
                individual_participants(chat_id, overlap.display_name.clone()),
                overlap.msg_count,
                IMESSAGE_SOURCE,
                OWNER_PHONE,
            ),
            |seeder, file| {
                let mut origin_guid: Option<String> = None;
                for (i, shared) in overlap.shared.iter().enumerate() {
                    let msg = shared.message(
                        format!("1to1-{chat_id}-{i}"),
                        chat_id,
                        IrService::IMessage,
                        IrMessageKind::IMessage,
                    );
                    seeder.emit(file, msg)?;
                }
                let shared_n = overlap.shared.len();
                for (i, &timestamp) in overlap
                    .timestamps
                    .iter()
                    .enumerate()
                    .skip(shared_n)
                    .take(overlap.msg_count.saturating_sub(shared_n))
                {
                    let from_me = i % 3 != 0;
                    let guid = format!("1to1-{chat_id}-{i}");
                    let mut msg = seeder.text_message(
                        &guid,
                        timestamp,
                        from_me,
                        chat_id,
                        SourceFlavor::IMessage,
                    );
                    seeder.decorate_message(
                        &mut msg,
                        i,
                        overlap.msg_count,
                        chat_id,
                        from_me,
                        &mut origin_guid,
                    );
                    seeder.emit(file, msg)?;
                }
                Ok(())
            },
        )
    }

    /// The Android side of an overlapping conversation: shared rows, then extra messages.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn overlap_android(&mut self, staging: &Path, overlap: &Overlap<'_>) -> Result<()> {
        let chat_id = overlap.chat_id;
        let android_total = overlap.shared.len() + overlap.extra_n;
        let path = staging.join(conversation_file_name(chat_id));
        self.write_conversation_file(
            &path,
            ConversationFileHeader::one_to_one(
                chat_id,
                individual_participants(chat_id, overlap.display_name.clone()),
                android_total,
                SBR_SOURCE,
                OWNER_PHONE,
            ),
            |seeder, file| {
                for (i, shared) in overlap.shared.iter().enumerate() {
                    let msg = shared.message(
                        format!("sbr-shared-{chat_id}-{i}"),
                        chat_id,
                        IrService::Sms,
                        IrMessageKind::Sms,
                    );
                    seeder.emit(file, msg)?;
                }
                let mut timestamp = overlap.android_base_timestamp(seeder.cfg);
                for j in 0..overlap.extra_n {
                    timestamp = next_daytime_minute(timestamp);
                    let from_me = j % 4 == 0;
                    let guid = format!("sbr-extra-{chat_id}-{j}");
                    let mut msg = seeder.text_message(
                        &guid,
                        timestamp,
                        from_me,
                        chat_id,
                        SourceFlavor::SmsBackupRestore,
                    );
                    seeder.decorate_android_message(&mut msg, j, overlap.extra_n);
                    seeder.emit(file, msg)?;
                }
                Ok(())
            },
        )
    }
}

/// One participant for a one-to-one conversation: the other person's phone or email.
fn individual_participants(chat_id: &str, display_name: Option<String>) -> Vec<IrParticipant> {
    vec![IrParticipant {
        identity: Some(chat_id.into()),
        display_name,
    }]
}

/// Directory name for this backup source (`imessage`, `sms-backup-restore`, or `whatsapp`).
fn source_id(flavor: SourceFlavor) -> &'static str {
    match flavor {
        SourceFlavor::IMessage => IMESSAGE_SOURCE,
        SourceFlavor::SmsBackupRestore => SBR_SOURCE,
        SourceFlavor::Whatsapp => WHATSAPP_SOURCE,
    }
}

/// Prefix on message IDs so Android and WhatsApp IDs do not collide with iMessage.
fn guid_prefix(flavor: SourceFlavor) -> &'static str {
    match flavor {
        SourceFlavor::IMessage => "",
        SourceFlavor::SmsBackupRestore => "sbr-",
        SourceFlavor::Whatsapp => "wa-",
    }
}

impl<R: Rng> Seeder<'_, R> {
    /// Write a conversation for a phone or email that has no contact card.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn unassigned(&mut self, staging: &Path, ua: &Unassigned, msg_count: usize) -> Result<()> {
        let chat_id = &ua.handle;
        let participants = individual_participants(chat_id, ua.name_alias.clone());
        let fname = if ua.email_only {
            format!("email-{}.jsonl", chat_id.replace('@', "_at_"))
        } else {
            conversation_file_name(chat_id)
        };
        // A correspondent known only by an email address wrote to the Demo
        // Account's email, so that identity has messages too.
        let owner_identity = if ua.email_only {
            OWNER_EMAIL
        } else {
            OWNER_PHONE
        };
        self.write_conversation_file(
            &staging.join(fname),
            ConversationFileHeader::one_to_one(
                chat_id,
                participants,
                msg_count,
                IMESSAGE_SOURCE,
                owner_identity,
            ),
            |seeder, file| {
                let timestamps = seeder.timestamps(msg_count, 1.5, sample_direct_day_burst);
                for (i, &timestamp_ms) in timestamps.iter().enumerate() {
                    let from_me = i % 4 == 0;
                    let guid = format!("unassigned-{chat_id}-{i}");
                    let mut msg = seeder.text_message(
                        &guid,
                        timestamp_ms,
                        from_me,
                        chat_id,
                        SourceFlavor::IMessage,
                    );
                    if i == 2 && ua.name_alias.is_some() && !from_me {
                        msg.sender_identity = Some(String::new());
                    }
                    if should_attach_jpg(i, msg_count, seeder.cfg) {
                        seeder.add_jpg_attachment(&mut msg, i);
                    }
                    seeder.emit(file, msg)?;
                }
                Ok(())
            },
        )
    }

    /// Write one group conversation. The first group, when it has a title, starts
    /// with an announcement that names it.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn group(
        &mut self,
        staging: &Path,
        roster: &Roster,
        group: &crate::personas::GroupSpec,
    ) -> Result<()> {
        let chat_id = group_chat_id(group.index);
        let participants = group_participants(roster, group);
        if participants.len() < 2 && !group.phone_only {
            return Ok(());
        }

        // Demo group members always carry an address; a name-only participant has
        // nothing to send from.
        let handles: Vec<String> = participants
            .iter()
            .filter_map(|participant| participant.identity.clone())
            .collect();
        let msg_count = ((group.msgs_per_year * group.span_years).round() as isize).max(1) as usize;
        let timestamps = self.timestamps(msg_count, group.span_years, sample_group_day_burst);
        // The first group starts with a rename announcement so the UI has one to
        // show, when it has a title to have been named.
        let rename_title = group.title.as_deref().filter(|_| group.index == 0);
        let header_message_count = if rename_title.is_some() {
            msg_count + 1
        } else {
            msg_count
        };
        self.write_conversation_file(
            &staging.join(format!("group-{:03}.jsonl", group.index)),
            ConversationFileHeader {
                chat_id: &chat_id,
                conversation_type: IrConversationType::Group,
                group_title: group.title.clone(),
                participants,
                message_count: header_message_count,
                source: IMESSAGE_SOURCE,
                owner_identity: OWNER_PHONE,
            },
            |seeder, file| {
                if let Some(title) = rename_title {
                    let first_message_ts = timestamps
                        .first()
                        .copied()
                        .unwrap_or_else(|| seeder.cfg.reference_time.timestamp_millis());
                    let mut ann = seeder.text_message(
                        "grp-0-rename",
                        first_message_ts - 60_000,
                        true,
                        OWNER_PHONE,
                        SourceFlavor::IMessage,
                    );
                    ann.text.clear();
                    ann.message_kind = IrMessageKind::Announcement;
                    let im = ann.imessage.get_or_insert_with(IrImessage::default);
                    im.announcement = Some(format!("Demo User named the conversation “{title}”."));
                    seeder.emit(file, ann)?;
                }

                let mut origin_guid: Option<String> = None;
                for i in 0..msg_count {
                    let from_me = i % 7 == 0;
                    let sender = if from_me {
                        None
                    } else {
                        Some(handles[i % handles.len()].clone())
                    };
                    let guid = format!("grp-{}-{i}", group.index);
                    let peer = sender.as_deref().unwrap_or(OWNER_PHONE);
                    let mut msg = seeder.text_message(
                        &guid,
                        timestamps[i],
                        from_me,
                        peer,
                        SourceFlavor::IMessage,
                    );
                    msg.sender_identity = sender;
                    if should_attach_jpg(i, msg_count, seeder.cfg) {
                        seeder.add_jpg_attachment(&mut msg, i + group.index);
                    } else if should_attach_other(i, msg_count, seeder.cfg) {
                        seeder.add_attachment(&mut msg, i, OTHER_ATTACHMENTS);
                    }
                    let messages = &seeder.cfg.messages;
                    if messages.tapback_stride > 0
                        && i % messages.tapback_stride == 0
                        && msg_count >= 10
                        && !handles.is_empty()
                    {
                        let reactor = &handles[(i + 1) % handles.len()];
                        let kind = TAPBACK_KINDS.choose(&mut *seeder.rng).unwrap();
                        let emoji = tapback_emoji(kind, &mut *seeder.rng);
                        push_tapback(&mut msg, kind, emoji, reactor, false);
                    }
                    if messages.reply_stride > 0
                        && i % messages.reply_stride == 0
                        && origin_guid.is_some()
                    {
                        msg.reply_to = Some(ReplyTo {
                            guid: origin_guid.clone(),
                            part_index: Some(0),
                        });
                    }
                    if i % (messages.reply_stride.max(1) + 17) == 0 {
                        origin_guid = Some(guid.clone());
                    }
                    seeder.emit(file, msg)?;
                }
                Ok(())
            },
        )
    }
}

/// Participants for a group: roster contacts, or phone numbers no contact holds.
fn group_participants(roster: &Roster, group: &crate::personas::GroupSpec) -> Vec<IrParticipant> {
    if group.phone_only {
        return phone_only_participants(&group.phone_only_handles);
    }
    named_group_participants(roster, &group.member_idxs)
}

/// Group members who are only phone numbers, with no display names.
fn phone_only_participants(handles: &[String]) -> Vec<IrParticipant> {
    let mut participants = Vec::with_capacity(handles.len());
    for handle in handles {
        participants.push(IrParticipant {
            identity: Some(handle.clone()),
            display_name: None,
        });
    }
    participants
}

/// Group members looked up from the roster by index. The backup carries no
/// names for them; the demo's address book supplies those after the import.
fn named_group_participants(roster: &Roster, member_idxs: &[usize]) -> Vec<IrParticipant> {
    let mut participants = Vec::new();
    for &index in member_idxs {
        let Some(contact) = roster.contacts.get(index) else {
            continue;
        };
        participants.push(IrParticipant {
            identity: Some(contact.primary_phone().into()),
            display_name: None,
        });
    }
    participants
}

impl<R: Rng> Seeder<'_, R> {
    /// Write orphaned messages, ones the backup holds without recording which
    /// conversation they were said in, as the Apple Messages reader writes
    /// them: the ones [`ORPHAN_SENDER`] sent and the ones the account holder
    /// sent them, in a conversation of their own with that person as its only
    /// participant, and ones the account holder sent with no recipient
    /// recorded, in one with no participants.
    ///
    /// # Errors
    ///
    /// Returns an error if a file cannot be written.
    fn orphaned(&mut self, staging: &Path) -> Result<()> {
        let n = self.cfg.edge_cases.orphaned_messages.max(1);
        let timestamps = self.timestamps(n, 2.0, sample_direct_day_burst);
        // Every third message names nobody; the others alternate between
        // ORPHAN_SENDER's and the holder's to them.
        let (mut with_person, mut naming_nobody) = (Vec::new(), Vec::new());
        for (i, &timestamp_ms) in timestamps.iter().enumerate() {
            let guid = format!("orphan-{i}");
            let names_nobody = i % 3 == 2;
            let from_me = names_nobody || i % 3 == 1;
            let mut msg = self.text_message(
                &guid,
                timestamp_ms,
                from_me,
                ORPHAN_SENDER,
                SourceFlavor::IMessage,
            );
            msg.text = format!("Orphaned message #{i} (no conversation association)");
            if names_nobody {
                naming_nobody.push(msg);
            } else {
                with_person.push(msg);
            }
        }
        let sender = IrParticipant {
            identity: Some(ORPHAN_SENDER.into()),
            display_name: None,
        };
        for (file_name, sender, messages) in [
            ("orphaned-person.jsonl", Some(sender), with_person),
            ("orphaned-nobody.jsonl", None, naming_nobody),
        ] {
            if messages.is_empty() {
                continue;
            }
            let chat_id = orphaned_chat_id(sender.as_ref().and_then(|p| p.identity.as_deref()));
            self.write_conversation_file(
                &staging.join(file_name),
                ConversationFileHeader {
                    chat_id: &chat_id,
                    conversation_type: IrConversationType::Orphaned,
                    group_title: None,
                    participants: sender.into_iter().collect(),
                    message_count: messages.len(),
                    source: IMESSAGE_SOURCE,
                    owner_identity: OWNER_PHONE,
                },
                |seeder, file| {
                    for msg in messages {
                        seeder.emit(file, msg)?;
                    }
                    Ok(())
                },
            )?;
        }
        Ok(())
    }

    /// Create an iMessage conversation file with a header and no messages
    /// (the empty one-to-one or the empty group conversation).
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn create_empty_conversation_file(
        &mut self,
        staging: &Path,
        chat_id: &str,
        conversation_type: IrConversationType,
        member_phones: &[&str],
    ) -> Result<()> {
        let participants = member_phones
            .iter()
            .map(|handle| IrParticipant {
                identity: Some((*handle).into()),
                display_name: None,
            })
            .collect();
        self.write_conversation_file(
            &staging.join(format!("empty-{}.jsonl", sanitize_filename(chat_id))),
            ConversationFileHeader {
                chat_id,
                conversation_type,
                group_title: None,
                participants,
                message_count: 0,
                source: IMESSAGE_SOURCE,
                owner_identity: OWNER_PHONE,
            },
            |_, _| Ok(()),
        )
    }

    /// Add photos, other files, tapbacks, replies, and occasional SMS/RCS
    /// labels to an iMessage. `origin_guid` is the message every few messages
    /// reply to, the first message of an Apple Messages reply in a thread; this
    /// call may replace it.
    fn decorate_message(
        &mut self,
        msg: &mut IrMessage,
        i: usize,
        msg_count: usize,
        peer: &str,
        from_me: bool,
        origin_guid: &mut Option<String>,
    ) {
        if should_attach_jpg(i, msg_count, self.cfg) {
            self.add_jpg_attachment(msg, i);
            if i > 0 && i.is_multiple_of(40) {
                msg.text = PHOTO_CAPTIONS.choose(&mut *self.rng).unwrap().to_string();
            }
        } else if should_attach_photo_only(i, msg_count, self.cfg) {
            msg.text.clear();
            self.add_jpg_attachment(msg, i + 1);
        } else if should_attach_other(i, msg_count, self.cfg) {
            self.add_attachment(msg, i, OTHER_ATTACHMENTS);
        }
        let messages = &self.cfg.messages;
        if messages.tapback_stride > 0
            && i.is_multiple_of(messages.tapback_stride)
            && !from_me
            && msg_count >= 20
        {
            push_tapback(msg, "loved", None, peer, false);
        }
        if messages.reply_stride > 0
            && i.is_multiple_of(messages.reply_stride)
            && origin_guid.is_some()
            && msg_count >= 25
        {
            msg.reply_to = Some(ReplyTo {
                guid: origin_guid.clone(),
                part_index: Some(0),
            });
        }
        if i.is_multiple_of(messages.reply_stride.max(1) + 29) {
            *origin_guid = Some(msg.guid.clone());
        }
        maybe_mark_as_sms_or_rcs(
            msg,
            self.cfg.messages.apple_fallback_transport_fraction,
            &mut *self.rng,
        );
    }
}

/// Mark the message Deleted in the source app or Unsent when `i` falls on
/// either stride. Only Apple Messages records the marks (#1143), so only its
/// one-to-one conversations call this. An Unsent message keeps nothing, and
/// only an iMessage can be unsent, so a message with an attachment, one sent as
/// SMS or RCS, or a reply is left unmarked rather than made into one no backup
/// holds.
fn mark_deletion(msg: &mut IrMessage, i: usize, messages: &crate::config::MessagesConfig) {
    let on = |stride: usize| stride > 0 && i > 0 && i.is_multiple_of(stride);
    let can_be_unsent =
        msg.attachments.is_empty() && msg.service == IrService::IMessage && msg.reply_to.is_none();
    if on(messages.unsent_stride) && can_be_unsent {
        msg.deletion = Some(Deletion::Unsent);
        msg.text.clear();
        msg.reactions.clear();
    } else if on(messages.deleted_in_source_app_stride) {
        msg.deletion = Some(Deletion::DeletedInSourceApp);
    }
}

/// Give the message earlier versions when `i` falls on the edit stride, so the
/// Demo Account has edited messages to open (#1143). Only an iMessage can be
/// edited, so a message sent as SMS or RCS, an Unsent one, and one with no
/// words are left as they are. Every second edited message was edited twice.
///
/// The earlier versions are other sentences of the corpus, picked by the
/// message's guid rather than drawn from the random source, so every other
/// message of the Demo Data stays as it was, and two conversations' edited
/// messages differ. The original's time is the message's own, and
/// each edit follows two minutes later, inside the 15 minutes Apple Messages
/// allows.
fn mark_edited(
    msg: &mut IrMessage,
    i: usize,
    messages: &crate::config::MessagesConfig,
    corpus: &Corpus,
) {
    /// Where each earlier version's sentence sits from the first one's, and
    /// how long after the message was sent it was written.
    const VERSIONS: [(usize, i64); 2] = [(0, 0), (7, 120_000)];
    let stride = messages.edited_stride;
    if stride == 0 || i == 0 || !i.is_multiple_of(stride) {
        return;
    }
    if msg.service != IrService::IMessage
        || msg.deletion == Some(Deletion::Unsent)
        || !msg.text.chars().any(char::is_alphabetic)
    {
        return;
    }
    let count = if (i / stride).is_multiple_of(2) { 2 } else { 1 };
    let first = msg.guid.bytes().fold(0usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    msg.edits = VERSIONS
        .iter()
        .take(count)
        .map(|&(offset, after_ms)| EarlierVersion {
            part_index: 0,
            text: Some(corpus.sentence(first.wrapping_add(offset)).to_string()),
            edited_at_unix_ms: Some(msg.timestamp_unix_ms + after_ms),
        })
        .filter(|version| version.text.as_deref() != Some(msg.text.as_str()))
        .collect();
}

/// Sometimes mark an iMessage as SMS or RCS so the conversation view can show those labels.
fn maybe_mark_as_sms_or_rcs(msg: &mut IrMessage, fraction: f64, rng: &mut impl Rng) {
    if !rng.random_bool(fraction.clamp(0.0, 1.0)) {
        return;
    }
    if rng.random_bool(0.5) {
        msg.service = IrService::Sms;
    } else {
        msg.service = IrService::Rcs;
    }
    if matches!(
        msg.message_kind,
        IrMessageKind::Mms | IrMessageKind::Announcement
    ) {
        return;
    }
    msg.message_kind = IrMessageKind::Sms;
}

/// Create a buffered writer for a new conversation file.
///
/// # Errors
///
/// Returns an error if the file cannot be created.
fn open_jsonl(path: &Path) -> Result<BufWriter<File>> {
    let f = File::create(path).with_context(|| format!("create {}", path.display()))?;
    Ok(BufWriter::new(f))
}

/// Write one message as a JSON line. Empty iMessage extras are dropped first.
///
/// # Errors
///
/// Returns an error if the message cannot be serialized or written.
fn write_message(file: &mut BufWriter<File>, mut msg: IrMessage) -> Result<()> {
    if let Some(im) = msg.imessage.take() {
        msg.imessage = im.into_option();
    }
    writeln!(file, "{}", serde_json::to_string(&msg)?)?;
    Ok(())
}

impl<R: Rng> Seeder<'_, R> {
    /// Build a text message with a body from the book, or sometimes only an emoji.
    fn text_message(
        &mut self,
        guid: &str,
        timestamp_unix_ms: i64,
        from_me: bool,
        peer: &str,
        flavor: SourceFlavor,
    ) -> IrMessage {
        let text = if self.rng.random_bool(self.cfg.messages.emoji_probability) {
            (*EMOJI_ONLY.choose(&mut *self.rng).unwrap()).to_string()
        } else {
            self.corpus.pick_message(&mut *self.rng)
        };
        let (service, message_kind) = match flavor {
            SourceFlavor::IMessage => (IrService::IMessage, IrMessageKind::IMessage),
            SourceFlavor::SmsBackupRestore => (IrService::Sms, IrMessageKind::Sms),
            SourceFlavor::Whatsapp => (IrService::Whatsapp, IrMessageKind::Sms),
        };
        IrMessage {
            guid: guid.into(),
            timestamp_unix_ms,
            // Every source the demo imitates records milliseconds.
            time_precision: TimePrecision::Milliseconds,
            direction: if from_me {
                IrDirection::Outgoing
            } else {
                IrDirection::Incoming
            },
            service,
            message_kind,
            sender_identity: if from_me { None } else { Some(peer.into()) },
            sender_display_name: None,
            owner_identity: None,
            subject: None,
            text,
            attachments: vec![],
            reactions: Vec::new(),
            deletion: None,
            edits: Vec::new(),
            reply_to: None,
            imessage: None,
            source: None,
        }
    }

    /// Add photos or other files to an Android message and mark it as MMS when it has an attachment.
    fn decorate_android_message(&mut self, msg: &mut IrMessage, i: usize, msg_count: usize) {
        msg.service = IrService::Sms;
        if should_attach_jpg(i, msg_count, self.cfg) {
            self.add_jpg_attachment(msg, i);
            if i > 0 && i.is_multiple_of(40) {
                msg.text = PHOTO_CAPTIONS.choose(&mut *self.rng).unwrap().to_string();
            }
            msg.message_kind = IrMessageKind::Mms;
        } else if should_attach_other(i, msg_count, self.cfg) {
            self.add_attachment(msg, i, OTHER_ATTACHMENTS);
            msg.message_kind = IrMessageKind::Mms;
        } else {
            msg.message_kind = IrMessageKind::Sms;
        }
    }

    /// Write one message line and count it.
    fn emit(&mut self, file: &mut BufWriter<File>, msg: IrMessage) -> Result<()> {
        write_message(file, msg)?;
        self.stats.messages += 1;
        Ok(())
    }

    /// Spread `total` messages across the `span_years` before the reference
    /// time, with `sampler` deciding how busy each day is.
    fn timestamps(
        &mut self,
        total: usize,
        span_years: f64,
        sampler: fn(&mut R) -> usize,
    ) -> Vec<i64> {
        bursty_timestamps(
            total,
            span_years,
            self.cfg.reference_time,
            sampler,
            &mut *self.rng,
        )
    }
}

/// Spread messages across the date range with busy days, quiet gaps, and occasional floods.
fn bursty_timestamps<R: Rng, F: FnMut(&mut R) -> usize>(
    total: usize,
    span_years: f64,
    reference_time: chrono::DateTime<Utc>,
    mut sample_burst: F,
    rng: &mut R,
) -> Vec<i64> {
    if total == 0 {
        return Vec::new();
    }
    let span_days = ((span_years * 365.25).round() as i64).max(1);
    // Day 0 starts at midnight UTC `span_days` before the reference day, so
    // the last day is the day before it and every message is earlier than
    // the reference time.
    let start = midnight_utc(reference_time) - Duration::days(span_days);

    let per_day = assign_messages_to_days(total, span_days, &mut sample_burst, rng);
    let mut days: Vec<(i64, usize)> = per_day.into_iter().collect();
    days.sort_by_key(|(day, _)| *day);

    let mut out = Vec::with_capacity(total);
    for (day, count) in days {
        let day_start = start + Duration::days(day);
        append_day_timestamps(&mut out, day_start, count, rng);
    }
    out.sort_unstable();
    out
}

/// Assign bursts of messages to days, preferring recent days.
fn assign_messages_to_days<R: Rng, F: FnMut(&mut R) -> usize>(
    total: usize,
    span_days: i64,
    sample_burst: &mut F,
    rng: &mut R,
) -> HashMap<i64, usize> {
    let mut per_day: HashMap<i64, usize> = HashMap::new();
    let mut remaining = total;
    while remaining > 0 {
        let burst = sample_burst(rng).min(remaining);
        // Prefer recent days. Most days in the span get no messages.
        let unit: f64 = rng.random::<f64>().powf(0.65);
        let day_index = ((span_days - 1) as f64 * unit).round() as i64;
        let day = day_index.clamp(0, span_days - 1);
        *per_day.entry(day).or_default() += burst;
        remaining -= burst;
    }
    per_day
}

/// The first second of a day's message window: 08:00 UTC.
const DAY_WINDOW_START_SECS: i64 = 8 * 3600;
/// The last second of a day's message window: 23:00 UTC.
const DAY_WINDOW_END_SECS: i64 = 23 * 3600;

/// Midnight UTC at the start of `time`'s day.
fn midnight_utc(time: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    time.date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight exists")
        .and_utc()
}

/// One minute after `timestamp_ms`, or 8am UTC the next day when that minute
/// falls after 11pm UTC.
fn next_daytime_minute(timestamp_ms: i64) -> i64 {
    let next = timestamp_ms + 60_000;
    let ms_per_day = 24 * 3600 * 1000;
    if next.rem_euclid(ms_per_day) <= DAY_WINDOW_END_SECS * 1000 {
        return next;
    }
    next.div_euclid(ms_per_day) * ms_per_day + ms_per_day + DAY_WINDOW_START_SECS * 1000
}

/// Place a day's messages between 8am and 11pm UTC, a few seconds apart.
///
/// `day_start` is midnight UTC. The Demo Account's time zone is UTC, so the
/// messages read as sent during the day.
fn append_day_timestamps<R: Rng>(
    out: &mut Vec<i64>,
    day_start: chrono::DateTime<Utc>,
    count: usize,
    rng: &mut R,
) {
    let mut seconds = Vec::with_capacity(count);
    for _ in 0..count {
        seconds.push(rng.random_range(DAY_WINDOW_START_SECS..DAY_WINDOW_END_SECS));
    }
    seconds.sort_unstable();
    let mut placed: Vec<i64> = Vec::with_capacity(count);
    for (i, secs) in seconds.into_iter().enumerate() {
        // Nudge messages a few seconds apart so a burst is not one identical timestamp.
        let spacing = (i as i64) * rng.random_range(8..45);
        let mut spaced = (secs + spacing).min(DAY_WINDOW_END_SECS);
        if let Some(&prev) = placed.last()
            && spaced <= prev
        {
            spaced = prev + rng.random_range(12..90);
        }
        placed.push(spaced);
    }
    // Pull any message the nudging pushed past 11pm back inside the window,
    // still one second or more after the message before it.
    let mut latest = DAY_WINDOW_END_SECS;
    for secs in placed.iter_mut().rev() {
        *secs = (*secs).min(latest);
        latest = *secs - 1;
    }
    out.extend(
        placed
            .into_iter()
            .map(|secs| (day_start + Duration::seconds(secs)).timestamp_millis()),
    );
}

/// How many messages land on one active day in a one-to-one conversation.
fn sample_direct_day_burst(rng: &mut impl Rng) -> usize {
    let roll: f64 = rng.random();
    if roll < 0.08 {
        rng.random_range(12..=45)
    } else if roll < 0.20 {
        rng.random_range(1..=2)
    } else {
        rng.random_range(3..=10)
    }
}

/// How many messages land on one active day in a group conversation.
fn sample_group_day_burst(rng: &mut impl Rng) -> usize {
    let roll: f64 = rng.random();
    if roll < 0.10 {
        rng.random_range(16..=70)
    } else if roll < 0.22 {
        rng.random_range(1..=2)
    } else {
        rng.random_range(3..=12)
    }
}

/// Chat identifier for a group: a mix of `chat…` IDs and phone-number IDs.
///
/// A group past the last phone-number ID gets a `chat…` ID instead.
fn group_chat_id(index: usize) -> String {
    match index % 5 {
        0 => format!("chat{:010}", 1_000_000_000u64 + index as u64),
        1..=3 => phones::group_chat_phone(index)
            .unwrap_or_else(|| format!("chat{:010}", 3_000_000_000u64 + index as u64)),
        _ => format!("chat{:010}", 2_000_000_000u64 + index as u64),
    }
}

/// True when this message index should get a JPEG with a caption.
fn should_attach_jpg(i: usize, total: usize, cfg: &SeedConfig) -> bool {
    if total < 8 {
        return false;
    }
    let stride = (cfg.messages.jpg_base_stride + total / 50).max(20);
    i > 0 && i.is_multiple_of(stride)
}

/// True when this message should be a photo with no text.
fn should_attach_photo_only(i: usize, total: usize, cfg: &SeedConfig) -> bool {
    if total < 20 {
        return false;
    }
    let stride = (cfg.messages.jpg_base_stride * 2 + total / 40).max(40);
    i % stride == 5
}

/// True when this message should get a non-JPEG attachment (PDF, audio, sticker, and so on).
fn should_attach_other(i: usize, total: usize, cfg: &SeedConfig) -> bool {
    if total < 30 {
        return false;
    }
    let stride = (cfg.messages.other_base_stride + total / 30).max(50);
    i > 0 && i.is_multiple_of(stride)
}

impl<R: Rng> Seeder<'_, R> {
    /// Attach a JPEG from the demo photo list and count it.
    fn add_jpg_attachment(&mut self, msg: &mut IrMessage, idx: usize) {
        let photo = &JPG_PHOTOS[idx % JPG_PHOTOS.len()];
        let (digest_sha256, size_bytes) = digest_fields(self.attachment_digests, photo.path);
        msg.attachments.push(IrAttachment {
            path: Some(photo.path.into()),
            original_name: Some(photo.original_name.into()),
            mime_type: Some("image/jpeg".into()),
            digest_sha256,
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            bytes: None,
            size_bytes,
            missing_reason: None,
        });
        self.stats.attachment_refs += 1;
    }

    /// Attach a non-JPEG file. Audio files get a short transcription.
    fn add_attachment(&mut self, msg: &mut IrMessage, idx: usize, files: &[(&str, &str, bool)]) {
        let (path, mime, is_sticker) = files[idx % files.len()];
        let (digest_sha256, size_bytes) = digest_fields(self.attachment_digests, path);
        let transcription = if mime.starts_with("audio/") {
            Some("Hey, just leaving a quick voice note.".into())
        } else {
            None
        };
        msg.attachments.push(IrAttachment {
            path: Some(path.into()),
            original_name: Some(filename_from_relative_path(path).into()),
            mime_type: Some(mime.into()),
            digest_sha256,
            is_sticker,
            transcription,
            sticker_effect: None,
            bytes: None,
            size_bytes,
            missing_reason: None,
        });
        self.stats.attachment_refs += 1;
    }
}

/// Look up the content fingerprint and size for an attachment path, if the file was written.
fn digest_fields(
    digests: &HashMap<String, (String, u64)>,
    path: &str,
) -> (Option<String>, Option<u64>) {
    match digests.get(path) {
        Some((digest, byte_len)) if !digest.is_empty() => (Some(digest.clone()), Some(*byte_len)),
        _ => (None, None),
    }
}

/// Last path segment of `path`, or the whole string if it has no `/`.
fn filename_from_relative_path(path: &str) -> &str {
    match path.rsplit('/').next() {
        Some(name) => name,
        None => path,
    }
}

/// Optional emoji for a tapback of kind `"emoji"`.
fn tapback_emoji(kind: &str, rng: &mut impl Rng) -> Option<String> {
    if kind != "emoji" {
        return None;
    }
    if !rng.random_bool(0.5) {
        return None;
    }
    let emoji = *EMOJI_ONLY.choose(rng).unwrap();
    Some(emoji.to_string())
}

/// Add a reaction (heart, thumbs-up, and similar) from `sender` onto `msg`.
fn push_tapback(
    msg: &mut IrMessage,
    kind: &str,
    emoji: Option<String>,
    sender: &str,
    from_me: bool,
) {
    msg.reactions.push(Reaction {
        part_index: 0,
        kind: kind.to_string(),
        emoji,
        is_from_me: from_me,
        reactor_identity: (!from_me).then(|| sender.to_string()),
        reactor_display_name: None,
    });
}

/// File name of the conversation with `chat_id`: the handle, made safe for a
/// file name, with `.jsonl` added.
fn conversation_file_name(chat_id: &str) -> String {
    sanitize_filename(chat_id) + ".jsonl"
}

/// Turn a phone or email into a safe file name (`+` becomes `p`, `@` becomes `a`).
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '+' => 'p',
            '@' => 'a',
            ':' | '/' | '\\' => '_',
            _ if c.is_ascii_alphanumeric() => c,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    use super::*;
    use crate::testutil::small_config;

    /// A line still in the buffer when the disk is full stops the seed, and
    /// the file is not counted. The conversation file is a link to
    /// `/dev/full`, which fails every write with ENOSPC; the header fits in
    /// the buffer, so only the flush at the end of the file sees the error.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_failed_last_write_stops_the_seed() {
        let temp = tempfile::tempdir().expect("create test directory");
        let cfg = small_config(temp.path());
        let corpus = Corpus::load_pride_and_prejudice().expect("load the corpus");
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let digests = HashMap::new();
        let cancel = AtomicBool::new(false);
        let mut seeder = Seeder {
            cfg: &cfg,
            corpus: &corpus,
            rng: &mut rng,
            attachment_digests: &digests,
            cancel: &cancel,
            stats: GenStats::default(),
        };
        let staging = temp.path().join("imessage");
        fs::create_dir(&staging).expect("create staging directory");
        std::os::unix::fs::symlink(
            "/dev/full",
            staging.join(format!(
                "empty-{}.jsonl",
                sanitize_filename(EMPTY_ONE_TO_ONE_HANDLE)
            )),
        )
        .expect("link the conversation file to /dev/full");

        let result = seeder.create_empty_conversation_file(
            &staging,
            EMPTY_ONE_TO_ONE_HANDLE,
            IrConversationType::Individual,
            &[],
        );

        assert!(result.is_err(), "a failed flush must be an error");
        assert_eq!(seeder.stats.conversation_files, 0);
    }
}
