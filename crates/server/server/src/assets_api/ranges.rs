//! Byte ranges (RFC 9110, section 14) on the two routes that answer an
//! asset's bytes, so a media element streams a video and seeks in it rather
//! than loading the whole file first (`docs/architecture/media.md`).
//!
//! One range is served. A `Range` the server does not serve as one range
//! (another unit, several ranges, a range that cannot be read) is ignored and
//! the whole file answered, which RFC 9110 allows; only a range that selects
//! no byte of the file is refused, with `416 Range Not Satisfiable`.

use axum::http::{HeaderMap, header};

/// What a request's `Range` asks of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selection {
    /// The whole file: no `Range`, or one the server does not serve.
    Whole,
    /// Bytes `start` to `end` of the file, both included.
    Part {
        /// The first byte.
        start: u64,
        /// The last byte.
        end: u64,
    },
    /// A range that selects no byte of the file.
    Unsatisfiable,
}

/// What the request's `Range` and `If-Range` select of a file `length` bytes
/// long whose entity tag is `etag`, if it has one.
///
/// `If-Range` holds the range to the version of the file the client already
/// has: a range is served only when it names `etag`, compared strongly
/// (RFC 9110, section 13.1.5). A file with no entity tag, or an `If-Range`
/// that names another or a date, answers the whole file.
pub(crate) fn select(headers: &HeaderMap, length: u64, etag: Option<&str>) -> Selection {
    let Some(range) = headers.get(header::RANGE) else {
        return Selection::Whole;
    };
    if let Some(if_range) = headers.get(header::IF_RANGE) {
        let names_this_file = etag.is_some_and(|etag| {
            if_range
                .to_str()
                .is_ok_and(|value| value.trim() == etag && !etag.starts_with("W/"))
        });
        if !names_this_file {
            return Selection::Whole;
        }
    }
    range
        .to_str()
        .ok()
        .and_then(|range| parse_one(range, length))
        .unwrap_or(Selection::Whole)
}

/// One `bytes=` range against a file `length` bytes long, or `None` for a
/// value the server does not serve.
fn parse_one(range: &str, length: u64) -> Option<Selection> {
    let (unit, spec) = range.trim().split_once('=')?;
    if !unit.trim().eq_ignore_ascii_case("bytes") || spec.contains(',') {
        return None;
    }
    let (first, last) = spec.trim().split_once('-')?;
    let (first, last) = (first.trim(), last.trim());
    let selection = match (position(first), position(last)) {
        // A suffix: the last `n` bytes, or all of a file shorter than `n`.
        (None, Some(suffix)) if first.is_empty() => {
            if suffix == 0 || length == 0 {
                Selection::Unsatisfiable
            } else {
                Selection::Part {
                    start: length.saturating_sub(suffix),
                    end: length - 1,
                }
            }
        }
        (Some(start), None) if last.is_empty() => clamped_part(start, u64::MAX, length),
        (Some(start), Some(end)) if start <= end => clamped_part(start, end, length),
        _ => return None,
    };
    Some(selection)
}

/// The range from `start` to `end`, cut at the end of the file.
fn clamped_part(start: u64, end: u64, length: u64) -> Selection {
    if start >= length {
        Selection::Unsatisfiable
    } else {
        Selection::Part {
            start,
            end: end.min(length - 1),
        }
    }
}

/// A byte position: one or more digits. A number too large for `u64` is past
/// the end of any file, so it reads as the largest.
fn position(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(text.parse().unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(name.clone(), value.parse().unwrap());
        }
        headers
    }

    fn range(value: &str, length: u64) -> Selection {
        select(&with(&[(header::RANGE, value)]), length, None)
    }

    #[test]
    fn one_range_selects_its_bytes_cut_at_the_end_of_the_file() {
        let part = |start, end| Selection::Part { start, end };
        assert_eq!(range("bytes=0-0", 10), part(0, 0));
        assert_eq!(range("bytes=2-5", 10), part(2, 5));
        assert_eq!(range("bytes=2-", 10), part(2, 9));
        assert_eq!(range("bytes=-3", 10), part(7, 9));
        assert_eq!(range("bytes=-30", 10), part(0, 9));
        assert_eq!(range("bytes=4-400", 10), part(4, 9));
        assert_eq!(range("Bytes = 1-2", 10), part(1, 2));
        assert_eq!(range("bytes=9-99999999999999999999999", 10), part(9, 9));
    }

    #[test]
    fn a_range_that_selects_no_byte_is_unsatisfiable() {
        assert_eq!(range("bytes=10-", 10), Selection::Unsatisfiable);
        assert_eq!(range("bytes=10-20", 10), Selection::Unsatisfiable);
        assert_eq!(range("bytes=-0", 10), Selection::Unsatisfiable);
        assert_eq!(range("bytes=0-", 0), Selection::Unsatisfiable);
        assert_eq!(range("bytes=-5", 0), Selection::Unsatisfiable);
        assert_eq!(
            range("bytes=99999999999999999999999-", 10),
            Selection::Unsatisfiable
        );
    }

    #[test]
    fn a_range_the_server_does_not_serve_selects_the_whole_file() {
        for value in [
            "items=0-3",
            "bytes=0-1,4-5",
            "bytes=5-2",
            "bytes=-",
            "bytes=a-b",
            "bytes=+1-2",
            "bytes 0-3",
            "bytes=0-3-4",
        ] {
            assert_eq!(range(value, 10), Selection::Whole, "{value}");
        }
        assert_eq!(select(&HeaderMap::new(), 10, None), Selection::Whole);
    }

    #[test]
    fn if_range_serves_the_range_only_for_the_files_own_strong_tag() {
        let ask = |if_range: &str, etag: Option<&str>| {
            select(
                &with(&[(header::RANGE, "bytes=0-1"), (header::IF_RANGE, if_range)]),
                10,
                etag,
            )
        };
        let part = Selection::Part { start: 0, end: 1 };
        assert_eq!(ask("\"abc\"", Some("\"abc\"")), part);
        assert_eq!(ask("\"other\"", Some("\"abc\"")), Selection::Whole);
        assert_eq!(ask("W/\"abc\"", Some("\"abc\"")), Selection::Whole);
        assert_eq!(
            ask("Tue, 15 Nov 1994 08:12:31 GMT", Some("\"abc\"")),
            Selection::Whole
        );
        assert_eq!(ask("\"abc\"", None), Selection::Whole);
    }
}
