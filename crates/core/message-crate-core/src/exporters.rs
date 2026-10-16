//! Backup-type forms, dropdown labels, and validation used by the desktop app.
//!
//! [`Form`] is the GUI field set. [`Form::to_config`] turns it into a typed
//! [`ExporterConfig`] after checking required paths and options, with what it
//! needs to know beyond the form given in a [`FormContext`].

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use media::MediaTools;

use crate::config::{
    AppleConfig, AttachmentFiles, ExporterConfig, GoSmsProConfig, ImazingConfig, MediaConfig,
    ObfuscateConfig, OpenExtractConfig, OutputConfig, OutputFormat, RunContext,
    SmsBackupPlusConfig, SmsBackupRestoreConfig, SourceConfig, WhatsappConfig,
};

/// Which backup type the user selected (iMessage, WhatsApp, SMS Backup & Restore, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Exporter {
    /// GO SMS Pro backup type.
    GoSmsPro,
    /// iMazing backup type.
    Imazing,
    #[default]
    /// iMessage / iPhone backup type.
    Imessage,
    /// OpenExtract backup type.
    OpenExtract,
    /// SMS Backup & Restore backup type.
    SmsBackupRestore,
    /// SMS Backup+ backup type.
    SmsBackupPlus,
    /// WhatsApp backup type.
    Whatsapp,
}

impl Exporter {
    /// Every backup type, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::GoSmsPro,
        Self::Imazing,
        Self::Imessage,
        Self::OpenExtract,
        Self::SmsBackupRestore,
        Self::SmsBackupPlus,
        Self::Whatsapp,
    ];

    /// The id this backup type's exporter writes into a conversation file's
    /// `export.source`, which the import stamps on every message it writes
    /// and search's `source:` field matches. Each exporter writes the value
    /// this returns, and `api_types::SOURCE_IDS`, the list
    /// search accepts, holds every one (a test here checks it).
    pub const fn source_id(self) -> &'static str {
        use api_types::source_id;
        match self {
            Self::GoSmsPro => source_id::GO_SMS_PRO,
            Self::Imazing => source_id::IMAZING,
            Self::Imessage => source_id::IMESSAGE,
            Self::OpenExtract => source_id::OPENEXTRACT,
            Self::SmsBackupRestore => source_id::SMS_BACKUP_RESTORE,
            Self::SmsBackupPlus => source_id::SMS_BACKUP_PLUS,
            Self::Whatsapp => source_id::WHATSAPP,
        }
    }
}

/// Android vs iOS for the WhatsApp / wtsexporter bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhatsappPlatform {
    #[default]
    /// Android WhatsApp backup layout.
    Android,
    /// iOS WhatsApp backup layout.
    Ios,
}

impl fmt::Display for WhatsappPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Android => "Android",
            Self::Ios => "iOS",
        })
    }
}

/// Create `path` and parents if missing.
///
/// # Errors
///
/// Returns an error string when the directory cannot be created.
pub fn ensure_output_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| {
        format!(
            "Could not create output directory {}: {error}",
            path.display()
        )
    })
}

/// iPhone vs Mac backup layout for iMessage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApplePlatform {
    #[default]
    /// Auto-detect the backup layout.
    Auto,
    /// macOS backup layout.
    MacOs,
    /// iOS backup layout.
    Ios,
}

impl fmt::Display for ApplePlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "Auto-detect",
            Self::MacOs => "macOS",
            Self::Ios => "iOS backup",
        })
    }
}

/// GUI field set for one backup type, plus shared output and media options.
#[derive(Debug, Clone, Default)]
pub struct Form {
    /// Primary input path (source backup file or directory).
    pub input: String,
    /// Output directory for the export.
    pub output: String,
    /// Owner phone numbers, one per line or separated by commas or
    /// semicolons; spaces stay inside a number (marks outgoing messages).
    pub owner_phones: String,
    /// Owner email addresses, one per line or separated by commas or
    /// semicolons (marks outgoing messages).
    pub owner_emails: String,
    /// Optional zone for timestamps that carry none: a fixed UTC offset
    /// (`UTC-05:00`) or an IANA name (`America/New_York`).
    pub timezone: String,
    /// The country of the phone the backup came from, as an ISO 3166-1
    /// alpha-2 code (`GB`), or blank when the person names none. Every phone
    /// number the run writes without its `+` code is read as a number in it
    /// (#1676). The server applies it to every file of the run; SMS Backup+
    /// also keys its numbers by it, because it decides who is one person
    /// before the server sees the files.
    pub phone_country: String,
    /// Whether to rewrite output with stable fake identities.
    pub obfuscate: bool,
    /// Optional hex seed for reproducible obfuscation.
    pub obfuscate_seed: String,
    /// iMessage chat database path (Apple sources).
    pub db_path: String,
    /// Apple backup attachment root directory.
    pub attachment_root: String,
    /// macOS AddressBook path (Apple sources).
    pub apple_contacts: String,
    /// iPhone backup decryption password, for iMessage and for WhatsApp
    /// from an iPhone backup (never written to `export.ini`).
    pub backup_password: String,
    /// Packaging format projected from the common message (`json` default).
    pub output_format: OutputFormat,
    /// Whether the export copies the original attachment files.
    pub attachments: AttachmentFiles,
    /// iPhone vs Mac backup layout.
    pub apple_platform: ApplePlatform,
    /// Android vs iOS WhatsApp layout.
    pub whatsapp_platform: WhatsappPlatform,
    /// WhatsApp backup encryption key.
    pub whatsapp_key: String,
    /// WhatsApp backup file path.
    pub whatsapp_backup: String,
    /// WhatsApp contacts database (`wa.db` / `ContactsV2.sqlite`) path.
    pub whatsapp_wa: String,
    /// WhatsApp media directory path.
    pub whatsapp_media: String,
    /// WhatsApp message database path.
    pub whatsapp_db: String,
    /// Whether the backup is a WhatsApp Business backup.
    pub is_business_app: bool,
}

/// What [`Form::to_config`] needs to know beyond the form. The caller passes
/// it in, so validating a form reads no process-wide state.
#[derive(Debug, Clone)]
pub struct FormContext<'a> {
    /// The Scratch Directory the run's scratch data goes under
    /// ([`ExporterConfig::scratch_dir`]).
    pub scratch_dir: &'a Path,
    /// The run's tools, whose Tools Directory is where the WhatsApp
    /// exporter finds `wtsexporter` ([`MediaTools::tools_dir`]).
    pub tools: Arc<dyn MediaTools>,
}

impl Form {
    /// Validate the form and build a typed [`ExporterConfig`] for `exporter`
    /// in `ctx`.
    ///
    /// # Errors
    ///
    /// Returns one string per validation problem (missing path, bad seed, …).
    pub fn to_config(
        &self,
        exporter: Exporter,
        ctx: FormContext<'_>,
    ) -> Result<ExporterConfig, Vec<String>> {
        let mut errors = Vec::new();
        let obfuscate = self.validate_obfuscate(&mut errors);
        self.validate_phone_country(&mut errors);

        let config = match exporter {
            Exporter::Imessage => self.to_imessage_config(obfuscate, ctx, &mut errors),
            Exporter::Whatsapp => self.to_whatsapp_config(obfuscate, ctx, &mut errors),
            Exporter::Imazing => self.to_imazing_config(obfuscate, ctx, &mut errors),
            Exporter::OpenExtract => self.to_openextract_config(obfuscate, ctx, &mut errors),
            Exporter::GoSmsPro => self.to_go_sms_pro_config(obfuscate, ctx, &mut errors),
            Exporter::SmsBackupRestore => self.to_sms_restore_config(obfuscate, ctx, &mut errors),
            Exporter::SmsBackupPlus => self.to_sms_plus_config(obfuscate, ctx, &mut errors),
        };

        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }

    /// The country the form states for the phone, `None` when it states
    /// none or names a country the phone table does not hold.
    #[must_use]
    pub fn phone_country(&self) -> Option<&'static phone::Country> {
        phone::country(self.phone_country.trim())
    }

    /// Refuse a phone country the phone table does not hold. Blank is no
    /// country, which is allowed: a number without its `+` code then keeps
    /// its digits.
    fn validate_phone_country(&self, errors: &mut Vec<String>) {
        let code = self.phone_country.trim();
        if !code.is_empty() && phone::country(code).is_none() {
            errors.push(format!(
                "Phone country \"{code}\" is not a country code Message Crate knows."
            ));
        }
    }

    /// The [`ExporterConfig`] every source builder ends in. The caller gives
    /// the source's `inputs`, `obfuscate`, `media`, and `source`, and the
    /// form's context, which names the Scratch Directory and the tools the
    /// WhatsApp exporter uses. The output directory and output format come from
    /// the form. A run started from the form does not resume. Its sinks drop
    /// what they get until the caller sets its own, and nothing sets its
    /// cancel flag ([`RunContext::new`]).
    fn exporter_config(
        &self,
        inputs: Vec<PathBuf>,
        obfuscate: ObfuscateConfig,
        ctx: FormContext<'_>,
        media: MediaConfig,
        source: SourceConfig,
    ) -> ExporterConfig {
        ExporterConfig {
            inputs,
            output: OutputConfig {
                obfuscate,
                media: MediaConfig {
                    tools: ctx.tools,
                    ..media
                },
                ..OutputConfig::new(self.output.trim(), self.output_format)
            },
            context: RunContext::new(ctx.scratch_dir),
            source,
        }
    }

    /// Build an iMessage config, pushing path problems onto `errors`.
    fn to_imessage_config(
        &self,
        obfuscate: ObfuscateConfig,
        ctx: FormContext<'_>,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        required_text(&self.output, "Output directory", errors);
        let media = self.media();
        let copy_method = match self.attachments {
            AttachmentFiles::Skip => "disabled".into(),
            AttachmentFiles::Originals => "clone".into(),
        };
        let platform = match self.apple_platform {
            ApplePlatform::Auto => None,
            other => Some(other),
        };
        let inputs = ir::trimmed(&self.db_path)
            .map(|p| vec![PathBuf::from(p)])
            .unwrap_or_default();
        self.exporter_config(
            inputs,
            obfuscate,
            ctx,
            media,
            SourceConfig::Apple(AppleConfig {
                platform,
                attachment_root: ir::nonempty(&self.attachment_root),
                copy_method,
                apple_contacts: non_empty_path(&self.apple_contacts),
                backup_password: ir::nonempty(&self.backup_password),
                use_caller_id: true,
            }),
        )
    }

    /// Build a WhatsApp config, pushing path and key problems onto `errors`.
    ///
    /// `input` is optional here: when set it becomes the backup search root
    /// (and an allowed media root) for the wtsexporter bridge, which falls
    /// back to its working directory when no input is given.
    fn to_whatsapp_config(
        &self,
        obfuscate: ObfuscateConfig,
        ctx: FormContext<'_>,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let inputs = if self.input.trim().is_empty() {
            Vec::new()
        } else {
            require_single_existing_path(&self.input, "Input", errors)
                .into_iter()
                .collect()
        };
        required_text(&self.output, "Output", errors);
        if self.whatsapp_platform == WhatsappPlatform::Ios && self.whatsapp_backup.trim().is_empty()
        {
            errors.push("Backup path is required for iOS.".into());
        }
        // One number: a WhatsApp account has exactly one. Android has no other
        // source for it, so an empty field stops the form here rather than
        // running an import that records no owner. iPhone reads the number
        // from the backup and falls back to this field, so it may be empty.
        let owner_phone = values(&self.owner_phones).first().map(|p| p.to_string());
        if self.whatsapp_platform == WhatsappPlatform::Android && owner_phone.is_none() {
            errors.push("Owner's WhatsApp number is required.".into());
        }
        let media = self.media();
        self.exporter_config(
            inputs,
            obfuscate,
            ctx,
            media,
            SourceConfig::Whatsapp(WhatsappConfig {
                platform: Some(self.whatsapp_platform),
                json: None,
                key: ir::nonempty(&self.whatsapp_key),
                backup: non_empty_path(&self.whatsapp_backup),
                backup_password: if self.whatsapp_platform == WhatsappPlatform::Ios {
                    ir::nonempty(&self.backup_password)
                } else {
                    None
                },
                wa: non_empty_path(&self.whatsapp_wa),
                media: non_empty_path(&self.whatsapp_media),
                db: non_empty_path(&self.whatsapp_db),
                is_business_app: self.is_business_app,
                owner_phone,
            }),
        )
    }

    /// Build an iMazing config, pushing path problems onto `errors`.
    fn to_imazing_config(
        &self,
        obfuscate: ObfuscateConfig,
        ctx: FormContext<'_>,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let input = require_single_existing_path(&self.input, "Input", errors);
        required_text(&self.output, "Output", errors);
        let media = self.media();
        self.exporter_config(
            input.into_iter().collect(),
            obfuscate,
            ctx,
            media,
            SourceConfig::Imazing(ImazingConfig {
                timezone: ir::nonempty(&self.timezone),
            }),
        )
    }

    /// Build an OpenExtract config, pushing path problems onto `errors`.
    fn to_openextract_config(
        &self,
        obfuscate: ObfuscateConfig,
        ctx: FormContext<'_>,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let input = require_single_existing_path(&self.input, "Input", errors);
        required_text(&self.output, "Output", errors);
        let media = self.media();
        self.exporter_config(
            input.into_iter().collect(),
            obfuscate,
            ctx,
            media,
            SourceConfig::OpenExtract(OpenExtractConfig {}),
        )
    }

    /// Build a GO SMS Pro config from the shared Android fields.
    fn to_go_sms_pro_config(
        &self,
        obfuscate: ObfuscateConfig,
        ctx: FormContext<'_>,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let (inputs, media, owner_phones) = self.android_common(errors);
        self.exporter_config(
            inputs,
            obfuscate,
            ctx,
            media,
            SourceConfig::GoSmsPro(GoSmsProConfig { owner_phones }),
        )
    }

    /// Build an SMS Backup & Restore config from the shared Android fields.
    fn to_sms_restore_config(
        &self,
        obfuscate: ObfuscateConfig,
        ctx: FormContext<'_>,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let (inputs, media, owner_phones) = self.android_common(errors);
        // SMS Backup & Restore writes each backup as one file, and an
        // Import Run reads one backup.
        if let Some(dir) = inputs.iter().find(|input| input.is_dir()) {
            errors.push(format!(
                "Input must be one SMS Backup & Restore .xml file, not a directory: {}",
                dir.display()
            ));
        }
        self.exporter_config(
            inputs,
            obfuscate,
            ctx,
            media,
            SourceConfig::SmsBackupRestore(SmsBackupRestoreConfig { owner_phones }),
        )
    }

    /// Build an SMS Backup+ config, including owner emails.
    fn to_sms_plus_config(
        &self,
        obfuscate: ObfuscateConfig,
        ctx: FormContext<'_>,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let (inputs, media, owner_phones) = self.android_common(errors);
        let owner_emails: Vec<String> = values(&self.owner_emails)
            .into_iter()
            .map(str::to_string)
            .collect();
        if owner_emails.is_empty() {
            errors.push("At least one email address is required.".into());
        }
        self.exporter_config(
            inputs,
            obfuscate,
            ctx,
            media,
            SourceConfig::SmsBackupPlus(SmsBackupPlusConfig {
                owner_phones,
                owner_emails,
                phone_country: self.phone_country(),
                verbose: true,
            }),
        )
    }

    /// Shared Android backup fields: input path, owner phones, media.
    fn android_common(&self, errors: &mut Vec<String>) -> (Vec<PathBuf>, MediaConfig, Vec<String>) {
        let input = require_single_existing_path(&self.input, "Input", errors);
        required_text(&self.output, "Output", errors);
        let mut owner_phones = Vec::new();
        for phone in values(&self.owner_phones) {
            owner_phones.push(phone.to_string());
        }
        if owner_phones.is_empty() {
            errors.push("At least one phone number is required.".into());
        }
        let media = self.media();
        (input.into_iter().collect(), media, owner_phones)
    }

    /// The media options every exporter gets from the form: whether to copy
    /// the original attachment files. The tools come from the form's context
    /// ([`Self::exporter_config`]).
    fn media(&self) -> MediaConfig {
        MediaConfig {
            files: self.attachments,
            ..MediaConfig::default()
        }
    }

    /// Fake-name rewrite flag and optional hex seed.
    fn validate_obfuscate(&self, errors: &mut Vec<String>) -> ObfuscateConfig {
        let seed = validate_obfuscate_seed(&self.obfuscate_seed, errors);
        ObfuscateConfig {
            enabled: self.obfuscate || seed.is_some(),
            seed,
        }
    }
}

/// Require one existing file or directory; push a message onto `errors` if missing.
fn require_single_existing_path(
    value: &str,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<PathBuf> {
    let paths = lines(value);
    if paths.is_empty() {
        errors.push(format!("{label} is required."));
        return None;
    }
    if paths.len() > 1 {
        errors.push(format!("{label} must be a single file or directory."));
        return None;
    }
    let path = paths[0];
    if !Path::new(path).exists() {
        errors.push(format!("{label} path does not exist: {path}"));
    }
    Some(PathBuf::from(path))
}

/// Push `"{label} is required."` when `value` is blank.
fn required_text(value: &str, label: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{label} is required."));
    }
}

/// Return a trimmed hex seed, or push an error when it is not a valid seed.
fn validate_obfuscate_seed(seed: &str, errors: &mut Vec<String>) -> Option<String> {
    let seed = seed.trim();
    if seed.is_empty() {
        return None;
    }
    match obfuscate::check_seed_hex(seed) {
        Ok(()) => Some(seed.to_string()),
        Err(message) => {
            errors.push(message);
            None
        }
    }
}

/// Non-empty trimmed lines from a multiline text field.
fn lines(value: &str) -> Vec<&str> {
    value.lines().filter_map(ir::trimmed).collect()
}

/// Non-empty trimmed values split on newlines, commas or semicolons.
///
/// Spaces do not split: people type phone numbers with them
/// (`+1 555-555-0119`), so `"+1555 +1666"` is one value.
fn values(value: &str) -> Vec<&str> {
    value
        .split(['\n', ',', ';'])
        .filter_map(ir::trimmed)
        .collect()
}

/// `Some(path)` when the string is not blank.
fn non_empty_path(value: &str) -> Option<PathBuf> {
    ir::trimmed(value).map(PathBuf::from)
}

#[cfg(test)]
mod tests;
