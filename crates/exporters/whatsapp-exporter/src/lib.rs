//! Convert WhatsApp chats (via KnugiHK wtsexporter JSON) into the shared
//! conversation structure ([`ir::ConversationDocument`]) every exporter writes.
//!
//! Library entry: [`run()`] for the full pipeline; the desktop app calls it
//! in process.

mod emit;
mod ios_backup;
mod jid;
mod owner;
mod read;
mod run;
mod wtsexporter;

pub use run::run;
pub use wtsexporter::{
    WTSEXPORTER_RELEASE, WTSEXPORTER_REPO, WTSEXPORTER_TROUBLESHOOTING, WtsexporterFile,
    not_pinned_reason, pinned_wtsexporter, wtsexporter_file_name, wtsexporter_in_dir,
};

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;
