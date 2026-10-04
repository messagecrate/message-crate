//! What can stop an import, by kind.
//!
//! [`ImportFailure`] is a reason the person who sent the file can act on.
//! [`ImportError`] is what the import pipeline returns: one of those, or an
//! internal failure (I/O or the database) the sender cannot fix by changing
//! the file. The HTTP interface maps each kind to a status once, in
//! `server.rs`, and no handler picks a status from an `anyhow` error.
//! Nothing here looks inside an `anyhow` error for a kind: each stage's
//! error type keeps the two apart from the line that raises them.

use std::fmt;
use std::path::{Path, PathBuf};

use message_ir::{UnsafeAttachmentPath, UnsupportedSchemaVersion};

use super::promote::PromoteError;
use super::staging::StagingError;
use crate::assets_api::Sha256;

/// A reason an import stopped that the sender can fix by changing the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportFailure {
    /// A line is not JSON at all. Only this kind is a request that cannot be
    /// read.
    NotJson { line: usize, detail: String },
    /// The conversation header's `schema_version` is not the one this server
    /// reads. Nothing is upgraded: the sender re-exports with current tools.
    SchemaVersion {
        refusal: UnsupportedSchemaVersion,
        line: usize,
    },
    /// A line is JSON and breaks a rule of message-ir: a header or message
    /// with the wrong fields, a message before any header, or a file with no
    /// header.
    Invalid { line: usize, detail: String },
    /// The batch has no bytes.
    Empty,
    /// An attachment path on the message at `line` could leave the folder it
    /// is read from.
    UnsafeAttachmentPath {
        refusal: UnsafeAttachmentPath,
        line: usize,
    },
    /// The message at `line` states a SHA-256 for an attachment that is not
    /// 64 hex digits.
    AttachmentSha256Invalid {
        path: String,
        stated: String,
        line: usize,
    },
    /// An attachment's bytes do not hash to the SHA-256 the message at `line`
    /// states for it.
    AttachmentMismatch {
        path: String,
        stated: Sha256,
        actual: Sha256,
        line: usize,
    },
    /// Messages whose `guid` is empty. The guid index is what makes a
    /// retried batch store nothing twice, and every exporter writes a guid,
    /// so a message without one is refused rather than stored outside it.
    /// `lines` holds the first [`MISSING_GUID_LINES_NAMED`] such lines, in
    /// order, and `total` counts them all.
    MissingGuid { lines: Vec<usize>, total: usize },
}

/// How many lines without a guid a refusal names; the rest are counted.
pub const MISSING_GUID_LINES_NAMED: usize = 10;

/// The server's `import` command reads files, so the line is a line of the file.
impl fmt::Display for ImportFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sentence("the file"))
    }
}

impl std::error::Error for ImportFailure {}

impl ImportFailure {
    /// The line the failure is on, counted from 1 with blank lines included,
    /// when it is about a line. For messages without a guid, the first of
    /// them.
    #[must_use]
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::NotJson { line, .. }
            | Self::SchemaVersion { line, .. }
            | Self::Invalid { line, .. }
            | Self::UnsafeAttachmentPath { line, .. }
            | Self::AttachmentSha256Invalid { line, .. }
            | Self::AttachmentMismatch { line, .. } => Some(*line),
            Self::MissingGuid { lines, .. } => lines.first().copied(),
            Self::Empty => None,
        }
    }

    /// The sentence for a failure in one batch of an Import Run.
    ///
    /// A batch is the body of one request, which a client such as Upload
    /// packs from parts of one or more staged files, so its line is a line of
    /// the batch and not of any file the sender has. The client turns it into
    /// a file and line of its own.
    #[must_use]
    pub fn batch_sentence(&self) -> String {
        self.sentence("the batch")
    }

    /// The sentence, naming the line as a line of `whole`.
    fn sentence(&self, whole: &str) -> String {
        match self {
            Self::NotJson { line, detail } => {
                format!("Could not read line {line} of {whole}: {detail}.")
            }
            Self::SchemaVersion { refusal, line } => {
                format!("{refusal} (line {line} of {whole}).")
            }
            Self::Invalid { line, detail } => format!("Line {line} of {whole}: {detail}."),
            Self::Empty => {
                "The batch is empty: send at least one conversation header and its messages."
                    .to_string()
            }
            Self::UnsafeAttachmentPath { refusal, line } => {
                format!("Line {line} of {whole}: {refusal}.")
            }
            Self::AttachmentSha256Invalid { path, stated, line } => format!(
                "Line {line} of {whole}: the attachment {path} states the SHA-256 {stated}, which is not 64 hex digits."
            ),
            Self::AttachmentMismatch {
                path,
                stated,
                actual,
                line,
            } => format!(
                "Line {line} of {whole}: the bytes of the attachment {path} hash to {actual}, not to the SHA-256 {stated} the line states."
            ),
            Self::MissingGuid { lines, total } => {
                let named: Vec<String> = lines.iter().map(ToString::to_string).collect();
                let rest = total.saturating_sub(lines.len());
                let which = match (named.as_slice(), rest) {
                    ([one], 0) => format!("The message on line {one} of {whole} has"),
                    ([init @ .., last], 0) => format!(
                        "The messages on lines {} and {last} of {whole} have",
                        init.join(", ")
                    ),
                    (named, rest) => format!(
                        "The messages on lines {} and {rest} more of {whole} have",
                        named.join(", ")
                    ),
                };
                format!("{which} no guid; every message needs one.")
            }
        }
    }
}

/// What the import pipeline returns when it stops.
///
/// Each stage returns its own error type, and each one says whether the
/// sender can fix what stopped it: [`crate::jsonl::ReadRecordsError`],
/// [`super::staging::StagingError`] and [`super::promote::PromoteError`],
/// while `models::parse_ir_lines` refuses with an [`ImportFailure`] alone.
/// The conversions below sort them into these kinds by variant, so a stage
/// that adds a refusal changes a type, and the compiler names every place
/// that has to decide what it is.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The sender can fix it by changing the file. `failure` is what the
    /// sender is told; `file` is the file it was in, for a command line or a
    /// log.
    #[error("{}: {failure}", file.display())]
    Rejected {
        failure: ImportFailure,
        file: PathBuf,
    },
    /// The import run was discarded or completed while the batch uploaded,
    /// found when the run is checked again under the write lock. It is
    /// refused as the check before the body refuses it.
    #[error(transparent)]
    Run(#[from] crate::db::imports::ImportLookupError),
    /// I/O, the database, or a bug: nothing the sender can change.
    #[error(transparent)]
    Internal(anyhow::Error),
}

impl From<PromoteError> for ImportError {
    fn from(err: PromoteError) -> Self {
        match err {
            PromoteError::Internal(err) => Self::Internal(err),
        }
    }
}

impl ImportError {
    /// What staging `file` stopped on, sorted by its kind, with the file a
    /// refusal was in.
    pub(super) fn staging(file: &Path, err: StagingError) -> Self {
        match err {
            StagingError::Rejected(failure) => Self::Rejected {
                failure,
                file: file.to_path_buf(),
            },
            StagingError::Internal(err) => Self::Internal(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ImportError, ImportFailure, UnsupportedSchemaVersion};

    #[test]
    fn schema_version_names_both_versions_and_the_line() {
        let f = ImportFailure::SchemaVersion {
            refusal: UnsupportedSchemaVersion { found: 3 },
            line: 1,
        };
        assert_eq!(
            f.to_string(),
            "This file is schema version 3; Message Crate reads version 6 (line 1 of the file)."
        );
    }

    #[test]
    fn a_batch_failure_names_the_line_of_the_batch() {
        let f = ImportFailure::NotJson {
            line: 3,
            detail: "boom".into(),
        };
        assert_eq!(f.line(), Some(3));
        assert_eq!(
            f.batch_sentence(),
            "Could not read line 3 of the batch: boom."
        );
    }

    #[test]
    fn not_json_names_the_line_and_the_detail() {
        let f = ImportFailure::NotJson {
            line: 12,
            detail: "expected value at line 1 column 1".into(),
        };
        assert_eq!(
            f.to_string(),
            "Could not read line 12 of the file: expected value at line 1 column 1."
        );
    }

    #[test]
    fn missing_guid_names_one_line() {
        let f = ImportFailure::MissingGuid {
            lines: vec![3],
            total: 1,
        };
        assert_eq!(f.line(), Some(3));
        assert_eq!(
            f.to_string(),
            "The message on line 3 of the file has no guid; every message needs one."
        );
    }

    #[test]
    fn missing_guid_names_every_line_of_the_batch_up_to_the_limit() {
        let f = ImportFailure::MissingGuid {
            lines: vec![2, 5, 9],
            total: 3,
        };
        assert_eq!(f.line(), Some(2));
        assert_eq!(
            f.batch_sentence(),
            "The messages on lines 2, 5 and 9 of the batch have no guid; every message needs one."
        );
    }

    #[test]
    fn missing_guid_counts_the_lines_past_the_limit() {
        let f = ImportFailure::MissingGuid {
            lines: vec![2, 3],
            total: 42,
        };
        assert_eq!(
            f.to_string(),
            "The messages on lines 2, 3 and 40 more of the file have no guid; every message needs one."
        );
    }

    /// The `import` command and `reset-demo` print an error as `anyhow`
    /// does: a refusal names the file it was in before the sentence.
    #[test]
    fn a_command_line_prints_a_rejection_with_its_file() {
        let err = anyhow::Error::from(ImportError::Rejected {
            failure: ImportFailure::Invalid {
                line: 2,
                detail: "boom".into(),
            },
            file: "export/+15555550101.jsonl".into(),
        });
        assert_eq!(
            format!("{err:#}"),
            "export/+15555550101.jsonl: Line 2 of the file: boom."
        );
    }
}
