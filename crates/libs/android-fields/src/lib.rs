//! Fields of Android's message store, read one way by the readers of the
//! backups made on Android: SMS Backup & Restore, GO SMS Pro, and SMS
//! Backup+.
//!
//! SMS Backup & Restore, GO SMS Pro and SMS Backup+ copy a message's `date`
//! from Android's message store, so all three read it with
//! [`unix_secs_from_date_ms`], and a change to what counts as a readable date
//! is made once. Android stores an MMS's `date` in seconds, and SMS Backup+
//! copies it as it is, so SMS Backup+ reads an MMS's date with
//! [`unix_secs_from_date_secs`], which holds it to the same rule.
//!
//! GO SMS Pro and SMS Backup+ build a message's [`VendorFields`] with
//! [`android_source`], and SMS Backup & Restore and SMS Backup+ read an
//! attachment's name with [`valid_filename`].

use message_ir::{PendingConversation, PendingMessage, VendorFields};
use serde_json::{Map, Value};

/// The last millisecond of the year 9999, the latest `date` read as real.
pub const MAX_DATE_MS: i64 = 253_402_300_799_999;

/// Unix seconds from an Android backup's millisecond `date`, or `None` for
/// anything but a whole number of milliseconds from 0 to [`MAX_DATE_MS`], so
/// `NaN`, `inf`, and out-of-range values never become a timestamp. The text
/// is read as it is: a caller that trims its field trims it first.
///
/// Only ASCII digits are read, with no leading zero except in `0` itself.
/// Android never writes a `+` or a leading zero, so a date with either
/// comes from an edited or damaged file and counts as unreadable.
#[must_use]
pub fn unix_secs_from_date_ms(date_ms: &str) -> Option<f64> {
    let millis = whole_number(date_ms).filter(|ms| *ms <= MAX_DATE_MS)?;
    // Exact: every value up to MAX_DATE_MS fits in an f64 mantissa.
    Some(millis as f64 / 1000.0)
}

/// Unix seconds from a `date` Android stores in seconds, as it does an
/// MMS's, read by the rule of [`unix_secs_from_date_ms`]: a whole number,
/// written as Android writes it, whose milliseconds are at most
/// [`MAX_DATE_MS`].
#[must_use]
pub fn unix_secs_from_date_secs(date_secs: &str) -> Option<f64> {
    let secs = whole_number(date_secs)
        .filter(|secs| secs.checked_mul(1000).is_some_and(|ms| ms <= MAX_DATE_MS))?;
    // Exact: every value up to MAX_DATE_MS / 1000 fits in an f64 mantissa.
    Some(secs as f64)
}

/// A whole number from 0 up written as Android writes one: ASCII digits
/// only, with no sign and no leading zero except in `0` itself.
fn whole_number(text: &str) -> Option<i64> {
    let digits_only = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let leading_zero = text.len() > 1 && text.starts_with('0');
    if !digits_only || leading_zero {
        return None;
    }
    text.parse::<i64>().ok()
}

/// `value` trimmed, unless it is blank or the literal `null` / `none` that
/// some backups write where an attachment name is missing.
#[must_use]
pub fn valid_filename(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && !value.eq_ignore_ascii_case("null")
        && !value.eq_ignore_ascii_case("none"))
    .then(|| value.to_string())
}

/// Android's message `type` (1 received, 2 sent, and so on) as a number, or
/// `None` when the field is blank or not a whole number. `message-ir-format`'s
/// CSV reader reads its `android_type` column the same way; keep the two in
/// step.
fn parse_android_type(s: &str) -> Option<i32> {
    s.trim().parse().ok()
}

/// The [`VendorFields`] of one message from an Android backup: `fields`
/// with the conversation title added as `android_group_title`, and the
/// message's `android_type` extra read with `parse_android_type`. The
/// title is stored as data only; filenames do not use it.
pub fn android_source(
    conversation: &PendingConversation,
    msg: &PendingMessage,
    mut fields: Map<String, Value>,
) -> VendorFields {
    if let Some(title) = conversation
        .display_name
        .as_deref()
        .filter(|t| !t.is_empty())
    {
        fields.insert(
            "android_group_title".into(),
            Value::String(title.to_string()),
        );
    }
    VendorFields {
        android_type: parse_android_type(msg.extra_str("android_type")),
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_DATE_MS, unix_secs_from_date_ms, unix_secs_from_date_secs, valid_filename};

    #[test]
    fn dates_read_as_whole_milliseconds_in_range() {
        for (date, secs) in [
            ("0", Some(0.0)),
            ("1", Some(0.001)),
            ("1400773261000", Some(1_400_773_261.0)),
            ("1400773261123", Some(1_400_773_261_123.0 / 1000.0)),
            ("253402300799999", Some(253_402_300_799_999.0 / 1000.0)),
            ("253402300800000", None),
            ("NaN", None),
            ("nan", None),
            ("inf", None),
            ("-inf", None),
            ("infinity", None),
            ("1e400", None),
            ("1.4e12", None),
            ("1400773261000.5", None),
            ("", None),
            (" 1400773261000", None),
            ("-1", None),
            ("+1400773261000", None),
            ("+0", None),
            ("0001400773261000", None),
            ("01", None),
            ("00", None),
            ("abc", None),
            ("99999999999999999999999", None),
            ("9223372036854775807", None),
        ] {
            assert_eq!(unix_secs_from_date_ms(date), secs, "date {date:?}");
        }
    }

    #[test]
    fn second_dates_read_by_the_millisecond_rule() {
        let last_second = MAX_DATE_MS / 1000;
        for (date, secs) in [
            ("0".to_string(), Some(0.0)),
            ("1400773261".to_string(), Some(1_400_773_261.0)),
            (last_second.to_string(), Some(last_second as f64)),
            ((last_second + 1).to_string(), None),
            // A millisecond date of 1978 or later, read as seconds, is past
            // the bound.
            ("1400773261000".to_string(), None),
            ("9223372036854775807".to_string(), None),
            ("99999999999999999999999".to_string(), None),
            ("1400773261.5".to_string(), None),
            ("01400773261".to_string(), None),
            ("+1400773261".to_string(), None),
            ("-1".to_string(), None),
            (" 1400773261".to_string(), None),
            (String::new(), None),
            ("NaN".to_string(), None),
        ] {
            assert_eq!(unix_secs_from_date_secs(&date), secs, "date {date:?}");
        }
    }

    #[test]
    fn a_blank_name_or_the_word_null_or_none_is_no_file_name() {
        for missing in ["", "   ", "null", "NULL", "none"] {
            assert_eq!(valid_filename(missing), None, "{missing:?}");
        }
    }

    #[test]
    fn a_real_name_comes_back_trimmed() {
        assert_eq!(valid_filename(" a.jpg ").as_deref(), Some("a.jpg"));
    }
}
