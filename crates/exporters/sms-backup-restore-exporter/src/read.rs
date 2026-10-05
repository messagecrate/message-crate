//! Read SMS Backup & Restore XML into [`ConversationDocument`] values.

use anyhow::{Result, bail};
use media::{CompressOptions, MediaMode};
use message_crate_core::{
    CancelFlag, LogSink, MediaConfig, ProgressSink, check_cancel, discover_files,
    document_messages, is_cancelled,
};
use message_csv::format_local_ts;
use message_ir::{
    ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, HandleType,
    IrAttachment, IrConversationType, IrDirection, IrMessage, IrMessageKind, IrParticipant,
    IrService, IrSource, MessageCopy, MessageGuid, MessageIdentity, SCHEMA_VERSION, TimePrecision,
    one_copy_per_message, owner_sender,
};
use message_staging::{
    AttachmentSource, AttachmentSpool, CountedAttachments, PathSources, load_attachment_source,
};
use phone::{Handle, OwnerHandleSet};
use sbr::{
    AttachmentBlob, ConversationKind, ParseStats, Record, infer_owner_phones, parse_file_with,
};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = "sms-backup-restore";
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
    /// Messages dropped outside the configured date range.
    pub skipped_out_of_range: u64,
    /// Messages dropped with no usable address.
    pub skipped_unknown_address: u64,
    /// SMS dropped for an unknown `type`.
    pub skipped_unknown_type: u64,
    /// Draft/outbox/failed/queued messages dropped.
    pub skipped_draft_or_outbox: u64,
    /// MMS dropped with no participants.
    pub skipped_empty_participants: u64,
    /// Parts with undecodable base64.
    pub skipped_unreadable_part: u64,
    /// Character references dropped because they are not a character.
    pub dropped_character_references: u64,
    /// Repeated copies of a message dropped, one copy of each kept.
    pub duplicates_dropped: u64,
    /// What could not be read, each with the file it was in.
    pub errors: Vec<ReadError>,
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
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file, self.reason)
    }
}

impl ReadReport {
    /// One log line for each kind of message the read dropped or skipped,
    /// leaving out the kinds it found none of, then one line for every
    /// error, so a person can tell what did not come across.
    pub fn log_lines(&self) -> Vec<String> {
        let counts = [
            (
                self.duplicates_dropped,
                "Dropped",
                "repeated copy of a message",
                "repeated copies of messages",
            ),
            (
                self.skipped_invalid_date,
                "Skipped",
                "message with an invalid date",
                "messages with an invalid date",
            ),
            (
                self.skipped_out_of_range,
                "Skipped",
                "message outside the date range",
                "messages outside the date range",
            ),
            (
                self.skipped_unknown_address,
                "Skipped",
                "message with no usable address",
                "messages with no usable address",
            ),
            (
                self.skipped_unknown_type,
                "Skipped",
                "message of an unknown type",
                "messages of an unknown type",
            ),
            (
                self.skipped_draft_or_outbox,
                "Skipped",
                "draft or unsent message",
                "drafts or unsent messages",
            ),
            (
                self.skipped_empty_participants,
                "Skipped",
                "MMS with no participants",
                "MMS with no participants",
            ),
            (
                self.skipped_unreadable_part,
                "Skipped",
                "message part that could not be read",
                "message parts that could not be read",
            ),
            (
                self.dropped_character_references,
                "Dropped",
                "character reference that is not a character",
                "character references that are not characters",
            ),
        ];
        counts
            .into_iter()
            .filter(|(count, ..)| *count > 0)
            .map(|(count, verb, one, many)| {
                let what = if count == 1 { one } else { many };
                format!("{verb} {count} {what}")
            })
            .chain(
                self.errors
                    .iter()
                    .map(|error| format!("xml warning: {error}")),
            )
            .collect()
    }
}

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
    /// A directory under the input whose files the read leaves out: the run's
    /// output when it sits inside the backup's directory, so a backup or an
    /// attachment an earlier run wrote there is never read back in as input.
    pub exclude_dir: Option<&'a Path>,
    /// How to write attachment files after parse.
    pub media: MediaMode,
    /// Image/video compress settings used when `media` converts or compresses.
    pub compress: CompressOptions,
    /// Human-readable notes and warnings while reading.
    pub log: Option<&'a LogSink>,
    /// Typed progress events while staging attachments.
    pub progress: Option<&'a ProgressSink>,
    /// Cancellation flag checked between files.
    pub cancel: Option<&'a CancelFlag>,
}

#[derive(Debug, Clone)]
struct PendingAttachment {
    original_name: Option<String>,
    mime_type: Option<String>,
    digest: String,
    size_bytes: u64,
}

#[derive(Debug, Clone)]
struct PendingMessage {
    sort_key: f64,
    is_from_me: bool,
    /// The sender's handle key, for an incoming message.
    sender: Option<String>,
    sender_display_name: Option<String>,
    text: String,
    subject: String,
    attachments: Vec<PendingAttachment>,
    message_kind: &'static str,
    date_ms: String,
    contact_name: String,
    android_type: String,
    source_fields: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Default)]
struct PendingConversation {
    kind: ConversationKind,
    group_title: Option<String>,
    /// Each participant's handle key and kind.
    participants: Vec<(String, HandleType)>,
    messages: Vec<PendingMessage>,
}

/// The XML files to read: the file itself, or every `.xml` under the directory
/// outside `exclude_dir`.
fn collect_xml_paths(input: &Path, exclude_dir: Option<&Path>) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        return Ok(vec![input.to_path_buf()]);
    }
    if !input.is_dir() {
        bail!("input is not a file or directory: {}", input.display());
    }
    let mut paths = discover_files(input, &|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
            && !exclude_dir.is_some_and(|dir| p.starts_with(dir))
    })?;
    paths.sort();
    if paths.is_empty() {
        bail!("no .xml files found in {}", input.display());
    }
    Ok(paths)
}

/// Add one file's parse counts onto the report.
fn merge_stats(report: &mut ReadReport, stats: ParseStats) {
    report.sms_seen += stats.sms_seen;
    report.mms_seen += stats.mms_seen;
    report.skipped_invalid_date += stats.skipped_invalid_date;
    report.skipped_unknown_address += stats.skipped_unknown_address;
    report.skipped_unknown_type += stats.skipped_unknown_type;
    report.skipped_draft_or_outbox += stats.skipped_draft_or_outbox;
    report.skipped_empty_participants += stats.skipped_empty_participants;
    report.skipped_unreadable_part += stats.skipped_unreadable_part;
    report.dropped_character_references += stats.dropped_character_references;
}

/// Pending attachments for a message's decoded parts, each payload written
/// to the spool when there is one. The record, and the decoded bytes with
/// it, are dropped once this returns.
///
/// # Errors
///
/// Returns an error when a payload cannot be written to the spool.
fn queue_attachments(
    blobs: &[AttachmentBlob],
    spool: Option<&AttachmentSpool>,
) -> Result<Vec<PendingAttachment>> {
    blobs
        .iter()
        .map(|blob| {
            if let Some(spool) = spool {
                spool.put(&blob.data)?;
            }
            Ok(PendingAttachment {
                original_name: blob.original_name.clone(),
                mime_type: blob.mime_type.clone(),
                digest: blob.digest_hex.clone(),
                size_bytes: blob.data.len() as u64,
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
    let mode = if options.spool.is_some() {
        options.media
    } else {
        MediaMode::Disabled
    };
    CountedAttachments::new(
        document_messages(documents),
        MediaConfig {
            mode,
            compress: options.compress.clone(),
        },
        PathSources::OnDisk,
        |att| {
            options
                .spool
                .and_then(|spool| spool.source(att))
                .unwrap_or((AttachmentSource::Missing, None))
        },
        options.log,
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
/// cancelled.
pub fn stage_read_attachments(
    documents: &mut [ConversationDocument],
    options: &ReadOptions<'_>,
) -> Result<()> {
    let attachments_dir = options.attachments_dir.unwrap_or_else(|| Path::new(""));
    spooled_attachments(documents, options)
        .stage(
            attachments_dir,
            load_attachment_source,
            options.log,
            options.progress,
            options.cancel,
        )
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// The conversation id: `chat-<key>` for groups, else the peer's handle key.
fn chat_id(record: &Record) -> String {
    match record.conversation_kind {
        ConversationKind::Group => format!("chat-{}", record.chat_key),
        ConversationKind::Individual => record.chat_key.clone(),
    }
}

/// Append a parsed SMS or MMS to its conversation, creating the conversation on first sight.
fn add_record(
    conversations: &mut BTreeMap<String, PendingConversation>,
    record: Record,
    attachments: Vec<PendingAttachment>,
) -> Result<()> {
    let id = chat_id(&record);
    let peers = record
        .participants
        .iter()
        .map(|(h, _)| (h.key().to_string(), h.kind()))
        .collect();
    let conversation = conversations
        .entry(id)
        .or_insert_with(|| PendingConversation {
            kind: record.conversation_kind,
            group_title: record.group_title.clone(),
            participants: peers,
            messages: Vec::new(),
        });
    let sender = record.sender.map(Handle::into_key);
    let source_fields = serde_json::to_value(&record.source_fields)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    conversation.messages.push(PendingMessage {
        sort_key: record.timestamp_secs,
        is_from_me: record.is_from_me,
        sender,
        sender_display_name: record.sender_display_name,
        text: record.text,
        subject: record.subject,
        attachments,
        message_kind: record.message_kind,
        date_ms: record.date_ms,
        contact_name: record.contact_name,
        android_type: record.android_type,
        source_fields,
    });
    Ok(())
}

impl PendingMessage {
    /// The UTC instant in milliseconds and how finely the XML recorded it:
    /// the `date` attribute's milliseconds, else the sort key's whole second.
    fn time(&self) -> (i64, TimePrecision) {
        match self.date_ms.trim().parse::<i64>() {
            Ok(ms) => (ms, TimePrecision::Milliseconds),
            Err(_) => (
                (self.sort_key as i64).saturating_mul(1000),
                TimePrecision::Seconds,
            ),
        }
    }

    /// Digests of the attachments, for the message's identity.
    fn attachment_digests(&self) -> Vec<String> {
        self.attachments.iter().map(|a| a.digest.clone()).collect()
    }
}

/// Sort by time and keep one copy of each message
/// ([`one_copy_per_message`]), each with the time it keeps. Returns how many
/// copies it dropped.
fn dedupe(messages: &mut Vec<PendingMessage>) -> u64 {
    messages.sort_by(|a, b| a.sort_key.total_cmp(&b.sort_key));
    let prepared: Vec<_> = messages
        .iter()
        .map(|m| (m.time(), m.attachment_digests()))
        .collect();
    let copies: Vec<MessageCopy<'_>> = messages
        .iter()
        .zip(&prepared)
        .map(|(m, ((ms, precision), digests))| MessageCopy {
            is_from_me: m.is_from_me,
            sender: m.sender.as_deref(),
            timestamp_unix_ms: *ms,
            precision: *precision,
            text: &m.text,
            attachment_digests: digests,
            vendor_key: None,
        })
        .collect();
    let kept = one_copy_per_message(&copies);
    let before = messages.len();
    let mut kept = kept.into_iter();
    messages.retain_mut(|m| match kept.next().flatten() {
        Some(ms) => {
            if m.time().0 != ms {
                m.date_ms = ms.to_string();
            }
            true
        }
        None => false,
    });
    (before - messages.len()) as u64
}

/// Display names seen per sender handle across the conversation.
fn names_by_handle(conversation: &PendingConversation) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for message in &conversation.messages {
        if let (Some(sender), Some(name)) = (
            &message.sender,
            message
                .sender_display_name
                .as_deref()
                .and_then(message_ir::trimmed),
        ) {
            names
                .entry(sender.clone())
                .or_insert_with(|| name.to_string());
        }
        if let Some(name) = sbr::contact_name(&message.contact_name, conversation.kind) {
            for (peer, _) in &conversation.participants {
                names
                    .entry(peer.clone())
                    .or_insert_with(|| name.to_string());
            }
        }
    }
    names
}

/// Project one pending conversation into a document and fold its counts into the report.
fn to_document(
    id: &str,
    conversation: &PendingConversation,
    owner_identity: Option<&str>,
    report: &mut ReadReport,
) -> ConversationDocument {
    let export = ExportMeta {
        source: EXPORT_SOURCE.into(),
        tool: EXPORT_TOOL.into(),
        tool_version: EXPORT_TOOL_VERSION.into(),
        owner_identity: owner_identity.map(str::to_string),
        owner_display_name: None,
    };
    let owner = owner_sender(&export);
    let messages = conversation
        .messages
        .iter()
        .map(|message| {
            if message.is_from_me {
                report.sent += 1;
            } else {
                report.received += 1;
            }
            ir_message(id, message, &owner)
        })
        .collect();
    let mut document = ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export,
        conversation: ConversationMeta {
            chat_identifier: id.into(),
            conversation_type: match conversation.kind {
                ConversationKind::Individual => IrConversationType::Individual,
                ConversationKind::Group => IrConversationType::Group,
            },
            group_title: conversation.group_title.clone(),
            participants: ir_participants(conversation),
            stats: ConversationStats::default(),
        },
        messages,
        packaging_stem_suffix: None,
    };
    document.finalize_stats();
    document
}

/// The IR message for one pending message. `owner` (handle, display name)
/// stands in as the sender of anything sent from this phone; the timestamp
/// is the record's own milliseconds when it parses, else the sort key.
fn ir_message(
    chat_id: &str,
    message: &PendingMessage,
    owner: &(Option<String>, Option<String>),
) -> IrMessage {
    let (timestamp_unix_ms, _) = message.time();
    let digests = message.attachment_digests();
    let (sender_identity, sender_display_name) = if message.is_from_me {
        owner.clone()
    } else {
        (message.sender.clone(), message.sender_display_name.clone())
    };
    IrMessage {
        guid: MessageGuid::new(&MessageIdentity {
            chat: chat_id,
            is_from_me: message.is_from_me,
            sender: message.sender.as_deref(),
            timestamp_unix_ms,
            text: &message.text,
            attachment_digests: &digests,
            vendor_key: None,
        })
        .into_string(),
        timestamp_unix_ms,
        direction: if message.is_from_me {
            IrDirection::Outgoing
        } else {
            IrDirection::Incoming
        },
        service: IrService::Sms,
        message_kind: IrMessageKind::parse(message.message_kind),
        sender_identity,
        sender_display_name,
        owner_identity: None,
        subject: (!message.subject.is_empty()).then(|| message.subject.clone()),
        text: message.text.clone(),
        attachments: message.attachments.iter().map(ir_attachment).collect(),
        reactions: Vec::new(),
        deletion: None,
        edits: Vec::new(),
        imessage: None,
        source: IrSource {
            android_type: message.android_type.trim().parse().ok(),
            fields: message.source_fields.clone(),
        }
        .into_option(),
    }
}

/// The IR attachment for one pending attachment. Neither a path nor the
/// bytes: whoever stages it reads the payload from the spool by its digest.
fn ir_attachment(a: &PendingAttachment) -> IrAttachment {
    IrAttachment {
        path: None,
        original_name: a.original_name.clone(),
        mime_type: a.mime_type.clone(),
        digest_sha256: (!a.digest.is_empty()).then(|| a.digest.clone()),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: Some(a.size_bytes),
        missing_reason: None,
        bytes: None,
    }
}

/// Every participant with the kind of address it is, named when the XML
/// named it.
fn ir_participants(conversation: &PendingConversation) -> Vec<IrParticipant> {
    let names = names_by_handle(conversation);
    conversation
        .participants
        .iter()
        .map(|(handle, kind)| IrParticipant {
            identity: Some(handle.clone()),
            display_name: names.get(handle).cloned(),
            identity_type: Some(*kind),
        })
        .collect()
}

/// Parse SMS Backup & Restore XML into conversation documents.
///
/// Drops duplicate messages. Each attachment's payload is left in
/// `options.spool` for [`stage_read_attachments`] or the write queue to
/// stage.
///
/// # Errors
///
/// Returns an error when no XML files are found, owner phones cannot be
/// inferred, or a file cannot be parsed.
pub fn read_backup(
    input: &Path,
    options: ReadOptions<'_>,
) -> Result<(Vec<ConversationDocument>, ReadReport)> {
    let paths = collect_xml_paths(input, options.exclude_dir)?;
    let mut owner_phones = options.owner_phones.to_vec();
    if owner_phones.is_empty() {
        // Owner inference is best-effort: the main pass below already reports
        // per-file parse errors, so one malformed file must not abort the whole
        // export. Only give up when no file could be parsed at all.
        let mut parse_errors = Vec::new();
        for path in &paths {
            match infer_owner_phones(path) {
                Ok(phones) => owner_phones.extend(phones),
                Err(error) => parse_errors.push(format!("{}: {error:#}", path.display())),
            }
        }
        if owner_phones.is_empty() && !parse_errors.is_empty() && parse_errors.len() == paths.len()
        {
            bail!(
                "could not infer owner phones from any input file ({}), and none were supplied",
                parse_errors.join("; ")
            );
        }
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
    for path in paths {
        check_cancel(options.cancel)?;
        // Each record's attachment payloads go to the spool as the record is
        // parsed; staging waits until every conversation is built. Messages
        // that parse before an XML error are kept; stats are merged even
        // when the file is truncated.
        let mut stats = ParseStats::default();
        // A spool that cannot be written stops the read rather than counting
        // as an error in one file.
        let mut spool_error = None;
        let parse_result = parse_file_with(&path, owners.as_ref(), &mut stats, |record| {
            check_cancel(options.cancel)?;
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
                    report.errors.push(ReadError::new(&path, &error));
                    Ok(())
                }
            }
        });
        merge_stats(&mut report, stats);
        if let Some(error) = spool_error {
            return Err(error);
        }
        if let Err(error) = parse_result {
            if is_cancelled(options.cancel) || error.to_string() == "cancelled" {
                return Err(error);
            }
            report.errors.push(ReadError::new(&path, &error));
        }
    }
    check_cancel(options.cancel)?;
    let mut documents = Vec::new();
    for (id, mut conversation) in conversations {
        report.duplicates_dropped += dedupe(&mut conversation.messages);
        conversation.messages.retain(|message| {
            let valid = format_local_ts(message.sort_key as i64).is_some();
            if !valid {
                report.skipped_invalid_date += 1;
            }
            valid
        });
        if conversation.messages.is_empty() {
            continue;
        }
        documents.push(to_document(
            &id,
            &conversation,
            owner_identity.as_deref(),
            &mut report,
        ));
        report.conversations += 1;
    }
    Ok((documents, report))
}

#[cfg(test)]
mod tests;
