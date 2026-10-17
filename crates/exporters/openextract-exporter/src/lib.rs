//! Convert OpenExtract conversation CSV into the shared conversation
//! structure ([`ir::ConversationDocument`]) every exporter writes.
//!
//! Library entry: [`run()`] for the full pipeline; the desktop app calls it
//! in process.

mod emit;
mod read;
mod run;

pub use run::run;

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;
