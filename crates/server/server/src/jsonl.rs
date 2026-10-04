//! Read message-ir JSONL files (one JSON object per line) into import records.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::imports_api::ImportFailure;
use crate::models::{self, ExportRecord};

/// Why [`read_records`] could not read a file: a line the sender can fix, or
/// a file the server cannot read.
#[derive(Debug, thiserror::Error)]
pub enum ReadRecordsError {
    /// A line breaks a rule of message-ir, or is not text at all.
    #[error("failed to parse message-ir JSONL in {}: {failure}", path.display())]
    Rejected {
        path: PathBuf,
        failure: ImportFailure,
    },
    /// The file cannot be opened or read: nothing the sender can change.
    #[error("failed to read {}", path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Read a message-ir JSON Lines conversation file (one JSON object per line)
/// into import records.
///
/// Parses line-by-line so the full file is not held as a second string buffer
/// before deserialization (records still accumulate in memory).
///
/// # Errors
///
/// Returns [`ReadRecordsError::Rejected`] when a line is not valid
/// message-ir JSON, and [`ReadRecordsError::Unreadable`] when the file cannot
/// be opened or a line cannot be read.
pub fn read_records(path: &Path) -> Result<Vec<ExportRecord>, ReadRecordsError> {
    let unreadable = |source| ReadRecordsError::Unreadable {
        path: path.to_path_buf(),
        source,
    };
    let rejected = |failure| ReadRecordsError::Rejected {
        path: path.to_path_buf(),
        failure,
    };
    let file = File::open(path).map_err(unreadable)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    for (line_no, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            // Bytes that are not UTF-8 are not text, so not JSON: the
            // sender's to fix, like any other line that cannot be read.
            Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                return Err(rejected(ImportFailure::NotJson {
                    line: line_no + 1,
                    detail: "the line is not valid UTF-8".into(),
                }));
            }
            Err(err) => return Err(unreadable(err)),
        };
        // A blank line is kept: `parse_ir_lines` skips it but still counts
        // it, so a failure names the line as it is numbered in the file.
        lines.push(line);
    }
    models::parse_ir_lines(lines).map_err(rejected)
}

#[cfg(test)]
mod tests {
    use super::{ReadRecordsError, read_records};
    use crate::imports_api::ImportFailure;

    const HEADER: &str = r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"t","tool_version":"1","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550101","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550101","display_name":"Sam"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1400773261000,"last_timestamp_unix_ms":1400773261000}}}"#;

    fn message(guid: &str) -> String {
        format!(
            r#"{{"guid":"{guid}","timestamp_unix_ms":1400773261000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}}"#
        )
    }

    /// Blank lines are lines of the file, so a failure below them names the
    /// line a person finds when they open the file, not the count of the
    /// lines that hold JSON.
    #[test]
    fn a_bad_line_below_blank_lines_is_named_by_its_line_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversation.jsonl");
        // Line 1 is the header, lines 2 and 3 are blank, lines 4 to 9 are
        // messages, and line 10 is not JSON.
        let mut lines = vec![HEADER.to_string(), String::new(), "   ".to_string()];
        lines.extend((1..=6).map(|i| message(&format!("g{i}"))));
        lines.push("this is not json".to_string());
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        match read_records(&path).unwrap_err() {
            ReadRecordsError::Rejected {
                failure: ImportFailure::NotJson { line, .. },
                ..
            } => assert_eq!(line, 10),
            other => panic!("expected NotJson, got {other:?}"),
        }
    }

    /// A file the server cannot open is its own fault, not the sender's: the
    /// error says the file is unreadable and carries no line to fix.
    #[test]
    fn a_file_that_cannot_be_opened_is_unreadable_not_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone.jsonl");

        match read_records(&path).unwrap_err() {
            ReadRecordsError::Unreadable {
                path: named,
                source,
            } => {
                assert_eq!(named, path);
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }
}
