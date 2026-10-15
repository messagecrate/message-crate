//! Every length cap on text the server stores: the names a person types (an
//! account's display name and username, a contact's name, and the name of a
//! Contact Group, Message Tag or Saved Search), API Token labels, identity
//! addresses, Import Error and note text, the program, install and directory
//! an Import Run names, and Import Run JSON.
//!
//! A name, an address or an Import Error's text is stored, returned on every
//! list that shows it, and written into the Audit Trail and the server log, so
//! each has a stated upper bound rather than the 32 MiB cap on a JSON body
//! (#2183). A value over its cap is `validation-failed`, never cut short, so
//! the caller learns the rule the first time it breaks it, as with `limit` in
//! `paging.rs`. Every cap is refused in one wording, `"<field>: must be at
//! most N characters"`, by the checks below, and each is also written in its
//! field's OpenAPI description. A field's other rules, such as which names
//! are reserved, stay with its route. A route whose field may not be blank
//! checks it with [`required_capped_text`], so that refusal has one wording
//! too, `"<field> required"`.

use crate::api_error::ApiError;

/// Most characters in a name a person types: an account's display name and a
/// contact's name.
pub const MAX_PERSON_NAME_CHARS: usize = 200;
/// Most characters in a username. A username is ASCII, so this is its
/// length in bytes too.
pub const MAX_USERNAME_CHARS: usize = 128;
/// Most characters in the name of a Contact Group, a Message Tag or a Saved
/// Search: a name that fits the sidebar.
pub const MAX_COLLECTION_NAME_CHARS: usize = 80;
/// Most characters in an API token's label.
pub const MAX_API_TOKEN_LABEL_CHARS: usize = 120;
/// Most characters in an identity address a person types: an email address's
/// local part (64) and domain (255) with the `@` between them come to 320, and
/// a phone number is far shorter.
pub const MAX_IDENTITY_ADDRESS_CHARS: usize = 320;
/// Most characters in an Import Error's `item` or `reason`.
pub const MAX_IMPORT_ERROR_TEXT_CHARS: usize = 2_000;
/// Most characters in an Import Run note's `item` or `text`, the same room
/// an Import Error's text has: a note names the same kind of item and says
/// what happened to it in one sentence.
pub const MAX_IMPORT_NOTE_TEXT_CHARS: usize = 2_000;
/// Most characters in the name of the program that runs an import. The
/// desktop app sends `message-crate`.
pub const MAX_IMPORT_TOOL_CHARS: usize = 64;
/// Most characters in the id of the copy of the app that creates an Import
/// Run. The
/// desktop app sends a UUID, 36 characters.
pub const MAX_DEVICE_ID_CHARS: usize = 64;
/// Most characters in a path a client stores, such as an Import Run's
/// directory: Linux's `PATH_MAX`, 4,096 bytes. A path Linux accepts has no
/// more characters than bytes, so every one fits. macOS takes 1,024 bytes.
/// Windows takes 260 characters unless long paths are turned on.
pub const MAX_PATH_CHARS: usize = 4_096;
/// Most bytes in each JSON value an Import Run stores as it is created
/// (`form`, `source_fingerprint`, `source_identities`), measured as the
/// server writes it. The desktop app sends well under 4 KiB in each.
pub const MAX_IMPORT_RUN_JSON_BYTES: usize = 64 * 1024;

/// A value over its cap: the sentence names the field and the cap.
#[derive(Debug, thiserror::Error)]
#[error("{field}: must be at most {max} {unit}")]
pub struct TooLong {
    field: String,
    max: usize,
    unit: &'static str,
}

impl From<TooLong> for ApiError {
    fn from(error: TooLong) -> Self {
        Self::validation(error.to_string())
    }
}

/// A value that may not be blank and is, or is over its cap.
#[derive(Debug, thiserror::Error)]
pub enum RequiredTextError {
    /// The trimmed value is empty. Holds the field's name.
    #[error("{0} required")]
    Blank(String),
    /// The trimmed value is over its cap.
    #[error(transparent)]
    TooLong(#[from] TooLong),
}

impl From<RequiredTextError> for ApiError {
    fn from(error: RequiredTextError) -> Self {
        Self::validation(error.to_string())
    }
}

/// The sentence of each [`TooLong`] among `results`, in order, for a route
/// that checks several fields and names every one over its cap in one
/// `validation-failed`.
pub fn too_long_sentences<'a, T: 'a>(
    results: impl IntoIterator<Item = &'a Result<T, TooLong>>,
) -> impl Iterator<Item = String> {
    results
        .into_iter()
        .filter_map(|result| result.as_ref().err().map(ToString::to_string))
}

/// `value` trimmed, or [`TooLong`] naming `field` when the trimmed value is
/// over `max_chars` characters. A blank value passes: whether a field may be
/// blank is its route's rule, not this one's.
///
/// # Errors
///
/// [`TooLong`] when the trimmed value is longer than `max_chars`.
pub fn capped_text<'a>(field: &str, value: &'a str, max_chars: usize) -> Result<&'a str, TooLong> {
    let trimmed = value.trim();
    if trimmed.chars().count() > max_chars {
        return Err(TooLong {
            field: field.to_string(),
            max: max_chars,
            unit: "characters",
        });
    }
    Ok(trimmed)
}

/// `value` trimmed, for a field that may not be blank: [`capped_text`],
/// then a refusal naming `field` when nothing is left after the trim.
///
/// # Errors
///
/// [`RequiredTextError::TooLong`] when the trimmed value is longer than
/// `max_chars`, and [`RequiredTextError::Blank`] when it is empty.
pub fn required_capped_text<'a>(
    field: &str,
    value: &'a str,
    max_chars: usize,
) -> Result<&'a str, RequiredTextError> {
    let trimmed = capped_text(field, value, max_chars)?;
    if trimmed.is_empty() {
        return Err(RequiredTextError::Blank(field.to_string()));
    }
    Ok(trimmed)
}

/// `path` as given, or [`TooLong`] naming `field` when it is over
/// `max_chars` characters. Unlike [`capped_text`] it is not trimmed: a space
/// at either end of a path is part of a file's name, and a path stored
/// without it would name another file.
///
/// # Errors
///
/// [`TooLong`] when `path` is longer than `max_chars`.
pub fn capped_path<'a>(field: &str, path: &'a str, max_chars: usize) -> Result<&'a str, TooLong> {
    if path.chars().count() > max_chars {
        return Err(TooLong {
            field: field.to_string(),
            max: max_chars,
            unit: "characters",
        });
    }
    Ok(path)
}

/// [`TooLong`] naming `field` when the JSON the server would store for it is
/// over `max_bytes` bytes.
///
/// # Errors
///
/// [`TooLong`] when `json` is longer than `max_bytes`.
pub fn capped_json(field: &str, json: &str, max_bytes: usize) -> Result<(), TooLong> {
    if json.len() > max_bytes {
        return Err(TooLong {
            field: field.to_string(),
            max: max_bytes,
            unit: "bytes of JSON",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal<E: Into<ApiError>>(result: Result<impl std::fmt::Debug, E>) -> Vec<String> {
        match result.map_err(Into::into) {
            Err(ApiError::ValidationFailed(errors)) => errors,
            other => panic!("expected validation-failed, got {other:?}"),
        }
    }

    /// The cap counts characters, not bytes, so a name in a script that
    /// takes several bytes a letter gets the same room as one in ASCII.
    #[test]
    fn text_at_the_cap_passes_trimmed_and_one_over_is_refused() {
        let at_cap = "é".repeat(5);
        assert_eq!(
            capped_text("name", &format!("  {at_cap} "), 5).unwrap(),
            at_cap
        );
        assert_eq!(
            refusal(capped_text("name", &"é".repeat(6), 5)),
            vec!["name: must be at most 5 characters".to_string()]
        );
    }

    #[test]
    fn a_required_value_is_refused_blank_and_over_its_cap() {
        assert_eq!(required_capped_text("name", " ab ", 2).unwrap(), "ab");
        assert_eq!(
            refusal(required_capped_text("name", "   ", 2)),
            vec!["name required".to_string()]
        );
        assert_eq!(
            refusal(required_capped_text("name", "abc", 2)),
            vec!["name: must be at most 2 characters".to_string()]
        );
    }

    /// A path keeps the spaces at its ends, which are part of its name, and
    /// counts characters as a name does.
    #[test]
    fn a_path_at_the_cap_passes_as_given_and_one_over_is_refused() {
        assert_eq!(capped_path("run_dir", " /é/ ", 5).unwrap(), " /é/ ");
        assert_eq!(
            refusal(capped_path("run_dir", "/é/é/", 4)),
            vec!["run_dir: must be at most 4 characters".to_string()]
        );
    }

    #[test]
    fn json_at_the_cap_passes_and_one_byte_over_is_refused() {
        assert!(capped_json("form", "[1,2]", 5).is_ok());
        assert_eq!(
            refusal(capped_json("form", "[1,23]", 5)),
            vec!["form: must be at most 5 bytes of JSON".to_string()]
        );
    }
}
