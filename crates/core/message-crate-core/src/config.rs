//! Shared typed export configuration for the exporter crates and the desktop app.
//!
//! Every exporter's `run` takes a [`RunContext`] (the run's plumbing), an
//! [`OutputConfig`] (what it writes), its inputs, and its own backup type's
//! config. [`ExporterConfig`] bundles them for the desktop app, which picks
//! the exporter by its [`SourceConfig`] variant.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use media::{MediaTools, NoTools};

use crate::exporters::{ApplePlatform, Exporter, WhatsappPlatform};
use crate::pipeline::IssueSink;
use crate::process::{CancelFlag, LogSink, Sinks};
use crate::progress::ProgressSink;
use crate::transforms::ExportTransforms;

/// Output packaging projected from the common message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    /// Per-conversation CSV.
    Csv,
    /// Per-conversation directory of `.eml` files (see <https://messagecrate.app/docs/developer/formats/mail-archive/>).
    Eml,
    /// Per-conversation `.mbox` (mboxrd) mailbox file.
    Mbox,
    /// Per-conversation common message JSON (default; see <https://messagecrate.app/docs/developer/architecture/common-message/>).
    #[default]
    Json,
    /// Per-conversation common message as JSON Lines (header + one message per line).
    Jsonl,
    /// One XML file holding every conversation, written by the merged archive
    /// the caller supplies; the crate that owns that archive names the file.
    Xml,
    /// SMS Backup+ mail: a directory of `.eml` files per conversation with the
    /// `X-smssync-*` headers, holding only SMS and MMS.
    SmsBackupPlus,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Csv => "CSV (per conversation)",
            Self::Eml => "EML archive (mail directories)",
            Self::Mbox => "MBOX (per conversation)",
            Self::Json => "JSON (common message)",
            Self::Jsonl => "JSONL (common message lines)",
            Self::Xml => "XML (one file)",
            Self::SmsBackupPlus => "EML (SMS Backup+)",
        })
    }
}

impl OutputFormat {
    /// Short format id (`json`, `jsonl`, `csv`, …) that the export form stores.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Eml => "eml",
            Self::Mbox => "mbox",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Xml => "xml",
            Self::SmsBackupPlus => "sms-backup-plus",
        }
    }

    /// True for mail-archive packaging (EML directories or MBOX files).
    pub fn is_mail_archive(self) -> bool {
        matches!(self, Self::Eml | Self::Mbox)
    }
}

/// What the Import form builds for one exporter: the inputs it reads, the
/// output it writes, the run's plumbing, and the options of its own backup
/// type. The desktop app hands each part to the exporter that
/// [`Self::source`] names; no exporter takes the whole.
#[derive(Debug, Clone)]
pub struct ExporterConfig {
    /// Input paths (usually one). SMS Backup+ may pass several; WhatsApp may leave empty.
    pub inputs: Vec<PathBuf>,
    /// Where the export goes and what it does to what it writes.
    pub output: OutputConfig,
    /// The Scratch Directory, the cancel flag, and the sinks the run reports through.
    pub context: RunContext,
    /// The options of the one backup type this run reads.
    pub source: SourceConfig,
}

/// The plumbing of one run: where its scratch data goes, the flag that
/// cancels it, and the sinks it reports through. Nothing here changes what
/// the export holds.
#[derive(Debug, Clone)]
pub struct RunContext {
    /// The desktop Scratch Directory. What a run writes that is not output
    /// (the attachment spool, the databases `imessage-reader` decrypts) goes
    /// in a scratch directory under it ([`crate::ScratchDir`]), never under
    /// [`OutputConfig::directory`]. The exporter crates do not know the
    /// app's directories, so the app names it here.
    pub scratch_dir: PathBuf,
    /// Shared cancel flag for in-process jobs. A run that cannot be
    /// cancelled holds `CancelFlag::default()`, which nobody sets.
    pub cancel: CancelFlag,
    /// Human-readable mid-run notes and warnings. The desktop app sets a sink
    /// and shows the lines in its log panel.
    pub log: LogSink,
    /// Typed progress events (stage, done, total, bytes). The desktop app
    /// sets a sink and drives its progress bar from the events; a run with no
    /// bar holds [`ProgressSink::none`]. Log lines are never read for counts.
    pub progress: ProgressSink,
    /// Each row for the Import Run's record (an item skipped or failed), sent
    /// the moment the run records it. The desktop app sets a sink and passes
    /// each row to its window, so an app that closes mid-run keeps the rows
    /// that had arrived; a run that keeps no record holds [`IssueSink::none`].
    pub issues: IssueSink,
}

impl RunContext {
    /// The run's log, progress and cancel flag, for a step that takes them
    /// together.
    pub fn sinks(&self) -> Sinks<'_> {
        Sinks {
            log: &self.log,
            progress: &self.progress,
            cancel: &self.cancel,
        }
    }

    /// A run whose scratch data goes under `scratch_dir`, that nothing
    /// cancels, and whose sinks drop what they get. The caller sets the
    /// cancel flag and any sink it reads.
    pub fn new(scratch_dir: impl Into<PathBuf>) -> Self {
        Self {
            scratch_dir: scratch_dir.into(),
            cancel: CancelFlag::default(),
            log: LogSink::none(),
            progress: ProgressSink::none(),
            issues: IssueSink::none(),
        }
    }
}

/// Where an export goes and what it does to what it writes: the output
/// directory, the packaging, media handling, fake names, and whether it
/// continues an interrupted export.
#[derive(Debug, Clone)]
pub struct OutputConfig {
    /// Output directory the export is written to (packaging plus `attachments/`).
    /// It holds output only.
    pub directory: PathBuf,
    /// Packaging format (`csv` / `eml` / `mbox` / `json` / `jsonl` / `xml` /
    /// `sms-backup-plus`).
    pub format: OutputFormat,
    /// Fake-name rewrite settings; `None`-equivalent when disabled.
    pub obfuscate: ObfuscateConfig,
    /// Whether the export copies the original attachment files.
    pub media: MediaConfig,
    /// Continue an interrupted export in the same output directory: previous
    /// output is kept, and conversations already written are skipped. Only
    /// the desktop's import resume sets this.
    pub resume: bool,
}

impl OutputConfig {
    /// An export into `directory` packaged as `format`, that copies
    /// attachments as they are, keeps every name, and starts afresh. The
    /// caller sets any other choice.
    pub fn new(directory: impl Into<PathBuf>, format: OutputFormat) -> Self {
        Self {
            directory: directory.into(),
            format,
            obfuscate: ObfuscateConfig::default(),
            media: MediaConfig::default(),
            resume: false,
        }
    }

    /// True when fake-name rewrite is on, or a seed was supplied.
    pub fn obfuscate_active(&self) -> bool {
        self.obfuscate.enabled || self.obfuscate.seed.is_some()
    }
}

/// Require a single input path (most exporters).
///
/// # Errors
///
/// Returns an error when no input is set, or when more than one path is set.
pub fn require_input(inputs: &[PathBuf]) -> Result<&Path, String> {
    match inputs {
        [path] => Ok(path.as_path()),
        [] => Err("input is required".into()),
        _ => Err("expected a single input path".into()),
    }
}

/// The run-wide settings every exporter's convert step takes beside its own
/// inputs. [`ConvertRun::new`] builds them, so a new run-wide setting is
/// added to this struct and to [`ConvertRun::new`], not to each exporter's
/// arguments or to each `run` that fills them. A convert step that unpacks
/// every field still names the new one, so each exporter decides what it
/// does with it.
///
/// It holds the run's cancel flag and issue sink themselves: a clone of
/// either shares it, so a cancel set on the [`RunContext`] reaches the run.
///
/// The default writes [`OutputFormat::Json`] with no transforms, cannot be
/// cancelled, does not resume, and sends its rows nowhere.
#[derive(Debug, Clone, Default)]
pub struct ConvertRun {
    /// Media and obfuscation applied as conversations are written.
    pub transforms: ExportTransforms,
    /// The packaging the run writes.
    pub output_format: OutputFormat,
    /// Checked between files and before writing.
    pub cancel: CancelFlag,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
    /// Where each row for the Import Run's record goes as the run records it.
    pub issues: IssueSink,
}

impl ConvertRun {
    /// The run-wide settings of a run in `context` that writes `output`, with
    /// the transforms [`ExportTransforms::new`] builds.
    pub fn new(context: &RunContext, output: &OutputConfig) -> Self {
        Self {
            transforms: ExportTransforms::new(context, output),
            output_format: output.format,
            cancel: context.cancel.clone(),
            resume: output.resume,
            issues: context.issues.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
/// Fake-name rewrite: on/off plus an optional hex seed for repeatable output.
pub struct ObfuscateConfig {
    /// Whether fake-name rewrite is enabled.
    pub enabled: bool,
    /// Optional hex seed for repeatable obfuscation.
    pub seed: Option<String>,
}

/// What Staging does with attachment files: copy the originals, or copy
/// none. Staging never converts or compresses. That happens only in the
/// Media Stage, after the Staging Review, from the staged originals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AttachmentFiles {
    /// Copy each attachment's original file into `attachments/`.
    #[default]
    Originals,
    /// Copy no attachment files. Each attachment keeps its record, marked
    /// `not_copied`.
    Skip,
}

impl AttachmentFiles {
    /// True when attachment files are copied.
    pub fn copies(self) -> bool {
        self == Self::Originals
    }
}

#[derive(Debug, Clone)]
/// How an export handles attachment files, and where its tools are.
pub struct MediaConfig {
    /// Copy the original attachment files, or none.
    pub files: AttachmentFiles,
    /// The Tools Directory the WhatsApp exporter finds `wtsexporter` in
    /// ([`MediaTools::tools_dir`]). The caller builds them from where it
    /// keeps its tools ([`media::InstalledTools`]); a test passes a fake.
    pub tools: Arc<dyn MediaTools>,
}

impl Default for MediaConfig {
    /// Copy the originals, with no tools at all ([`NoTools`]).
    fn default() -> Self {
        Self {
            files: AttachmentFiles::Originals,
            tools: Arc::new(NoTools),
        }
    }
}

/// The options of the backup type a run reads, one variant per exporter.
/// Each exporter's `run` takes its own variant's config, so the desktop app
/// picks the exporter by the variant.
#[derive(Debug, Clone)]
pub enum SourceConfig {
    /// GO SMS Pro backup source.
    GoSmsPro(GoSmsProConfig),
    /// SMS Backup & Restore XML backup source.
    SmsBackupRestore(SmsBackupRestoreConfig),
    /// SMS Backup+ archive source.
    SmsBackupPlus(SmsBackupPlusConfig),
    /// OpenExtract backup source.
    OpenExtract(OpenExtractConfig),
    /// iMazing backup source.
    Imazing(ImazingConfig),
    /// iMessage / iPhone backup source.
    Apple(AppleConfig),
    /// WhatsApp backup source.
    Whatsapp(WhatsappConfig),
}

impl SourceConfig {
    /// The backup type this config is for.
    pub fn exporter(&self) -> Exporter {
        match self {
            Self::GoSmsPro(_) => Exporter::GoSmsPro,
            Self::SmsBackupRestore(_) => Exporter::SmsBackupRestore,
            Self::SmsBackupPlus(_) => Exporter::SmsBackupPlus,
            Self::OpenExtract(_) => Exporter::OpenExtract,
            Self::Imazing(_) => Exporter::Imazing,
            Self::Apple(_) => Exporter::Imessage,
            Self::Whatsapp(_) => Exporter::Whatsapp,
        }
    }
}

#[derive(Debug, Clone, Default)]
/// Convert an existing export directory to another output format
/// (`message-reexport`). Not a backup type, so not a [`SourceConfig`].
pub struct FormatConfig {
    /// When the Export Run this conversion is part of started. Export pulls
    /// JSON Lines from the server and then converts them, so its run starts
    /// well before the conversion does. `None` for Settings → Convert, which
    /// is a run of its own.
    pub run_started: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
/// GO SMS Pro extras: owner phone numbers used to mark outgoing messages.
pub struct GoSmsProConfig {
    /// Owner phone numbers used to mark outgoing messages.
    pub owner_phones: Vec<String>,
}

#[derive(Debug, Clone)]
/// SMS Backup & Restore extras: owner phone numbers used to mark outgoing messages.
pub struct SmsBackupRestoreConfig {
    /// Owner phone numbers used to mark outgoing messages.
    pub owner_phones: Vec<String>,
}

#[derive(Debug, Clone)]
/// SMS Backup+ extras: owner phones/emails and log flags.
pub struct SmsBackupPlusConfig {
    /// Owner phone numbers used to mark outgoing messages.
    pub owner_phones: Vec<String>,
    /// Owner email addresses used to mark outgoing messages.
    pub owner_emails: Vec<String>,
    /// The country of the phone the archive came from, when the import form
    /// states it. SMS Backup+ keys a number written without its `+` code in
    /// it, so the two spellings of one number are one person before the
    /// server sees them (#1676).
    pub phone_country: Option<&'static phone::Country>,
    /// Whether to emit verbose log lines.
    pub verbose: bool,
}

#[derive(Debug, Clone, Default)]
/// OpenExtract has no options of its own.
pub struct OpenExtractConfig {}

#[derive(Debug, Clone, Default)]
/// iMazing extras: the zone its dates are read in.
pub struct ImazingConfig {
    /// Zone for iMazing's timestamps, which carry none: a fixed UTC offset
    /// (`UTC-05:00`) or an IANA name (`America/New_York`). When `None`,
    /// dates are interpreted in host-local time.
    pub timezone: Option<String>,
}

#[derive(Debug, Clone)]
/// iMessage / iPhone backup extras: platform, copy method, address book, password.
pub struct AppleConfig {
    /// iPhone vs Mac backup layout; `None` means auto-detect.
    pub platform: Option<ApplePlatform>,
    /// Custom attachment root (macOS backups).
    pub attachment_root: Option<String>,
    /// `disabled`, `clone`, `basic`, or `full`.
    pub copy_method: String,
    /// macOS AddressBook path.
    pub apple_contacts: Option<PathBuf>,
    /// Apple backup decryption password.
    pub backup_password: Option<String>,
    /// Use the destination caller id as the outgoing From display name.
    pub use_caller_id: bool,
}

impl Default for AppleConfig {
    fn default() -> Self {
        Self {
            platform: None,
            attachment_root: None,
            copy_method: "clone".into(),
            apple_contacts: None,
            backup_password: None,
            use_caller_id: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
/// WhatsApp extras: Android vs iOS, key, backup directory, and optional media/db paths.
pub struct WhatsappConfig {
    /// Android vs iOS backup layout.
    pub platform: Option<WhatsappPlatform>,
    /// Optional path to an existing `result.json` to convert (skips wtsexporter).
    pub json: Option<PathBuf>,
    /// WhatsApp backup decryption key.
    pub key: Option<String>,
    /// Encrypted backup or iOS backup path.
    pub backup: Option<PathBuf>,
    /// iPhone backup decryption password, for an encrypted iPhone backup
    /// (never written to `export.ini`).
    pub backup_password: Option<String>,
    /// Contacts database (`wa.db` / `ContactsV2.sqlite`) path.
    pub wa: Option<PathBuf>,
    /// WhatsApp media directory path.
    pub media: Option<PathBuf>,
    /// Explicit `msgstore.db` path.
    pub db: Option<PathBuf>,
    /// Whether the backup is a WhatsApp Business backup.
    pub is_business_app: bool,
    /// The account holder's WhatsApp phone number, as typed on the form.
    /// Android has nothing else: a crypt backup does not carry the number.
    /// iPhone reads the number from the backup's preferences and uses this
    /// value only when that key is missing.
    pub owner_phone: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_input_returns_the_single_input_path() {
        let inputs = [PathBuf::from("backup/chat.db")];
        assert_eq!(require_input(&inputs), Ok(Path::new("backup/chat.db")));
    }

    #[test]
    fn require_input_refuses_a_config_with_no_input() {
        assert_eq!(require_input(&[]), Err("input is required".to_string()));
    }

    #[test]
    fn require_input_refuses_more_than_one_input() {
        let inputs = [PathBuf::from("a.xml"), PathBuf::from("b.xml")];
        assert_eq!(
            require_input(&inputs),
            Err("expected a single input path".to_string())
        );
    }
}
