//! Write and read [`message_ir::ConversationDocument`] in each format Message
//! Crate emits itself: JSON, JSON Lines (one JSON object per line), CSV, EML
//! and MBOX. [`FormatSink`] buffers documents, applies obfuscation and the
//! media mode, and writes them; a merged archive owned by another crate
//! plugs in through [`MergedArchive`].
//!
//! The resumable write path lives in `message-staging`, directory convert in
//! `message-reexport`, the run model in `message-crate-core`, and the
//! schema types in `message-ir`.

mod clean;
mod export_transforms;
mod format_sink;
mod normalize;
mod placeholders;
mod read_csv;
mod read_json;
mod read_mail;
mod util;
mod write;

pub use clean::{EXPORT_SENTINEL, clean_previous_ir_output, mark_export_directory};
pub use export_transforms::clear_attachments_when_disabled;
pub use format_sink::{FormatSink, MergedArchive, write_documents_through_sink};
pub use read_csv::read_conversation_csv;
pub use read_json::{
    read_conversation_json, read_conversation_jsonl, read_conversation_jsonl_with_lines,
};
pub use read_mail::{read_conversation_eml_dir, read_conversation_mbox};
pub use util::{is_complete_file, load_attachment_bytes};
pub use write::{
    CSV_HEADERS, document_to_mail_messages, write_conversation_jsonl, write_conversation_jsonl_to,
    write_format,
};

#[cfg(test)]
use normalize::normalize_document_for_compare;
#[cfg(test)]
use write::write_conversation_csv;

/// A temporary directory an export marked with the sentinel, for a test
/// that writes into one.
#[cfg(test)]
pub(crate) fn export_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(EXPORT_SENTINEL), "").expect("sentinel");
    dir
}

/// Run `f` with the permissions of `dir` set to `mode`, then set them back to
/// `0o755` before returning, so the temporary directory can still be
/// removed.
///
/// `None`, with a line on stderr, when a file can still be written into
/// `dir` under `mode`: a user such as root gets past the permissions, cannot
/// exercise the failure, and the test has nothing to check.
#[cfg(all(test, unix))]
pub(crate) fn with_directory_mode<T>(
    dir: &std::path::Path,
    mode: u32,
    f: impl FnOnce() -> T,
) -> Option<T> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
        .expect("set the directory's mode");
    let probe = dir.join("probe");
    let result = if std::fs::write(&probe, b"").is_ok() {
        let _ = std::fs::remove_file(&probe);
        eprintln!(
            "skipped: {} can still be written with mode {mode:o}",
            dir.display()
        );
        None
    } else {
        Some(f())
    };
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))
        .expect("restore the directory's mode");
    result
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
