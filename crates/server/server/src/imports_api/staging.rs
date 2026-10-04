//! Stage message-ir JSONL rows into the temporary import tables.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use message_ir::{HandleService, HandleType, nonempty, trimmed};
use sqlx::SqliteConnection;

use crate::assets_api::{self, AssetError, AssetStats, StoredAsset};
use crate::config::validate_source_id;
use crate::db::handles::{
    HandleIdCache, handle_type_of, upsert_handle_row, upsert_handle_row_cached,
};
use crate::db::staging::{
    self as db_staging, StagingAttachment, StagingConversation, StagingMessage, StagingTapback,
};
use crate::import_media;
use crate::jsonl::{self, ReadRecordsError};
use crate::models::{
    AttachmentRecord, ConversationRecord, ExportRecord, MessageRecord, TapbackRecord, clean_body,
};
use media::MediaMode;

use super::contact_name::{
    IncomingSender, count_other_identity, ensure_contact_for_handle, is_account_identity,
    resolve_incoming_sender_handle,
};
use super::{ImportFailure, ImportOptions, ImportStats};

/// Why staging a file stopped: a refusal the sender can fix by changing the
/// file, or a fault of the server.
///
/// Only the line that finds the problem decides which, by the variant it
/// builds; nothing later looks inside an error to sort it.
#[derive(Debug, thiserror::Error)]
pub(crate) enum StagingError {
    /// A line of the file breaks a rule the sender can fix.
    #[error(transparent)]
    Rejected(#[from] ImportFailure),
    /// I/O, the asset store, or the database: nothing the sender can change.
    #[error(transparent)]
    Internal(anyhow::Error),
}

impl From<ReadRecordsError> for StagingError {
    fn from(err: ReadRecordsError) -> Self {
        match err {
            ReadRecordsError::Rejected { failure, .. } => Self::Rejected(failure),
            err @ ReadRecordsError::Unreadable { .. } => Self::Internal(err.into()),
        }
    }
}

struct PreparedAttachment {
    record: AttachmentRecord,
    stored: Option<StoredAsset>,
}

/// Size on disk of a stored blob, or `None` when it is not there.
fn stored_size_bytes(assets_dir: &Path, assets_path: Option<&str>) -> Option<i64> {
    let rel = assets_path?;
    let meta = std::fs::metadata(assets_dir.join(rel)).ok()?;
    Some(meta.len() as i64)
}

/// The file an attachment path names inside `export_dir`, refusing a path
/// that could leave it, for the message on `line`.
fn safe_source(export_dir: &Path, rel: &str, line: usize) -> Result<PathBuf, ImportFailure> {
    message_ir::safe_attachment_path(export_dir, rel)
        .map_err(|refusal| ImportFailure::UnsafeAttachmentPath { refusal, line })
}

/// Convert/compress when requested; `None` means fall through to claimed-sha / path store.
fn try_store_converted(
    att: &mut AttachmentRecord,
    export_dir: &Path,
    assets_dir: &Path,
    asset_stats: &mut AssetStats,
    media: MediaMode,
    media_work: &Path,
    line: usize,
) -> Result<Option<StoredAsset>, StagingError> {
    if !matches!(media, MediaMode::Convert | MediaMode::Compress) {
        return Ok(None);
    }
    // A blank path converts nothing. `store_claimed_or_path`, which runs
    // next, refuses it through `safe_source`.
    let Some(rel) = att.path.as_deref().and_then(trimmed) else {
        return Ok(None);
    };
    let source = safe_source(export_dir, rel, line)?;
    if !source.is_file() {
        return Ok(None);
    }
    let Some(resolved) =
        import_media::resolve_for_store(&source, att.mime_type.as_deref(), media, media_work)
            .map_err(StagingError::Internal)?
    else {
        return Ok(None);
    };
    // Bytes may have changed; drop any claimed SHA-256 fingerprint from the export.
    att.sha256 = None;
    att.mime_type = resolved.mime_type.or(att.mime_type.take());
    assets_api::hash_and_store(
        &resolved.path,
        assets_dir,
        att.mime_type.as_deref(),
        asset_stats,
    )
    .map_err(StagingError::Internal)
}

/// Store an attachment by the sha256 the export claims (reusing an existing blob) or by
/// hashing its file, counting the ones whose file is missing.
fn store_claimed_or_path(
    att: &AttachmentRecord,
    export_dir: &Path,
    assets_dir: &Path,
    asset_stats: &mut AssetStats,
    line: usize,
) -> Result<Option<StoredAsset>, StagingError> {
    // Checked before the stored-fingerprint lookup, which never reads the
    // file: `attachments.path` keeps the path as sent, and an Export writes
    // the file there, so a path the check refuses is never stored. A path of
    // spaces is checked too, and refused as `.` is.
    let safe_path = att
        .path
        .as_deref()
        .map(|rel| safe_source(export_dir, rel, line))
        .transpose()?;
    if let Some(sha) = att.sha256.as_deref().and_then(trimmed) {
        let claimed = assets_api::Sha256::parse(sha);
        if let Ok(claimed) = &claimed
            && let Some(found) = assets_api::lookup_by_sha256(assets_dir, claimed)
        {
            asset_stats.deduped += 1;
            return Ok(Some(StoredAsset {
                mime_type: att.mime_type.clone().or(found.mime_type),
                ..found
            }));
        }
        if let Some(source) = safe_path {
            let claimed = match claimed {
                Ok(claimed) => claimed,
                Err(_) if !source.is_file() => {
                    asset_stats.missing += 1;
                    return Ok(None);
                }
                // A stated fingerprint that is not one: the sender's to fix,
                // naming the line and the path as sent.
                Err(_) => {
                    return Err(ImportFailure::AttachmentSha256Invalid {
                        path: att.path.clone().unwrap_or_default(),
                        stated: sha.to_string(),
                        line,
                    }
                    .into());
                }
            };
            return match assets_api::store_verified(
                &source,
                &claimed,
                assets_dir,
                att.mime_type.as_deref(),
                false,
                false,
            ) {
                Ok((stored, already)) => {
                    if already {
                        asset_stats.deduped += 1;
                    } else {
                        asset_stats.copied += 1;
                    }
                    Ok(Some(stored))
                }
                Err(_) if !source.is_file() => {
                    asset_stats.missing += 1;
                    Ok(None)
                }
                // The export states a fingerprint its file does not have:
                // the sender's to fix, naming the line and the path as sent.
                Err(AssetError::Mismatch { claimed, actual }) => {
                    Err(ImportFailure::AttachmentMismatch {
                        path: att.path.clone().unwrap_or_default(),
                        stated: claimed,
                        actual,
                        line,
                    }
                    .into())
                }
                Err(err) => Err(StagingError::Internal(err.into())),
            };
        }
        asset_stats.missing += 1;
        return Ok(None);
    }

    if let Some(source) = safe_path {
        return assets_api::hash_and_store(
            &source,
            assets_dir,
            att.mime_type.as_deref(),
            asset_stats,
        )
        .map_err(StagingError::Internal);
    }
    asset_stats.missing += 1;
    Ok(None)
}

/// Stage every attachment of one message into the asset store, converting first when the media mode asks for it.
fn prepare_attachments(
    export_dir: &Path,
    assets_dir: &Path,
    attachments: Vec<AttachmentRecord>,
    asset_stats: &mut AssetStats,
    media: MediaMode,
    media_work: &Path,
    line: usize,
) -> Result<Vec<PreparedAttachment>, StagingError> {
    if media == MediaMode::Disabled {
        return Ok(Vec::new());
    }

    let mut prepared = Vec::with_capacity(attachments.len());
    for mut att in attachments {
        let stored = match try_store_converted(
            &mut att,
            export_dir,
            assets_dir,
            asset_stats,
            media,
            media_work,
            line,
        )? {
            Some(stored) => Some(stored),
            None => store_claimed_or_path(&att, export_dir, assets_dir, asset_stats, line)?,
        };
        prepared.push(PreparedAttachment {
            record: att,
            stored,
        });
    }
    Ok(prepared)
}

/// Per-import insert state. Message / attachment / tapback rows flush in
/// multi-row chunks. Handle ids are remembered so the same sender is not
/// looked up on every message.
pub(super) struct StagingInserts {
    account_id: i64,
    import_id: Option<i64>,
    handles: HandleIdCache,
    /// Handle ids of the account holder's own addresses met in this run, by
    /// address and platform. Apart from `handles`, whose entries each have a
    /// contact: an owner's handle never gets one (ADR-0015).
    owners: HashMap<(String, String), i64>,
    /// The account's identities, as `(normalized address, handle type)`.
    /// A participant at one of them is the holder, who is never a
    /// participant (ADR-0015, #1093).
    identities: HashSet<(String, HandleType)>,
}

impl StagingInserts {
    /// Fresh insert state for one import run, with the identities the account
    /// holds when the run starts.
    pub(super) fn new(
        account_id: i64,
        import_id: Option<i64>,
        identities: HashSet<(String, HandleType)>,
    ) -> Self {
        Self {
            account_id,
            import_id,
            handles: HandleIdCache::new(),
            owners: HashMap::new(),
            identities,
        }
    }
}

/// One participant as the conversation header records it: handle (the name,
/// typed `Other`, for a person named with no address), the name this backup
/// used for them, and the handle type when the source said.
type StagedParticipant = (String, Option<String>, Option<HandleType>);

/// The source id for a conversation: its header's `export.source` when sources come from the files, else the fixed override.
///
/// # Errors
///
/// Refuses a header, when sources come from the files, whose
/// `export.source` is missing or is not a valid source id: the sender's to
/// fix in the file.
fn resolve_conversation_source(
    opts: &ImportOptions<'_>,
    conversation: &ConversationRecord,
) -> Result<String, ImportFailure> {
    if !opts.source_from_jsonl {
        return Ok(opts.source.to_string());
    }
    let refuse = |detail: String| ImportFailure::Invalid {
        line: conversation.line,
        detail,
    };
    let Some(source) = conversation.export_source.as_deref().and_then(trimmed) else {
        return Err(refuse(format!(
            "conversation '{}' has no export.source, which a directory import needs",
            conversation.chat_identifier
        )));
    };
    validate_source_id(source)
        .map_err(|err| refuse(format!("export.source '{source}' is not valid: {err:#}")))?;
    Ok(source.to_string())
}

/// Messages with no conversation of their own live in `orphaned.jsonl`
/// (older bundles used `orphaned.json`). Its header's chat id names the
/// file's conversation, not a person.
fn is_orphaned_export(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    stem.eq_ignore_ascii_case("orphaned")
}

/// Stage one JSON Lines file: each conversation header with the messages
/// that follow it.
///
/// # Errors
///
/// Returns [`StagingError::Rejected`] when a line breaks a rule the sender
/// can fix, and [`StagingError::Internal`] when the file cannot be read or a
/// file or row cannot be written.
pub(super) async fn import_file_to_staging(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    opts: &ImportOptions<'_>,
    path: &Path,
    asset_stats: &mut AssetStats,
    media_work: &Path,
) -> Result<ImportStats, StagingError> {
    let mut staging = FileStaging {
        tx,
        stmts,
        opts,
        source_file: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown.jsonl")
            .to_string(),
        asset_stats,
        media_work,
        stats: ImportStats::default(),
    };
    // `jsonl::read_records` refuses a file whose first record is not a
    // conversation header, so every message here follows one.
    let mut pending: Option<StagedConversation> = None;
    let mut messages: Vec<MessageRecord> = Vec::new();

    for record in jsonl::read_records(path)? {
        match record {
            ExportRecord::Conversation(c) => {
                if let Some(header) = pending.take() {
                    staging.stage(header, std::mem::take(&mut messages)).await?;
                }
                let source = resolve_conversation_source(opts, &c)?;
                pending = Some(StagedConversation::from_record(c, source));
            }
            ExportRecord::Message(m) => messages.push(m),
        }
    }

    let Some(header) = pending else {
        return Err(StagingError::Internal(anyhow!(
            "{} has no conversation header",
            path.display()
        )));
    };
    staging.stage(header, messages).await?;
    Ok(staging.stats)
}

/// A conversation header as read from the file, with the source it resolved to.
struct StagedConversation {
    chat_identifier: String,
    /// `phone` | `whatsapp` for handle rows, when the header says.
    platform_service: Option<String>,
    conversation_type: String,
    group_title: Option<String>,
    participants: Vec<StagedParticipant>,
    source: String,
}

impl StagedConversation {
    fn from_record(record: ConversationRecord, source: String) -> Self {
        Self {
            chat_identifier: record.chat_identifier,
            platform_service: record.service,
            conversation_type: record.conversation_type,
            // A title of blanks is no title, as the conversation list shows
            // it, so it never replaces a real one in a merge.
            group_title: record.group_title.filter(|t| !t.trim().is_empty()),
            participants: record
                .participants
                .into_iter()
                .map(|p| (p.handle, p.name_alias, p.handle_type))
                .collect(),
            source,
        }
    }
}

/// One JSON Lines file being staged: the connection and the per-import insert
/// state, the options, the counters its conversations add to, and the file's
/// name for the conversation rows.
struct FileStaging<'a> {
    tx: &'a mut SqliteConnection,
    stmts: &'a mut StagingInserts,
    opts: &'a ImportOptions<'a>,
    source_file: String,
    asset_stats: &'a mut AssetStats,
    media_work: &'a Path,
    stats: ImportStats,
}

impl FileStaging<'_> {
    /// Stage one conversation: its media on disk, then its handle, conversation
    /// row, participants, and message rows in the staging tables.
    ///
    /// # Errors
    ///
    /// Returns [`StagingError::Rejected`] when an attachment breaks a rule
    /// the sender can fix, and [`StagingError::Internal`] when a media file
    /// cannot be stored or a row cannot be written.
    async fn stage(
        &mut self,
        conversation: StagedConversation,
        messages: Vec<MessageRecord>,
    ) -> Result<(), StagingError> {
        // Copy or convert media first: it needs no database rows, and a failure
        // here leaves nothing half-written.
        let prepared_messages = prepare_message_attachments(
            self.opts,
            self.opts.assets_dir,
            messages,
            self.asset_stats,
            self.media_work,
        )?;
        self.write_rows(conversation, prepared_messages)
            .await
            .map_err(StagingError::Internal)
    }

    /// Write one conversation's handle, conversation row, participants, and
    /// message rows, its media already stored. Every row it writes was
    /// accepted when it was read, so any error here is the server's.
    ///
    /// # Errors
    ///
    /// Returns an error when a row cannot be written.
    async fn write_rows(
        &mut self,
        conversation: StagedConversation,
        mut prepared_messages: Vec<(MessageRecord, Vec<PreparedAttachment>)>,
    ) -> Result<()> {
        let mut stats = ImportStats::default();
        // The title's time: the latest message of this copy, when it has a
        // title. Every timestamp has one fixed RFC 3339 form, so the greatest
        // string is the latest instant.
        let group_title_at = conversation.group_title.as_ref().and_then(|_| {
            prepared_messages
                .iter()
                .map(|(m, _)| m.timestamp.as_str())
                .max()
                .map(str::to_owned)
        });
        let platform = platform_for(
            conversation.platform_service.as_deref(),
            &conversation.source,
        );

        // What the header says each participant's address is. The exporter
        // knows its source's ids, and `Handle::parse` does not: a WhatsApp
        // `123456@lid` has an `@` and is no email address.
        let header_types = header_handle_types(&conversation.participants);
        let individual = conversation
            .conversation_type
            .eq_ignore_ascii_case("individual");
        // Conversation identity: the chat handle. A group's id is the group's
        // key and nobody's address, so it is `Other` whatever its shape (a
        // WhatsApp `…@g.us` has an `@`). A one-to-one chat's id takes the type
        // the header gives the participant with the same address, and
        // `Handle::parse`'s only when no participant has it.
        let chat_handle_type = if individual {
            header_types
                .get(conversation.chat_identifier.trim())
                .copied()
                .unwrap_or_else(|| handle_type_of(&conversation.chat_identifier))
        } else {
            HandleType::Other
        };
        let (chat_handle_id, flagged, chat_cached) = upsert_handle_row_cached(
            self.tx,
            &mut self.stmts.handles,
            self.stmts.account_id,
            &conversation.chat_identifier,
            chat_handle_type,
            Some(platform.as_str()),
        )
        .await?;
        if flagged {
            stats.phones_needing_review += 1;
        }
        // Only a one-to-one chat's identifier is a person. A group's id (or
        // `orphaned`) names the conversation, so it gets a handle row and no
        // contact; the people in it get theirs as participants. Exporters
        // write `orphaned.jsonl` under an `individual` header, so the file
        // name, not the type, says it is the orphaned conversation. The handle
        // cache is no guide here: it says this run has seen the handle, not
        // that anything gave it a contact.
        //
        // A chat keyed by a name (`name:Alice`) or by nobody (`nameless:`) is
        // no address either. The key keeps the conversation apart from any
        // other, such as the sender `AMAZON` from a person named "AMAZON".
        // The person a name-keyed chat is with gets their contact from their
        // participant record, an identity of type `other` holding the name,
        // so the key giving them a second contact would make one person two.
        //
        // A one-to-one chat with one of the account's own identities is a
        // conversation the holder has with themselves (Apple Messages' chat
        // with the owner's number, WhatsApp's "Message yourself"). The holder
        // is never a participant and gets no contact, so the chat id gets
        // none either, and the received copy of each note has no sender
        // (#1094). Its header's participant, if any, is the holder and
        // `insert_participant` drops it.
        let chat_is_an_address = individual
            && !is_orphaned_export(Path::new(&self.source_file))
            && message_ir::name_of_chat_id(&conversation.chat_identifier).is_none()
            && conversation.chat_identifier != message_ir::NAMELESS_CHAT_ID;
        let with_yourself = chat_is_an_address
            && is_account_identity(
                &self.stmts.identities,
                &conversation.chat_identifier,
                chat_handle_type,
            );
        if chat_is_an_address && !with_yourself {
            count_other_identity(chat_handle_type, chat_cached, &mut stats);
            let _ = ensure_contact_for_handle(
                self.tx,
                self.stmts.account_id,
                self.stmts.import_id,
                chat_handle_id,
                None,
                &mut stats,
            )
            .await?;
        }
        let conversation_id = db_staging::insert_conversation(
            self.tx,
            &StagingConversation {
                account_id: self.stmts.account_id,
                chat_handle_id,
                conversation_type: &conversation.conversation_type,
                group_title: conversation.group_title.as_deref(),
                group_title_at: group_title_at.as_deref(),
                source_file: &self.source_file,
            },
        )
        .await?;
        stats.conversations = 1;

        for participant in conversation.participants {
            insert_participant(
                self.tx,
                self.stmts,
                conversation_id,
                participant,
                platform,
                &mut stats,
            )
            .await?;
        }

        // The received copy of a note to yourself came from the holder, who
        // is nobody's sender here. A reaction in it is the holder's own, so
        // it is marked as theirs, as a reaction they sent anywhere is.
        if with_yourself {
            for (msg, _) in &mut prepared_messages {
                msg.sender = None;
                for tapback in &mut msg.tapbacks {
                    tapback.is_from_me = true;
                    tapback.sender = None;
                }
            }
        }
        let first_sort_order =
            db_staging::first_sort_order(self.tx, self.stmts.account_id, chat_handle_id).await?;
        let pending_rows = resolve_message_rows(
            self.tx,
            self.stmts,
            prepared_messages,
            first_sort_order,
            platform,
            &header_types,
            &mut stats,
        )
        .await?;
        let msg_chunk = db_staging::message_chunk_rows();
        for chunk in pending_rows.chunks(msg_chunk) {
            flush_staging_message_chunk(
                self.tx,
                self.stmts,
                &mut stats,
                conversation_id,
                &conversation.source,
                self.opts.assets_dir,
                chunk,
            )
            .await?;
        }
        self.stats.merge_file(&stats);
        Ok(())
    }
}

/// The type the header gives each participant address that has one, keyed by
/// the trimmed address.
fn header_handle_types(participants: &[StagedParticipant]) -> HashMap<String, HandleType> {
    participants
        .iter()
        .filter_map(|(handle, _, handle_type)| Some((handle.trim().to_string(), (*handle_type)?)))
        .collect()
}

/// Platform for chat and participant handles: the conversation's own hint,
/// else WhatsApp for a WhatsApp export, else phone.
fn platform_for(platform_service: Option<&str>, source: &str) -> HandleService {
    platform_service.map_or_else(
        || {
            if source.eq_ignore_ascii_case("whatsapp") {
                HandleService::Whatsapp
            } else {
                HandleService::Phone
            }
        },
        HandleService::parse,
    )
}

/// Stage every message's attachments on disk, pairing each message with what
/// was kept.
///
/// # Errors
///
/// Returns [`StagingError::Rejected`] when an attachment's path or stated
/// SHA-256 is wrong, and [`StagingError::Internal`] when a media file cannot
/// be copied or converted.
fn prepare_message_attachments(
    opts: &ImportOptions<'_>,
    assets_dir: &Path,
    messages: Vec<MessageRecord>,
    asset_stats: &mut AssetStats,
    media_work: &Path,
) -> Result<Vec<(MessageRecord, Vec<PreparedAttachment>)>, StagingError> {
    let mut prepared = Vec::with_capacity(messages.len());
    for mut msg in messages {
        let attachments = prepare_attachments(
            opts.asset_root,
            assets_dir,
            std::mem::take(&mut msg.attachments),
            asset_stats,
            opts.media,
            media_work,
            msg.line,
        )?;
        prepared.push((msg, attachments));
    }
    Ok(prepared)
}

/// Insert one participant row, bound to its handle, and give the handle a
/// contact.
///
/// # Errors
///
/// Returns an error when a handle or contact row cannot be written.
async fn insert_participant(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    conversation_id: i64,
    (handle, name_alias, handle_type): StagedParticipant,
    platform: HandleService,
    stats: &mut ImportStats,
) -> Result<()> {
    // Prefer the source-provided type; fall back to `Handle::parse`.
    let handle_type = handle_type.unwrap_or_else(|| handle_type_of(&handle));
    // The account holder is never a participant: a member at one of the
    // account's identities gets no handle, contact or participant row. The
    // exporters drop the addresses their backup names as the owner's; this
    // catches the ones only the account knows (#1093).
    if is_account_identity(&stmts.identities, &handle, handle_type) {
        return Ok(());
    }
    let (handle_id, flagged, cached) = upsert_handle_row_cached(
        tx,
        &mut stmts.handles,
        stmts.account_id,
        &handle,
        handle_type,
        Some(platform.as_str()),
    )
    .await?;
    if flagged {
        stats.phones_needing_review += 1;
    }
    count_other_identity(handle_type, cached, stats);
    let backup_name = name_alias.as_deref().and_then(nonempty);
    ensure_contact_for_handle(
        tx,
        stmts.account_id,
        stmts.import_id,
        handle_id,
        backup_name.as_deref(),
        stats,
    )
    .await?;
    // `participants.name_alias` keeps what this backup called them in this
    // conversation. It is the second clause of the naming rule, never the
    // first.
    if db_staging::insert_participant(tx, conversation_id, handle_id, backup_name.as_deref())
        .await?
    {
        stats.participants += 1;
    }
    Ok(())
}

/// Resolve each message's body text and sender handle into a row ready for
/// the bulk staging insert. The messages take `sort_order` in the source's
/// order, counting up from `first_sort_order`. A sender the header names as a
/// participant takes the type the header gives it, so the sender and the
/// participant are one identity; any other sender keeps the type its message
/// record carries.
///
/// # Errors
///
/// Returns an error when a sender handle cannot be written.
async fn resolve_message_rows(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    prepared: Vec<(MessageRecord, Vec<PreparedAttachment>)>,
    first_sort_order: i64,
    platform: HandleService,
    header_types: &HashMap<String, HandleType>,
    stats: &mut ImportStats,
) -> Result<Vec<PendingStagingMessage>> {
    let mut rows = Vec::with_capacity(prepared.len());
    for (sort_order, (msg, attachments)) in (first_sort_order..).zip(prepared) {
        let body = if msg.is_announcement {
            clean_body(msg.announcement.as_deref()).or_else(|| clean_body(msg.text.as_deref()))
        } else {
            clean_body(msg.text.as_deref())
        };
        let sender_platform = msg
            .service
            .as_deref()
            .map_or(platform, HandleService::parse);
        let sender_handle_id = resolve_incoming_sender_handle(
            tx,
            &mut stmts.handles,
            &stmts.identities,
            stmts.account_id,
            stmts.import_id,
            IncomingSender {
                is_from_me: msg.is_from_me,
                address: msg.sender.as_deref(),
                handle_type: msg
                    .sender
                    .as_deref()
                    .and_then(|address| header_types.get(address.trim()))
                    .copied()
                    .or(msg.sender_handle_type),
                platform: sender_platform.as_str(),
            },
            stats,
        )
        .await?;
        let owner_handle_id =
            resolve_owner_handle(tx, stmts, msg.owner.as_deref(), sender_platform.as_str()).await?;
        rows.push(PendingStagingMessage {
            msg,
            attachments,
            sender_handle_id,
            owner_handle_id,
            sender_platform: sender_platform.as_str().to_string(),
            body,
            sort_order,
        });
    }
    Ok(rows)
}

/// The `handles` row for the account holder's own address on a message,
/// creating it when this run is the first to meet it. It gets no contact: the
/// holder is not a person the import met (ADR-0015). `None` when the backup
/// names no owner.
///
/// # Errors
///
/// Returns an error when the handle cannot be written.
async fn resolve_owner_handle(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    address: Option<&str>,
    platform: &str,
) -> Result<Option<i64>> {
    let Some(address) = address else {
        return Ok(None);
    };
    let key = (address.to_string(), platform.to_string());
    if let Some(&id) = stmts.owners.get(&key) {
        return Ok(Some(id));
    }
    let (id, _) = upsert_handle_row(
        tx,
        stmts.account_id,
        address,
        handle_type_of(address),
        Some(platform),
    )
    .await?;
    stmts.owners.insert(key, id);
    Ok(Some(id))
}

struct PendingStagingMessage {
    msg: MessageRecord,
    attachments: Vec<PreparedAttachment>,
    sender_handle_id: Option<i64>,
    owner_handle_id: Option<i64>,
    sender_platform: String,
    body: Option<String>,
    sort_order: i64,
}

/// Bulk-insert one chunk of message rows, then their attachments and tapbacks keyed by the ids returned.
async fn flush_staging_message_chunk(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    stats: &mut ImportStats,
    conversation_id: i64,
    source: &str,
    assets_dir: &Path,
    chunk: &[PendingStagingMessage],
) -> Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }
    let mut by_sort = insert_message_rows(tx, stmts, conversation_id, source, chunk).await?;

    let mut att_rows = Vec::new();
    let mut tap_rows = Vec::new();
    for row in chunk {
        // Consume the RETURNING id so a conflicted row (duplicate guid) is
        // skipped instead of attaching children to another message.
        let Some(message_id) = by_sort.remove(&row.sort_order) else {
            stats.messages_deduped += 1;
            continue;
        };
        stats.messages += 1;
        att_rows.extend(
            row.attachments
                .iter()
                .map(|prepared| attachment_row(message_id, prepared, assets_dir)),
        );
        for tap in &row.msg.tapbacks {
            tap_rows.push(tapback_row(tx, stmts, stats, message_id, row, tap).await?);
        }
    }

    stats.attachments += db_staging::insert_attachments(tx, &att_rows).await?;
    stats.tapbacks += db_staging::insert_tapbacks(tx, &tap_rows).await?;
    Ok(())
}

/// Insert the chunk's message rows in one statement. Returns the new ids by
/// sort order; a row the insert skipped (duplicate guid) has no entry.
async fn insert_message_rows(
    tx: &mut SqliteConnection,
    stmts: &StagingInserts,
    conversation_id: i64,
    source: &str,
    chunk: &[PendingStagingMessage],
) -> Result<HashMap<i64, i64>> {
    let rows: Vec<StagingMessage<'_>> = chunk
        .iter()
        .map(|row| StagingMessage {
            conversation_id,
            account_id: stmts.account_id,
            source,
            guid: &row.msg.guid,
            timestamp: &row.msg.timestamp,
            is_from_me: row.msg.is_from_me as i64,
            sender_handle_id: row.sender_handle_id,
            owner_handle_id: row.owner_handle_id,
            service: row.msg.service.as_deref(),
            subject: row.msg.subject.as_deref(),
            body: row.body.as_deref(),
            is_announcement: row.msg.is_announcement as i64,
            is_reply: row.msg.is_reply as i64,
            thread_originator_guid: row.msg.thread_originator_guid.as_deref(),
            thread_originator_part: row.msg.thread_originator_part,
            num_replies: row.msg.num_replies,
            deletion: row.msg.deletion.map(message_ir::Deletion::as_str),
            sort_order: row.sort_order,
            import_id: stmts.import_id,
        })
        .collect();
    db_staging::insert_messages(tx, &rows).await
}

/// The row for one of a staged message's attachments: the stored blob's
/// digest, path, and type when the file was stored, the record's own
/// type and missing reason when it was not.
fn attachment_row(
    message_id: i64,
    prepared: &PreparedAttachment,
    assets_dir: &Path,
) -> StagingAttachment {
    let att = &prepared.record;
    let (sha256, assets_path, mime_type) = match &prepared.stored {
        Some(stored) => (
            Some(stored.sha256.clone()),
            Some(stored.assets_path.clone()),
            stored.mime_type.clone().or_else(|| att.mime_type.clone()),
        ),
        None => (None, None, att.mime_type.clone()),
    };
    let size_bytes = stored_size_bytes(assets_dir, assets_path.as_deref())
        .or_else(|| att.size_bytes.map(|n| n as i64));
    let missing_reason = if sha256.is_none() {
        att.missing_reason.clone()
    } else {
        None
    };
    StagingAttachment {
        message_id,
        path: att.path.clone(),
        original_name: att.original_name.clone(),
        mime_type,
        is_sticker: att.is_sticker as i64,
        transcription: att.transcription.clone(),
        sha256,
        assets_path,
        size_bytes,
        missing_reason,
    }
}

/// The row for one tapback on a staged message, its sender resolved to a
/// handle the way an incoming message's sender is.
async fn tapback_row(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    stats: &mut ImportStats,
    message_id: i64,
    row: &PendingStagingMessage,
    tap: &TapbackRecord,
) -> Result<StagingTapback> {
    let sender_handle_id = resolve_incoming_sender_handle(
        tx,
        &mut stmts.handles,
        &stmts.identities,
        stmts.account_id,
        stmts.import_id,
        IncomingSender {
            is_from_me: tap.is_from_me,
            address: tap.sender.as_deref(),
            handle_type: None,
            platform: &row.sender_platform,
        },
        stats,
    )
    .await?;
    Ok(StagingTapback {
        message_id,
        part_index: tap.part_index,
        kind: tap.kind.clone(),
        emoji: tap.emoji.clone(),
        is_from_me: tap.is_from_me as i64,
        sender_handle_id,
    })
}

#[cfg(test)]
mod tests;
