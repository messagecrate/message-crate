//! Length caps on the names, addresses, Import Error text and Import Run
//! JSON that #2183 bounded. The Contact Group, Message Tag and Saved Search
//! names and the username have older checks of their own (#2665).
//!
//! A name, an address or an Import Error's text is stored, returned on every
//! list that shows it, and written into the Audit Trail and the server log, so
//! each has a stated upper bound rather than the 32 MiB cap on a JSON body
//! (#2183). A value over its cap is `validation-failed`, never cut short, so
//! the caller learns the rule the first time it breaks it, as with `limit` in
//! `paging.rs`. Each cap is also written in its field's OpenAPI description.

use crate::server::ApiError;

/// Most characters in a name a person types: an account's display name and a
/// contact's name.
pub const MAX_PERSON_NAME_CHARS: usize = 200;
/// Most characters in an identity address a person types: an email address's
/// local part (64) and domain (255) with the `@` between them come to 320, and
/// a phone number is far shorter.
pub const MAX_IDENTITY_ADDRESS_CHARS: usize = 320;
/// Most characters in an Import Error's `item` or `reason`.
pub const MAX_IMPORT_ERROR_TEXT_CHARS: usize = 2_000;
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

    fn refusal(result: Result<impl std::fmt::Debug, TooLong>) -> Vec<String> {
        match result.map_err(ApiError::from) {
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
    fn json_at_the_cap_passes_and_one_byte_over_is_refused() {
        assert!(capped_json("form", "[1,2]", 5).is_ok());
        assert_eq!(
            refusal(capped_json("form", "[1,23]", 5)),
            vec!["form: must be at most 5 bytes of JSON".to_string()]
        );
    }
}
