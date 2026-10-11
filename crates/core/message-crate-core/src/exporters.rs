//! Backup-type forms, dropdown labels, and validation used by the desktop app.
//!
//! [`Form`] is the GUI field set. [`Form::to_config`] turns it into a typed
//! [`ExporterConfig`] after checking required paths and options.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use media::{MaxResolution, MediaMode};

use crate::config::{
    AppleConfig, ExporterConfig, GoSmsProConfig, ImazingConfig, MediaConfig, ObfuscateConfig,
    OpenExtractConfig, OutputFormat, SmsBackupPlusConfig, SmsBackupRestoreConfig, SourceConfig,
    WhatsappConfig,
};

/// Validation message when Convert or Compress is selected and ffmpeg is missing.
pub const CONVERT_COMPRESS_FFMPEG_REQUIRED: &str = "Convert and Compress need ffmpeg and ffprobe, both on PATH or both in the Tools Directory. Settings → System shows which can't be used and where the Tools Directory is.";

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

/// How attachments are copied or converted when writing output files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttachmentMedia {
    #[default]
    /// Copy attachments without re-encoding.
    Clone,
    /// Re-encode attachments to a standard format.
    Convert,
    /// Re-encode and compress videos (720p/1080p/4k).
    Compress,
    /// Do not copy attachments.
    Disabled,
}

impl fmt::Display for AttachmentMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Clone => "Copy",
            Self::Convert => "Convert",
            Self::Compress => "Convert & compress",
            Self::Disabled => "Do not copy",
        })
    }
}

impl AttachmentMedia {
    /// The [`MediaMode`] this export-form choice maps to.
    pub fn media_mode(self) -> MediaMode {
        match self {
            Self::Clone => MediaMode::Clone,
            Self::Convert => MediaMode::Convert,
            Self::Compress => MediaMode::Compress,
            Self::Disabled => MediaMode::Disabled,
        }
    }

    /// True when convert or compress is selected (ffmpeg must be on PATH).
    pub fn needs_ffmpeg(self) -> bool {
        matches!(self, Self::Convert | Self::Compress)
    }

    /// Parse a media-mode wire string (`clone`, `convert`, `compress`,
    /// `disabled`), or `None` if unknown.
    pub fn parse(s: &str) -> Option<Self> {
        MediaMode::parse(s).map(|mode| match mode {
            MediaMode::Clone => Self::Clone,
            MediaMode::Convert => Self::Convert,
            MediaMode::Compress => Self::Compress,
            MediaMode::Disabled => Self::Disabled,
        })
    }
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
#[derive(Debug, Clone)]
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
    /// Attachment handling choice for the export.
    pub attachment_media: AttachmentMedia,
    /// Compress-only long-edge cap (720p/1080p/4k).
    pub media_max_resolution: MaxResolution,
    /// Compress-only max frame rate.
    pub media_max_fps: String,
    /// Compress-only minimum video size, as a whole number of megabytes
    /// (e.g. `20`).
    pub media_min_size: String,
    /// Compress-only: skip already-efficient HEVC videos.
    pub media_skip_efficient: bool,
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

impl Default for Form {
    fn default() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            owner_phones: String::new(),
            owner_emails: String::new(),
            timezone: String::new(),
            phone_country: String::new(),
            obfuscate: false,
            obfuscate_seed: String::new(),
            db_path: String::new(),
            attachment_root: String::new(),
            apple_contacts: String::new(),
            backup_password: String::new(),
            output_format: OutputFormat::default(),
            attachment_media: AttachmentMedia::default(),
            media_max_resolution: MaxResolution::default(),
            media_max_fps: "30".into(),
            media_min_size: "20".into(),
            media_skip_efficient: true,
            apple_platform: ApplePlatform::default(),
            whatsapp_platform: WhatsappPlatform::default(),
            whatsapp_key: String::new(),
            whatsapp_backup: String::new(),
            whatsapp_wa: String::new(),
            whatsapp_media: String::new(),
            whatsapp_db: String::new(),
            is_business_app: false,
        }
    }
}

impl Form {
    /// Validate the form and build a typed [`ExporterConfig`] for `exporter`,
    /// whose scratch data goes under `scratch_dir`, the Scratch Directory
    /// ([`ExporterConfig::scratch_dir`]).
    ///
    /// # Errors
    ///
    /// Returns one string per validation problem (missing path, bad seed, …).
    pub fn to_config(
        &self,
        exporter: Exporter,
        scratch_dir: &Path,
    ) -> Result<ExporterConfig, Vec<String>> {
        let mut errors = Vec::new();
        let obfuscate = self.validate_obfuscate(&mut errors);
        self.validate_phone_country(&mut errors);

        let config = match exporter {
            Exporter::Imessage => self.to_imessage_config(obfuscate, scratch_dir, &mut errors),
            Exporter::Whatsapp => self.to_whatsapp_config(obfuscate, scratch_dir, &mut errors),
            Exporter::Imazing => self.to_imazing_config(obfuscate, scratch_dir, &mut errors),
            Exporter::OpenExtract => {
                self.to_openextract_config(obfuscate, scratch_dir, &mut errors)
            }
            Exporter::GoSmsPro => self.to_go_sms_pro_config(obfuscate, scratch_dir, &mut errors),
            Exporter::SmsBackupRestore => {
                self.to_sms_restore_config(obfuscate, scratch_dir, &mut errors)
            }
            Exporter::SmsBackupPlus => self.to_sms_plus_config(obfuscate, scratch_dir, &mut errors),
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
    /// Scratch Directory. The output directory and output format come from
    /// the form. A run started from the form does not resume. It has no
    /// sinks, no cancel flag, and no zone for timestamps that carry none.
    /// The iMazing builder sets that zone on the result.
    fn exporter_config(
        &self,
        inputs: Vec<PathBuf>,
        obfuscate: ObfuscateConfig,
        scratch_dir: &Path,
        media: MediaConfig,
        source: SourceConfig,
    ) -> ExporterConfig {
        ExporterConfig {
            inputs,
            output: PathBuf::from(self.output.trim()),
            scratch_dir: scratch_dir.to_path_buf(),
            timezone: None,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            issues: None,
            output_format: self.output_format,
            resume: false,
            source,
        }
    }

    /// Build an iMessage config, pushing path and ffmpeg problems onto `errors`.
    fn to_imessage_config(
        &self,
        obfuscate: ObfuscateConfig,
        scratch_dir: &Path,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        required_text(&self.output, "Output directory", errors);
        let obfuscate_active = self.obfuscate || !self.obfuscate_seed.trim().is_empty();
        if !obfuscate_active && self.attachment_media.needs_ffmpeg() && !media::ffmpeg_available() {
            errors.push(CONVERT_COMPRESS_FFMPEG_REQUIRED.into());
        }
        let media = self.media_config_for(
            matches!(self.attachment_media, AttachmentMedia::Compress),
            errors,
        );
        let copy_method = match self.attachment_media {
            AttachmentMedia::Disabled => "disabled".into(),
            _ => "clone".into(),
        };
        let platform = match self.apple_platform {
            ApplePlatform::Auto => None,
            other => Some(other),
        };
        let inputs = message_ir::trimmed(&self.db_path)
            .map(|p| vec![PathBuf::from(p)])
            .unwrap_or_default();
        self.exporter_config(
            inputs,
            obfuscate,
            scratch_dir,
            media,
            SourceConfig::Apple(AppleConfig {
                platform,
                attachment_root: message_ir::nonempty(&self.attachment_root),
                copy_method,
                apple_contacts: non_empty_path(&self.apple_contacts),
                backup_password: message_ir::nonempty(&self.backup_password),
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
        scratch_dir: &Path,
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
        let media = self.validate_media(errors);
        self.exporter_config(
            inputs,
            obfuscate,
            scratch_dir,
            media,
            SourceConfig::Whatsapp(WhatsappConfig {
                platform: Some(self.whatsapp_platform),
                json: None,
                key: message_ir::nonempty(&self.whatsapp_key),
                backup: non_empty_path(&self.whatsapp_backup),
                backup_password: if self.whatsapp_platform == WhatsappPlatform::Ios {
                    message_ir::nonempty(&self.backup_password)
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

    /// Build an iMazing config, pushing path and media problems onto `errors`.
    fn to_imazing_config(
        &self,
        obfuscate: ObfuscateConfig,
        scratch_dir: &Path,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let input = require_single_existing_path(&self.input, "Input", errors);
        required_text(&self.output, "Output", errors);
        let media = self.validate_media(errors);
        let mut config = self.exporter_config(
            input.into_iter().collect(),
            obfuscate,
            scratch_dir,
            media,
            SourceConfig::Imazing(ImazingConfig {}),
        );
        config.timezone = message_ir::nonempty(&self.timezone);
        config
    }

    /// Build an OpenExtract config, pushing path problems onto `errors`.
    fn to_openextract_config(
        &self,
        obfuscate: ObfuscateConfig,
        scratch_dir: &Path,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let input = require_single_existing_path(&self.input, "Input", errors);
        required_text(&self.output, "Output", errors);
        let media = self.validate_media(errors);
        self.exporter_config(
            input.into_iter().collect(),
            obfuscate,
            scratch_dir,
            media,
            SourceConfig::OpenExtract(OpenExtractConfig {}),
        )
    }

    /// Build a GO SMS Pro config from the shared Android fields.
    fn to_go_sms_pro_config(
        &self,
        obfuscate: ObfuscateConfig,
        scratch_dir: &Path,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let (inputs, media, owner_phones) = self.android_common(errors);
        self.exporter_config(
            inputs,
            obfuscate,
            scratch_dir,
            media,
            SourceConfig::GoSmsPro(GoSmsProConfig { owner_phones }),
        )
    }

    /// Build an SMS Backup & Restore config from the shared Android fields.
    fn to_sms_restore_config(
        &self,
        obfuscate: ObfuscateConfig,
        scratch_dir: &Path,
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
            scratch_dir,
            media,
            SourceConfig::SmsBackupRestore(SmsBackupRestoreConfig { owner_phones }),
        )
    }

    /// Build an SMS Backup+ config, including owner emails.
    fn to_sms_plus_config(
        &self,
        obfuscate: ObfuscateConfig,
        scratch_dir: &Path,
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
            scratch_dir,
            media,
            SourceConfig::SmsBackupPlus(SmsBackupPlusConfig {
                owner_phones,
                owner_emails,
                phone_country: self.phone_country(),
                verbose: true,
                include_summary: true,
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
        let media = self.validate_media(errors);
        (input.into_iter().collect(), media, owner_phones)
    }

    /// Media options for every exporter except iMessage; compress settings are
    /// validated only in Compress mode.
    fn validate_media(&self, errors: &mut Vec<String>) -> MediaConfig {
        let mode = self.attachment_media.media_mode();
        let obfuscate_active = self.obfuscate || !self.obfuscate_seed.trim().is_empty();
        // Obfuscate skips copy/convert, so ffmpeg is not required.
        if !obfuscate_active && mode.needs_tools() && !media::ffmpeg_available() {
            errors.push(CONVERT_COMPRESS_FFMPEG_REQUIRED.into());
        }
        self.media_config_for(matches!(mode, MediaMode::Compress), errors)
    }

    /// Fake-name rewrite flag and optional hex seed.
    fn validate_obfuscate(&self, errors: &mut Vec<String>) -> ObfuscateConfig {
        let seed = validate_obfuscate_seed(&self.obfuscate_seed, errors);
        ObfuscateConfig {
            enabled: self.obfuscate || seed.is_some(),
            seed,
        }
    }

    /// Attachment copy/convert/compress options; optionally check compress fields.
    fn media_config_for(&self, validate_compress: bool, errors: &mut Vec<String>) -> MediaConfig {
        let mode = self.attachment_media.media_mode();
        let compress = if validate_compress || matches!(mode, MediaMode::Compress) {
            match self.compress_options() {
                Ok(options) => options,
                Err(error) => {
                    errors.push(error);
                    media::CompressOptions::default()
                }
            }
        } else {
            media::CompressOptions::default()
        };
        MediaConfig { mode, compress }
    }

    /// Compress options parsed from the form; checked while the form becomes a config.
    ///
    /// # Errors
    ///
    /// Returns an error when Max FPS is empty or is not a number above 0, or
    /// when Minimum Video File Size is empty or is not a whole number of
    /// megabytes, in the words [`media::compress_options_from_form`] gives.
    pub fn compress_options(&self) -> Result<media::CompressOptions, String> {
        media::compress_options_from_form(
            self.media_max_resolution,
            &self.media_max_fps,
            &self.media_min_size,
            self.media_skip_efficient,
        )
        .map_err(|e| format!("{e:#}"))
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
    value.lines().filter_map(message_ir::trimmed).collect()
}

/// Non-empty trimmed values split on newlines, commas or semicolons.
///
/// Spaces do not split: people type phone numbers with them
/// (`+1 555-555-0119`), so `"+1555 +1666"` is one value.
fn values(value: &str) -> Vec<&str> {
    value
        .split(['\n', ',', ';'])
        .filter_map(message_ir::trimmed)
        .collect()
}

/// `Some(path)` when the string is not blank.
fn non_empty_path(value: &str) -> Option<PathBuf> {
    message_ir::trimmed(value).map(PathBuf::from)
}

#[cfg(test)]
mod tests;
