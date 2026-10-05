//! Convert iMazing Messages / WhatsApp rows into the shared conversation
//! structure, then write the chosen output format via [`ExportWriter`].

use crate::attachments::{FolderFiles, attachment_cell, file_name_second, mime_hint, row_sources};
use crate::attachments_emit::{
    attachment_content_key, attachment_digests, attachment_matches_any_file_key,
    attachment_source_key, pending_attachment_to_ir,
};
use crate::parse::{DiscoveredCsv, RawRow, SourceKind, discover_csv_files, parse_csv_file};
use crate::parse_emit::{
    Session, group_vendor_id, group_vendor_id_with_name, is_notification, is_outgoing,
    parse_message_date, resolve_sender, session_key,
};
use crate::unnamed_files::{FolderRows, UnnamedFile, unnamed_files};
use anyhow::Result;
use message_crate_core::{
    CancelFlag, ExportReport, ExportTransforms, IssueSink, OutputFormat, prepare_outputs,
    project_conversation,
};
use message_csv::Zone;
use message_ir::{
    ConversationKey, ExportMeta, IrAttachment, IrParticipant, IrService, IrSource,
    PendingAttachment, PendingConversation, PendingMessage, ProjectedRole, ProjectionHooks,
};
use message_staging::{AttachmentSource, ExportWriter};
use phone::Handle;
use serde_json::Map;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = "imazing";
const EXPORT_TOOL: &str = "iMazing";
const EXPORT_TOOL_VERSION: &str = "3.5.5";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TransportFamily {
    Messages,
    WhatsApp,
}

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub timezone: Option<&'a str>,
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
    /// Where each Import Error and note goes as the run records it.
    pub issues: Option<&'a IssueSink>,
}

/// Convert iMazing Messages / WhatsApp CSV(s) under `input`.
///
/// `timezone`: a fixed UTC offset (`UTC-05:00`) or an IANA zone name
/// (`America/New_York`). When `None`, use the host local zone.
/// When `transforms` copies attachments, media files are copied into `output/attachments/`.
/// When `cancel` is set, cooperative cancellation is checked between CSV files.
///
/// # Errors
///
/// Returns an error when output overlaps input, a CSV cannot be parsed, or the
/// user cancels.
pub(crate) fn convert_export(args: ConvertExportArgs<'_>) -> Result<ExportReport> {
    let ConvertExportArgs {
        input,
        output,
        timezone,
        transforms,
        output_format,
        cancel,
        resume,
        issues,
    } = args;
    let tz = Zone::parse(timezone)?;
    let (inputs, output) = prepare_outputs(&[input.to_path_buf()], output)?;
    let input = &inputs[0];
    let writer = ExportWriter::open(&output, output_format, transforms, resume)?;
    let copy_attachments = writer.copies_attachments();

    let mut ingest = Ingest {
        tz,
        copy_attachments,
        conversations: BTreeMap::new(),
        claims: Vec::new(),
        folder_texts: BTreeMap::new(),
        whatsapp_folders: HashSet::new(),
        report: ExportReport::with_issues(issues.cloned()),
    };
    for (csv_index, discovered) in discover_csv_files(input)?.iter().enumerate() {
        message_crate_core::check_cancel(cancel)?;
        ingest.ingest_file(csv_index, discovered)?;
    }
    if copy_attachments {
        ingest.attach_unnamed_files()?;
        ingest.tell_apart_files_of_one_name();
    }
    let Ingest {
        mut conversations,
        mut report,
        ..
    } = ingest;
    separate_groups_with_one_earliest_row(&mut conversations);

    let export = message_crate_core::export_meta(
        EXPORT_SOURCE,
        EXPORT_TOOL,
        EXPORT_TOOL_VERSION,
        None,
        None,
    );
    let mut documents = Vec::new();
    let mut sources = Vec::new();
    for (
        _,
        Conversation {
            key,
            address,
            mut convo,
            ..
        },
    ) in conversations
    {
        let hooks = ImazingProjection {
            export: &export,
            key: &key,
            address: address.as_ref(),
            sources: RefCell::new(Vec::new()),
        };
        let chat_id = convo.chat_id.clone();
        let Some(doc) = project_conversation(&chat_id, &mut convo, &hooks, &mut report) else {
            continue;
        };
        sources.extend(hooks.sources.into_inner());
        documents.push(doc);
    }

    let mut source_iter = sources.into_iter();
    writer.finish(
        documents,
        &mut |att| match source_iter.next().flatten() {
            Some(path) => {
                // iMazing's rows carry no size; stat the source so the byte
                // counters and the headroom check see it.
                let hint = att
                    .size_bytes
                    .or_else(|| std::fs::metadata(&path).ok().map(|m| m.len()));
                (AttachmentSource::Path(path), hint)
            }
            None => (AttachmentSource::Missing, att.size_bytes),
        },
        cancel,
        &mut report,
    )?;

    Ok(report)
}

/// One conversation being read: its key, and its messages so far.
struct Conversation {
    key: ConversationKey,
    /// A one-to-one conversation's address (`Session::address`).
    address: Option<Handle>,
    convo: PendingConversation,
    /// For a group, its rows' digests, earliest first (`Session::row_digests`).
    row_digests: Vec<[u8; 32]>,
}

/// Which pending conversation a session's rows go to.
///
/// A one-to-one conversation is one conversation across every CSV that
/// names its address. A group session is a conversation of its own, even
/// when another starts with the same row: `separate_groups_with_one_earliest_row`
/// decides which ones are one group once every CSV is read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ConvoKey {
    /// Keeps a Messages conversation and a WhatsApp conversation with the
    /// same peer apart.
    family: TransportFamily,
    chat_id: String,
    /// For a group, the CSV session it was read from.
    group_session: Option<GroupSession>,
}

/// The CSV session a group was read from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GroupSession {
    /// The CSV's place in discovery order.
    csv_index: usize,
    /// The session's `Chat Session` value.
    session_name: String,
}

/// Give every group a key of its own when several start with the same row.
///
/// A group's key is its earliest row, and two groups can start with the same
/// row: the account holder sends one message to two new groups in the same
/// second. Of the groups that share an earliest row:
///
/// - Those with one session name are one group, read from two exports in
///   the same input folder, and are merged. Groups with different session
///   names are never merged, even when one's rows are the first rows of the
///   other's.
/// - Each other one hashes its earliest rows, as few as tell it apart from
///   every one of the others ([`group_vendor_id`]).
///
/// Such a key depends on the groups it is told apart from, so it changes
/// when one of them is gone from the phone, or a new one shares more of its
/// earliest rows.
fn separate_groups_with_one_earliest_row(conversations: &mut BTreeMap<ConvoKey, Conversation>) {
    let mut by_chat_id: BTreeMap<(TransportFamily, String), Vec<ConvoKey>> = BTreeMap::new();
    for key in conversations
        .keys()
        .filter(|key| key.group_session.is_some())
    {
        by_chat_id
            .entry((key.family, key.chat_id.clone()))
            .or_default()
            .push(key.clone());
    }
    for mut keys in by_chat_id.into_values().filter(|keys| keys.len() > 1) {
        // Longest first, so a group read from an older export folds into
        // the newest one.
        keys.sort_by_key(|key| std::cmp::Reverse(conversations[key].row_digests.len()));
        let mut kept: Vec<ConvoKey> = Vec::new();
        for key in keys {
            let Some(same_group) = kept
                .iter()
                .find(|held| session_name(held) == session_name(&key))
                .cloned()
            else {
                kept.push(key);
                continue;
            };
            let older = conversations.remove(&key).expect("listed above");
            merge_group_into(
                conversations.get_mut(&same_group).expect("kept above"),
                older,
            );
        }
        if kept.len() < 2 {
            continue;
        }
        let ids: Vec<String> = kept
            .iter()
            .map(|key| {
                let digests = &conversations[key].row_digests;
                let shared = kept
                    .iter()
                    .filter(|other| *other != key)
                    .map(|other| {
                        digests
                            .iter()
                            .zip(&conversations[other].row_digests)
                            .take_while(|(a, b)| a == b)
                            .count()
                    })
                    .max()
                    .unwrap_or(0);
                let same_rows = kept
                    .iter()
                    .any(|other| other != key && conversations[other].row_digests == *digests);
                if same_rows {
                    return group_vendor_id_with_name(digests, session_name(key).unwrap_or(""));
                }
                // One row past the longest shared run is one of its own.
                // `group_vendor_id` takes all the rows when there are fewer.
                group_vendor_id(digests, shared + 1)
            })
            .collect();
        for (key, id) in kept.iter().zip(ids) {
            let conversation = conversations.get_mut(key).expect("kept above");
            if let ConversationKey::Group { vendor_id, .. } = &mut conversation.key {
                *vendor_id = id;
            }
            conversation.convo.chat_id = conversation.key.chat_id();
        }
    }
}

/// The session name of a group's [`ConvoKey`].
fn session_name(key: &ConvoKey) -> Option<&str> {
    key.group_session
        .as_ref()
        .map(|session| session.session_name.as_str())
}

/// Fold `other`, the same group read from another export, into `into`.
fn merge_group_into(into: &mut Conversation, other: Conversation) {
    into.convo.messages.extend(other.convo.messages);
    if let (
        ConversationKey::Group { members, .. },
        ConversationKey::Group {
            members: other_members,
            ..
        },
    ) = (&mut into.key, other.key)
    {
        for member in other_members {
            let same = |held: &IrParticipant| {
                held.identity == member.identity && held.display_name == member.display_name
            };
            if !members.iter().any(same) {
                members.push(member);
            }
        }
    }
}

/// Parse-time state shared across every CSV file in one export.
struct Ingest {
    tz: Zone,
    copy_attachments: bool,
    conversations: BTreeMap<ConvoKey, Conversation>,
    /// Every row matched to a file, in the order the rows were read.
    claims: Vec<FileClaim>,
    /// Each Messages chat folder's Messages row texts, keyed by the row's
    /// `Message Date` as iMazing writes it into a file name, filled only when
    /// attachments are copied. A Messages chat folder is one that holds a
    /// Messages CSV; only iMazing's Messages export writes files without a
    /// row (Live Photo videos, link previews), so only these folders are
    /// walked for them.
    folder_texts: BTreeMap<PathBuf, HashMap<String, Vec<String>>>,
    /// Every chat folder that holds a WhatsApp CSV. A file there that no row
    /// names may be a WhatsApp file, so `attach_unnamed_files` counts none of
    /// them, and attaches only a Live Photo video whose picture a Messages
    /// row names.
    whatsapp_folders: HashSet<PathBuf>,
    report: ExportReport,
}

/// The rows [`Ingest::tell_apart_files_of_one_name`] compares: those of one
/// conversation and one second whose row names one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NameGroup {
    convo_key: ConvoKey,
    /// The row's `Attachment` cell, which is its attachment's `rel_path`.
    name: String,
    second: i64,
}

impl NameGroup {
    /// The group of a message in `convo_key`, or `None` when its row names
    /// no file.
    fn of(convo_key: &ConvoKey, message: &PendingMessage) -> Option<Self> {
        let attachment = message.attachments.first()?;
        Some(Self {
            convo_key: convo_key.clone(),
            name: attachment.rel_path.clone(),
            second: message.sort_key,
        })
    }
}

/// A row matched to a file on disk, and the message the row became.
struct FileClaim {
    source: PathBuf,
    /// Whether the row's `Attachment type` is Image.
    is_image: bool,
    /// The row's `Attachment` cell.
    csv_name: String,
    /// Where the row sits in the export: the CSV's place in discovery order,
    /// then the row's place in that CSV.
    order: (usize, usize),
    convo_key: ConvoKey,
    message: usize,
}

impl Ingest {
    /// Parse one CSV and fold every chat session in it into the pending conversations.
    ///
    /// A file that fails to parse is recorded in the report and skipped so
    /// one bad export does not stop the rest.
    ///
    /// # Errors
    ///
    /// Returns an error when the CSV's chat folder, or one of its entries,
    /// cannot be read while looking for the rows' files.
    fn ingest_file(&mut self, csv_index: usize, discovered: &DiscoveredCsv) -> Result<()> {
        match discovered.kind {
            SourceKind::Messages => self.report.bump("messages_files", 1),
            SourceKind::WhatsApp => self.report.bump("whatsapp_files", 1),
        }
        let folder = csv_folder(discovered).to_path_buf();
        if discovered.kind == SourceKind::WhatsApp {
            self.whatsapp_folders.insert(folder.clone());
        }
        let rows = match parse_csv_file(&discovered.path, discovered.kind) {
            Ok(rows) => rows,
            Err(e) => {
                self.report.error(
                    discovered.path.display().to_string(),
                    format!("{}: {e:#}", message_crate_core::CSV_NOT_READ),
                );
                return Ok(());
            }
        };
        // Each row's second as iMazing writes it into a file name, worked
        // out once for both uses below.
        let seconds: Vec<Option<String>> = rows
            .iter()
            .map(|row| file_name_second(&row.message_date))
            .collect();
        // Only a run that copies attachments looks for a row's file.
        let sources = if self.copy_attachments {
            row_sources(&rows, &seconds, &FolderFiles::read(&folder)?)
        } else {
            vec![None; rows.len()]
        };
        // Only a run that copies attachments looks at the folder's files
        // (`attach_unnamed_files`), and only in a Messages chat folder, so
        // only a Messages CSV in such a run gives texts.
        let mut texts = (self.copy_attachments && discovered.kind == SourceKind::Messages)
            .then(|| self.folder_texts.entry(folder).or_default());
        let mut by_session: BTreeMap<String, Vec<(usize, &RawRow)>> = BTreeMap::new();
        for (row_index, row) in rows.iter().enumerate() {
            if let Some(second) = &seconds[row_index]
                && let Some(texts) = texts.as_mut()
                && !row.text.is_empty()
            {
                texts
                    .entry(second.clone())
                    .or_default()
                    .push(row.text.clone());
            }
            by_session
                .entry(row.chat_session.clone())
                .or_default()
                .push((row_index, row));
        }
        let csv = CsvContext {
            index: csv_index,
            sources: &sources,
        };
        for (session, session_rows) in by_session {
            self.ingest_session(&csv, discovered, &session, &session_rows);
        }
        Ok(())
    }

    /// Work out the key of one chat session, then add each of its rows.
    ///
    /// The rows of one session in one CSV are one conversation; a real
    /// export writes one session to each CSV.
    fn ingest_session(
        &mut self,
        csv: &CsvContext<'_>,
        discovered: &DiscoveredCsv,
        session_name: &str,
        rows: &[(usize, &RawRow)],
    ) {
        let session_rows: Vec<&RawRow> = rows.iter().map(|(_, row)| *row).collect();
        let session = session_key(discovered.kind, session_name, &session_rows);
        let csv_path = discovered.path.display();
        if session.key.is_name_only() {
            self.report.caveat(
                message_crate_core::NAME_ONLY_CHAT,
                1,
                format!("{csv_path} ({session_name})"),
                message_crate_core::NAME_ONLY_CHAT_NOTE,
            );
        }
        for label in &session.unresolved_roster_labels {
            self.report.caveat(
                "unresolved_group_participants",
                1,
                format!("{csv_path} ({label})"),
                "The group's name lists this member, but no message gives their phone number or \
                 email address, so they are kept by name.",
            );
        }
        let chat_id = session.key.chat_id();
        let convo_key = ConvoKey {
            family: TransportFamily::from_kind(discovered.kind),
            chat_id: chat_id.clone(),
            group_session: session.key.is_group().then(|| GroupSession {
                csv_index: csv.index,
                session_name: session_name.to_string(),
            }),
        };
        self.conversations
            .entry(convo_key.clone())
            .or_insert_with(|| {
                let is_group = session.key.is_group();
                let mut convo = PendingConversation::new(
                    chat_id,
                    is_group,
                    is_group.then(|| session_name.to_string()),
                    Vec::new(),
                );
                convo
                    .extra
                    .insert("source_kind".into(), discovered.kind.as_str().to_string());
                Conversation {
                    key: session.key.clone(),
                    address: session.address.clone(),
                    convo,
                    row_digests: session.row_digests.clone(),
                }
            });
        for &(row_index, row) in rows {
            let Some(message) = self.message_from_row(discovered, row, &session, csv, row_index)
            else {
                continue;
            };
            let messages = &mut self
                .conversations
                .get_mut(&convo_key)
                .expect("conversation inserted above")
                .convo
                .messages;
            let source = message.extra_str(&attachment_source_key(0));
            if !source.is_empty() {
                self.claims.push(FileClaim {
                    source: PathBuf::from(source),
                    is_image: row.attachment_type.trim().eq_ignore_ascii_case("image"),
                    csv_name: row.attachment.clone(),
                    order: (csv.index, row_index),
                    convo_key: convo_key.clone(),
                    message: messages.len(),
                });
            }
            messages.push(message);
        }
    }

    /// Build the pending message for one CSV row, or `None` when the row has
    /// no usable date or repeats a row already seen in this conversation.
    fn message_from_row(
        &mut self,
        discovered: &DiscoveredCsv,
        row: &RawRow,
        session: &Session,
        csv: &CsvContext<'_>,
        row_index: usize,
    ) -> Option<PendingMessage> {
        let Some(secs) = parse_message_date(&row.message_date, self.tz) else {
            self.report.skipped_invalid_date += 1;
            return None;
        };
        let is_notification = is_notification(&row.msg_type);
        let is_from_me = !is_notification && is_outgoing(&row.msg_type);
        let (sender_identity, sender_display_name) =
            resolve_sender(row, is_from_me, is_notification, session);
        let (attachments, attachment_extra) =
            attachment_for_row(row, csv.sources[row_index].as_deref());
        let service = if row.service.trim().is_empty() {
            match discovered.kind {
                SourceKind::WhatsApp => "WhatsApp".to_string(),
                SourceKind::Messages => "SMS".to_string(),
            }
        } else {
            row.service.clone()
        };

        let mut extra = BTreeMap::new();
        extra.insert(
            "is_notification".into(),
            if is_notification { "true" } else { "false" }.into(),
        );
        extra.insert("subject".into(), row.subject.clone());
        extra.insert("contact_name".into(), session.contact_name.clone());
        extra.insert("service".into(), service);
        extra.insert("imazing_status".into(), row.status.clone());
        extra.insert("imazing_type".into(), row.msg_type.clone());
        extra.insert("reactions".into(), row.reactions.clone());
        extra.insert("replying_to".into(), row.replying_to.clone());
        extra.insert("forwarded".into(), row.forwarded.clone());
        extra.insert("attachment_info".into(), row.attachment_info.clone());
        extra.insert("delivered_date".into(), row.delivered_date.clone());
        extra.insert("read_date".into(), row.read_date.clone());
        extra.insert("edited_date".into(), row.edited_date.clone());
        extra.insert("deleted_date".into(), row.deleted_date.clone());
        extra.insert("sent_date".into(), row.sent_date.clone());
        extra.extend(attachment_extra);

        Some(PendingMessage {
            sort_key: secs,
            is_from_me,
            sender_identity,
            sender_display_name: (!sender_display_name.is_empty()).then_some(sender_display_name),
            text: row.text.clone(),
            attachments,
            extra,
        })
    }

    /// Deal with the files that no row names in each Messages chat folder:
    /// attach a Live Photo's video to the message of the Messages Image row
    /// that names its picture, and count link previews and every other such
    /// file in the report. In a folder that also holds a WhatsApp CSV, such a
    /// file may be WhatsApp's, so only the Live Photo videos are dealt with.
    ///
    /// Runs only when attachments are copied, because only then is any row
    /// matched to a file, so only then is "named by no row" known.
    ///
    /// # Errors
    ///
    /// Returns an error when a chat folder, or one of its entries, cannot be
    /// read.
    fn attach_unnamed_files(&mut self) -> Result<()> {
        let named: HashSet<PathBuf> = self.claims.iter().map(|c| c.source.clone()).collect();
        // Each picture an Image row names, with those rows in CSV order.
        let mut pictures: HashMap<PathBuf, Vec<usize>> = HashMap::new();
        for (index, claim) in self.claims.iter().enumerate() {
            if claim.is_image && claim.convo_key.family == TransportFamily::Messages {
                pictures
                    .entry(claim.source.clone())
                    .or_default()
                    .push(index);
            }
        }
        for rows in pictures.values_mut() {
            rows.sort_by_key(|&index| self.claims[index].order);
        }
        let mut found = Vec::new();
        for (folder, texts_at) in &self.folder_texts {
            let rows = FolderRows {
                named: &named,
                pictures: &pictures,
                texts_at,
            };
            let counted = !self.whatsapp_folders.contains(folder);
            found.extend(
                unnamed_files(folder, &rows)?
                    .into_iter()
                    .map(|file| (file, counted)),
            );
        }
        for (file, counted) in found {
            match file {
                UnnamedFile::LivePhotoVideo { video, picture } => {
                    self.attach_live_photo_video(&video, &picture, &pictures[&picture]);
                }
                UnnamedFile::LinkPreview if counted => {
                    self.report.bump("link_previews_already_in_message", 1);
                }
                UnnamedFile::Other if counted => self.report.bump("files_named_by_no_row", 1),
                UnnamedFile::LinkPreview | UnnamedFile::Other => {}
            }
        }
        Ok(())
    }

    /// Give the dedupe step what tells apart the rows of one conversation and
    /// one second that name one file and that iMazing wrote different files
    /// for (`image0.jpg`, `image0 2.jpg`): the SHA-256 of each file. The
    /// `Attachment` cell is the same for every such row, so without the
    /// digest the step keeps one of them and drops the other row and its
    /// picture. Files with the same content give one digest, so their rows
    /// are still one message. A file that cannot be read is told apart by
    /// its path; the writer reports it when it copies the file.
    ///
    /// A group is hashed only when one CSV holds two or more of its rows
    /// with different files. Its rows from every CSV are then hashed, so
    /// the copies a second export of the chat holds still match. A row of
    /// the group that has no file, from an export that lacks the files,
    /// matches any of the hashed copies ([`attachment_digests`]) rather than
    /// staying as a message of its own.
    ///
    /// Every other row's cell already tells it apart, and its message id
    /// stays the same whether its file is found and whatever other export
    /// the run reads.
    fn tell_apart_files_of_one_name(&mut self) {
        let mut groups: BTreeMap<NameGroup, Vec<usize>> = BTreeMap::new();
        for (index, claim) in self.claims.iter().enumerate() {
            let message = &self.conversations[&claim.convo_key].convo.messages[claim.message];
            if let Some(group) = NameGroup::of(&claim.convo_key, message) {
                groups.entry(group).or_default().push(index);
            }
        }
        let mut digests: Vec<(ConvoKey, usize, String)> = Vec::new();
        let mut hashed: BTreeSet<NameGroup> = BTreeSet::new();
        for (group, claims) in groups {
            let mut files_by_csv: BTreeMap<usize, HashSet<&Path>> = BTreeMap::new();
            for &index in &claims {
                let claim = &self.claims[index];
                files_by_csv
                    .entry(claim.order.0)
                    .or_default()
                    .insert(claim.source.as_path());
            }
            if files_by_csv.values().all(|files| files.len() < 2) {
                continue;
            }
            for index in claims {
                let claim = &self.claims[index];
                let digest = message_ir::file_sha256(&claim.source)
                    .unwrap_or_else(|_| claim.source.to_string_lossy().into_owned());
                digests.push((claim.convo_key.clone(), claim.message, digest));
            }
            hashed.insert(group);
        }
        for (convo_key, message, digest) in digests {
            self.messages_mut(&convo_key)[message]
                .extra
                .insert(attachment_content_key(0), digest);
        }
        let convo_keys: BTreeSet<ConvoKey> =
            hashed.iter().map(|group| group.convo_key.clone()).collect();
        for convo_key in convo_keys {
            for message in self.messages_mut(&convo_key) {
                let no_file = message.extra_str(&attachment_source_key(0)).is_empty();
                if no_file
                    && NameGroup::of(&convo_key, message)
                        .is_some_and(|group| hashed.contains(&group))
                {
                    message
                        .extra
                        .insert(attachment_matches_any_file_key(0), "true".into());
                }
            }
        }
    }

    /// The messages of the conversation `convo_key` names.
    fn messages_mut(&mut self, convo_key: &ConvoKey) -> &mut Vec<PendingMessage> {
        &mut self
            .conversations
            .get_mut(convo_key)
            .expect("a claim names a conversation that exists")
            .convo
            .messages
    }

    /// Add `video` to the message of the first of `rows`, the claims of the
    /// Image rows that name `picture` in CSV order. When more than one row
    /// names it, the report says which picture and that the first row took
    /// the video, as a note: the run did what it says, so it is no error.
    fn attach_live_photo_video(&mut self, video: &Path, picture: &Path, rows: &[usize]) {
        let first = &self.claims[rows[0]];
        if rows.len() > 1 {
            self.report.note(
                picture.display().to_string(),
                format!(
                    "{} rows name this picture; its Live Photo video goes to the first of them in the CSV",
                    rows.len()
                ),
            );
        }
        // The picture's name as the row gives it, with the video's extension:
        // the name the phone gave the video.
        let extension = video
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mov")
            .to_ascii_lowercase();
        let name = Path::new(&first.csv_name)
            .with_extension(extension)
            .to_string_lossy()
            .into_owned();
        let message = &mut self
            .conversations
            .get_mut(&first.convo_key)
            .expect("a claim names a conversation that exists")
            .convo
            .messages[first.message];
        message.extra.insert(
            attachment_source_key(message.attachments.len()),
            video.to_string_lossy().into_owned(),
        );
        message.attachments.push(PendingAttachment {
            rel_path: name.clone(),
            content_type: mime_hint("", &name).unwrap_or_default(),
            digest_sha256: None,
            name_hint: Some(name),
            size_bytes: None,
        });
        self.report.bump("live_photo_videos", 1);
    }
}

/// The chat folder a CSV sits in: the folder iMazing wrote its media into.
fn csv_folder(discovered: &DiscoveredCsv) -> &Path {
    discovered.path.parent().unwrap_or_else(|| Path::new("."))
}

/// The attachment a row names (iMazing rows carry at most one), plus the
/// sticker and transcription metadata that rides on the message.
///
/// `source` is the file iMazing wrote for the row ([`row_sources`]).
fn attachment_for_row(
    row: &RawRow,
    source: Option<&Path>,
) -> (Vec<PendingAttachment>, BTreeMap<String, String>) {
    if row.attachment.is_empty() {
        return (Vec::new(), BTreeMap::new());
    }
    let cell = attachment_cell(&row.attachment, &row.attachment_type);
    let attachment = PendingAttachment {
        rel_path: row.attachment.clone(),
        content_type: cell.meta.mime_type.clone().unwrap_or_default(),
        digest_sha256: None,
        name_hint: cell.meta.original_name.clone(),
        size_bytes: None,
    };
    let mut extra = BTreeMap::new();
    extra.insert(
        "is_sticker".into(),
        if cell.is_sticker { "true" } else { "false" }.into(),
    );
    extra.insert(
        "transcription".into(),
        cell.transcription.unwrap_or_default(),
    );
    extra.insert(
        "sticker_effect".into(),
        cell.sticker_effect.unwrap_or_default(),
    );
    if let Some(src) = source {
        extra.insert(attachment_source_key(0), src.to_string_lossy().into_owned());
    }
    (vec![attachment], extra)
}

/// What the rows read from one CSV share: its place in the export and the
/// file iMazing wrote for each row.
struct CsvContext<'a> {
    /// The CSV's place in discovery order.
    index: usize,
    /// Each row's file ([`row_sources`]), in the CSV's order. All `None`
    /// when the run does not copy attachments.
    sources: &'a [Option<PathBuf>],
}

/// `__whatsapp` for WhatsApp chats so their files do not collide with Messages files for the same peer.
fn imazing_packaging_stem_suffix(source_kind: &str) -> Option<String> {
    if source_kind == "whatsapp" {
        Some("__whatsapp".into())
    } else {
        None
    }
}

/// iMazing deltas of the shared [`message_ir::pending_to_document`]
/// projection, for one conversation.
struct ImazingProjection<'a> {
    export: &'a ExportMeta,
    /// The key of the conversation being projected.
    key: &'a ConversationKey,
    /// A one-to-one conversation's address (`Session::address`).
    address: Option<&'a Handle>,
    /// The source file of each attachment of the messages the projection
    /// keeps, in the order it writes them, which is the order the writer
    /// asks for them. A message the dedupe step drops is never mapped, so
    /// its file is not here and cannot go to the next message.
    sources: RefCell<Vec<Option<PathBuf>>>,
}

impl ProjectionHooks for ImazingProjection<'_> {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, msg: &PendingMessage) -> IrService {
        IrService::parse(msg.extra_str("service"))
    }

    fn role(&self, msg: &PendingMessage) -> ProjectedRole {
        if msg.extra_flag("is_notification") {
            ProjectedRole::Notification
        } else if msg.is_from_me {
            ProjectedRole::Outgoing
        } else {
            ProjectedRole::Incoming
        }
    }

    fn subject(&self, msg: &PendingMessage) -> Option<String> {
        msg.extra_opt("subject")
    }

    fn attachment_digests(&self, msg: &PendingMessage) -> Vec<String> {
        attachment_digests(msg)
    }

    fn attachment_to_ir(&self, att: &PendingAttachment, msg: &PendingMessage) -> IrAttachment {
        // A message's attachments have different names: the row's file, and
        // a Live Photo video named as the picture with the video's extension.
        let source = msg
            .attachments
            .iter()
            .position(|held| held.rel_path == att.rel_path)
            .map(|index| msg.extra_str(&attachment_source_key(index)))
            .filter(|source| !source.is_empty())
            .map(PathBuf::from);
        self.sources.borrow_mut().push(source);
        pending_attachment_to_ir(att, msg)
    }

    /// A group's members come from its key, never from its chat id. A
    /// one-to-one conversation's one participant is the person it is with:
    /// their address, or for a conversation keyed by a name, the name and no
    /// address.
    fn participants(&self, _chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        match self.key {
            ConversationKey::Group { members, .. } => members.clone(),
            ConversationKey::OneToOne(handle) => vec![IrParticipant {
                identity: Some(handle.clone()),
                display_name: convo.first_contact_name(),
                identity_type: self.address.map(Handle::kind),
            }],
            ConversationKey::NameOnly(_) => vec![IrParticipant {
                identity: None,
                display_name: convo.first_contact_name(),
                identity_type: None,
            }],
        }
    }

    fn packaging_stem_suffix(&self, convo: &PendingConversation) -> Option<String> {
        imazing_packaging_stem_suffix(convo.extra_str("source_kind"))
    }

    fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
        let mut fields = Map::new();
        // Session string is not a real group title: stored as data only
        // (the document's `group_title` stays `None`, matching the previous
        // CSV/mail stem).
        let session_title = convo.display_name.as_deref().unwrap_or("");
        if !session_title.is_empty() {
            fields.insert(
                "group_title".into(),
                serde_json::Value::String(session_title.to_string()),
            );
        }
        for key in [
            "imazing_status",
            "imazing_type",
            "reactions",
            "replying_to",
            "forwarded",
            "attachment_info",
            "delivered_date",
            "read_date",
            "edited_date",
            "deleted_date",
            "sent_date",
        ] {
            let val = msg.extra_str(key);
            if !val.is_empty() {
                fields.insert(key.into(), serde_json::Value::String(val.to_string()));
            }
        }
        IrSource {
            android_type: None,
            fields,
        }
    }
}

#[cfg(test)]
mod tests;
