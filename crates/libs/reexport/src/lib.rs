//! Convert an existing Message Crate output directory to another format.

use anyhow::{Context, Result, bail};
use media::{CompressOptions, MediaMode};
pub use message_crate_core::RunResult;
use message_crate_core::{
    ATTACHMENTS_MISSING, ExportReport, ExportTransforms, ExporterConfig, LogSink, MediaConfig,
    OutputFormat, SourceConfig, attachment_size_hint, document_messages, prepare_outputs,
};
use message_ir::{ConversationDocument, IrMessage};
use message_ir_format::{
    CSV_HEADERS, EXPORT_SENTINEL, FormatSink, MergedArchive, clean_previous_ir_output,
    read_conversation_csv, read_conversation_eml_dir, read_conversation_json,
    read_conversation_jsonl, read_conversation_mbox,
};
use message_staging::{
    AttachmentSource, AttachmentSpool, CountedAttachments, Disk, PathSources, check_headroom,
    load_attachment_source,
};
use sms_backup_plus_exporter::SmsBackupPlusArchive;
use sms_backup_restore_exporter::{
    ReadOptions, SbrArchive, read_backup, spooled_attachments, stage_read_attachments,
};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Convert the prior export in `config.inputs[0]` to `config.output_format`.
///
/// # Errors
///
/// Returns an error when the input is missing or ambiguous, the output is or
/// holds the input directory, an artifact cannot be read, or the output cannot be written.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let input = config.require_input().map_err(anyhow::Error::msg)?;
    if config.output.as_os_str().is_empty() {
        bail!("output directory is required");
    }
    let report = convert_export(input, config)?;
    Ok(RunResult::new(report.log_lines(), &report.report))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetectedExport {
    format: OutputFormat,
}

#[derive(Debug, Default)]
struct ReexportReport {
    detected_format: String,
    /// The name of the format written when it holds only SMS and MMS, for
    /// the line that says how many messages were left out of it.
    sms_only_format: Option<&'static str>,
    /// Conversations written, attachments staged, what media conversion
    /// did, and obfuscation.
    report: ExportReport,
}

impl ReexportReport {
    /// Lines for the run's log. The desktop app shows the last line again as
    /// the run's summary, so the left-out line comes before `Conversations:`
    /// and never closes the log.
    fn log_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Detected input format: {}", self.detected_format)];
        lines.extend(
            self.sms_only_format
                .and_then(|format| self.report.not_sms_or_mms_line(format)),
        );
        lines.push(format!("Conversations: {}", self.report.conversations));
        if self.report.attachments_saved > 0 {
            lines.push(format!(
                "  saved {} attachments",
                self.report.attachments_saved
            ));
        }
        let missing = self.report.extra(ATTACHMENTS_MISSING);
        if missing > 0 {
            lines.push(format!("  {missing} attachments missing"));
        }
        lines.extend(self.report.media_lines());
        lines
    }
}

/// Detect the input format, copy attachments if needed, and write the new export.
fn convert_export(input_dir: &Path, config: &ExporterConfig) -> Result<ReexportReport> {
    // SMS Backup+ mail records when its backup was made: the start of the
    // Export Run this conversion is part of, or of this run when it is one.
    let started = match &config.source {
        SourceConfig::Format(format) => format.run_started,
        _ => None,
    }
    .unwrap_or_else(chrono::Utc::now);
    // The output is cleaned below, so one that is or holds the input is
    // refused before anything is written.
    let (inputs, output) = prepare_outputs(&[input_dir.to_path_buf()], &config.output)?;

    let detected = detect_ir_export(input_dir)?;
    let transforms = ExportTransforms::from_config(config);
    let copy_attachments = transforms.copies_attachments();

    // The input is read before the output is cleaned, so an input the read
    // refuses, such as a file of another schema version or a broken SMS
    // backup, stops the run with the previous output left as it was.
    let (mut documents, sms_backup) = if detected.format == OutputFormat::Xml {
        let backup = SmsBackupRead::open(&inputs[0], output, &config.scratch_dir, copy_attachments);
        (backup.read(config)?, Some(backup))
    } else {
        (read_conversation_files(input_dir, detected.format)?, None)
    };
    if documents.is_empty() {
        bail!("no conversations loaded from {}", input_dir.display());
    }

    // A mail export's reader holds the attachments in memory, and the
    // export has no `attachments/` directory to copy, so its attachments are
    // staged even when the media is only cloned.
    let stages_again = matches!(transforms.media, MediaMode::Convert | MediaMode::Compress)
        || (copy_attachments && detected.format.is_mail_archive());
    // Only the input's own sources, staged below from what the disk check
    // counted, need the reasons taken before that count.
    let stages_from_input = copy_attachments && stages_again && sms_backup.is_none();
    let reasons = if stages_from_input {
        missing_reasons(&documents)
    } else {
        Vec::new()
    };

    // Every attachment the conversion writes is counted against the disk
    // that holds the output before anything is written or cleaned there,
    // from where the conversion reads it: the input's files, the spooled
    // ones staged from a backup, and the bytes a mail export held, with
    // every embedded copy in a mail or merged archive. An attachment with
    // no file there counts for nothing (#1743). What the clean will free
    // counts as free, and a run that will not fit leaves the earlier output
    // as it was. The input's attachments are staged from the sources
    // counted here; a backup's are staged by its own step below. Either
    // way each attachment is on the disk once: a backup's staged files are
    // staged again under the same names, and the convert pass replaces each
    // one in place, so the check counts one copy (#1759).
    let mut from_input = None;
    if copy_attachments {
        let counted = match &sms_backup {
            Some(backup) => backup.counted(&mut documents, config),
            None => attachments_in(
                &mut documents,
                input_dir,
                staging_media(&transforms),
                // A file found missing is logged only by a run that then
                // stages: a cloned copy of `attachments/` reads no file.
                config.log.as_ref().filter(|_| stages_from_input),
            ),
        };
        let needed = counted.bytes_to_write(config.output_format);
        let freed = previous_attachment_bytes(&config.output);
        check_headroom(&config.output, needed.saturating_sub(freed), Disk::Staging)?;
        if stages_from_input {
            from_input = Some(counted);
        }
    }

    clean_previous_ir_output(&config.output)?;

    // A run that stages from the input writes every attachment from there,
    // so it copies none of the input's `attachments/`: the copy would stay
    // beside the staged files, under the input's names, on a disk the
    // check counted for one copy (#1759).
    if copy_attachments && from_input.is_none() {
        copy_attachments_dir(input_dir, &config.output)?;
    }
    let mut report = ExportReport::default();
    if let Some(counted) = from_input {
        stage_again(counted, config, &mut report)?;
        keep_missing_reasons(&mut documents, reasons, &mut report);
    } else {
        if let Some(backup) = sms_backup {
            backup.stage(&mut documents, config)?;
        }
        if stages_again {
            apply_reexport_convert(&mut documents, config, &transforms, &mut report)?;
        }
    }

    let mut sink = FormatSink::open(&config.output, config.output_format, transforms)?;
    report.conversations = documents.len() as u64;
    let sms_only = sms_only_archive(config.output_format, started);
    let sms_only_format = sms_only.as_ref().map(|archive| archive.format_name());
    if let Some(archive) = sms_only {
        sink = sink.with_archive(archive);
        // The archive writes nothing for a conversation with no SMS or MMS,
        // so only the others are counted.
        report.conversations = documents
            .iter()
            .filter(|doc| doc.messages.iter().any(IrMessage::is_sms_or_mms))
            .count() as u64;
    }
    for document in documents {
        sink.write_document(document)?;
    }
    sink.finish(&mut report)?;

    Ok(ReexportReport {
        detected_format: detected.format.as_str().to_string(),
        sms_only_format,
        report,
    })
}

/// The archive that writes `format`, for a format that holds only SMS and
/// MMS and so leaves every other message out. `None` for every other format,
/// which the sink writes itself.
fn sms_only_archive(
    format: OutputFormat,
    started: chrono::DateTime<chrono::Utc>,
) -> Option<Box<dyn MergedArchive>> {
    match format {
        OutputFormat::Xml => Some(Box::new(SbrArchive)),
        OutputFormat::SmsBackupPlus => Some(Box::new(SmsBackupPlusArchive::new(started))),
        _ => None,
    }
}

/// Every attachment of `documents` paired with its source, a file under
/// `dir` at the attachment's path or the bytes a mail export held, and
/// counted by the rule every run counts by: one with no file there counts
/// for nothing and stages as `file_missing`.
fn attachments_in<'a>(
    documents: &'a mut [ConversationDocument],
    dir: &Path,
    media: MediaConfig,
    log: Option<&LogSink>,
) -> CountedAttachments<'a> {
    CountedAttachments::new(
        document_messages(documents),
        media,
        PathSources::OnDisk,
        |att| {
            let hint = attachment_size_hint(att);
            let source = match (att.bytes.take(), att.path.as_deref()) {
                (Some(bytes), _) => AttachmentSource::Bytes(bytes),
                (None, Some(rel)) => AttachmentSource::Path(
                    dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)),
                ),
                (None, None) => AttachmentSource::Missing,
            };
            (source, hint)
        },
        log,
    )
}

/// Stage the attachments again through the shared step from the files in
/// the output: those a backup's own step staged there, or, for a run that
/// copies no attachments (an obfuscated one with the media converted or
/// compressed), whatever is there. A convert or compress pass then rewrites
/// each document's paths, hashes and MIME types. Adds the distinct files
/// written to `report.attachments_saved` and the attachments left without
/// a file to `attachments_missing`.
fn apply_reexport_convert(
    documents: &mut [ConversationDocument],
    config: &ExporterConfig,
    transforms: &ExportTransforms,
    report: &mut ExportReport,
) -> Result<()> {
    let reasons = missing_reasons(documents);
    let counted = attachments_in(
        documents,
        &config.output,
        staging_media(transforms),
        config.log.as_ref(),
    );
    stage_again(counted, config, report)?;
    keep_missing_reasons(documents, reasons, report);
    Ok(())
}

/// The media settings a conversion stages its attachments with.
fn staging_media(transforms: &ExportTransforms) -> MediaConfig {
    MediaConfig {
        mode: transforms.media,
        compress: transforms.compress.clone(),
    }
}

/// Stage `counted` into the output's `attachments/`, adding the distinct
/// files written to `report.attachments_saved`.
fn stage_again(
    counted: CountedAttachments<'_>,
    config: &ExporterConfig,
    report: &mut ExportReport,
) -> Result<()> {
    let saved = counted
        .stage(
            &config.output.join("attachments"),
            load_attachment_source,
            config.log.as_ref(),
            config.progress.as_ref(),
            config.cancel.as_ref(),
        )
        .map_err(anyhow::Error::msg)?;
    report.attachments_saved += saved;
    Ok(())
}

/// Every attachment's `missing_reason` across `documents`, in order, taken
/// before a staging pass that marks the ones it finds no file for.
fn missing_reasons(documents: &[ConversationDocument]) -> Vec<Option<String>> {
    documents
        .iter()
        .flat_map(|doc| doc.messages.iter())
        .flat_map(|msg| msg.attachments.iter())
        .map(|att| att.missing_reason.clone())
        .collect()
}

/// Count the attachments a staging pass left without a file in
/// `attachments_missing`. One the input already gave a `missing_reason`
/// (from `reasons`, as [`missing_reasons`] took them) keeps that reason
/// rather than becoming `file_missing`.
fn keep_missing_reasons(
    documents: &mut [ConversationDocument],
    reasons: Vec<Option<String>>,
    report: &mut ExportReport,
) {
    let mut missing = 0;
    for (att, reason) in documents
        .iter_mut()
        .flat_map(|doc| doc.messages.iter_mut())
        .flat_map(|msg| msg.attachments.iter_mut())
        .zip(reasons)
    {
        if att.missing_reason.as_deref() == Some("file_missing") {
            missing += 1;
            if reason.is_some() {
                att.missing_reason = reason;
            }
        }
    }
    if missing > 0 {
        report.bump(ATTACHMENTS_MISSING, missing);
    }
}

/// The bytes of the `attachments/` an earlier export left in `output`, which
/// [`clean_previous_ir_output`] deletes before this run writes. Nothing is
/// deleted from a directory without the export's mark, so nothing counts.
fn previous_attachment_bytes(output: &Path) -> u64 {
    if !output.join(EXPORT_SENTINEL).is_file() {
        return 0;
    }
    let mut total = 0u64;
    let mut pending = vec![output.join("attachments")];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(entry.path()),
                Ok(kind) if kind.is_file() => {
                    total = total.saturating_add(entry.metadata().map_or(0, |m| m.len()));
                }
                _ => {}
            }
        }
    }
    total
}

/// An SMS Backup & Restore read whose attachments wait in the spool until
/// the output has been cleaned.
struct SmsBackupRead {
    /// The backup's directory, resolved.
    input: PathBuf,
    /// The output directory, resolved. The read leaves it out, since it may
    /// sit inside the backup's directory and hold a backup an earlier run wrote.
    output: PathBuf,
    /// The output's `attachments/`, where the spooled attachments are staged.
    attachments_dir: PathBuf,
    /// The payloads the read spooled; `None` when the run does not copy
    /// attachments. The spool is removed when this is dropped.
    spool: Option<AttachmentSpool>,
}

impl SmsBackupRead {
    /// Prepare to read the backup in `input` for a conversion into
    /// `output`, both resolved by `prepare_outputs`. Each payload goes to
    /// a spool under `scratch_dir`, the Scratch Directory, as its record is
    /// read, so the backup's attachments are never all in memory and never
    /// in the output before they are staged.
    fn open(input: &Path, output: PathBuf, scratch_dir: &Path, copy_attachments: bool) -> Self {
        Self {
            input: input.to_path_buf(),
            attachments_dir: output.join("attachments"),
            output,
            // No copy directory: the output still holds an earlier conversion
            // while the backup is read, so the copy is checked once, before
            // the clean, counting what the clean frees.
            spool: copy_attachments.then(|| AttachmentSpool::new(scratch_dir)),
        }
    }

    /// The options the read and the staging share. The read leaves the
    /// attachments in the spool, for [`SmsBackupRead::stage`].
    fn options<'a>(&'a self, config: &'a ExporterConfig) -> ReadOptions<'a> {
        ReadOptions {
            owner_phones: &[],
            attachments_dir: Some(&self.attachments_dir),
            spool: self.spool.as_ref(),
            exclude_dir: Some(&self.output),
            media: if self.spool.is_some() {
                MediaMode::Clone
            } else {
                MediaMode::Disabled
            },
            compress: CompressOptions::default(),
            log: None,
            progress: None,
            cancel: config.cancel.as_ref(),
        }
    }

    /// Read every conversation in the backup without writing to the output
    /// outside the spool. What the reader dropped, skipped and could not
    /// read goes to the log the moment the read returns, so a run that then
    /// fails, such as one that found no conversation, still says why. It
    /// comes before every line of the run's summary, `Conversations:`
    /// included.
    fn read(&self, config: &ExporterConfig) -> Result<Vec<ConversationDocument>> {
        let (documents, report) = read_backup(&self.input, self.options(config))?;
        for line in report.log_lines() {
            config.emit_log(line);
        }
        Ok(documents)
    }

    /// Every attachment of `documents` paired with its spooled payload and
    /// counted, for the disk check made before the output is cleaned.
    fn counted<'a>(
        &self,
        documents: &'a mut [ConversationDocument],
        config: &ExporterConfig,
    ) -> CountedAttachments<'a> {
        spooled_attachments(documents, &self.options(config))
    }

    /// Stage the spooled attachments into the output's `attachments/`.
    fn stage(self, documents: &mut [ConversationDocument], config: &ExporterConfig) -> Result<()> {
        stage_read_attachments(documents, &self.options(config))
    }
}

/// Read every conversation file or EML directory of `format` in `input_dir`.
/// Any file the read refuses stops the whole read.
fn read_conversation_files(
    input_dir: &Path,
    format: OutputFormat,
) -> Result<Vec<ConversationDocument>> {
    list_artifacts(input_dir, format)?
        .into_iter()
        .map(|path| read_artifact(&path, format))
        .collect()
}

/// Read one conversation file or EML directory in the detected format.
fn read_artifact(path: &Path, format: OutputFormat) -> Result<ConversationDocument> {
    match format {
        OutputFormat::Json => read_conversation_json(path),
        OutputFormat::Jsonl => read_conversation_jsonl(path),
        OutputFormat::Csv => read_conversation_csv(path),
        OutputFormat::Mbox => read_conversation_mbox(path),
        OutputFormat::Eml => read_conversation_eml_dir(path),
        OutputFormat::Xml => unreachable!("XML handled above"),
        OutputFormat::SmsBackupPlus => unreachable!("never detected as an input"),
    }
}

/// Detect a single Message Crate export format in `input_dir`.
fn detect_ir_export(input_dir: &Path) -> Result<DetectedExport> {
    if !input_dir.is_dir() {
        bail!("input is not a directory: {}", input_dir.display());
    }

    let mut present = Vec::new();
    let mut samples = Vec::new();
    for entry in fs::read_dir(input_dir).with_context(|| format!("read {}", input_dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored_artifact(&name) {
            continue;
        }
        let format = if path.is_file() {
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match extension.as_str() {
                "xml" if name.eq_ignore_ascii_case("smses.xml") || looks_like_smses(&path) => {
                    Some(OutputFormat::Xml)
                }
                "json" if looks_like_ir_json(&path)? => Some(OutputFormat::Json),
                "jsonl" if looks_like_ir_jsonl(&path)? => Some(OutputFormat::Jsonl),
                "csv" if looks_like_ir_csv(&path)? => Some(OutputFormat::Csv),
                "mbox" => Some(OutputFormat::Mbox),
                _ => None,
            }
        } else if path.is_dir() && dir_has_eml(&path)? {
            Some(OutputFormat::Eml)
        } else {
            None
        };
        if let Some(format) = format {
            if !present.contains(&format) {
                present.push(format);
            }
            samples.push(if path.is_dir() {
                format!("{name}/")
            } else {
                name
            });
        }
    }
    present.sort_by_key(|format| match format {
        OutputFormat::Xml => 0,
        OutputFormat::Json => 1,
        OutputFormat::Jsonl => 2,
        OutputFormat::Csv => 3,
        OutputFormat::Mbox => 4,
        OutputFormat::Eml => 5,
        OutputFormat::SmsBackupPlus => 6,
    });

    match present.as_slice() {
        [format] => Ok(DetectedExport { format: *format }),
        [] => bail!(
            "unsupported input: no Message Crate IR export found in {} \
             (expected smses.xml, *.json, *.jsonl, *.csv, *.mbox, or EML directories)",
            input_dir.display()
        ),
        formats => bail!(
            "unsupported input: mixed formats in {} ({}); found: {}",
            input_dir.display(),
            formats
                .iter()
                .map(|format| format.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            samples.join(", ")
        ),
    }
}

/// List conversation files or EML directories for the detected format.
fn list_artifacts(input_dir: &Path, format: OutputFormat) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(input_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored_artifact(&name) {
            continue;
        }
        let matches = match format {
            OutputFormat::Xml => {
                path.is_file()
                    && (name.eq_ignore_ascii_case("smses.xml") || looks_like_smses(&path))
            }
            OutputFormat::Json => {
                path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("json")
                    && looks_like_ir_json(&path)?
            }
            OutputFormat::Jsonl => {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension == "jsonl")
                    && looks_like_ir_jsonl(&path)?
            }
            OutputFormat::Csv => {
                path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("csv")
                    && looks_like_ir_csv(&path)?
            }
            OutputFormat::Mbox => {
                path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("mbox")
            }
            OutputFormat::Eml => path.is_dir() && dir_has_eml(&path)?,
            // Never detected as an input: its directories are found as `Eml`.
            OutputFormat::SmsBackupPlus => false,
        };
        if matches {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        bail!(
            "no {} artifacts found in {}",
            format.as_str(),
            input_dir.display()
        );
    }
    Ok(paths)
}

/// True for sidecar files that are not conversation artifacts. `.tmp`
/// covers `.xml.tmp` as well.
fn ignored_artifact(name: &str) -> bool {
    name == "attachments"
        || name.starts_with('.')
        || name.ends_with(".meta.json")
        || name.ends_with(".tmp")
        || name.ends_with(".xml.sbrbody")
}

/// True when the first line of `path` looks like `<smses`.
fn looks_like_smses(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut first_line = String::new();
    let _ = BufReader::new(file).read_line(&mut first_line);
    first_line.to_ascii_lowercase().contains("<smses")
}

/// True when `path` is a conversation JSON file: an object with a numeric
/// `schema_version` and `export`, `conversation` and `messages` keys.
///
/// The version is not compared, so a file of another version is an export
/// here and its read refuses it by name.
fn looks_like_ir_json(path: &Path) -> Result<bool> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    Ok(has_ir_header(&value) && value.get("messages").is_some())
}

/// True when the first line of `path` is a JSON Lines conversation header:
/// an object with a numeric `schema_version` and `export` and
/// `conversation` keys, and no `messages`.
///
/// The version is not compared, so a file of another version is an export
/// here and its read refuses it by name.
fn looks_like_ir_jsonl(path: &Path) -> Result<bool> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let Some(Ok(first_line)) = BufReader::new(file).lines().next() else {
        return Ok(false);
    };
    let value: serde_json::Value = match serde_json::from_str(&first_line) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    Ok(has_ir_header(&value) && value.get("messages").is_none())
}

/// True when `value` has a numeric `schema_version` and `export` and
/// `conversation` keys, whatever the version.
fn has_ir_header(value: &serde_json::Value) -> bool {
    value
        .get("schema_version")
        .is_some_and(serde_json::Value::is_u64)
        && value.get("export").is_some()
        && value.get("conversation").is_some()
}

/// True when `path` has every column in [`CSV_HEADERS`].
fn looks_like_ir_csv(path: &Path) -> Result<bool> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(BufReader::new(file));
    let Ok(headers) = reader.headers() else {
        return Ok(false);
    };
    let headers: HashSet<&str> = headers.iter().collect();
    Ok(CSV_HEADERS.iter().all(|header| headers.contains(header)))
}

/// True when `dir` contains at least one `.eml` file.
fn dir_has_eml(dir: &Path) -> Result<bool> {
    for entry in fs::read_dir(dir)? {
        if entry?
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("eml"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Copy `input_dir/attachments` into `output_dir/attachments` when present.
fn copy_attachments_dir(input_dir: &Path, output_dir: &Path) -> Result<()> {
    let source = input_dir.join("attachments");
    if !source.is_dir() {
        return Ok(());
    }
    let destination = output_dir.join("attachments");
    fs::create_dir_all(&destination)?;
    copy_dir_recursive(&source, &destination)
}

/// Recursively copy files from `source` into `destination`.
fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            fs::create_dir_all(&to)?;
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copy {} → {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
