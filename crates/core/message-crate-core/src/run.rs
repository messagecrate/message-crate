//! The shared exporter run skeleton and tail.

use crate::config::{ConvertRun, OutputConfig, RunContext};
use crate::pipeline::{ExportReport, RunResult};
use crate::process::check_cancel;

/// The shared exporter run skeleton: cancel check, the run-wide settings
/// ([`ConvertRun::new`]), conversion, and result assembly.
///
/// Exporters no longer resolve names from a contacts file. A backup that
/// carries its own contact data (Apple's address book, WhatsApp's contacts
/// database) is read by that exporter directly; everything else arrives at the
/// server as raw identities and is reconciled there.
///
/// # Errors
///
/// Returns an error when the user cancels or conversion fails.
pub fn run_pipeline(
    context: &RunContext,
    output: &OutputConfig,
    convert: impl FnOnce(ConvertRun) -> anyhow::Result<ExportReport>,
) -> anyhow::Result<RunResult> {
    check_cancel(&context.cancel)?;
    let report = convert(ConvertRun::new(context, output))?;
    Ok(finish_run(output, &report))
}

/// The run tail shared by exporters whose middle diverges (WhatsApp,
/// iMessage): log-line and summary assembly.
pub fn finish_run(output: &OutputConfig, report: &ExportReport) -> RunResult {
    let mut messages: Vec<String> = report.obfuscated_line().into_iter().collect();
    report.summary_lines(output.format, &output.directory, &mut messages);
    RunResult::new(messages, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AttachmentFiles, MediaConfig, ObfuscateConfig, OutputFormat};
    use crate::pipeline::{IssueSink, RunIssue, RunIssueKind};
    use std::sync::{Arc, Mutex};

    fn output(files: AttachmentFiles, obfuscate: bool) -> OutputConfig {
        OutputConfig {
            obfuscate: ObfuscateConfig {
                enabled: obfuscate,
                seed: None,
            },
            media: MediaConfig {
                files,
                ..MediaConfig::default()
            },
            ..OutputConfig::new("out", OutputFormat::Jsonl)
        }
    }

    fn context() -> RunContext {
        RunContext::new("/cache")
    }

    /// A run-wide setting the config holds and the convert step never sees
    /// is a setting the run ignores: a resumed run that cleans its output,
    /// or a cancel the reader never checks.
    #[test]
    fn run_pipeline_hands_convert_every_run_wide_setting_of_the_config() {
        let mut context = context();
        let mut output = output(AttachmentFiles::Originals, false);
        let rows = Arc::new(Mutex::new(Vec::new()));
        let rows_seen = Arc::clone(&rows);
        context.issues = IssueSink::new(move |issue| rows_seen.lock().unwrap().push(issue));
        output.format = OutputFormat::Csv;
        output.resume = true;

        run_pipeline(&context, &output, |run| {
            assert!(Arc::ptr_eq(&run.cancel, &context.cancel));
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
        let result = run_pipeline(&context(), &output(AttachmentFiles::Skip, true), |run| {
            let t = run.transforms;
            assert!(t.obfuscate);
            assert_eq!(t.files, AttachmentFiles::Skip);
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
    fn run_pipeline_returns_the_convert_error() {
        let err = run_pipeline(
            &context(),
            &output(AttachmentFiles::Originals, false),
            |_| anyhow::bail!("unreadable backup"),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "unreadable backup");
    }
}
