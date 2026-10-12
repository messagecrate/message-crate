//! Full export pipeline; the desktop app calls it in process.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_crate_core::{
    OutputConfig, RunContext, RunResult, SmsBackupRestoreConfig, prepare_outputs, require_input,
};
use std::path::PathBuf;

/// Convert, then apply media transforms and obfuscation.
///
/// # Errors
///
/// Returns an error when there is not exactly one input, the output
/// directory holds the input, conversion fails, media processing fails for
/// every candidate file, or the user cancels.
pub fn run(
    context: &RunContext,
    output: &OutputConfig,
    inputs: &[PathBuf],
    source: &SmsBackupRestoreConfig,
) -> Result<RunResult> {
    message_crate_core::check_cancel(&context.cancel)?;
    let input = require_input(inputs).map_err(anyhow::Error::msg)?;
    let (inputs, output_dir) = prepare_outputs(&[input.to_path_buf()], &output.directory)?;
    message_crate_core::run_pipeline(context, output, |convert_run| {
        convert_export(ConvertExportArgs {
            input: &inputs[0],
            output_dir: &output_dir,
            scratch_dir: &context.scratch_dir,
            owner_phones: &source.owner_phones,
            convert_run,
        })
    })
}

#[cfg(test)]
mod tests {
    use message_crate_core::{SmsBackupRestoreConfig, testutil::jsonl_test_run};
    use std::fs;
    use std::path::Path;

    /// The names in `dir`, sorted.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The writer cleans its output directory before it writes, so an output
    /// that holds the backup would delete the backup being read.
    #[test]
    fn run_refuses_an_output_that_holds_the_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("backup");
        fs::create_dir_all(&input).unwrap();
        let xml = input.join("sms-1.xml");
        fs::write(
            &xml,
            r#"<smses count="1"><sms address="+15555550101" date="1400773261000" type="1" body="hello" /></smses>"#,
        )
        .unwrap();

        for output in [input.clone(), tmp.path().to_path_buf()] {
            let config = jsonl_test_run(
                &[&xml],
                &output,
                SmsBackupRestoreConfig {
                    owner_phones: vec!["+15555550100".into()],
                },
            );
            let before = entries(&output);
            let err = config.run(crate::run).unwrap_err().to_string();
            assert!(
                err.contains("must not be the same as, or contain, the input"),
                "{}: {err}",
                output.display()
            );
            assert!(xml.is_file(), "the backup is left as it was");
            assert_eq!(
                entries(&output),
                before,
                "nothing is written to {}",
                output.display()
            );
        }
    }
}
