//! The zone a naive wall-clock time is read in: the machine's, a fixed UTC
//! offset, or an IANA zone by name.

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;

/// How a timestamp with no zone of its own is turned into an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Zone {
    /// The zone of the machine running the export.
    Local,
    /// A fixed offset such as `UTC-05:00`; never has a gap or a repeat.
    Fixed(FixedOffset),
    /// An IANA zone such as `America/New_York`, with its daylight-saving rules.
    Named(Tz),
}

impl Zone {
    /// Parse a zone string: `None` or blank is [`Zone::Local`], `UTC±HH:MM`
    /// (see [`parse_utc_offset`]) is [`Zone::Fixed`], and anything else must
    /// be an IANA zone name.
    ///
    /// # Errors
    ///
    /// Returns an error when the string is neither a UTC offset nor a known
    /// zone name.
    pub(crate) fn parse(raw: Option<&str>) -> Result<Self> {
        let Some(name) = raw.and_then(message_ir::trimmed) else {
            return Ok(Self::Local);
        };
        if let Ok(offset) = parse_utc_offset(name) {
            return Ok(Self::Fixed(offset));
        }
        match name.parse::<Tz>() {
            Ok(tz) => Ok(Self::Named(tz)),
            Err(_) => bail!("unknown time zone {name:?}: expected UTC±HH:MM or an IANA name"),
        }
    }

    /// The instant `naive` names in this zone.
    ///
    /// A wall-clock time that happens twice (the clocks fall back) is the
    /// earlier instant. A time that never happens (the clocks spring forward)
    /// is read with the offset in force just before the change, so 02:30 in a
    /// zone that jumped from 02:00 to 03:00 is the instant 03:30 shows on the
    /// new clock; this is what Java's `ZonedDateTime` and Python's `zoneinfo`
    /// do. `None` only when the arithmetic leaves chrono's range.
    pub(crate) fn instant(self, naive: NaiveDateTime) -> Option<DateTime<Utc>> {
        match self {
            Self::Local => instant_in(&Local, naive),
            Self::Fixed(offset) => instant_in(&offset, naive),
            Self::Named(tz) => instant_in(&tz, naive),
        }
    }
}

fn instant_in<Z: TimeZone>(zone: &Z, naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    if let Some(dt) = zone.from_local_datetime(&naive).earliest() {
        return Some(dt.with_timezone(&Utc));
    }
    // `naive` sits in a gap the clocks jumped over. Walk back an hour at a
    // time to a wall clock that did exist, and read `naive` with the offset
    // in force then. Daylight-saving gaps are an hour or less; the longest
    // gap in the zone database is the day Samoa skipped on 2011-12-30, hence
    // the bound of 26 hours.
    let before = (1..=26)
        .filter_map(|hours| naive.checked_sub_signed(Duration::hours(hours)))
        .find_map(|earlier| zone.from_local_datetime(&earlier).earliest())?;
    let offset = before.offset().fix();
    Some((naive - offset).and_utc())
}

/// Parse a fixed UTC offset string into [`FixedOffset`].
///
/// Accepted forms, in any case:
/// - `UTC` or `Z` → +00:00
/// - `UTC±HH:MM`, such as `UTC-05:00`, `UTC+05:30`, `UTC+05:45`
/// - `UTC±H` or `UTC±HH`, whole hours, such as `UTC-5`
///
/// IANA names are rejected here; [`Zone::parse`] accepts both.
fn parse_utc_offset(raw: &str) -> Result<FixedOffset> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("empty UTC offset");
    }
    let upper = s.to_ascii_uppercase();
    if upper == "UTC" || upper == "Z" {
        return FixedOffset::east_opt(0).ok_or_else(|| anyhow::anyhow!("invalid UTC offset"));
    }
    let rest = upper
        .strip_prefix("UTC")
        .ok_or_else(|| anyhow::anyhow!("expected UTC offset like UTC-05:00, got {raw:?}"))?;
    let (sign, body) = match rest.chars().next() {
        Some('+') => (1i32, &rest[1..]),
        Some('-') => (-1i32, &rest[1..]),
        _ => {
            bail!("expected UTC±HH:MM (e.g. UTC-05:00), got {raw:?}");
        }
    };
    let (hours, minutes) = parse_hh_mm(body)?;
    if hours > 14 || (hours == 14 && minutes > 0) {
        bail!("UTC offset out of range: {raw:?}");
    }
    if minutes >= 60 {
        bail!("invalid minutes in UTC offset: {raw:?}");
    }
    let secs = sign * (hours * 3600 + minutes * 60);
    FixedOffset::east_opt(secs).ok_or_else(|| anyhow::anyhow!("invalid UTC offset: {raw:?}"))
}

/// Hours and minutes from `HH` or `HH:MM`.
fn parse_hh_mm(body: &str) -> Result<(i32, i32)> {
    let (hh, mm) = match body.split_once(':') {
        Some((hh, mm)) => (hh, Some(mm)),
        None => (body, None),
    };
    let hours: i32 = hh
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid hours in UTC offset: {body:?}"))?;
    let minutes: i32 = match mm {
        Some(mm) => mm
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid minutes in UTC offset: {body:?}"))?,
        None => 0,
    };
    Ok((hours, minutes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn naive(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    #[test]
    fn blank_is_local_and_offsets_and_names_parse() {
        assert_eq!(Zone::parse(None).unwrap(), Zone::Local);
        assert_eq!(Zone::parse(Some("  ")).unwrap(), Zone::Local);
        assert_eq!(
            Zone::parse(Some("UTC-05:00")).unwrap(),
            Zone::Fixed(FixedOffset::west_opt(5 * 3600).unwrap())
        );
        assert_eq!(
            Zone::parse(Some("America/New_York")).unwrap(),
            Zone::Named(chrono_tz::America::New_York)
        );
        let err = Zone::parse(Some("Not/AZone")).unwrap_err().to_string();
        assert!(err.contains("Not/AZone"), "{err}");
    }

    #[test]
    fn fixed_offset_is_exact() {
        let zone = Zone::parse(Some("UTC+05:30")).unwrap();
        let at = zone.instant(naive(2020, 1, 1, 12, 0)).unwrap();
        assert_eq!(at.timestamp(), 1_577_860_200);
    }

    #[test]
    fn spring_forward_gap_uses_the_offset_before_the_change() {
        let zone = Zone::parse(Some("America/New_York")).unwrap();
        // 02:30 never showed on a New York clock on 2026-03-08; read as EST.
        let at = zone.instant(naive(2026, 3, 8, 2, 30)).unwrap();
        assert_eq!(at.timestamp(), 1_772_955_000);
        // A half-hour gap: Lord Howe Island moves 02:00 +10:30 to 02:30 +11:00.
        let zone = Zone::parse(Some("Australia/Lord_Howe")).unwrap();
        let at = zone.instant(naive(2025, 10, 5, 2, 15)).unwrap();
        assert_eq!(at.to_rfc3339(), "2025-10-04T15:45:00+00:00");
        // A whole skipped day: Samoa went from 2011-12-29 24:00 (-10:00)
        // straight to 2011-12-31 00:00 (+14:00).
        let zone = Zone::parse(Some("Pacific/Apia")).unwrap();
        let at = zone.instant(naive(2011, 12, 30, 12, 0)).unwrap();
        assert_eq!(at.to_rfc3339(), "2011-12-30T22:00:00+00:00");
    }

    #[test]
    fn fall_back_repeat_is_the_earlier_instant() {
        let zone = Zone::parse(Some("America/New_York")).unwrap();
        let at = zone.instant(naive(2025, 11, 2, 1, 30)).unwrap();
        assert_eq!(at.timestamp(), 1_762_061_400);
    }

    #[test]
    fn rejects_iana() {
        assert!(parse_utc_offset("America/New_York").is_err());
        assert!(parse_utc_offset("Not/AZone").is_err());
    }

    #[test]
    fn parses_every_accepted_shape() {
        for (raw, secs) in [
            ("UTC", 0),
            ("utc", 0),
            ("Z", 0),
            ("z", 0),
            ("UTC-5", -5 * 3600),
            ("UTC+9", 9 * 3600),
            ("UTC+00:00", 0),
            ("UTC-05:00", -5 * 3600),
            ("UTC+05:30", 5 * 3600 + 30 * 60),
            ("UTC+05:45", 5 * 3600 + 45 * 60),
            ("UTC-14:00", -14 * 3600),
            ("UTC+14:00", 14 * 3600),
        ] {
            assert_eq!(
                parse_utc_offset(raw).unwrap().local_minus_utc(),
                secs,
                "{raw}"
            );
        }
    }

    #[test]
    fn refuses_offsets_past_fourteen_hours_and_bad_minutes() {
        for raw in [
            "UTC+14:30",
            "UTC+14:01",
            "UTC+15",
            "UTC-15:00",
            "UTC+05:60",
            "UTC+5:5:5",
            "UTC5",
            "",
        ] {
            assert!(parse_utc_offset(raw).is_err(), "{raw}");
        }
    }
}
