//! The `message-crate-server` binary: parses the command line and runs it on
//! a Tokio runtime. Everything else lives in the library crate.

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            message_crate_server::logging::write_failure(&error);
            ExitCode::from(message_crate_server::cli::exit_code(&error))
        }
    }
}

fn run() -> Result<()> {
    message_crate_server::logging::init();
    let cli = message_crate_server::cli::Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(message_crate_server::cli::run(cli));
    // Without waiting for the blocking pool: once the command has returned,
    // a task still on it is a request a stopping `serve` gave up on after
    // its drain limit, such as an import batch, and dropping the runtime
    // would wait for it however long it takes (#2122). SQLite rolls back
    // what it had not committed.
    rt.shutdown_background();
    result
}
