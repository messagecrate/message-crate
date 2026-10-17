//! The closed set of values an [`Attachment::missing_reason`] takes.
//!
//! Every writer and reader of the field in Rust uses these rather than
//! writing the string again. The strings are what the conversation file and
//! the server store, so they never change with a rename here; tests compare
//! against the literal strings to catch a change.
//!
//! [`Attachment::missing_reason`]: crate::Attachment::missing_reason

/// The file the conversation file names is not where it should be: the
/// backup did not hold it, or the staged copy is gone.
pub const FILE_MISSING: &str = "file_missing";

/// The file is over the size limit, so its bytes were not kept.
pub const TOO_LARGE: &str = "too_large";

/// The person chose not to copy attachments (the Import form's Skip), so the
/// file is absent by request, not lost.
pub const NOT_COPIED: &str = "not_copied";

/// An Export could not fetch the file from the server, so it is not in the
/// Export's directory.
pub const NOT_FETCHED: &str = "not_fetched";

/// The import found an attachment with no path and no reason: the exporter
/// dropped the path without saying why.
pub const NO_PATH: &str = "no_path";

/// The start of the reason for a file that could not be converted; the
/// detail follows it ([`convert_failed`]).
const CONVERT_FAILED_PREFIX: &str = "convert_failed: ";

/// The reason for a file that could not be converted, with `detail` saying
/// why: `convert_failed: <detail>`.
pub fn convert_failed(detail: &str) -> String {
    format!("{CONVERT_FAILED_PREFIX}{detail}")
}

/// Whether `reason` records a file that could not be converted.
pub fn is_convert_failed(reason: &str) -> bool {
    reason.starts_with(CONVERT_FAILED_PREFIX)
}
