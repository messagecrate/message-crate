//! Every backup type the Import screen offers, wired to its exporter in one
//! place.
//!
//! [`IMPORT_SOURCES`] holds each key the Import screen sends, the backup type
//! it names, and how it fills the [`Form`] that `Form::to_config` checks.
//! [`exporter`] holds each exporter: the programs from the Tools Directory it
//! runs, and its `run`, given the config of its own type. `extract` looks
//! both up and names no exporter itself. A new backup type adds its keys to
//! the table and an arm to [`exporter`], which does not compile until it has
//! one.

use std::path::PathBuf;

use message_crate_core::{
    ApplePlatform, Exporter, ExporterConfig, Form, OutputConfig, RunContext, RunResult,
    SourceConfig, WhatsappPlatform,
};

use super::ExtractOptions;
use crate::tool_downloads::Program;

/// One key the Import screen sends for a backup, such as `imessage-ios`.
pub(super) struct ImportSource {
    /// The key, as the Import screen sends it.
    pub key: &'static str,
    /// The backup type the key names.
    pub exporter: Exporter,
    /// Fill the form's fields for this key from the backup's `path` and the
    /// Import screen's options.
    pub fill: fn(&mut Form, &str, &ExtractOptions),
}

/// Every key the Import screen sends. Several keys can name one backup type:
/// iMessage reads an iPhone backup, a Mac's database, or a jailbreak copy.
pub(super) const IMPORT_SOURCES: &[ImportSource] = &[
    ImportSource {
        key: "imessage-ios",
        exporter: Exporter::Imessage,
        fill: |form, path, options| {
            form.db_path = path.to_string();
            form.apple_platform = ApplePlatform::Ios;
            form.backup_password.clone_from(&options.backup_password);
        },
    },
    ImportSource {
        key: "imessage-macos",
        exporter: Exporter::Imessage,
        fill: fill_imessage_mac,
    },
    ImportSource {
        key: "imessage-jailbreak",
        exporter: Exporter::Imessage,
        fill: fill_imessage_mac,
    },
    ImportSource {
        key: "sms-backup-restore",
        exporter: Exporter::SmsBackupRestore,
        fill: fill_android_sms,
    },
    ImportSource {
        key: "go-sms-pro",
        exporter: Exporter::GoSmsPro,
        fill: fill_android_sms,
    },
    ImportSource {
        key: "sms-backup-plus",
        exporter: Exporter::SmsBackupPlus,
        fill: |form, path, options| {
            fill_android_sms(form, path, options);
            form.owner_emails = options.owner_emails.join("\n");
        },
    },
    ImportSource {
        key: "openextract",
        exporter: Exporter::OpenExtract,
        fill: |form, path, _| {
            form.input = path.to_string();
        },
    },
    ImportSource {
        key: "imazing",
        exporter: Exporter::Imazing,
        fill: |form, path, options| {
            form.input = path.to_string();
            // iMazing dates carry no zone, so the exporter reads them in this
            // one. Without it the exporter falls back to the machine's zone,
            // and the same directory gives different instants on different
            // machines.
            form.timezone.clone_from(&options.timezone);
        },
    },
    // Both WhatsApp platforms take the holder's number as an owner phone:
    // Android's only source, iPhone's fallback when the backup's
    // preferences carry no owner key.
    ImportSource {
        key: "whatsapp-android",
        exporter: Exporter::Whatsapp,
        fill: |form, path, options| {
            form.input = path.to_string();
            form.owner_phones = options.owner_phones.join("\n");
            form.whatsapp_platform = WhatsappPlatform::Android;
            form.whatsapp_key.clone_from(&options.whatsapp_key);
            form.whatsapp_wa.clone_from(&options.whatsapp_wa);
            form.whatsapp_media.clone_from(&options.whatsapp_media);
            form.whatsapp_db.clone_from(&options.whatsapp_db);
        },
    },
    ImportSource {
        key: "whatsapp-ios",
        exporter: Exporter::Whatsapp,
        fill: |form, path, options| {
            form.input = path.to_string();
            form.owner_phones = options.owner_phones.join("\n");
            form.whatsapp_platform = WhatsappPlatform::Ios;
            form.whatsapp_backup = path.to_string();
            form.backup_password.clone_from(&options.backup_password);
            form.whatsapp_wa.clone_from(&options.whatsapp_wa);
            form.is_business_app = options.is_business_app;
        },
    },
];

/// The Import screen's `key`, or `None` for a key it does not send.
pub(super) fn import_source(key: &str) -> Option<&'static ImportSource> {
    IMPORT_SOURCES.iter().find(|source| source.key == key)
}

/// A Mac's Messages database, or a jailbreak copy of one.
fn fill_imessage_mac(form: &mut Form, path: &str, options: &ExtractOptions) {
    form.db_path = path.to_string();
    form.apple_platform = ApplePlatform::MacOs;
    form.attachment_root.clone_from(&options.attachment_root);
    form.apple_contacts.clone_from(&options.apple_contacts);
    // The Import screen offers obfuscation for iPhone backups and the
    // Android SMS sources, not for Mac Messages or a jailbreak copy.
    form.obfuscate = false;
}

/// An Android SMS backup: the file or directory, and the owner's numbers.
fn fill_android_sms(form: &mut Form, path: &str, options: &ExtractOptions) {
    form.input = path.to_string();
    form.owner_phones = options.owner_phones.join("\n");
}

/// The exporter's `run`, holding its own config, waiting for the rest of
/// what every `run` takes.
type RunExporter<'a> =
    Box<dyn FnOnce(&RunContext, &OutputConfig, &[PathBuf]) -> anyhow::Result<RunResult> + 'a>;

/// One exporter, ready to run on the config it was looked up by.
pub(super) struct ExporterEntry<'a> {
    /// The programs the Tools Directory holds that the exporter runs: a
    /// WhatsApp import runs wtsexporter. Staging copies attachments as they
    /// are whatever the person chose, so it never runs ffmpeg; the Media
    /// Stage does (`transcode_staging`).
    pub programs: &'static [Program],
    /// The exporter's `run`, given `source`'s own config.
    pub run: RunExporter<'a>,
}

/// The exporter that reads `source`, with `source`'s config, which is of the
/// exporter's own type.
pub(super) fn exporter(source: &SourceConfig) -> ExporterEntry<'_> {
    match source {
        SourceConfig::GoSmsPro(config) => ExporterEntry {
            programs: &[],
            run: Box::new(|context, output, inputs| {
                go_sms_pro_exporter::run(context, output, inputs, config)
            }),
        },
        SourceConfig::SmsBackupRestore(config) => ExporterEntry {
            programs: &[],
            run: Box::new(|context, output, inputs| {
                sms_backup_restore_exporter::run(context, output, inputs, config)
            }),
        },
        SourceConfig::SmsBackupPlus(config) => ExporterEntry {
            programs: &[],
            run: Box::new(|context, output, inputs| {
                sms_backup_plus_exporter::run(context, output, inputs, config)
            }),
        },
        SourceConfig::OpenExtract(config) => ExporterEntry {
            programs: &[],
            run: Box::new(|context, output, inputs| {
                openextract_exporter::run(context, output, inputs, config)
            }),
        },
        SourceConfig::Imazing(config) => ExporterEntry {
            programs: &[],
            run: Box::new(|context, output, inputs| {
                imazing_exporter::run(context, output, inputs, config)
            }),
        },
        SourceConfig::Apple(config) => ExporterEntry {
            programs: &[],
            run: Box::new(|context, output, inputs| {
                imessage_ir_exporter::run(context, output, inputs, config)
            }),
        },
        SourceConfig::Whatsapp(config) => ExporterEntry {
            programs: &[Program::Wtsexporter],
            run: Box::new(|context, output, inputs| {
                whatsapp_exporter::run(context, output, inputs, config)
            }),
        },
    }
}

/// Run the exporter `config.source` names on `config`.
///
/// # Errors
///
/// Returns an error if the exporter fails.
pub(super) fn run(config: &ExporterConfig) -> anyhow::Result<RunResult> {
    (exporter(&config.source).run)(&config.context, &config.output, &config.inputs)
}
