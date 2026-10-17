//! Full export pipeline; the desktop app calls it in process.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::{Result, bail};
use message_crate_core::{OutputConfig, RunContext, RunResult, SmsBackupPlusConfig};
use std::path::PathBuf;

/// Check the owner's phone number and email address, then run the shared
/// pipeline.
///
/// # Errors
///
/// Returns an error when an input, owner phone, or owner email is missing,
/// conversion fails, media processing fails for every candidate file, or the
/// user cancels.
pub fn run(
    context: &RunContext,
    output: &OutputConfig,
    inputs: &[PathBuf],
    source: &SmsBackupPlusConfig,
) -> Result<RunResult> {
    message_crate_core::check_cancel(&context.cancel)?;

    if source.owner_phones.is_empty() {
        bail!("SMS Backup+ needs the backup device's phone number");
    }
    if source.owner_emails.is_empty() {
        bail!("SMS Backup+ needs the backup device's email address");
    }

    message_crate_core::run_pipeline(context, output, |convert_run| {
        convert_export(ConvertExportArgs {
            inputs,
            output_dir: &output.directory,
            owner_phones: &source.owner_phones,
            owner_emails: &source.owner_emails,
            phone_country: source.phone_country,
            verbose: source.verbose,
            log: &context.log,
            convert_run,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::emit::{ConvertExportArgs, convert_export};
    use message_crate_core::{
        ConvertRun, LogSink, OutputFormat, SmsBackupPlusConfig, testutil::jsonl_test_run,
    };
    use std::path::Path;

    fn source(phones: &[&str], emails: &[&str]) -> SmsBackupPlusConfig {
        SmsBackupPlusConfig {
            owner_phones: phones.iter().map(|p| (*p).to_string()).collect(),
            owner_emails: emails.iter().map(|e| (*e).to_string()).collect(),
            verbose: false,
            phone_country: None,
        }
    }

    /// The error the desktop app shows for a run the exporter refuses.
    fn refusal(inputs: &[&Path], source: SmsBackupPlusConfig) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let config = jsonl_test_run(inputs, &tmp.path().join("out"), source);
        config.run(run).unwrap_err().to_string()
    }

    /// The exporters have no command line, so a refusal says what is
    /// missing in the words of the desktop form and names no flag.
    #[test]
    fn a_refused_run_names_what_is_missing_and_no_flag() {
        let input = tempfile::tempdir().unwrap();
        assert_eq!(
            refusal(&[input.path()], source(&[], &["owner@example.com"])),
            "SMS Backup+ needs the backup device's phone number"
        );
        assert_eq!(
            refusal(&[input.path()], source(&["+15555550100"], &[])),
            "SMS Backup+ needs the backup device's email address"
        );
        assert_eq!(
            refusal(&[], source(&["+15555550100"], &["owner@example.com"])),
            "SMS Backup+ needs a backup directory"
        );

        let cache = tempfile::tempdir().unwrap();
        let no_input = convert_export(ConvertExportArgs {
            inputs: &[] as &[&Path],
            output_dir: input.path(),
            owner_phones: &["+15555550100".into()],
            owner_emails: &["owner@example.com".into()],
            verbose: false,
            log: &LogSink::none(),
            phone_country: phone::country("US"),
            convert_run: ConvertRun {
                scratch_dir: cache.path().to_path_buf(),
                output_format: OutputFormat::Jsonl,
                ..ConvertRun::default()
            },
        })
        .unwrap_err()
        .to_string();
        assert_eq!(no_input, "SMS Backup+ needs a backup directory");
    }
}
