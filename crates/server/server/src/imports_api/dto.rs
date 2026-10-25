//! The request and response types of the `/v1/imports` routes, and the
//! checks that turn a request's fields into what the database records.

use api_types::{ImportErrorKind, ImportMode};
use serde::{Deserialize, Serialize};

use super::pipeline::ImportCounts;
use crate::api_error::ApiError;
use crate::text_caps::{
    MAX_IMPORT_ERROR_TEXT_CHARS, MAX_IMPORT_NOTE_TEXT_CHARS, MAX_IMPORT_RUN_JSON_BYTES,
    capped_json, capped_text, too_long_sentences,
};

/// Import result: the batch's import counts. The batch hides no
/// duplicates; the run's end does, once for the whole run.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CreateImportBatchResponse {
    /// Source id of the Import Run the batch belongs to, such as `imessage`.
    pub(super) source: String,
    /// Id of the account the batch imported into.
    pub(super) account: i64,
    #[serde(flatten)]
    pub(super) counts: ImportCounts,
}

/// Source, mode and tool for a new Import Run. The bearer token
/// names the account.
// `max_length` takes only a literal; `the_reference_states_the_import_run_text_caps`
// holds each to its `text_caps` constant.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateImportRequest {
    /// Source id the run imports, such as `imessage` or `whatsapp`, which
    /// every message the run brings in records. It holds lowercase letters,
    /// digits, `-` and `_`, starts with a letter or a digit, and is at most
    /// 64 characters. Any other id, one with a space around it included, is
    /// refused with `422 Unprocessable Entity`.
    pub(crate) source: String,
    /// What happens to the messages this source brought in before: `replace`
    /// removes them on the run's first batch, and `append` keeps them and
    /// adds only new ones. `append` when the request leaves it out.
    #[serde(default)]
    pub(crate) mode: ImportMode,
    /// The country of the phone the backup came from, as an ISO 3166-1
    /// alpha-2 code such as `GB`, which `GET /v1/phone-countries` lists.
    /// Every phone number the run's files write without its `+` code is
    /// read as a number in that country and stored in its `+` form. Null,
    /// or left out, states no country: such a number is stored as the
    /// digits typed and matches no `+` number until a country is picked for
    /// it. A code the list does not hold is refused with
    /// `422 Unprocessable Entity`.
    #[serde(default)]
    pub(crate) phone_country: Option<String>,
    /// Name of the program that runs the import, such as
    /// `message-crate-import`, stored on the run trimmed. Null when the
    /// request leaves it out, or blank. Over 64 characters is refused with
    /// `422 Unprocessable Entity`.
    #[serde(default)]
    #[schema(max_length = 64)]
    pub(crate) tool: Option<String>,
    /// Absolute path to the run directory on the client that owns this
    /// Import Run, stored as given, spaces at its ends included. Over 4,096
    /// characters is refused with `422 Unprocessable Entity`.
    #[serde(default)]
    #[schema(max_length = 4096)]
    pub(crate) run_dir: Option<String>,
    /// Which copy of the app is creating the Import Run, stored trimmed. Null when
    /// the request leaves it out, or blank. Over 64 characters is refused
    /// with `422 Unprocessable Entity`.
    #[serde(default)]
    #[schema(max_length = 64)]
    pub(crate) device_id: Option<String>,
    /// Import form snapshot, stored so the screen can be restored.
    ///
    /// Credentials are stripped before storage: a `backupPassword` or
    /// `whatsappKey` posted here is dropped rather than persisted. Over
    /// 65,536 bytes of JSON is refused with `422 Unprocessable Entity`.
    #[serde(default)]
    pub(crate) form: Option<serde_json::Value>,
    /// Source path, size, mtime, and message count. Over 65,536 bytes of
    /// JSON is refused with `422 Unprocessable Entity`.
    #[serde(default)]
    pub(crate) source_fingerprint: Option<serde_json::Value>,
    /// Addresses the backup's device sent from, when the client read them.
    /// Over 65,536 bytes of JSON is refused with `422 Unprocessable Entity`.
    #[serde(default)]
    pub(crate) source_identities: Option<serde_json::Value>,
}

/// The new Import Run's id.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CreateImportResponse {
    /// Id of the new Import Run, which its routes name in their path, as
    /// `/v1/imports/{id}`.
    pub(crate) id: i64,
}

/// Final stats and issues for a running Import Run. The outcome is stated
/// once, as `status`. The run's message and attachment counts are not part
/// of it: the server counts what the run holds, since a resumed Upload's
/// client knows only what the resume sent.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CompleteImportRequest {
    /// How the run ended: `completed`, `completed_with_issues` or `failed`.
    pub(crate) status: String,
    /// Bytes the Upload sent, as the client counted them. Null or left out
    /// keeps the count the run already holds.
    #[serde(default)]
    pub(crate) bytes_uploaded: Option<i64>,
    /// How long the whole run took, in milliseconds, as the client timed it.
    /// Null or left out stores none.
    #[serde(default)]
    pub(crate) duration_ms: Option<i64>,
    /// Milliseconds Staging spent reading the backup ("Parse backup"). Null
    /// or left out stores none.
    #[serde(default)]
    pub(crate) parse_ms: Option<i64>,
    /// Milliseconds Staging spent copying the attachments ("Attachments").
    /// Null or left out stores none.
    #[serde(default)]
    pub(crate) attachments_ms: Option<i64>,
    /// Milliseconds Staging spent writing the conversation files ("Preparing
    /// messages"). Null or left out stores none.
    #[serde(default)]
    pub(crate) prepare_ms: Option<i64>,
    /// Milliseconds the Upload took. Null or left out stores none.
    #[serde(default)]
    pub(crate) upload_ms: Option<i64>,
    /// The run's final counts as the client made them, such as files and
    /// messages parsed, inserted, deduplicated and failed. It becomes the
    /// run's `summary` and replaces the plan the last Review stored there.
    /// Null or left out leaves the run with no summary.
    #[serde(default)]
    pub(crate) summary: Option<serde_json::Value>,
    /// The run's Import Errors: every item that failed or was skipped. An
    /// empty list when the request leaves it out.
    #[serde(default)]
    pub(crate) issues: Vec<ImportIssueRequest>,
    /// The run's notes, apart from its Import Errors. A note never makes a
    /// run `completed_with_issues`.
    #[serde(default)]
    pub(crate) notes: Vec<ImportNoteRequest>,
}

/// The Import Errors a discarded run recorded before it was given up. A run
/// that paused and is then discarded never posts `complete`, so its issues
/// and notes come with the discard.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct DiscardImportRequest {
    /// The run's Import Errors so far. The list is empty when the run
    /// recorded none.
    pub(crate) issues: Vec<ImportIssueRequest>,
    /// The run's notes so far. The list is empty when the run recorded none.
    pub(crate) notes: Vec<ImportNoteRequest>,
}

/// One note a Stage of the Import Run recorded: something it did with an
/// item that is worth knowing but did not fail, such as a message it kept
/// with a caveat.
// `max_length` takes only a literal; `the_reference_states_the_import_run_text_caps`
// holds each to `MAX_IMPORT_NOTE_TEXT_CHARS`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct ImportNoteRequest {
    /// Stage the note came from.
    pub(crate) stage: crate::db::imports::ImportIssueStage,
    /// The file, message or address the note is about. Stored trimmed; over
    /// 2,000 characters is refused with `422 Unprocessable Entity`.
    #[schema(max_length = 2000)]
    pub(crate) item: String,
    /// What the run did with it, in one sentence. Stored trimmed; over 2,000
    /// characters is refused with `422 Unprocessable Entity`.
    #[schema(max_length = 2000)]
    pub(crate) text: String,
}

/// The requested notes as the database records them, their text trimmed.
/// Adds to `errors` every `item` or `text` over its cap (`text_caps`).
fn note_rows(
    notes: &[ImportNoteRequest],
    errors: &mut Vec<String>,
) -> Vec<crate::db::imports::ImportNoteRow> {
    let mut rows = Vec::with_capacity(notes.len());
    for (at, note) in notes.iter().enumerate() {
        let cap = |field: &str, text| {
            capped_text(
                &format!("notes[{at}].{field}"),
                text,
                MAX_IMPORT_NOTE_TEXT_CHARS,
            )
        };
        let (item, text) = (cap("item", &note.item), cap("text", &note.text));
        errors.extend(too_long_sentences([&item, &text]));
        if let (Ok(item), Ok(text)) = (item, text) {
            rows.push(crate::db::imports::ImportNoteRow {
                stage: note.stage,
                item: item.to_string(),
                text: text.to_string(),
            });
        }
    }
    rows
}

/// `value`, or none when it is empty. A new Import Run stores an empty
/// `tool`, `run_dir` or `device_id` as none, as it stores one left out. A
/// blank `tool` or `device_id` is empty once trimmed. A whitespace `run_dir`
/// is kept, because it is not trimmed and could name a directory.
pub(super) fn empty_as_none(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

/// The issues and notes a completion or a discard carries, as the database
/// records them. Refuses the request naming every rule [`import_issue_inputs`]
/// and [`note_rows`] find broken, in both lists, after `errors`, the rules
/// the rest of the body broke. Checked before a connection is opened, so a
/// refused request writes nothing.
pub(super) fn issues_and_notes(
    issues: &[ImportIssueRequest],
    notes: &[ImportNoteRequest],
    mut errors: Vec<String>,
) -> Result<
    (
        Vec<crate::db::imports::ImportIssueInput>,
        Vec<crate::db::imports::ImportNoteRow>,
    ),
    ApiError,
> {
    let issues = import_issue_inputs(issues, &mut errors);
    let notes = note_rows(notes, &mut errors);
    ApiError::unless_broken(errors)?;
    Ok((issues, notes))
}

/// One error or skip a Stage of the Import Run reported.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct ImportIssueRequest {
    /// `error` when the item failed, or `skip` when the run left it out
    /// without importing it, such as an attachment it did not upload. Every
    /// other kind is refused with `422 Unprocessable Entity`: a note goes in
    /// `notes`, and the desktop app applies a `resolved` row to the record
    /// before it sends it.
    pub(crate) kind: ImportErrorKind,
    /// Stage the issue came from.
    pub(crate) stage: crate::db::imports::ImportIssueStage,
    /// What the issue is about, such as a conversation file or an
    /// attachment's path. Stored trimmed; over 2,000 characters is refused
    /// with `422 Unprocessable Entity`.
    // `max_length` takes only a literal; a test holds it to
    // `MAX_IMPORT_ERROR_TEXT_CHARS`.
    #[schema(max_length = 2000)]
    pub(crate) item: String,
    /// Why the item failed or was skipped, in one sentence. Stored trimmed;
    /// over 2,000 characters is refused with `422 Unprocessable Entity`.
    #[schema(max_length = 2000)]
    pub(crate) reason: String,
}

/// The requested issues as the database records them, their text trimmed.
/// Adds to `errors` every `item` or `reason` over its cap (`text_caps`).
fn import_issue_inputs(
    issues: &[ImportIssueRequest],
    errors: &mut Vec<String>,
) -> Vec<crate::db::imports::ImportIssueInput> {
    let mut inputs = Vec::with_capacity(issues.len());
    for (at, issue) in issues.iter().enumerate() {
        let cap = |field: &str, text| {
            capped_text(
                &format!("issues[{at}].{field}"),
                text,
                MAX_IMPORT_ERROR_TEXT_CHARS,
            )
        };
        let (item, reason) = (cap("item", &issue.item), cap("reason", &issue.reason));
        errors.extend(too_long_sentences([&item, &reason]));
        if let (Ok(item), Ok(reason)) = (item, reason) {
            inputs.push(crate::db::imports::ImportIssueInput {
                kind: issue.kind,
                stage: issue.stage,
                item: item.to_string(),
                reason: reason.to_string(),
            });
        }
    }
    inputs
}

/// The sentence refusing `status`, or none when a completion takes it.
pub(super) fn import_status_refusal(status: &str) -> Option<String> {
    match status {
        "completed" | "completed_with_issues" | "failed" => None,
        other => Some(format!(
            "invalid import status '{other}'; expected 'completed', 'completed_with_issues', or 'failed'"
        )),
    }
}

/// Keys a stored form snapshot must never carry.
///
/// The invariant: `imports.form_json` is a durable record read back to
/// restore the Import screen, and a secret typed once for one run must not
/// outlive it there. The desktop client already drops these before posting,
/// but the client is the wrong place to enforce it -- an older build, a
/// script, or a client not written yet would break the rule silently. The
/// snapshot is flat, so removing them at the top level is the whole job.
const FORM_CREDENTIAL_KEYS: [&str; 2] = ["backupPassword", "whatsappKey"];

/// Remove [`FORM_CREDENTIAL_KEYS`] from a form snapshot before it is stored.
pub(super) fn strip_form_credentials(form: &serde_json::Value) -> serde_json::Value {
    let mut stripped = form.clone();
    if let Some(fields) = stripped.as_object_mut() {
        for key in FORM_CREDENTIAL_KEYS {
            fields.remove(key);
        }
    }
    stripped
}

/// Serialize an optional JSON body field for storage as TEXT.
pub(super) fn optional_json_string(
    value: Option<&serde_json::Value>,
    field: &str,
) -> Result<Option<String>, ApiError> {
    match value {
        None => Ok(None),
        Some(v) => serde_json::to_string(v)
            .map(Some)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize {field}: {e}"))),
    }
}

/// [`optional_json_string`] for a value an Import Run stores as it is
/// created. Adds to `errors` the sentence refusing it when it is over
/// [`MAX_IMPORT_RUN_JSON_BYTES`], and still returns it, so the route goes on
/// to name every other rule the body breaks. Only a value that cannot be
/// serialized returns early, as an internal failure.
pub(super) fn capped_json_string(
    value: Option<&serde_json::Value>,
    field: &str,
    errors: &mut Vec<String>,
) -> Result<Option<String>, ApiError> {
    let json = optional_json_string(value, field)?;
    if let Some(json) = &json
        && let Err(too_long) = capped_json(field, json, MAX_IMPORT_RUN_JSON_BYTES)
    {
        errors.push(too_long.to_string());
    }
    Ok(json)
}

/// `GET /v1/imports`: a page, narrowed to one `status` when given. One of the
/// two lists with a filter parameter, beside the Export Run list
/// (`docs/architecture/rules/http-api.md`): it has no search language.
#[derive(Debug, Deserialize)]
pub(crate) struct ListImportsQuery {
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
    /// `started_at` or `-started_at`; newest first when absent.
    #[serde(default)]
    pub(crate) sort: Option<String>,
}

/// One stored import note: something the run did with an item that is worth
/// knowing but did not fail.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportNote {
    /// Stage the note came from.
    pub(crate) stage: crate::db::imports::ImportIssueStage,
    /// The file, message or address the note is about.
    pub(super) item: String,
    /// What the run did with it.
    pub(super) text: String,
}

/// One stored import issue.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportIssue {
    /// `error` when the item failed, or `skip` when the run left it out
    /// without importing it.
    pub(crate) kind: ImportErrorKind,
    /// Stage the issue came from.
    pub(crate) stage: crate::db::imports::ImportIssueStage,
    /// What the issue is about, such as a conversation file or an
    /// attachment's path.
    pub(super) item: String,
    /// Why the item failed or was skipped.
    pub(super) reason: String,
}

/// An Import Run: one per import, the same record wherever the interface
/// hands one run out. It is the run as a list answers it, the
/// `ImportRunSummary`, with the issues and the notes the run recorded.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportRun {
    /// Everything a list answers for the run: the counts Settings shows,
    /// everything the desktop app needs to resume a running run, and how
    /// many issues it recorded.
    #[serde(flatten)]
    pub(crate) run: ImportRunSummary,
    /// Issues the run recorded, oldest first.
    pub(crate) issues: Vec<ImportIssue>,
    /// Notes the run recorded, oldest first, apart from its issues.
    pub(crate) notes: Vec<ImportNote>,
}

/// An Import Run as a list of runs answers it: every field of the run but
/// its issues and notes. A run may record any number of either, so a page
/// that carried them would have no bound on its size; `GET /v1/imports/{id}`
/// answers them (#1559,
/// `docs/architecture/rules/http-api.md`, API-44).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportRunSummary {
    /// Import Run id.
    pub(crate) id: i64,
    /// Source id the run imports.
    pub(crate) source: String,
    /// Importing tool, e.g. `message-crate-import`.
    pub(crate) tool: Option<String>,
    /// Import mode (`replace` or `append`).
    pub(crate) mode: String,
    /// The country the run states for phone numbers written without their
    /// `+` code, as an ISO 3166-1 alpha-2 code; null when it states none.
    pub(crate) phone_country: Option<String>,
    /// Lifecycle status.
    pub(crate) status: crate::db::imports::ImportStatus,
    /// UTC time the run started.
    pub(crate) started_at: String,
    /// UTC time the run finished, when it has.
    pub(crate) finished_at: Option<String>,
    /// Messages counted for the run.
    pub(crate) message_count: i64,
    /// Attachments counted for the run.
    pub(crate) attachment_count: i64,
    /// Bytes uploaded so far.
    pub(crate) bytes_uploaded: i64,
    /// Total wall-clock duration, when finished.
    pub(crate) duration_ms: Option<i64>,
    /// Time spent parsing, when finished.
    pub(crate) parse_ms: Option<i64>,
    /// Time spent on attachments, when finished.
    pub(crate) attachments_ms: Option<i64>,
    /// Time spent preparing conversation files, when finished.
    pub(crate) prepare_ms: Option<i64>,
    /// Time spent uploading, when finished.
    pub(crate) upload_ms: Option<i64>,
    /// Where a running run is; null once it is over.
    pub(crate) stage: Option<crate::db::imports::ImportStage>,
    /// Absolute path to the run directory on the client that owns the run.
    pub(crate) run_dir: Option<String>,
    /// Which install created the run.
    pub(crate) device_id: Option<String>,
    /// Import form snapshot, or null.
    pub(crate) form: serde_json::Value,
    /// Source path, size, mtime, and message count, or null.
    pub(crate) source_fingerprint: serde_json::Value,
    /// Addresses the backup's device sent from (JSON array), or null.
    pub(crate) source_identities: serde_json::Value,
    /// When the backup the run read was made, UTC: the newest Backup Date
    /// the messages of its conversation files carry (each message's
    /// `backup_taken_at_unix_ms`). Null until a file that names one is
    /// imported, and for a run whose files name none.
    pub(crate) backup_taken_at: Option<String>,
    /// What the person approved at the last Review they passed, which
    /// `PATCH /v1/imports/{id}` writes with its `summary`. A canceled run
    /// keeps it. A run that completed or failed holds instead the final
    /// counts its `POST /v1/imports/{id}/complete` sent, and null when that
    /// request sent none, because completing replaces the plan. Null too
    /// while no Review has stored one.
    pub(crate) summary: serde_json::Value,
    /// How many issues the run recorded.
    pub(crate) issue_count: u64,
    /// How many notes the run recorded.
    pub(crate) note_count: u64,
    /// Contacts this run created.
    pub(crate) contacts_new: u64,
    /// Contacts it only changed.
    pub(crate) contacts_changed: u64,
}

impl From<crate::db::imports::ListedImport> for ImportRunSummary {
    fn from(listed: crate::db::imports::ListedImport) -> Self {
        let row = listed.row;
        Self {
            id: row.id,
            source: row.source,
            tool: row.tool,
            mode: row.mode,
            phone_country: row.phone_country,
            status: row.status,
            started_at: row.started_at,
            finished_at: row.finished_at,
            message_count: row.message_count,
            attachment_count: row.attachment_count,
            bytes_uploaded: row.bytes_uploaded,
            duration_ms: row.duration_ms,
            parse_ms: row.parse_ms,
            attachments_ms: row.attachments_ms,
            prepare_ms: row.prepare_ms,
            upload_ms: row.upload_ms,
            stage: row.stage,
            run_dir: row.run_dir,
            device_id: row.device_id,
            form: crate::db::imports::json_column(row.form_json),
            source_fingerprint: crate::db::imports::json_column(row.source_fingerprint),
            source_identities: crate::db::imports::json_column(row.source_identities),
            backup_taken_at: row.backup_taken_at,
            summary: crate::db::imports::json_column(row.summary_json),
            issue_count: listed.issue_count,
            note_count: listed.note_count,
            contacts_new: listed.contacts.new_count,
            contacts_changed: listed.contacts.changed_count,
        }
    }
}

/// An Import Run as the administrator's list of another account's runs answers
/// it: its source, mode, times, outcome and counts, and nothing of what the
/// backup held (`docs/adr/0008-the-owner-holds-no-messages.md`, "What the owner
/// may see"). The run's summary, its issues, its notes and its form say whom
/// the account talks to, so they stay out, and a field reaches the
/// administrator only by being added here.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct AdminImportRunSummary {
    /// Import Run id.
    pub(crate) id: i64,
    /// Source id the run imports.
    source: String,
    /// Importing tool, e.g. `message-crate-import`.
    tool: Option<String>,
    /// Import mode (`replace` or `append`).
    mode: String,
    /// Lifecycle status.
    status: crate::db::imports::ImportStatus,
    /// UTC time the run started.
    started_at: String,
    /// UTC time the run finished, when it has.
    finished_at: Option<String>,
    /// Messages counted for the run.
    message_count: i64,
    /// Attachments counted for the run.
    attachment_count: i64,
    /// Bytes uploaded so far.
    bytes_uploaded: i64,
    /// Total wall-clock duration, when finished.
    duration_ms: Option<i64>,
    /// Time spent parsing, when finished.
    parse_ms: Option<i64>,
    /// Time spent on attachments, when finished.
    attachments_ms: Option<i64>,
    /// Time spent preparing conversation files, when finished.
    prepare_ms: Option<i64>,
    /// Time spent uploading, when finished.
    upload_ms: Option<i64>,
    /// The whole numbers the run's summary reported, by name. The summary's
    /// other values, among them the addresses a staging summary lists, are
    /// the account's own.
    counts: std::collections::BTreeMap<String, i64>,
    /// Issues the run recorded. Each names the conversation it was about, so
    /// the administrator reads how many and not which.
    pub(super) issue_count: u64,
    /// Notes the run recorded. Each names a file or an address, so the
    /// administrator reads how many and not which.
    note_count: u64,
    /// Contacts this run created.
    contacts_new: u64,
    /// Contacts it only changed.
    contacts_changed: u64,
}

/// One Import Run as the administrator reads it under another account:
/// everything the list answers for it, and the attachments the run wrote whose
/// Thumbnail or Preview the server's media pass could not make, with why. A run
/// may write any number of them, so the list leaves them out
/// (`docs/architecture/rules/http-api.md`, API-45). Each is named by its file
/// name, type and size, which the administrator may read
/// (`docs/adr/0008-the-owner-holds-no-messages.md`, "What the owner may see");
/// never by its message, its conversation, or its bytes.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct AdminImportRun {
    /// Everything the list answers for the run.
    #[serde(flatten)]
    pub(crate) run: AdminImportRunSummary,
    /// How many attachments the run last wrote: those of the messages it
    /// added, and the stored ones it gave their missing file, less any a
    /// later run wrote since. `not_made_attachments` are out of these.
    pub(super) written_attachment_count: u64,
    /// How many of `not_made_attachments` have no Thumbnail and how many no
    /// Preview, for each kind of failure, leaving out a kind none has. Empty
    /// until the media pass has tried the run's attachments, which it does
    /// after the run ends.
    pub(super) not_made_counts: Vec<crate::db::attachment_versions::NotMadeCount>,
    /// The attachments the run wrote whose Thumbnail or Preview could not be
    /// made, by id, each with why for each version not made.
    pub(super) not_made_attachments: Vec<crate::db::attachment_versions::NotMadeAttachment>,
}

impl From<crate::db::imports::ListedImport> for AdminImportRunSummary {
    fn from(listed: crate::db::imports::ListedImport) -> Self {
        let row = listed.row;
        let counts = match crate::db::imports::json_column(row.summary_json) {
            serde_json::Value::Object(summary) => summary
                .into_iter()
                .filter_map(|(name, value)| value.as_i64().map(|n| (name, n)))
                .collect(),
            _ => std::collections::BTreeMap::new(),
        };
        Self {
            id: row.id,
            source: row.source,
            tool: row.tool,
            mode: row.mode,
            status: row.status,
            started_at: row.started_at,
            finished_at: row.finished_at,
            message_count: row.message_count,
            attachment_count: row.attachment_count,
            bytes_uploaded: row.bytes_uploaded,
            duration_ms: row.duration_ms,
            parse_ms: row.parse_ms,
            attachments_ms: row.attachments_ms,
            prepare_ms: row.prepare_ms,
            upload_ms: row.upload_ms,
            counts,
            issue_count: listed.issue_count,
            note_count: listed.note_count,
            contacts_new: listed.contacts.new_count,
            contacts_changed: listed.contacts.changed_count,
        }
    }
}

/// New stage for a running Import Run.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct UpdateImportRequest {
    /// The stage the run moves to.
    pub(crate) stage: crate::db::imports::ImportStage,
    /// What the person approved at the Review they just passed, when they passed one.
    ///
    /// Recorded here rather than at completion so an approval survives a
    /// reload: the summary shown at a Review is recomputed from the directory, but
    /// what was approved is a different question and only the run
    /// remembers it. Absent leaves the stored `summary_json` untouched, because
    /// most stage changes carry nothing, and treating absent as null would
    /// throw away the plan the outcome is later judged against.
    #[serde(default)]
    pub(crate) summary: Option<serde_json::Value>,
}
