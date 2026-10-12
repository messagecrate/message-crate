//! Full export pipeline; the desktop app calls it in process.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_crate_core::{GoSmsProConfig, OutputConfig, RunContext, RunResult, require_input};
use std::path::PathBuf;

/// Convert, then apply media transforms and obfuscation.
///
/// # Errors
///
/// Returns an error when there is not exactly one input, conversion fails,
/// media processing fails for every candidate file, or the user cancels.
pub fn run(
    context: &RunContext,
    output: &OutputConfig,
    inputs: &[PathBuf],
    source: &GoSmsProConfig,
) -> Result<RunResult> {
    message_crate_core::check_cancel(&context.cancel)?;
    let input = require_input(inputs).map_err(anyhow::Error::msg)?;
    message_crate_core::run_pipeline(context, output, |convert_run| {
        convert_export(ConvertExportArgs {
            input_dir: input,
            output_dir: &output.directory,
            scratch_dir: &context.scratch_dir,
            owner_phones: &source.owner_phones,
            convert_run,
        })
    })
}
