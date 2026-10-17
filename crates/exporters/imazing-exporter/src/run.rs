//! Full export pipeline for the in-process desktop app.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_crate_core::{ImazingConfig, OutputConfig, RunContext, RunResult, require_input};
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
    source: &ImazingConfig,
) -> Result<RunResult> {
    message_crate_core::check_cancel(&context.cancel)?;
    let input = require_input(inputs).map_err(anyhow::Error::msg)?;
    message_crate_core::run_pipeline(context, output, |convert_run| {
        convert_export(ConvertExportArgs {
            input,
            output: &output.directory,
            timezone: source.timezone.as_deref(),
            convert_run,
        })
    })
}
