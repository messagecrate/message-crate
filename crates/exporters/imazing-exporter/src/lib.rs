//! Convert iMazing Messages / WhatsApp CSV
//! into the shared conversation structure ([`ir::ConversationDocument`])
//! every exporter writes.
//!
//! Library entry: [`run()`] for the full pipeline; the desktop app calls it
//! in process.

mod attachments;
mod attachments_emit;
mod chat_directory;
mod emit;
mod read;
mod row_marks;
mod run;
mod unnamed_files;
mod zone;

pub use run::run;

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;

#[cfg(test)]
#[path = "../tests/run_issues.rs"]
mod run_issues;
