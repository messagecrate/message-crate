//! Fields of Android's message store, read one way by every Android backup
//! reader.
//!
//! SMS Backup & Restore and GO SMS Pro both copy a message's `date` from
//! Android's message store, so both read it with [`unix_secs_from_date_ms`],
//! and a change to what counts as a readable date is made once.

/// The last millisecond of the year 9999, the latest `date` read as real.
pub const MAX_DATE_MS: i64 = 253_402_300_799_999;

/// Unix seconds from an Android backup's millisecond `date`, or `None` for
/// anything but a whole number of milliseconds from 0 to [`MAX_DATE_MS`], so
/// `NaN`, `inf`, and out-of-range values never become a timestamp. The text
/// is read as it is: a caller that trims its field trims it first.
#[must_use]
pub fn unix_secs_from_date_ms(date_ms: &str) -> Option<f64> {
    let millis = date_ms
        .parse::<i64>()
        .ok()
        .filter(|ms| (0..=MAX_DATE_MS).contains(ms))?;
    // Exact: every value up to MAX_DATE_MS fits in an f64 mantissa.
    Some(millis as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::unix_secs_from_date_ms;

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
            ("abc", None),
            ("99999999999999999999999", None),
            ("9223372036854775807", None),
        ] {
            assert_eq!(unix_secs_from_date_ms(date), secs, "date {date:?}");
        }
    }
}
