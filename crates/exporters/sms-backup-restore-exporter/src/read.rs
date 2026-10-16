//! Read SMS Backup & Restore XML into [`ConversationDocument`] values.

use anyhow::{Result, bail};

use ir::{
    ConversationDocument, ExportMeta, Message, MessageKind, Participant, PendingAttachment,
    PendingConversation, PendingMessage, Projection, ProjectionHooks, Transport, VendorFields,
    prepare_conversation, project_pending,
};
use message_crate_core::{
    AttachmentFiles, Counter, DUPLICATES_DROPPED, ItemKind, LogSink, RunLogLevel,
    SKIPPED_INVALID_DATE, SKIPPED_UNKNOWN_ADDRESS, SKIPPED_UNKNOWN_TYPE, SKIPPED_UNREADABLE_PART,
    Sinks, check_cancel, document_messages, import_error_lines, item_line,
};
use message_crate_csv::format_local_ts;
use phone::{Handle, OwnerHandleSet};
use sbr::{
    AttachmentBytes, ConversationKind, ParseStats, Record, infer_owner_phones, parse_file_with,
};
use staging::{
    AttachmentSource, AttachmentSpool, CountedAttachments, PathSources, load_attachment_source,
};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const EXPORT_SOURCE: &str = message_crate_core::Exporter::SmsBackupRestore.source_id();
const EXPORT_TOOL: &str = "SMS Backup & Restore";
const EXPORT_TOOL_VERSION: &str = "10.26.003";

/// Counts from parsing SMS Backup & Restore XML into conversation documents.
#[derive(Debug, Default)]
pub struct ReadReport {
    /// Number of conversation documents produced.
    pub conversations: u64,
    /// SMS elements parsed.
    pub sms_seen: u64,
    /// MMS elements parsed.
    pub mms_seen: u64,
    /// Outgoing messages in produced documents.
    pub sent: u64,
    /// Incoming messages in produced documents.
    pub received: u64,
    /// Messages dropped for an invalid date.
    pub skipped_invalid_date: u64,
    /// Messages dropped with no usable address.
    pub skipped_unknown_address: u64,
    /// SMS dropped for an unknown `type`.
    pub skipped_unknown_type: u64,
    /// Draft/outbox/failed/queued messages dropped.
    pub skipped_never_sent: u64,
    /// MMS dropped with no participants.
    pub skipped_empty_participants: u64,
    /// Repeated copies of a message dropped, one copy of each kept.
    pub duplicates_dropped: u64,
    /// What could not be read, each with the file it was in.
    pub errors: Vec<ReadError>,
    /// Each kept message with a part or character references left out of
    /// it, so the run can name it.
    pub left_out: Vec<LeftOut>,
}

/// One message the read kept with a part or character references left out
/// of it, and the file it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    /// The file, as a path.
    pub file: String,
    /// The message, by its time and its `address` attribute as the file
    /// writes it, such as `message of 2014-05-22T15:41:01Z with +15555550101`.
    pub message: String,
    /// What was left out of it.
    pub counts: LeftOutCounts,
}

/// What the read left out of one message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LeftOutCounts {
    /// Parts left out because their `data` is not base64.
    pub unreadable_parts: u64,
    /// Character references left out because they are not a character.
    pub dropped_character_references: u64,
}

impl LeftOutCounts {
    /// Whether nothing was left out.
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

/// Something in one backup file the reader could not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError {
    /// The file, as a path.
    pub file: String,
    /// Why, as the parser said it.
    pub reason: String,
}

impl ReadError {
    fn new(path: &Path, error: &anyhow::Error) -> Self {
        Self {
            file: path.display().to_string(),
            reason: format!("{error:#}"),
        }
    }

    /// What happened to the file, worded to follow it, in the words every
    /// run gives it.
    pub fn what_happened(&self) -> String {
        format!("could not be read in full: {}", self.reason)
    }
}

/// The sentence that names the file and says what happened to it, as the
/// log gives it under the Import Errors heading.
impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&item_line(
            ItemKind::File,
            &self.file,
            &self.what_happened(),
        ))
    }
}

impl ReadReport {
    /// Parts left out of kept messages because their `data` is not base64.
    pub fn skipped_unreadable_part(&self) -> u64 {
        self.left_out
            .iter()
            .map(|m| m.counts.unreadable_parts)
            .sum()
    }

    /// Character references left out of kept messages because they are not
    /// a character.
    pub fn dropped_character_references(&self) -> u64 {
        self.left_out
            .iter()
            .map(|m| m.counts.dropped_character_references)
            .sum()
    }

    /// Each count the read reports, beside its [`Counter`], in the order
    /// Convert's log gives them. An import's summary gives the invalid dates
    /// and the repeated copies first, from fields of its own, and the rest
    /// in this order, so this order starts with those two.
    pub(crate) fn counts(&self) -> [(Counter, u64); 10] {
        [
            (SKIPPED_INVALID_DATE, self.skipped_invalid_date),
            (DUPLICATES_DROPPED, self.duplicates_dropped),
            (SMS_SEEN, self.sms_seen),
            (MMS_SEEN, self.mms_seen),
            (SKIPPED_UNKNOWN_ADDRESS, self.skipped_unknown_address),
            (SKIPPED_UNKNOWN_TYPE, self.skipped_unknown_type),
            (SKIPPED_NEVER_SENT, self.skipped_never_sent),
            (SKIPPED_EMPTY_PARTICIPANTS, self.skipped_empty_participants),
            (SKIPPED_UNREADABLE_PART, self.skipped_unreadable_part()),
            (
                DROPPED_CHARACTER_REFERENCES,
                self.dropped_character_references(),
            ),
        ]
    }

    /// [`Self::leveled_lines`] without their levels: what Convert's log
    /// gives, and an import's summary gives in the same words.
    pub fn log_lines(&self) -> Vec<String> {
        self.leveled_lines()
            .into_iter()
            .map(|(_, line)| line)
            .collect()
    }

    /// Write [`Self::leveled_lines`] to `log`, each at its level.
    pub fn write_log(&self, log: &LogSink) {
        for (level, line) in self.leveled_lines() {
            match level {
                RunLogLevel::Info => log.emit(line),
                RunLogLevel::Warn | RunLogLevel::Error => log.warn(line),
            }
        }
    }

    /// One log line for each count the read reports, leaving out the ones
    /// it found none of, then a sentence for every error under the Import
    /// Errors heading, so a person can tell what did not come across. Each
    /// line has the words an import's summary gives the same count or
    /// error. A count of what was read, or of repeated copies dropped,
    /// whose message is kept once, is at `INFO`; a count of what the run
    /// went on without, and every line under the Import Errors heading, at
    /// `WARN`.
    fn leveled_lines(&self) -> Vec<(RunLogLevel, String)> {
        /// The counts of what the run kept.
        const KEPT: [Counter; 3] = [SMS_SEEN, MMS_SEEN, DUPLICATES_DROPPED];
        self.counts()
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(counter, count)| {
                let level = if KEPT.iter().any(|kept| kept.key() == counter.key()) {
                    RunLogLevel::Info
                } else {
                    RunLogLevel::Warn
                };
                (level, counter.line(count))
            })
            .chain(
                import_error_lines(&self.errors)
                    .into_iter()
                    .map(|line| (RunLogLevel::Warn, line)),
            )
            .collect()
    }
}

/// `<sms>` elements read.
pub(crate) const SMS_SEEN: Counter = Counter::new("sms_seen", "Read 1 SMS", "Read {n} SMS");

/// `<mms>` elements read.
pub(crate) const MMS_SEEN: Counter = Counter::new("mms_seen", "Read 1 MMS", "Read {n} MMS");

/// Drafts and outbox, failed and queued messages skipped.
pub(crate) const SKIPPED_NEVER_SENT: Counter = Counter::new(
    "skipped_never_sent",
    "Skipped 1 draft or message never sent",
    "Skipped {n} drafts or messages never sent",
);

/// MMS skipped because they name no participant.
pub(crate) const SKIPPED_EMPTY_PARTICIPANTS: Counter = Counter::new(
    "skipped_empty_participants",
    "Skipped 1 MMS with no participants",
    "Skipped {n} MMS with no participants",
);

/// Character references left out of kept messages because they are not a
/// character, each message sent as a note.
pub(crate) const DROPPED_CHARACTER_REFERENCES: Counter = Counter::new(
    "dropped_character_references",
    "Left out 1 character reference that is not a character",
    "Left out {n} character references that are not characters",
);

/// Options for [`read_backup`].
#[derive(Debug)]
pub struct ReadOptions<'a> {
    /// Known owner phone numbers (empty triggers inference).
    pub owner_phones: &'a [String],
    /// Directory staged attachments are written to.
    pub attachments_dir: Option<&'a Path>,
    /// Where each attachment payload is written the moment its record is
    /// parsed, so no payload stays in memory; `None` when the run does not
    /// copy attachments, and then no payload is kept at all.
    pub spool: Option<&'a AttachmentSpool>,
    /// Whether attachment files are staged after parse.
    pub files: AttachmentFiles,
    /// Where the read writes its notes and warnings and the staging sends
    /// its progress, and the cancel flag checked between records.
    pub sinks: Sinks<'a>,
}

/// What one message carries beside its [`PendingMessage`]: the fields
/// only SMS Backup & Restore has, its time as the XML gives it, and what the
/// read left out of it. A message's `row` extra names its entry.
#[derive(Debug, Clone)]
struct MessageRow {
    /// Unix seconds, as the parser read them from the `date` attribute.
    timestamp_secs: f64,
    /// SMS or MMS.
    message_kind: MessageKind,
    /// The subject, when the message has one.
    subject: Option<String>,
    /// The `contact_name` attribute as the file writes it.
    contact_name: String,
    /// The `type` (SMS) or `msg_box` (MMS) attribute, when it is a number.
    android_type: Option<i32>,
    /// The vendor fields, written back as the message's `source`.
    source_fields: serde_json::Map<String, serde_json::Value>,
    /// What the read left out of the message.
    left_out: LeftOutCounts,
}

/// The messages the read has collected for one conversation, before
/// [`to_document`].
#[derive(Debug)]
struct ConversationInProgress {
    kind: ConversationKind,
    /// The conversation as the shared projection reads it: each
    /// participant's handle key in `participant_e164s`, the group title in
    /// `display_name`.
    pending: PendingConversation,
    /// Each message's [`MessageRow`], in the order the read added them.
    rows: Vec<MessageRow>,
}

/// The `extra` key of a [`PendingMessage`] that holds its index in
/// [`ConversationInProgress::rows`].
const ROW: &str = "row";

/// The backup file to read: SMS Backup & Restore writes each backup as one
/// `.xml` file, and an Import Run reads one backup.
fn require_backup_file(input: &Path) -> Result<()> {
    if input.is_dir() {
        bail!(
            "Input must be one SMS Backup & Restore .xml file, not a directory: {}",
            input.display()
        );
    }
    if !input.is_file() {
        bail!("input is not a file: {}", input.display());
    }
    Ok(())
}

/// Add the file's parse counts onto the report.
fn merge_stats(report: &mut ReadReport, stats: ParseStats) {
    report.sms_seen += stats.sms_seen;
    report.mms_seen += stats.mms_seen;
    report.skipped_invalid_date += stats.skipped_invalid_date;
    report.skipped_unknown_address += stats.skipped_unknown_address;
    report.skipped_unknown_type += stats.skipped_unknown_type;
    report.skipped_never_sent += stats.skipped_never_sent;
    report.skipped_empty_participants += stats.skipped_empty_participants;
}

/// The queued attachments for a message's decoded parts, each payload
/// written to the spool when there is one. The record, and the decoded
/// bytes with it, are dropped once this returns.
///
/// # Errors
///
/// Returns an error when a payload cannot be written to the spool.
fn queue_attachments(
    attachments: &[AttachmentBytes],
    spool: Option<&AttachmentSpool>,
) -> Result<Vec<PendingAttachment>> {
    attachments
        .iter()
        .map(|attachment| {
            if let Some(spool) = spool {
                spool.put(&attachment.data)?;
            }
            // Neither a path nor the bytes: whoever stages the attachment
            // reads the payload from the spool by its digest.
            Ok(PendingAttachment {
                rel_path: String::new(),
                content_type: attachment.mime_type.clone().unwrap_or_default(),
                digest_sha256: Some(attachment.digest_hex.clone()),
                name_hint: attachment.original_name.clone(),
                size_bytes: Some(attachment.data.len() as u64),
            })
        })
        .collect()
}

/// Every attachment of `documents` paired with the payload [`read_backup`]
/// left in `options.spool`, and counted, so a caller can check the disk for
/// room before it cleans or writes anything (#1743). An attachment the
/// spool does not hold, or every one when the run copies no attachments,
/// has no file and counts for nothing.
pub fn spooled_attachments<'a>(
    documents: &'a mut [ConversationDocument],
    options: &ReadOptions<'_>,
) -> CountedAttachments<'a> {
    let files = if options.spool.is_some() {
        options.files
    } else {
        AttachmentFiles::Skip
    };
    CountedAttachments::new(
        document_messages(documents),
        files,
        PathSources::OnDisk,
        |att| {
            options
                .spool
                .and_then(|spool| spool.source(att))
                .unwrap_or((AttachmentSource::Missing, None))
        },
        options.sinks.log,
    )
}

/// Stage the attachments [`read_backup`] left in `options.spool` into
/// `options.attachments_dir`, reading one spooled file at a time. A caller
/// stages only once the read has succeeded, so a backup the read refuses
/// writes nothing outside the spool.
///
/// # Errors
///
/// Returns an error when an attachment cannot be staged or the run is
/// canceled.
pub fn stage_read_attachments(
    documents: &mut [ConversationDocument],
    options: &ReadOptions<'_>,
) -> Result<()> {
    let attachments_dir = options.attachments_dir.unwrap_or_else(|| Path::new(""));
    spooled_attachments(documents, options).stage(
        attachments_dir,
        load_attachment_source,
        options.sinks,
    )?;
    Ok(())
}

/// The conversation id: `chat-<key>` for groups, else the peer's handle key.
fn chat_id(record: &Record) -> String {
    match record.conversation_kind {
        ConversationKind::Group => format!("chat-{}", record.chat_key),
        ConversationKind::OneToOne => record.chat_key.clone(),
    }
}

/// Append a parsed SMS or MMS to its conversation, creating the
/// conversation on first sight.
fn add_record(
    conversations: &mut BTreeMap<String, ConversationInProgress>,
    record: Record,
    attachments: Vec<PendingAttachment>,
) -> Result<()> {
    let id = chat_id(&record);
    let conversation = conversations.entry(id).or_insert_with_key(|id| {
        let peers = record
            .participants
            .iter()
            .map(|(h, _)| h.key().to_string())
            .collect();
        ConversationInProgress {
            kind: record.conversation_kind,
            pending: PendingConversation::new(
                id.as_str(),
                record.conversation_kind == ConversationKind::Group,
                record.group_title.clone(),
                peers,
            ),
            rows: Vec::new(),
        }
    });
    let source_fields = serde_json::to_value(&record.source_fields)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    let extra = BTreeMap::from([
        (ROW.to_string(), conversation.rows.len().to_string()),
        // Read by the shared `ir::message_time`.
        ("date_ms".to_string(), record.date_ms),
    ]);
    conversation.rows.push(MessageRow {
        timestamp_secs: record.timestamp_secs,
        message_kind: MessageKind::parse(record.message_kind),
        subject: (!record.subject.is_empty()).then_some(record.subject),
        contact_name: record.contact_name,
        android_type: record.android_type.trim().parse().ok(),
        source_fields,
        left_out: LeftOutCounts {
            unreadable_parts: record.unreadable_parts,
            dropped_character_references: record.dropped_character_references,
        },
    });
    conversation.pending.messages.push(PendingMessage {
        // Whole seconds, for the shared check that the time can be
        // represented; the order is the parser's own seconds
        // (`SbrProjection::message_order`) and the time the `date_ms`
        // extra (`ir::message_time`).
        sort_key: record.timestamp_secs as i64,
        is_from_me: record.is_from_me,
        sender_identity: record.sender.map(Handle::into_key).unwrap_or_default(),
        sender_display_name: record.sender_display_name,
        text: record.text,
        attachments,
        extra,
    });
    Ok(())
}

/// SMS Backup & Restore deltas of the shared [`project_pending`] projection,
/// for one conversation.
struct SbrProjection<'a> {
    export: &'a ExportMeta,
    /// When the file was made: every message's Backup Date.
    backup_taken_at_unix_ms: Option<i64>,
    kind: ConversationKind,
    rows: &'a [MessageRow],
}

impl SbrProjection<'_> {
    /// The [`MessageRow`] of one of the conversation's messages.
    fn row(&self, message: &PendingMessage) -> &MessageRow {
        let index: usize = message
            .extra_str(ROW)
            .parse()
            .expect("every message the read adds names its row");
        &self.rows[index]
    }

    /// Display names seen per sender handle across the conversation.
    fn names_by_handle(&self, conversation: &PendingConversation) -> HashMap<String, String> {
        let mut names = HashMap::new();
        for message in &conversation.messages {
            if let Some(name) = message
                .sender_display_name
                .as_deref()
                .and_then(ir::trimmed)
                .filter(|_| !message.sender_identity.is_empty())
            {
                names
                    .entry(message.sender_identity.clone())
                    .or_insert_with(|| name.to_string());
            }
            if let Some(name) = sbr::contact_name(&self.row(message).contact_name, self.kind) {
                for peer in &conversation.participant_e164s {
                    names
                        .entry(peer.clone())
                        .or_insert_with(|| name.to_string());
                }
            }
        }
        names
    }
}

impl ProjectionHooks for SbrProjection<'_> {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn backup_taken_at_unix_ms(&self) -> Option<i64> {
        self.backup_taken_at_unix_ms
    }

    fn service(&self, _message: &PendingMessage) -> Transport {
        Transport::Sms
    }

    fn vendor_fields(
        &self,
        _conversation: &PendingConversation,
        message: &PendingMessage,
    ) -> VendorFields {
        let row = self.row(message);
        VendorFields {
            android_type: row.android_type,
            fields: row.source_fields.clone(),
        }
    }

    fn subject(&self, message: &PendingMessage) -> Option<String> {
        self.row(message).subject.clone()
    }

    fn message_kind(&self, message: &PendingMessage) -> MessageKind {
        self.row(message).message_kind
    }

    fn message_order(&self, a: &PendingMessage, b: &PendingMessage) -> std::cmp::Ordering {
        self.row(a)
            .timestamp_secs
            .total_cmp(&self.row(b).timestamp_secs)
    }

    fn participants(&self, _chat_id: &str, conversation: &PendingConversation) -> Vec<Participant> {
        let names = self.names_by_handle(conversation);
        conversation
            .participant_e164s
            .iter()
            .map(|handle| Participant {
                identity: Some(handle.clone()),
                display_name: names.get(handle).cloned(),
                country: None,
            })
            .collect()
    }

    fn group_title(&self, conversation: &PendingConversation) -> Option<String> {
        conversation.display_name.clone()
    }
}

/// Project one conversation in progress into a document whose messages
/// carry the file's Backup Date `backup_taken_at_unix_ms`, or `None` when no
/// message has a time that can be represented, and fold its counts into the
/// report.
fn to_document(
    file_path: &str,
    conversation: ConversationInProgress,
    export: &ExportMeta,
    backup_taken_at_unix_ms: Option<i64>,
    report: &mut ReadReport,
) -> Option<ConversationDocument> {
    let ConversationInProgress {
        kind,
        mut pending,
        rows,
    } = conversation;
    let hooks = SbrProjection {
        export,
        backup_taken_at_unix_ms,
        kind,
        rows: &rows,
    };
    let (keep, skipped) =
        prepare_conversation(&mut pending, |a, b| hooks.message_order(a, b), |secs| secs);
    report.skipped_invalid_date += skipped;
    if !keep {
        return None;
    }
    let Projection {
        mut document,
        tally,
        kept,
    } = project_pending(&pending.chat_id, &pending, &hooks);
    report.sent += tally.sent;
    report.received += tally.received;
    report.duplicates_dropped += tally.duplicates;
    // Named only now that the dedupe step has run, because a repeated copy
    // dropped there can change which time the message keeps.
    for (message, index) in document.messages.iter().zip(kept) {
        let row = hooks.row(&pending.messages[index]);
        if !row.left_out.is_empty() {
            report.left_out.push(LeftOut {
                file: file_path.to_string(),
                message: describe(message, &row.source_fields, &pending),
                counts: row.left_out,
            });
        }
    }
    document.finalize_stats();
    Some(document)
}

/// A message as a person can find it in its file: `message of <UTC time>
/// with <address>`, the `address` attribute as the file writes it, so a
/// search of the file finds it. An MMS with no `address` attribute names
/// every identity in its conversation instead.
fn describe(
    message: &Message,
    source_fields: &serde_json::Map<String, serde_json::Value>,
    conversation: &PendingConversation,
) -> String {
    let ms = message.timestamp_unix_ms;
    let time =
        format_local_ts(ms.div_euclid(1000)).map_or_else(|| ms.to_string(), |(_, utc, _)| utc);
    let address = source_fields
        .get("attrs")
        .and_then(|attrs| attrs.get("address"))
        .and_then(serde_json::Value::as_str)
        .filter(|address| !address.trim().is_empty());
    let with = address.map_or_else(|| conversation.participant_e164s.join(", "), str::to_string);
    format!("message of {time} with {with}")
}

/// Parse one SMS Backup & Restore XML file into conversation documents.
///
/// Drops duplicate messages. Each attachment's payload is left in
/// `options.spool` for [`stage_read_attachments`] or the write queue to
/// stage. Every conversation is dated by the file's `backup_date`, or by
/// its modification time when it has none.
///
/// # Errors
///
/// Returns an error when `input` is not a file, is not valid XML, or a
/// payload cannot be written to the spool, and
/// [`Canceled`](message_crate_core::Canceled) when the run
/// is canceled. A file that is not valid XML is refused whole, in the
/// owner-phone pass and the message pass alike; a message that cannot be
/// kept in a valid file is counted and the read goes on.
pub fn read_backup(
    input: &Path,
    options: ReadOptions<'_>,
) -> Result<(Vec<ConversationDocument>, ReadReport)> {
    require_backup_file(input)?;
    let mut owner_phones = options.owner_phones.to_vec();
    if owner_phones.is_empty() {
        owner_phones = infer_owner_phones(input)?;
        owner_phones.sort();
        owner_phones.dedup();
    }
    let owners = if owner_phones.is_empty() {
        None
    } else {
        Some(OwnerHandleSet::from_phones(&owner_phones)?)
    };
    // from_phones guarantees at least one phone handle in the set.
    let owner_identity = owners
        .as_ref()
        .and_then(OwnerHandleSet::primary_owner_handle);
    let mut report = ReadReport::default();
    let mut conversations = BTreeMap::new();
    let file_path = input.display().to_string();
    check_cancel(options.sinks.cancel)?;
    // Each record's attachment payloads go to the spool as the record is
    // parsed; staging waits until every conversation is built.
    let mut stats = ParseStats::default();
    // A spool that cannot be written stops the read rather than counting
    // as an error in the file.
    let mut spool_error = None;
    let parse_result = parse_file_with(input, owners.as_ref(), &mut stats, |record| {
        check_cancel(options.sinks.cancel)?;
        let attachments = match queue_attachments(&record.attachments, options.spool) {
            Ok(attachments) => attachments,
            Err(error) => {
                let stop = anyhow::anyhow!("{error:#}");
                spool_error = Some(error);
                return Err(stop);
            }
        };
        match add_record(&mut conversations, record, attachments) {
            Ok(()) => Ok(()),
            Err(error) => {
                // Keep parsing the rest of the file; one bad record
                // must not abort the whole backup.
                report.errors.push(ReadError::new(input, &error));
                Ok(())
            }
        }
    });
    merge_stats(&mut report, stats);
    // When the backup was made: its `backup_date`, or the file's
    // modification time when it has none.
    let backup_taken_at_unix_ms = stats
        .backup_date_unix_ms
        .or_else(|| message_crate_core::file_modified_unix_ms(input));
    if let Some(error) = spool_error {
        return Err(error);
    }
    // A file that is not valid XML is refused whole, and so is a parse a
    // cancel stopped (#2028).
    parse_result?;
    check_cancel(options.sinks.cancel)?;
    let export = ExportMeta {
        source: EXPORT_SOURCE.into(),
        tool: EXPORT_TOOL.into(),
        tool_version: EXPORT_TOOL_VERSION.into(),
        owner_identity,
        owner_display_name: None,
        exported_at_unix_ms: None,
    };
    let mut documents = Vec::new();
    for conversation in conversations.into_values() {
        if let Some(document) = to_document(
            &file_path,
            conversation,
            &export,
            backup_taken_at_unix_ms,
            &mut report,
        ) {
            documents.push(document);
            report.conversations += 1;
        }
    }
    Ok((documents, report))
}

#[cfg(test)]
mod tests;
