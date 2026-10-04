//! Convert SMS Backup+ `.eml` trees into the shared conversation structure,
//! then write the chosen output format via [`ExportWriter`].

use crate::attachments_emit::queue_attachments;
use crate::email_numbers::{
    EmailNumbers, KeptByEmail, key_members_by_number, names_a_member_by_email,
};
use crate::flat_eml::Owner;
use crate::identity::{chat_id_for, timestamp_ms};
use crate::parse_emit::{ParsedEmlKind, collect_eml_paths, parse_one_eml};
use crate::types::ParsedMessage;
use anyhow::{Result, bail};
use message_crate_core::{
    CancelFlag, ExportReport, ExportTransforms, IssueSink, LogSink, OutputFormat, RunIssue,
    emit_issue, emit_log, prepare_outputs, project_conversation,
};
use message_ir::{
    ConversationDocument, ExportMeta, IrConversationType, IrDirection, IrParticipant, IrService,
    IrSource, PendingAttachment, PendingConversation, PendingMessage, ProjectionHooks,
    default_participants, parse_android_type,
};
use message_staging::{AttachmentSource, AttachmentSpool, ExportWriter};
use phone::{Handle, OwnerHandleSet};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = "sms-backup-plus";
const EXPORT_TOOL: &str = "SMS Backup+";
const EXPORT_TOOL_VERSION: &str = "1.5.11";

/// Report counter: received group messages whose `From` names nobody in the
/// group, kept with no sender. Counted after copies are reduced to one, and
/// each one is also an issue in the Import Run.
const GROUP_MESSAGES_WITHOUT_SENDER: &str = "group_messages_without_sender";

/// Report counter: received MMS whose `To` names a group but none of the
/// owner's numbers or email addresses, filed one-to-one under `From` instead,
/// or under `X-smssync-address` when `From` gives no address. Counted after
/// copies are reduced to one, and each one is also a note in the Import Run.
const GROUP_MESSAGES_OWNER_NOT_NAMED: &str = "group_messages_owner_not_named";

/// Report counter: messages that record no address for the other person,
/// kept in a conversation under the name the mail gives, or in the one that
/// names nobody. Counted after copies are reduced to one, and each one is
/// also a note in the Import Run.
const UNKNOWN_CHAT_MESSAGES: &str = "unknown_chat_messages";

/// Report counter: group members the archive names only by email address,
/// never with a number, so the address stays their key. Each is counted
/// once, however many mails name them (#1545), and is a note in the Import
/// Run.
const GROUP_MEMBERS_WITHOUT_NUMBER: &str = "group_members_without_number";

/// Report counter: group members whose email address the archive gives two
/// or more numbers, as a contact card two people share does, so the address
/// stays their key. Each is counted once. One number written in national
/// form in some mails and international form in others (`07700900123`,
/// `+447700900123`) counts as two, since only a `+` number is read as
/// international. Each is a note in the Import Run.
const GROUP_MEMBERS_WITH_SEVERAL_NUMBERS: &str = "group_members_with_several_numbers";

/// The EML's path relative to the input root it was found under, for the vendor `source` bag.
///
/// An EML given as an input itself is recorded under its file name: its path
/// relative to itself is empty, and an empty path names no file.
fn relative_eml_path(
    eml_path: &Path,
    inputs: &[PathBuf],
    file_inputs: &HashSet<PathBuf>,
) -> String {
    for root in inputs {
        if file_inputs.contains(root) && eml_path == root {
            return root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_else(|| eml_path.to_str().unwrap_or(""))
                .to_string();
        }
        if let Ok(rel) = eml_path.strip_prefix(root)
            && !rel.as_os_str().is_empty()
        {
            return rel.display().to_string();
        }
    }
    eml_path.display().to_string()
}

/// Get or create the pending conversation for `chat_id`, unioning rosters.
///
/// Unlike the shared `ensure_conversation` (which seeds a new entry only),
/// group membership changes over time here, and a later message's smaller
/// roster must not shrink the participant list. (A roster change that yields a
/// different `chat_key` still splits the conversation into fragments; this keeps
/// each fragment's participant list complete within that key.)
fn ensure_convo<'a>(
    map: &'a mut HashMap<String, PendingConversation>,
    chat_id: &str,
    is_group: bool,
    display_name: Option<String>,
    participant_e164s: Vec<String>,
) -> &'a mut PendingConversation {
    // Avoid allocating a new String on every message for an existing chat.
    if !map.contains_key(chat_id) {
        map.insert(
            chat_id.to_string(),
            PendingConversation::new(chat_id, is_group, display_name, Vec::new()),
        );
    }
    let convo = map
        .get_mut(chat_id)
        .expect("just inserted or already present");
    convo.participant_e164s.extend(participant_e164s);
    convo.participant_e164s.sort();
    convo.participant_e164s.dedup();
    convo
}

/// Map a parsed EML message onto the pending message shape.
///
/// The `date_ms` extra is set only when the mail carried milliseconds, so the
/// projection knows a whole-second copy from a millisecond one.
fn pending_from_parsed(msg: ParsedMessage, pending_atts: Vec<PendingAttachment>) -> PendingMessage {
    let name = msg.name_alias.clone().unwrap_or_default();
    PendingMessage {
        sort_key: msg.timestamp_secs as i64,
        is_from_me: msg.is_from_me,
        sender_handle: msg.sender.map(Handle::into_key).unwrap_or_default(),
        sender_display_name: msg.name_alias,
        text: msg.text,
        attachments: pending_atts,
        extra: {
            let mut e = BTreeMap::new();
            e.insert("smssync_id".into(), msg.smssync_id.unwrap_or_default());
            if msg.has_milliseconds {
                e.insert(
                    "date_ms".into(),
                    timestamp_ms(msg.timestamp_secs).to_string(),
                );
            }
            e.insert("contact_name".into(), name);
            e.insert("android_type".into(), msg.android_type);
            e.insert("eml_path".into(), msg.eml_path);
            e
        },
    }
}

/// Add a parsed message to its conversation. Copies of one message are
/// kept here and reduced to one by the shared projection.
fn add_message(
    conversations: &mut HashMap<String, PendingConversation>,
    msg: ParsedMessage,
    pending_atts: Vec<PendingAttachment>,
    report: &mut ExportReport,
) {
    let chat_id = chat_id_for(&msg);

    let peers: Vec<String> = msg
        .participants
        .iter()
        .map(|p| p.key().to_string())
        .collect();
    let convo = ensure_convo(
        conversations,
        &chat_id,
        msg.is_group(),
        msg.group_title.clone(),
        peers,
    );

    report.bump("messages_before_dedupe", 1);
    convo.messages.push(pending_from_parsed(msg, pending_atts));
}

/// True when the path has a `.eml` extension (any case).
pub(super) fn is_eml_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("eml"))
}

/// SMS Backup+ deltas of the shared [`message_ir::pending_to_document`] projection.
struct SbpProjection {
    export: ExportMeta,
}

impl ProjectionHooks for SbpProjection {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, _msg: &PendingMessage) -> IrService {
        IrService::Sms
    }

    /// Every handle is a [`Handle`] key already.
    fn normalize_handle(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// The default roster, with each identity's kind read from its key: an
    /// SMS Backup+ address can be an email address or a sender name.
    fn participants(&self, chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        let mut participants = default_participants(chat_id, convo, &str::to_string);
        for p in &mut participants {
            if let Some(handle) = p.handle.as_deref().and_then(Handle::parse) {
                p.handle_type = Some(handle.kind());
            }
        }
        participants
    }

    /// Of two copies of one message, the dedupe step keeps the first one
    /// this order puts first when nothing else decides. A copy that carries
    /// `X-smssync-id` comes first, because only some export routes keep it,
    /// and then the `.eml` path, so the copy kept does not depend on the
    /// order the files were found in. The id never decides whether two
    /// copies are one message.
    fn message_order(&self, a: &PendingMessage, b: &PendingMessage) -> std::cmp::Ordering {
        let no_id = |m: &PendingMessage| m.extra_str("smssync_id").trim().is_empty();
        a.sort_key
            .cmp(&b.sort_key)
            .then_with(|| no_id(a).cmp(&no_id(b)))
            .then_with(|| a.extra_str("eml_path").cmp(b.extra_str("eml_path")))
    }

    fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
        let mut fields = serde_json::Map::new();
        for key in ["smssync_id", "eml_path"] {
            let value = msg.extra_str(key);
            if !value.is_empty() {
                fields.insert(key.into(), serde_json::Value::String(value.to_string()));
            }
        }
        if let Some(title) = convo.display_name.as_deref().filter(|t| !t.is_empty()) {
            // Android group title stored as data only. Filenames do not use it.
            fields.insert(
                "android_group_title".into(),
                serde_json::Value::String(title.to_string()),
            );
        }
        IrSource {
            android_type: parse_android_type(msg.extra_str("android_type")),
            fields,
        }
    }
}

/// The mails, by EML path, whose messages are kept with a caveat. The
/// messages are counted, and a note sent for each, once the projection has
/// reduced copies to one ([`project_and_count`]).
#[derive(Default)]
struct Caveats {
    /// Received MMS whose `To` named none of the owner's addresses.
    owner_not_named: HashSet<String>,
    /// Messages that record no address for the other person.
    unknown_chat: HashSet<String>,
}

/// Project one conversation, then count the messages it kept with a caveat,
/// and the group messages it kept with no sender, from the messages
/// written: copies dropped by the projection are not counted. Each is sent
/// to the report's issue sink as it is counted: a group message with no
/// sender as a skip, the others as notes.
fn project_and_count(
    chat_id: &str,
    convo: &mut PendingConversation,
    hooks: &SbpProjection,
    caveats: &Caveats,
    report: &mut ExportReport,
) -> Option<ConversationDocument> {
    let doc = project_conversation(chat_id, convo, hooks, report)?;
    let is_group = doc.conversation.conversation_type == IrConversationType::Group;
    for msg in &doc.messages {
        let eml_path = msg
            .source
            .as_ref()
            .and_then(|s| s.fields.get("eml_path"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if caveats.owner_not_named.contains(eml_path) {
            report.caveat(
                GROUP_MESSAGES_OWNER_NOT_NAMED,
                1,
                eml_path,
                "This group message names none of your phone numbers or email addresses, so it \
                 is kept as a one-to-one message from its sender.",
            );
        }
        if caveats.unknown_chat.contains(eml_path) {
            report.caveat(
                UNKNOWN_CHAT_MESSAGES,
                1,
                eml_path,
                "This message records no phone number or email address for the other person, \
                 so it is kept in a conversation under the name the message gives, or with \
                 nobody.",
            );
        }
        if is_group && msg.direction == IrDirection::Incoming && msg.sender_handle.is_none() {
            report.bump(GROUP_MESSAGES_WITHOUT_SENDER, 1);
            emit_issue(report.issues.as_ref(), RunIssue {
                kind: "skip".into(),
                step: "parse".into(),
                item: format!("{eml_path} (sender)"),
                reason: "The sender of this group message could not be read and was left out. The message itself is kept.".into(),
            });
        }
    }
    Some(doc)
}

const EML_PROGRESS_EVERY: u64 = 5000;

/// Verbose-only log output: every method is a no-op unless
/// `SmsBackupPlusConfig::verbose` is set.
#[derive(Clone, Copy)]
struct Verbose<'a> {
    enabled: bool,
    log: Option<&'a LogSink>,
}

impl Verbose<'_> {
    /// Write one line when verbose.
    fn line(self, msg: impl AsRef<str>) {
        if self.enabled {
            emit_log(self.log, msg);
        }
    }

    /// Write a `label: N / total` line every [`EML_PROGRESS_EVERY`] items and at the end.
    fn progress(self, label: &str, processed: u64, total: u64) {
        if !self.enabled || total == 0 {
            return;
        }
        if processed == total || processed.is_multiple_of(EML_PROGRESS_EVERY) {
            emit_log(self.log, format!("{label}: {processed} / {total}"));
        }
    }

    /// List the first twenty error lines from the report, and how many more there were.
    fn errors(self, report: &ExportReport) {
        if !self.enabled || report.errors.is_empty() {
            return;
        }
        emit_log(self.log, format!("errors: {}", report.errors.len()));
        for err in report.errors.iter().take(20) {
            emit_log(self.log, format!("  {err}"));
        }
        if report.errors.len() > 20 {
            emit_log(
                self.log,
                format!("  … and {} more", report.errors.len() - 20),
            );
        }
    }
}

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a, P: AsRef<Path>> {
    pub inputs: &'a [P],
    pub output_dir: &'a Path,
    /// The app's cache folder, which the run's attachment spool goes under.
    pub cache_dir: &'a Path,
    pub owner_phones: &'a [String],
    pub owner_emails: &'a [String],
    pub verbose: bool,
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    pub log: Option<&'a LogSink>,
    /// Where each row for the Import Run's record goes as it is recorded.
    pub issues: Option<&'a IssueSink>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
}

/// Convert SMS Backup+ EML tree(s) into the shared conversation structure, then
/// write the chosen output format.
///
/// Copies of one message, such as two exports of one mailbox, are reduced to
/// one by the shared projection (`message_ir::one_copy_per_message`).
/// When `cancel` is set, cooperative cancellation is checked during the EML walk
/// and while merging parse results.
///
/// # Errors
///
/// Returns an error when no input is given, no `.eml` files are found,
/// output overlaps an input, a file cannot be read or written, or the user
/// cancels.
pub(crate) fn convert_export<P: AsRef<Path>>(
    args: ConvertExportArgs<'_, P>,
) -> Result<ExportReport> {
    let ConvertExportArgs {
        inputs,
        output_dir,
        cache_dir,
        owner_phones,
        owner_emails,
        verbose,
        transforms,
        output_format,
        cancel,
        log,
        issues,
        resume,
    } = args;
    // Checked before the output folder is cleaned, so a refused run leaves it.
    if inputs.is_empty() {
        bail!("SMS Backup+ needs a backup directory");
    }
    let verbose = Verbose {
        enabled: verbose,
        log,
    };
    let owner = Owner::new(OwnerHandleSet::from_phones(owner_phones)?, owner_emails);
    let owner_handle = owner
        .primary_handle()
        .expect("from_phones guarantees a phone owner handle");
    verbose.line(format!("owner phones: {}", owner_phones.len()));
    verbose.line(format!("owner emails: {}", owner.email_count()));
    verbose.line(format!("output: {}", output_dir.display()));

    let input_paths: Vec<PathBuf> = inputs.iter().map(|p| p.as_ref().to_path_buf()).collect();
    let (inputs, output_dir) = prepare_outputs(&input_paths, output_dir)?;
    let writer =
        ExportWriter::open(&output_dir, output_format, transforms, resume)?.with_spool(cache_dir);

    let eml_paths = collect_eml_paths(&inputs, cancel)?;
    verbose.line(format!(
        "scanning {} .eml files (parallel parse)",
        eml_paths.len()
    ));
    message_crate_core::check_cancel(cancel)?;

    let parse = ParseInputs {
        file_inputs: inputs.iter().filter(|p| p.is_file()).cloned().collect(),
        input_roots: inputs,
        owner,
    };
    let mut ingest = EmlIngest::new(writer.spool(), eml_paths.len(), issues);
    parse_all_emls(&eml_paths, &parse, cancel, verbose, &mut ingest)?;
    ingest.add_members_by_number(verbose);
    verbose.line(ingest.parse_summary());
    let EmlIngest {
        conversations,
        mut report,
        caveats,
        ..
    } = ingest;

    let hooks = SbpProjection {
        export: message_crate_core::export_meta(
            EXPORT_SOURCE,
            EXPORT_TOOL,
            EXPORT_TOOL_VERSION,
            Some(owner_handle),
            None,
        ),
    };
    let mut documents = Vec::new();
    for (chat_id, mut convo) in conversations {
        message_crate_core::check_cancel(cancel)?;
        if let Some(doc) = project_and_count(&chat_id, &mut convo, &hooks, &caveats, &mut report) {
            documents.push(doc);
        }
    }

    if !writer.use_queue() {
        verbose.line(format!(
            "writing {} conversation files (duplicates dropped so far: {})",
            documents.len(),
            report.duplicates_dropped
        ));
    }
    writer.finish(
        documents,
        &mut AttachmentSource::take_bytes,
        cancel,
        &mut report,
    )?;

    verbose.line(format!(
        "done: conversations={} messages={} duplicates_dropped={} attachments={} group_without_sender={} group_owner_not_named={}",
        report.conversations,
        report.messages,
        report.duplicates_dropped,
        report.attachments_saved,
        report.extra(GROUP_MESSAGES_WITHOUT_SENDER),
        report.extra(GROUP_MESSAGES_OWNER_NOT_NAMED),
    ));
    verbose.errors(&report);
    Ok(report)
}

/// Read-only inputs every parallel EML parse needs.
struct ParseInputs {
    /// The `ExporterConfig::inputs` paths after output preparation; relative
    /// EML paths are computed against these.
    input_roots: Vec<PathBuf>,
    /// The subset of `input_roots` that are single files rather than folders.
    file_inputs: HashSet<PathBuf>,
    owner: Owner,
}

/// How many EMLs one parallel batch parses before its results are folded in.
///
/// Chunking keeps attachment payloads from all being held in memory at once.
const EML_PARSE_CHUNK: usize = 256;

/// Parse every EML in parallel chunks and fold the outcomes into `ingest`.
///
/// # Errors
///
/// Returns an error when the run is cancelled.
fn parse_all_emls(
    eml_paths: &[PathBuf],
    inputs: &ParseInputs,
    cancel: Option<&CancelFlag>,
    verbose: Verbose<'_>,
    ingest: &mut EmlIngest<'_>,
) -> Result<()> {
    let total = eml_paths.len() as u64;
    let mut scanned = 0u64;
    for chunk in eml_paths.chunks(EML_PARSE_CHUNK) {
        message_crate_core::check_cancel(cancel)?;
        let outcomes: Vec<ParsedEmlKind> = chunk
            .par_iter()
            .map(|eml_path| parse_eml_path(eml_path, inputs, cancel))
            .collect();
        for outcome in outcomes {
            message_crate_core::check_cancel(cancel)?;
            scanned += 1;
            verbose.progress("scanned", scanned, total);
            ingest.absorb(outcome)?;
        }
    }
    Ok(())
}

/// Parse one EML on a worker thread. Checks cancel first so a cancelled run
/// stops reading files promptly.
fn parse_eml_path(
    eml_path: &Path,
    inputs: &ParseInputs,
    cancel: Option<&CancelFlag>,
) -> ParsedEmlKind {
    if message_crate_core::is_cancelled(cancel) {
        return ParsedEmlKind::Cancelled;
    }
    let rel_path = relative_eml_path(eml_path, &inputs.input_roots, &inputs.file_inputs);
    parse_one_eml(eml_path, rel_path, &inputs.owner)
}

/// Everything the scan accumulates: conversations and the counts that end
/// up in the report.
struct EmlIngest<'a> {
    /// Where attachment payloads are written as they are parsed; `None`
    /// when the run does not copy attachments.
    spool: Option<&'a AttachmentSpool>,
    conversations: HashMap<String, PendingConversation>,
    report: ExportReport,
    /// The mails whose messages are kept with a caveat.
    caveats: Caveats,
    /// The number each email address stands for, from the one-to-one mails
    /// that give both.
    email_numbers: EmailNumbers,
    /// Messages that name a group member by email address, held until the
    /// whole archive has been read and the member can be keyed by number.
    /// Their attachments are already queued.
    by_email: Vec<(ParsedMessage, Vec<PendingAttachment>)>,
}

impl<'a> EmlIngest<'a> {
    /// Empty state, pre-sized for the typical ratio of chats to EML files,
    /// whose report sends its rows to `issues`.
    fn new(
        spool: Option<&'a AttachmentSpool>,
        eml_count: usize,
        issues: Option<&IssueSink>,
    ) -> Self {
        Self {
            spool,
            conversations: HashMap::with_capacity((eml_count / 4).min(50_000)),
            report: ExportReport::with_issues(issues.cloned()),
            caveats: Caveats::default(),
            email_numbers: EmailNumbers::default(),
            by_email: Vec::new(),
        }
    }

    /// Fold one parsed EML into the pending conversations.
    ///
    /// # Errors
    ///
    /// Returns an error when a worker saw the cancel flag, or an attachment
    /// cannot be written to the spool.
    fn absorb(&mut self, outcome: ParsedEmlKind) -> Result<()> {
        match outcome {
            ParsedEmlKind::Cancelled => bail!("cancelled"),
            ParsedEmlKind::Flat { msg } => {
                self.report.bump("flat_eml", 1);
                if msg.unreadable_parts > 0 {
                    let text = match msg.unreadable_parts {
                        1 => "1 part of this message could not be read and was left out. The \
                              message itself is kept."
                            .to_string(),
                        n => format!(
                            "{n} parts of this message could not be read and were left out. \
                             The message itself is kept."
                        ),
                    };
                    self.report.caveat(
                        "skipped_unreadable_part",
                        msg.unreadable_parts,
                        msg.eml_path.as_str(),
                        text,
                    );
                }
                self.add_parsed(*msg)?;
            }
            ParsedEmlKind::FlatNone => self.report.bump("skipped_parse_error", 1),
            ParsedEmlKind::CallLog => self.report.bump("skipped_call_log", 1),
            ParsedEmlKind::NotSms => self.report.bump("skipped_not_sms_backup_plus", 1),
            ParsedEmlKind::IoError { path, reason } => self.report.error(
                path,
                format!("This file could not be read and was left out: {reason}"),
            ),
            ParsedEmlKind::ParseError { path, reason } => {
                self.report.bump("skipped_parse_error", 1);
                self.report.error(
                    path,
                    format!("This file could not be read as a mail and was left out: {reason}"),
                );
            }
        }
        Ok(())
    }

    /// Queue one message's attachments and add it to its conversation.
    ///
    /// # Errors
    ///
    /// Returns an error when an attachment cannot be written to the spool.
    fn add_parsed(&mut self, mut msg: ParsedMessage) -> Result<()> {
        let atts = queue_attachments(&std::mem::take(&mut msg.attachments), self.spool)?;
        if let Some(pair) = msg.email_number.take() {
            self.email_numbers.record(pair);
        }
        if names_a_member_by_email(&msg) {
            self.by_email.push((msg, atts));
        } else {
            self.add_to_conversation(msg, atts);
        }
        Ok(())
    }

    /// Add the messages held for naming a group member by email address,
    /// each member keyed by the one number the archive gives that address,
    /// and count the members it gives none, or several.
    fn add_members_by_number(&mut self, verbose: Verbose<'_>) {
        let numbers = std::mem::take(&mut self.email_numbers).into_numbers();
        let mut kept = KeptByEmail::default();
        for (mut msg, atts) in std::mem::take(&mut self.by_email) {
            key_members_by_number(&mut msg, &numbers, &mut kept);
            self.add_to_conversation(msg, atts);
        }
        for (counter, addresses, what, text) in [
            (
                GROUP_MEMBERS_WITHOUT_NUMBER,
                kept.without_number,
                "no number",
                "The backup gives this group member no phone number, so they are kept by this \
                 email address.",
            ),
            (
                GROUP_MEMBERS_WITH_SEVERAL_NUMBERS,
                kept.several_numbers,
                "more than one number",
                "The backup gives this email address more than one phone number, so the group \
                 member is kept by the email address.",
            ),
        ] {
            if addresses.is_empty() {
                continue;
            }
            for address in &addresses {
                self.report.caveat(counter, 1, address.as_str(), text);
            }
            verbose.line(format!(
                "group members with {what} in the archive, kept by email address: {}",
                addresses.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    /// Count one message and add it to its conversation.
    fn add_to_conversation(&mut self, msg: ParsedMessage, atts: Vec<PendingAttachment>) {
        if msg.chat_key.is_empty() {
            self.caveats.unknown_chat.insert(msg.eml_path.clone());
        }
        if msg.owner_not_named {
            self.caveats.owner_not_named.insert(msg.eml_path.clone());
        }
        add_message(&mut self.conversations, msg, atts, &mut self.report);
    }

    /// One line of parse counters for the verbose log.
    fn parse_summary(&self) -> String {
        format!(
            "parsed: flat_eml={} messages={} unknown_chat={} skipped_call_log={} skipped_not_sms_backup_plus={} skipped_parse_error={}",
            self.report.extra("flat_eml"),
            self.report.extra("messages_before_dedupe"),
            self.caveats.unknown_chat.len(),
            self.report.extra("skipped_call_log"),
            self.report.extra("skipped_not_sms_backup_plus"),
            self.report.extra("skipped_parse_error"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AttachmentBlob;
    use message_ir::{
        ConversationMeta, ConversationStats, IrMessage, IrMessageKind, SCHEMA_VERSION,
    };

    /// An EML given as an input is recorded under its own file name. An EML
    /// found in a folder input is recorded under its path inside that folder.
    #[test]
    fn an_eml_given_as_a_file_input_is_recorded_under_its_file_name() {
        let file = PathBuf::from("/backups/single/one.eml");
        let folder = PathBuf::from("/backups/tree");
        let inputs = vec![file.clone(), folder.clone()];
        let file_inputs: HashSet<PathBuf> = [file.clone()].into_iter().collect();

        assert_eq!(relative_eml_path(&file, &inputs, &file_inputs), "one.eml");
        assert_eq!(
            relative_eml_path(&folder.join("SMS").join("two.eml"), &inputs, &file_inputs),
            Path::new("SMS").join("two.eml").display().to_string()
        );
        // An EML under none of the inputs keeps its whole path.
        assert_eq!(
            relative_eml_path(Path::new("/elsewhere/three.eml"), &inputs, &file_inputs),
            Path::new("/elsewhere/three.eml").display().to_string()
        );
    }

    #[test]
    fn queue_attachments_keeps_message_on_single_failure() {
        let dir = tempfile::tempdir().unwrap();
        let att_dir = dir.path().join("attachments");
        std::fs::create_dir_all(&att_dir).unwrap();
        // Empty bytes: the runner records file_missing and continues.
        let blobs = vec![
            AttachmentBlob {
                filename: "missing.jpg".into(),
                original_name: None,
                mime_type: Some("image/jpeg".into()),
                digest_hex: "aaa".into(),
                data: vec![],
            },
            AttachmentBlob {
                filename: "ok.jpg".into(),
                original_name: None,
                mime_type: Some("image/jpeg".into()),
                digest_hex: "bbb".into(),
                data: vec![4, 5, 6],
            },
        ];
        let spool = AttachmentSpool::new(dir.path());
        let queued = queue_attachments(&blobs, Some(&spool)).unwrap();
        assert_eq!(queued.len(), 2);

        let mut atts: Vec<_> = queued.iter().map(PendingAttachment::to_ir).collect();
        let mut report = ExportReport::default();
        let mut doc = ConversationDocument {
            schema_version: SCHEMA_VERSION,
            export: ExportMeta {
                source: String::new(),
                tool: String::new(),
                tool_version: String::new(),
                owner_handle: None,
                owner_display_name: None,
            },
            conversation: ConversationMeta {
                chat_identifier: "test".into(),
                conversation_type: IrConversationType::Individual,
                group_title: None,
                participants: Vec::new(),
                stats: ConversationStats::default(),
            },
            messages: vec![IrMessage {
                guid: "g".into(),
                timestamp_unix_ms: 0,
                direction: IrDirection::Incoming,
                service: IrService::Sms,
                message_kind: IrMessageKind::Mms,
                sender_handle: None,
                sender_display_name: None,
                owner_handle: None,
                subject: None,
                text: "hi".into(),
                attachments: std::mem::take(&mut atts),
                imessage: None,
                source: None,
            }],
            packaging_stem_suffix: None,
        };
        let mut sources: Vec<Option<AttachmentSource>> = doc
            .messages
            .iter()
            .flat_map(|msg| msg.attachments.iter().map(|att| spool.source(att)))
            .map(|spooled| spooled.map(|(source, _)| source))
            .collect();
        report.attachments_saved += message_crate_core::stage_conversation_attachments(
            doc.messages.iter_mut(),
            &att_dir,
            &message_crate_core::MediaConfig::default(),
            |i| match sources.get_mut(i) {
                Some(Some(source)) => message_staging::load_attachment_source(source),
                _ => Ok(None),
            },
            None,
            None,
            None,
        )
        .unwrap();
        // The missing source stays on the message; the good one is staged.
        assert_eq!(doc.messages[0].attachments.len(), 2);
        assert_eq!(
            doc.messages[0].attachments[0].missing_reason.as_deref(),
            Some("file_missing")
        );
        assert!(doc.messages[0].attachments[1].path.is_some());
        assert_eq!(report.attachments_saved, 1);
        assert_eq!(std::fs::read_dir(&att_dir).unwrap().count(), 1);
    }

    /// The messages of every conversation as the shared projection writes
    /// them, with the copies of one message reduced to one.
    fn project(parsed: Vec<ParsedMessage>) -> (Vec<IrMessage>, ExportReport) {
        let mut ingest = EmlIngest::new(None, parsed.len(), None);
        for msg in parsed {
            ingest.add_parsed(msg).unwrap();
        }
        let hooks = SbpProjection {
            export: message_crate_core::export_meta(
                EXPORT_SOURCE,
                EXPORT_TOOL,
                EXPORT_TOOL_VERSION,
                Some("+15555550100".into()),
                None,
            ),
        };
        let mut report = ingest.report;
        let caveats = ingest.caveats;
        let mut messages = Vec::new();
        let mut conversations: Vec<_> = ingest.conversations.into_iter().collect();
        conversations.sort_by(|a, b| a.0.cmp(&b.0));
        for (chat_id, mut convo) in conversations {
            if let Some(doc) =
                project_and_count(&chat_id, &mut convo, &hooks, &caveats, &mut report)
            {
                messages.extend(doc.messages);
            }
        }
        (messages, report)
    }

    /// A message from +15555550101, with or without the milliseconds.
    fn parsed(timestamp_secs: f64, has_milliseconds: bool, eml_path: &str) -> ParsedMessage {
        ParsedMessage {
            chat_key: "+15555550101".into(),
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: Handle::parse("+15555550101").into_iter().collect(),
            timestamp_secs,
            has_milliseconds,
            is_from_me: false,
            sender: Handle::parse("+15555550101"),
            text: "hello".into(),
            attachments: Vec::new(),
            unreadable_parts: 0,
            name_alias: None,
            smssync_id: None,
            android_type: "1".into(),
            eml_path: eml_path.into(),
            owner_not_named: false,
            email_number: None,
        }
    }

    /// A MIME part that could not be decoded reaches the run's report by name.
    #[test]
    fn unreadable_parts_are_counted_in_the_report() {
        let mut ingest = EmlIngest::new(None, 2, None);
        for unreadable_parts in [2, 1] {
            let msg = ParsedMessage {
                unreadable_parts,
                ..parsed(1.0 + unreadable_parts as f64, true, "")
            };
            ingest
                .absorb(ParsedEmlKind::Flat { msg: Box::new(msg) })
                .unwrap();
        }
        assert_eq!(ingest.report.extra("skipped_unreadable_part"), 3);
    }

    #[test]
    fn verify_e3_1_two_group_senders_in_one_second_are_two_messages() {
        let group = |sender: &str, timestamp_secs: f64| ParsedMessage {
            chat_key: "+15555550111_+15555550122".into(),
            conversation_type: IrConversationType::Group,
            sender: Handle::parse(sender),
            text: "Happy birthday!".into(),
            ..parsed(timestamp_secs, true, "")
        };
        let (msgs, report) = project(vec![
            group("+15555550111", 1_600_000_000.1),
            group("+15555550122", 1_600_000_000.6),
        ]);
        assert_eq!(msgs.len(), 2);
        assert_eq!(report.duplicates_dropped, 0);
        assert_ne!(msgs[0].guid, msgs[1].guid);
    }

    /// SMS Backup+ reads milliseconds from `X-smssync-date` and whole
    /// seconds from `Date` when that header is missing, so two files of one
    /// message can differ in precision. One message comes out, with the
    /// millisecond time and one id, whichever file is read first.
    #[test]
    fn a_whole_second_copy_and_a_millisecond_copy_are_one_message_whichever_comes_first() {
        let whole = || parsed(1_609_459_200.0, false, "a/whole.eml");
        let exact = || parsed(1_609_459_200.876, true, "b/exact.eml");
        let (first, report) = project(vec![whole(), exact()]);
        let (second, _) = project(vec![exact(), whole()]);
        assert_eq!(first.len(), 1);
        assert_eq!(report.duplicates_dropped, 1);
        assert_eq!(first[0].timestamp_unix_ms, 1_609_459_200_876);
        assert_eq!(first[0].guid, second[0].guid);
    }

    /// Two millisecond copies with different times are two messages, such as
    /// the same text sent twice 300 ms apart.
    #[test]
    fn two_millisecond_copies_at_different_times_are_two_messages() {
        let (msgs, _) = project(vec![
            parsed(1_609_459_200.1, true, "a.eml"),
            parsed(1_609_459_200.4, true, "b.eml"),
        ]);
        assert_eq!(msgs.len(), 2);
        assert_ne!(msgs[0].guid, msgs[1].guid);
    }

    /// Between two copies the backup cannot tell apart, the one that carries
    /// `X-smssync-id` is kept, because only some export routes keep it. The
    /// id does not make them two messages.
    #[test]
    fn of_two_copies_the_one_with_an_smssync_id_is_kept() {
        let mut with_id = parsed(1_609_459_200.5, true, "b.eml");
        with_id.smssync_id = Some("276".into());
        let without_id = parsed(1_609_459_200.5, true, "a.eml");
        for order in [
            vec![with_id.clone(), without_id.clone()],
            vec![without_id, with_id],
        ] {
            let (msgs, _) = project(order);
            assert_eq!(msgs.len(), 1);
            assert_eq!(msgs[0].source.as_ref().unwrap().fields["smssync_id"], "276");
        }
    }
}
