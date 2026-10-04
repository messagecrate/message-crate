//! The `message-crate-server` binary: parses the command line and runs it on
//! a Tokio runtime. Everything else lives in the library crate.

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // What `fn main() -> Result<()>` writes for an error.
            eprintln!("Error: {error:?}");
            ExitCode::from(message_crate_server::cli::exit_code(&error))
        }
    }
}

fn run() -> Result<()> {
    message_crate_server::logging::init();
    let cli = message_crate_server::cli::Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(message_crate_server::cli::run(cli))
}
