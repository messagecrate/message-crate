//! Stage message-ir JSONL rows into the temporary import tables.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use message_ir::{IdentityService, IdentityType, nonempty, trimmed};
use sqlx::SqliteConnection;

use super::records::{
    AttachmentRecord, ConversationRecord, ExportRecord, HandleValue, MessageRecord, TapbackRecord,
    clean_body,
};
use crate::assets_api::{self, AssetError, AssetStats, StoredAsset};
use crate::db::handles::{
    HandleIdCache, handle_type_on, upsert_handle_row_cached, upsert_handle_row_in,
};
use crate::db::staging::{
    self as db_staging, BackupOrder, StagedCopy, StagingAttachment, StagingConversation,
    StagingEarlierVersion, StagingMessage, StagingMessageKey, StagingTapback,
};
use crate::import_media;
use crate::jsonl::{self, ReadRecordsError};
use media::MediaMode;

use super::contact_name::{
    IncomingSender, count_other_identity, ensure_contact_for_handle, is_account_identity,
    resolve_incoming_sender_handle,
};
use super::{ImportCounts, ImportFailure, ImportOptions};

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
    /// address and service. Apart from `handles`, whose entries each have a
    /// contact: an owner's handle never gets one (ADR-0015).
    owners: HashMap<(String, IdentityService), i64>,
    /// The account's identities, as `(normalized address, handle type)`.
    /// A participant at one of them is the holder, who is never a
    /// participant (ADR-0015, #1093).
    identities: HashSet<(String, IdentityType)>,
}

impl StagingInserts {
    /// Fresh insert state for one import run, with the identities the account
    /// holds when the run starts.
    /// Every phone number the run writes without its `+` code is read in
    /// `phone_country`, the country the run states, when it states one.
    pub(super) fn new(
        account_id: i64,
        import_id: Option<i64>,
        identities: HashSet<(String, IdentityType)>,
        phone_country: Option<&'static phone::Country>,
    ) -> Self {
        Self {
            account_id,
            import_id,
            handles: HandleIdCache::in_country(phone_country),
            owners: HashMap::new(),
            identities,
        }
    }
}

/// One participant as the conversation header records it: their address, or
/// the name the source gave in its place, and the name this backup used for
/// them.
type StagedParticipant = (HandleValue, Option<String>);

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
    conversation.directory_source().map(str::to_string)
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
) -> Result<ImportCounts, StagingError> {
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
        counts: ImportCounts::default(),
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
    Ok(staging.counts)
}

/// A conversation header as read from the file, with the source it resolved to.
struct StagedConversation {
    chat_identifier: String,
    /// `phone` | `whatsapp` for handle rows, when the header says.
    header_service: Option<String>,
    conversation_type: String,
    group_title: Option<String>,
    participants: Vec<StagedParticipant>,
    source: String,
    /// When the backup the file was read from was made, in the form a
    /// message's timestamp takes; `None` when the file does not say.
    backup_taken_at: Option<crate::models::StoredTime>,
}

impl StagedConversation {
    fn from_record(record: ConversationRecord, source: String) -> Self {
        Self {
            chat_identifier: record.chat_identifier,
            header_service: record.service,
            conversation_type: record.conversation_type,
            // A title of blanks is no title, as the conversation list shows
            // it, so it never replaces a real one in a merge.
            group_title: record.group_title.filter(|t| !t.trim().is_empty()),
            participants: record
                .participants
                .into_iter()
                .map(|p| (p.handle, p.name_alias))
                .collect(),
            source,
            backup_taken_at: record.backup_taken_at,
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
    counts: ImportCounts,
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
        let mut counts = ImportCounts::default();
        // The title's time: the latest message of this copy, when it has a
        // title. Every stored time has one text form, so the greatest is the
        // latest instant.
        let group_title_at = conversation.group_title.as_ref().and_then(|_| {
            prepared_messages
                .iter()
                .map(|(m, _)| &m.timestamp)
                .max()
                .cloned()
        });
        let service = service_for(conversation.header_service.as_deref(), &conversation.source);

        let individual = conversation
            .conversation_type
            .eq_ignore_ascii_case("individual");
        // Conversation identity: the chat handle. A group's id is the group's
        // key and nobody's address, so it is `Other` whatever its shape (a
        // WhatsApp `…@g.us` has an `@`), and so is an orphaned conversation's
        // `orphaned:` key. A one-to-one chat's id is an address, typed by the
        // service and its shape like every other (#1933).
        let chat_handle_type = if individual {
            handle_type_on(&conversation.chat_identifier, service)
        } else {
            IdentityType::Other
        };
        let (chat_handle_id, flagged, chat_cached) = upsert_handle_row_cached(
            self.tx,
            &mut self.stmts.handles,
            self.stmts.account_id,
            &conversation.chat_identifier,
            chat_handle_type,
            Some(service.as_str()),
        )
        .await?;
        if flagged {
            counts.phones_needing_review += 1;
        }
        // Only a one-to-one chat's identifier is a person. A group's id, and
        // the `orphaned:` key of a conversation of orphaned messages, name
        // the conversation, so they get a handle row and no contact; the
        // people in it get theirs as participants. The conversation's type
        // says which it is, whatever file it came in (#1095). The handle
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
            && message_ir::name_of_chat_id(&conversation.chat_identifier).is_none()
            && conversation.chat_identifier != message_ir::NAMELESS_CHAT_ID;
        let with_yourself = chat_is_an_address
            && is_account_identity(
                &self.stmts.identities,
                &conversation.chat_identifier,
                chat_handle_type,
                self.stmts.handles.country(),
            );
        if chat_is_an_address && !with_yourself {
            count_other_identity(chat_handle_type, chat_cached, &mut counts);
            let _ = ensure_contact_for_handle(
                self.tx,
                self.stmts.account_id,
                self.stmts.import_id,
                chat_handle_id,
                None,
                &mut counts,
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
                group_title_at: group_title_at.as_ref(),
                source_file: &self.source_file,
            },
        )
        .await?;
        counts.conversations = 1;
        if let (Some(import_id), Some(backup_taken_at)) =
            (self.stmts.import_id, conversation.backup_taken_at.as_ref())
        {
            crate::db::imports::note_backup_taken_at(self.tx, import_id, backup_taken_at).await?;
        }

        for participant in conversation.participants {
            insert_participant(
                self.tx,
                self.stmts,
                conversation_id,
                participant,
                service,
                &mut counts,
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
            service,
            &mut counts,
        )
        .await?;
        let msg_chunk = db_staging::message_chunk_rows();
        for chunk in pending_rows.chunks(msg_chunk) {
            flush_staging_message_chunk(
                self.tx,
                self.stmts,
                &mut counts,
                StagedSource {
                    conversation_id,
                    source: &conversation.source,
                    backup_taken_at: conversation.backup_taken_at.as_ref(),
                },
                self.opts.assets_dir,
                chunk,
            )
            .await?;
        }
        self.counts.merge_file(&counts);
        Ok(())
    }
}

/// The service of chat and participant handles: the conversation's own hint,
/// else WhatsApp for a WhatsApp export, else phone.
fn service_for(header_service: Option<&str>, source: &str) -> IdentityService {
    header_service.map_or_else(
        || {
            if source.eq_ignore_ascii_case("whatsapp") {
                IdentityService::Whatsapp
            } else {
                IdentityService::Phone
            }
        },
        IdentityService::parse,
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
    (value, name_alias): StagedParticipant,
    service: IdentityService,
    counts: &mut ImportCounts,
) -> Result<()> {
    let handle_type = value.handle_type_on(service);
    let handle = value.as_str();
    // The account holder is never a participant: a member at one of the
    // account's identities gets no handle, contact or participant row. The
    // exporters drop the addresses their backup names as the owner's; this
    // catches the ones only the account knows (#1093).
    if is_account_identity(
        &stmts.identities,
        handle,
        handle_type,
        stmts.handles.country(),
    ) {
        return Ok(());
    }
    let (handle_id, flagged, cached) = upsert_handle_row_cached(
        tx,
        &mut stmts.handles,
        stmts.account_id,
        handle,
        handle_type,
        Some(service.as_str()),
    )
    .await?;
    if flagged {
        counts.phones_needing_review += 1;
    }
    count_other_identity(handle_type, cached, counts);
    let backup_name = name_alias.as_deref().and_then(nonempty);
    ensure_contact_for_handle(
        tx,
        stmts.account_id,
        stmts.import_id,
        handle_id,
        backup_name.as_deref(),
        counts,
    )
    .await?;
    // `participants.name_alias` keeps what this backup called them in this
    // conversation. It is the second clause of the naming rule, never the
    // first.
    if db_staging::insert_participant(tx, conversation_id, handle_id, backup_name.as_deref())
        .await?
    {
        counts.participants += 1;
    }
    Ok(())
}

/// Resolve each message's body text and sender handle into a row ready for
/// the bulk staging insert. The messages take `sort_order` in the source's
/// order, counting up from `first_sort_order`. A sender's type is its
/// address's shape, within what its message's service carries, the rule a
/// participant's type follows, so a sender the header lists is the
/// participant's identity.
///
/// # Errors
///
/// Returns an error when a sender handle cannot be written.
async fn resolve_message_rows(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    prepared: Vec<(MessageRecord, Vec<PreparedAttachment>)>,
    first_sort_order: i64,
    service: IdentityService,
    counts: &mut ImportCounts,
) -> Result<Vec<PendingStagingMessage>> {
    let mut rows = Vec::with_capacity(prepared.len());
    for (sort_order, (msg, attachments)) in (first_sort_order..).zip(prepared) {
        let body = if msg.is_announcement {
            clean_body(msg.announcement.as_deref()).or_else(|| clean_body(msg.text.as_deref()))
        } else {
            clean_body(msg.text.as_deref())
        };
        let sender_service = msg
            .service
            .as_deref()
            .map_or(service, IdentityService::parse);
        let sender_handle_id = resolve_incoming_sender_handle(
            tx,
            &mut stmts.handles,
            &stmts.identities,
            stmts.account_id,
            stmts.import_id,
            IncomingSender {
                is_from_me: msg.is_from_me,
                value: msg.sender.as_ref(),
                service: sender_service,
            },
            counts,
        )
        .await?;
        let owner_handle_id =
            resolve_owner_handle(tx, stmts, msg.owner.as_deref(), sender_service).await?;
        rows.push(PendingStagingMessage {
            msg,
            attachments,
            sender_handle_id,
            owner_handle_id,
            sender_service,
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
    service: IdentityService,
) -> Result<Option<i64>> {
    let Some(address) = address else {
        return Ok(None);
    };
    let key = (address.to_string(), service);
    if let Some(&id) = stmts.owners.get(&key) {
        return Ok(Some(id));
    }
    let (id, _) = upsert_handle_row_in(
        tx,
        stmts.account_id,
        address,
        handle_type_on(address, service),
        Some(service.as_str()),
        stmts.handles.country(),
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
    sender_service: IdentityService,
    body: Option<String>,
    sort_order: i64,
}

/// Where one conversation's message rows are staged from: its staging
/// conversation, its source, and when its backup was made.
#[derive(Clone, Copy)]
struct StagedSource<'a> {
    conversation_id: i64,
    source: &'a str,
    backup_taken_at: Option<&'a crate::models::StoredTime>,
}

/// Bulk-insert one chunk of message rows, then their attachments, tapbacks
/// and earlier versions keyed by the ids returned.
async fn flush_staging_message_chunk(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    counts: &mut ImportCounts,
    staged_source: StagedSource<'_>,
    assets_dir: &Path,
    chunk: &[PendingStagingMessage],
) -> Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }
    let mut by_sort = insert_message_rows(tx, stmts, staged_source, chunk).await?;

    let mut att_rows = Vec::new();
    let mut tap_rows = Vec::new();
    let mut version_rows = Vec::new();
    let mut copies = Vec::new();
    for row in chunk {
        // Consume the RETURNING id so a conflicted row (duplicate guid), a
        // second copy of a staged message, is not inserted as a message of
        // its own. Its children go to the staged message once the chunk's
        // own are written.
        let Some(message_id) = by_sort.remove(&row.sort_order) else {
            counts.messages_deduped += 1;
            copies.push(row);
            continue;
        };
        counts.messages += 1;
        att_rows.extend(
            row.attachments
                .iter()
                .map(|prepared| attachment_row(message_id, prepared, assets_dir)),
        );
        for tap in &row.msg.tapbacks {
            tap_rows.push(tapback_row(tx, stmts, counts, message_id, row, tap).await?);
        }
        version_rows.extend(
            row.msg
                .earlier_versions
                .iter()
                .map(|version| StagingEarlierVersion::from_record(message_id, version)),
        );
    }

    counts.attachments += db_staging::insert_attachments(tx, &att_rows).await?;
    counts.tapbacks += db_staging::insert_tapbacks(tx, &tap_rows).await?;
    db_staging::insert_earlier_versions(tx, &version_rows).await?;
    for row in copies {
        add_staged_copy(tx, stmts, counts, staged_source, assets_dir, row).await?;
    }
    Ok(())
}

/// Give the message staged under `row`'s guid what `row`, another copy of it
/// from the same import, adds, by the rules a later import of the copy would
/// follow (`db::staging::promote_deletion_marks`,
/// `db::staging::write_edit_map`): the attachments and reactions the staged
/// message does not hold yet, and its mark and text as follows.
///
/// The staged row keeps what a file without a backup date gave it apart
/// from the date of the dated backups it met: the mark such a file gave
/// (`undated_deletion`) and whether its text came from one
/// (`undated_body`). So the date rules decide only between dated copies,
/// and the rules for files without one decide an undated copy's part,
/// whichever file is read first (#1989).
///
/// The mark follows [`db_staging::add_staged_copy_mark`]: a mark from a file
/// without a date stands, a later dated backup gives its mark or none, and
/// an earlier one gives nothing (#1741). The text: when both copies' texts
/// come from dated backups with different dates, a copy from the later
/// backup gives its text and earlier versions, and one from an earlier
/// backup gives neither (#1804); otherwise
/// ([`db_staging::later_backup`]) the copy gives them when it records a
/// later edit. A copy at the staged message's time that has milliseconds
/// marks it `milliseconds` ([`db_staging::add_staged_copy_milliseconds`]).
/// One import of two backups then stores what two separate imports of them
/// store, in either file order (#1806, #1837).
async fn add_staged_copy(
    tx: &mut SqliteConnection,
    stmts: &mut StagingInserts,
    counts: &mut ImportCounts,
    staged_source: StagedSource<'_>,
    assets_dir: &Path,
    row: &PendingStagingMessage,
) -> Result<()> {
    if row.msg.earlier_versions.is_empty()
        && row.attachments.is_empty()
        && row.msg.tapbacks.is_empty()
        && row.msg.deletion.is_none()
        && staged_source.backup_taken_at.is_none()
        && row.msg.time_precision == message_ir::TimePrecision::Seconds
    {
        return Ok(());
    }
    let key = StagingMessageKey {
        account_id: stmts.account_id,
        source: staged_source.source,
        guid: &row.msg.guid,
    };
    let staged = db_staging::staged_message_id(tx, key)
        .await?
        .with_context(|| format!("no staged message holds the copy of {}", row.msg.guid))?;
    if row.msg.time_precision == message_ir::TimePrecision::Milliseconds {
        db_staging::add_staged_copy_milliseconds(tx, staged, &row.msg.timestamp).await?;
    }
    let held = db_staging::staged_text(tx, staged).await?;
    let copy = StagedCopy {
        body: row.body.as_deref(),
        versions: &row.msg.earlier_versions,
        undated: staged_source.backup_taken_at.is_none(),
    };
    let order = if held.undated {
        BackupOrder::Undecided
    } else {
        db_staging::later_backup(staged_source.backup_taken_at, held.backup_taken_at.as_ref())
    };
    match order {
        BackupOrder::Later => {
            db_staging::replace_staged_text(tx, staged, &copy).await?;
        }
        BackupOrder::Earlier => {}
        BackupOrder::Undecided => {
            let taken = !row.msg.earlier_versions.is_empty()
                && db_staging::take_later_staged_copy(tx, staged, &copy).await?;
            if let (false, Some(backup)) = (taken, staged_source.backup_taken_at) {
                db_staging::note_backed_staged_text(tx, staged, &copy, backup).await?;
            }
        }
    }
    db_staging::add_staged_copy_mark(tx, staged, row.msg.deletion, staged_source.backup_taken_at)
        .await?;

    let att_rows: Vec<StagingAttachment> = row
        .attachments
        .iter()
        .map(|prepared| attachment_row(staged, prepared, assets_dir))
        .collect();
    counts.attachments += db_staging::add_copy_attachments(tx, &att_rows).await?;
    let mut tap_rows = Vec::with_capacity(row.msg.tapbacks.len());
    for tap in &row.msg.tapbacks {
        tap_rows.push(tapback_row(tx, stmts, counts, staged, row, tap).await?);
    }
    counts.tapbacks += db_staging::add_copy_tapbacks(tx, &tap_rows).await?;
    Ok(())
}

/// Insert the chunk's message rows in one statement. Returns the new ids by
/// sort order; a row the insert skipped (duplicate guid) has no entry.
async fn insert_message_rows(
    tx: &mut SqliteConnection,
    stmts: &StagingInserts,
    staged_source: StagedSource<'_>,
    chunk: &[PendingStagingMessage],
) -> Result<HashMap<i64, i64>> {
    let rows: Vec<StagingMessage<'_>> = chunk
        .iter()
        .map(|row| StagingMessage {
            conversation_id: staged_source.conversation_id,
            account_id: stmts.account_id,
            source: staged_source.source,
            guid: &row.msg.guid,
            timestamp: &row.msg.timestamp,
            time_precision: row.msg.time_precision,
            is_from_me: row.msg.is_from_me as i64,
            sender_handle_id: row.sender_handle_id,
            owner_handle_id: row.owner_handle_id,
            service: row.msg.service.as_deref(),
            subject: row.msg.subject.as_deref(),
            body: row.body.as_deref(),
            is_announcement: row.msg.is_announcement as i64,
            is_reply: i64::from(row.msg.reply_to.is_some()),
            reply_to_guid: row.msg.reply_to.as_ref().and_then(|r| r.guid.as_deref()),
            reply_to_part: row
                .msg
                .reply_to
                .as_ref()
                .and_then(|r| r.part_index)
                .map(i64::from),
            deletion: row.msg.deletion,
            sort_order: row.sort_order,
            import_id: stmts.import_id,
            backup_taken_at: staged_source.backup_taken_at,
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
    counts: &mut ImportCounts,
    message_id: i64,
    row: &PendingStagingMessage,
    tap: &TapbackRecord,
) -> Result<StagingTapback> {
    // A reaction names its sender by address only.
    let reactor = tap.sender.clone().map(HandleValue::Address);
    let sender_handle_id = resolve_incoming_sender_handle(
        tx,
        &mut stmts.handles,
        &stmts.identities,
        stmts.account_id,
        stmts.import_id,
        IncomingSender {
            is_from_me: tap.is_from_me,
            value: reactor.as_ref(),
            service: row.sender_service,
        },
        counts,
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
