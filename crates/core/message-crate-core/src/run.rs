//! The shared exporter run skeleton and tail.

use crate::config::{ConvertRun, ExporterConfig};
use crate::pipeline::{ExportReport, RunResult};
use crate::process::check_cancel;

/// The shared exporter run skeleton: cancel check, the run-wide settings
/// ([`ExporterConfig::convert_run`]), conversion, media-failure bail, and
/// result assembly.
///
/// Exporters no longer resolve names from a contacts file. A backup that
/// carries its own contact data (Apple's address book, WhatsApp's contacts
/// database) is read by that exporter directly; everything else arrives at the
/// server as raw identities and is reconciled there.
///
/// # Errors
///
/// Returns an error when the user cancels, conversion fails, or media
/// processing fails for every candidate file.
pub fn run_pipeline(
    config: &ExporterConfig,
    convert: impl FnOnce(ConvertRun) -> anyhow::Result<ExportReport>,
) -> anyhow::Result<RunResult> {
    check_cancel(&config.cancel)?;
    let report = convert(config.convert_run())?;
    finish_run(config, &report, config.media.mode.needs_tools())
}

/// The run tail shared by exporters whose middle diverges (WhatsApp,
/// iMessage): media-failure bail plus log-line and summary assembly.
///
/// # Errors
///
/// Returns an error when media processing fails for every candidate file.
pub fn finish_run(
    config: &ExporterConfig,
    report: &ExportReport,
    needs_tools: bool,
) -> anyhow::Result<RunResult> {
    report.check_media(needs_tools)?;
    let mut messages = report.media_lines();
    report.summary_lines(config.output_format, &config.output, &mut messages);
    Ok(RunResult::new(messages, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FormatConfig, MediaConfig, ObfuscateConfig, OutputFormat, SourceConfig};
    use crate::pipeline::{IssueSink, RunIssue, RunIssueKind};
    use crate::process::{CancelFlag, LogSink};
    use crate::progress::ProgressSink;
    use media::{CompressOptions, MediaMode};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    fn config(mode: MediaMode, obfuscate: bool) -> ExporterConfig {
        ExporterConfig {
            inputs: Vec::new(),
            output: PathBuf::from("out"),
            scratch_dir: PathBuf::from("/cache"),
            timezone: None,
            obfuscate: ObfuscateConfig {
                enabled: obfuscate,
                seed: None,
            },
            media: MediaConfig {
                mode,
                compress: CompressOptions::default(),
            },
            cancel: CancelFlag::default(),
            log: LogSink::none(),
            progress: ProgressSink::none(),
            issues: IssueSink::none(),
            output_format: OutputFormat::Jsonl,
            resume: false,
            source: SourceConfig::Format(FormatConfig::default()),
        }
    }

    /// A run-wide setting the config holds and the convert step never sees
    /// is a setting the run ignores: a resumed run that cleans its output,
    /// or a cancel the reader never checks.
    #[test]
    fn run_pipeline_hands_convert_every_run_wide_setting_of_the_config() {
        let mut config = config(MediaMode::Clone, false);
        let rows = Arc::new(Mutex::new(Vec::new()));
        let rows_seen = Arc::clone(&rows);
        config.issues = IssueSink::new(move |issue| rows_seen.lock().unwrap().push(issue));
        config.output_format = OutputFormat::Csv;
        config.resume = true;

        run_pipeline(&config, |run| {
            assert!(Arc::ptr_eq(&run.cancel, &config.cancel));
            run.issues.emit(RunIssue {
                kind: RunIssueKind::Error,
                step: "parse".into(),
                item: "a.txt".into(),
                reason: "unreadable".into(),
                conversation: None,
            });
            assert_eq!(run.output_format, OutputFormat::Csv);
            assert!(run.resume);
            Ok(ExportReport::default())
        })
        .unwrap();
        assert_eq!(rows.lock().unwrap().len(), 1);
    }

    #[test]
    fn run_pipeline_hands_the_config_transforms_to_convert_and_returns_the_summary() {
        let result = run_pipeline(&config(MediaMode::Convert, true), |run| {
            let t = run.transforms;
            assert!(t.obfuscate);
            assert_eq!(t.media, MediaMode::Convert);
            Ok(ExportReport {
                conversations: 1,
                messages: 3,
                attachments_saved: 2,
                ..ExportReport::default()
            })
        })
        .unwrap();

        assert_eq!(
            result.messages,
            ["Wrote jsonl export under out", "  Saved 2 attachments"]
        );
        assert_eq!((result.conversations, result.message_count), (1, 3));
    }

    #[test]
    fn run_pipeline_fails_when_media_failed_on_every_file() {
        let err = run_pipeline(&config(MediaMode::Convert, false), |_| {
            let mut report = ExportReport::default();
            report.media.errors.push("a.heic: ffmpeg not found".into());
            Ok(report)
        })
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "media processing failed for all candidate files"
        );
    }

    #[test]
    fn run_pipeline_returns_the_convert_error() {
        let err = run_pipeline(&config(MediaMode::Clone, false), |_| {
            anyhow::bail!("unreadable backup")
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "unreadable backup");
    }
}
