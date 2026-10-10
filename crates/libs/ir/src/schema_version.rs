//! The one check that refuses a file from another schema version.
//!
//! Every reader of a [`ConversationDocument`](crate::ConversationDocument) or
//! its JSON Lines header — the format reader, the import client, the server's
//! import — refuses a version other than [`SCHEMA_VERSION`] with the same
//! words, and refuses it before parsing the rest of the file. The refusal is
//! the one rule for every bump, whichever way the shapes differ. A version-12
//! file would parse under version 13's rules, because version 13 only made
//! the text of an earlier version optional. It is refused all the same: a
//! file is imported by its own version's rules or not at all, and the person
//! should read "schema version 12", not an import that quietly read an older
//! shape.

use crate::SCHEMA_VERSION;
use serde::Deserialize;
use std::fmt;

/// A document or header whose `schema_version` is not [`SCHEMA_VERSION`].
/// Nothing is upgraded: the file is re-exported with current tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedSchemaVersion {
    /// The version the file carries.
    pub found: u32,
}

impl fmt::Display for UnsupportedSchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "This file is schema version {}; Message Crate reads version {SCHEMA_VERSION}",
            self.found
        )
    }
}

impl std::error::Error for UnsupportedSchemaVersion {}

/// Refuse `found` unless it is [`SCHEMA_VERSION`].
///
/// # Errors
///
/// Returns [`UnsupportedSchemaVersion`] for any other version.
pub fn check_schema_version(found: u32) -> Result<(), UnsupportedSchemaVersion> {
    if found == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(UnsupportedSchemaVersion { found })
    }
}

/// Refuse a raw JSON document or header line whose `schema_version` is not
/// [`SCHEMA_VERSION`], reading nothing but that field.
///
/// Text that is not a JSON object, or has no numeric `schema_version`, passes:
/// the full parse that follows reports what is wrong with it.
///
/// # Errors
///
/// Returns [`UnsupportedSchemaVersion`] when the field is present and is
/// another version.
pub fn check_schema_version_in_json(json: &str) -> Result<(), UnsupportedSchemaVersion> {
    #[derive(Deserialize)]
    struct Peek {
        schema_version: u32,
    }
    match serde_json::from_str::<Peek>(json) {
        Ok(peek) => check_schema_version(peek.schema_version),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_version_found_and_the_version_read() {
        assert_eq!(
            check_schema_version(3).unwrap_err().to_string(),
            format!("This file is schema version 3; Message Crate reads version {SCHEMA_VERSION}")
        );
        assert_eq!(check_schema_version(SCHEMA_VERSION), Ok(()));
    }

    /// Version 9 kept a reply's link in the Apple extension's `is_reply` and
    /// `in_reply_to_guid`; version 10 keeps it in the message's own
    /// `reply_to`, for every source. A version-9 file is refused by its
    /// version, never read with its replies taken for plain messages.
    #[test]
    fn refuses_a_version_9_file_by_name() {
        assert_eq!(
            check_schema_version_in_json(r#"{"schema_version":9,"export":{}}"#)
                .unwrap_err()
                .to_string(),
            format!("This file is schema version 9; Message Crate reads version {SCHEMA_VERSION}")
        );
    }

    /// Version 10 had no `export.backup_taken_at_unix_ms`; version 11 says
    /// when the backup was made, and the import lets the later backup decide
    /// a message's mark and text. A version-10 file is refused by its
    /// version, never read as a file whose backup has no date.
    #[test]
    fn refuses_a_version_10_file_by_name() {
        assert_eq!(
            check_schema_version_in_json(r#"{"schema_version":10,"export":{}}"#)
                .unwrap_err()
                .to_string(),
            format!("This file is schema version 10; Message Crate reads version {SCHEMA_VERSION}")
        );
    }

    /// Version 11 had no `time_precision` on a message; version 12 says
    /// whether each time has milliseconds, and the import shows a
    /// whole-second message once when its source also holds it with
    /// milliseconds. A version-11 file is refused by its version, never read
    /// with its messages missing the field.
    #[test]
    fn refuses_a_version_11_file_by_name() {
        assert_eq!(
            check_schema_version_in_json(r#"{"schema_version":11,"export":{}}"#)
                .unwrap_err()
                .to_string(),
            format!("This file is schema version 11; Message Crate reads version {SCHEMA_VERSION}")
        );
    }

    /// Version 12 required the text of every earlier version; version 13
    /// lets a source record an edit without the text it replaced, as iMazing
    /// does. A version-12 file is refused by its version, never read by
    /// rules that are not its own.
    #[test]
    fn refuses_a_version_12_file_by_name() {
        assert_eq!(
            check_schema_version_in_json(r#"{"schema_version":12,"export":{}}"#)
                .unwrap_err()
                .to_string(),
            format!("This file is schema version 12; Message Crate reads version {SCHEMA_VERSION}")
        );
        assert_eq!(SCHEMA_VERSION, 13);
    }

    #[test]
    fn peeks_at_the_version_before_anything_else() {
        assert_eq!(
            check_schema_version_in_json(r#"{"schema_version":3,"export":{}}"#),
            Err(UnsupportedSchemaVersion { found: 3 })
        );
        assert_eq!(
            check_schema_version_in_json(&format!(
                r#"{{"schema_version":{SCHEMA_VERSION},"export":{{}}}}"#
            )),
            Ok(())
        );
        assert_eq!(check_schema_version_in_json(r#"{"export":{}}"#), Ok(()));
        assert_eq!(check_schema_version_in_json("not json"), Ok(()));
    }
}
