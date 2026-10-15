//! The attributes of an SMS Backup & Restore element, with their
//! references decoded.

use std::collections::{BTreeMap, HashMap};

/// The element's attributes as a map with lower-case keys.
///
/// Each value is taken raw and its references decoded, HTML entities such
/// as `&nbsp;` included. XML attribute-value normalisation is not applied:
/// it would turn a literal line break in a message into a space, and
/// SMS Backup & Restore files hold literal line breaks. A reference that is
/// not a character is dropped and added to `dropped`.
pub(crate) fn attrs(
    e: &quick_xml::events::BytesStart<'_>,
    dropped: &mut u64,
) -> HashMap<String, String> {
    e.attributes()
        .flatten()
        .map(|a| {
            let key = a.key.as_ref().to_ascii_lowercase();
            let value = decode_references(&a.value, dropped);
            (key, value)
        })
        .collect()
}

/// `raw` with its references decoded.
///
/// A numeric reference is decoded here: a UTF-16 surrogate pair written as
/// two references, as older SMS Backup & Restore versions write an emoji,
/// becomes one character. A reference that is not a character (`&#0;`, a
/// lone surrogate, a number past U+10FFFF) is dropped and added to
/// `dropped`. The text between numeric references, named entities such as
/// `&nbsp;` included, is decoded by `html_escape`.
fn decode_references(raw: &str, dropped: &mut u64) -> String {
    let mut out = String::with_capacity(raw.len());
    let (mut plain, mut from) = (0, 0);
    while let Some(offset) = raw[from..].find("&#") {
        let at = from + offset;
        let Some((code, len)) = numeric_reference(&raw[at..]) else {
            from = at + 2;
            continue;
        };
        out.push_str(&html_escape::decode_html_entities(&raw[plain..at]));
        let mut end = at + len;
        let ch = if (0xD800..=0xDBFF).contains(&code) {
            numeric_reference(&raw[end..])
                .filter(|(low, _)| (0xDC00..=0xDFFF).contains(low))
                .and_then(|(low, low_len)| {
                    end += low_len;
                    char::from_u32(0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00))
                })
        } else {
            char::from_u32(code)
        };
        match ch.filter(|c| *c != '\0') {
            Some(c) => out.push(c),
            None => *dropped += 1,
        }
        (plain, from) = (end, end);
    }
    out.push_str(&html_escape::decode_html_entities(&raw[plain..]));
    out
}

/// The code point and byte length of the `&#…;` or `&#x…;` reference that
/// `s` starts with. A number too large for `u32` is `u32::MAX`, which is
/// not a character.
fn numeric_reference(s: &str) -> Option<(u32, usize)> {
    let body = s.strip_prefix("&#")?;
    let (digits, radix, prefix) = match body.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16, 3),
        None => (body, 10, 2),
    };
    let count = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    if count == 0 || !digits[count..].starts_with(';') {
        return None;
    }
    let code = u32::from_str_radix(&digits[..count], radix).unwrap_or(u32::MAX);
    Some((code, prefix + count + 1))
}

/// The attribute value, or an empty string.
pub(crate) fn get<'a>(attrs: &'a HashMap<String, String>, key: &str) -> &'a str {
    attrs.get(key).map_or("", String::as_str)
}

/// The attributes as an ordered map, for the vendor bag.
pub(crate) fn btree(attrs: &HashMap<String, String>) -> BTreeMap<String, String> {
    attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// A body with HTML entities decoded and line endings normalized.
pub(crate) fn decode_body(raw: &str) -> String {
    html_escape::decode_html_entities(raw)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

/// Why a file is not valid XML: where the read stopped and the reason.
///
/// A file that is not valid XML is refused whole, never read in part,
/// because a file cut off or damaged partway is a bad copy of the backup
/// to fetch again, not a smaller backup (#2028).
#[derive(Debug)]
pub(crate) struct InvalidXml {
    position: u64,
    reason: String,
}

impl std::fmt::Display for InvalidXml {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "not valid XML at byte {}: {}",
            self.position, self.reason
        )
    }
}

impl std::error::Error for InvalidXml {}

impl InvalidXml {
    /// The error naming `path` and saying why it is not valid XML:
    /// `<path> is not valid XML at byte <n>: <reason>`.
    pub(crate) fn in_file(&self, path: &std::path::Path) -> anyhow::Error {
        anyhow::anyhow!("{} is {self}", path.display())
    }
}

/// `error` as [`InvalidXml::in_file`] words it when it is an [`InvalidXml`],
/// else `error` as it is, such as a cancel or a callback's own error.
pub(crate) fn with_file_named(error: anyhow::Error, path: &std::path::Path) -> anyhow::Error {
    match error.downcast_ref::<InvalidXml>() {
        Some(invalid) => invalid.in_file(path),
        None => error,
    }
}

/// The events of an XML file, ending in an [`InvalidXml`] where the file
/// is not valid XML. Unlike the parser alone, it refuses a file that ends
/// while an element is still open, which is how a file cut off between two
/// elements ends.
pub(crate) struct XmlEvents<R> {
    reader: quick_xml::Reader<R>,
    /// The names of the elements open at the current event, outermost
    /// first.
    open: Vec<String>,
}

impl<R: std::io::BufRead> XmlEvents<R> {
    /// The events `reader` reads.
    pub(crate) fn new(reader: quick_xml::Reader<R>) -> Self {
        Self {
            reader,
            open: Vec::new(),
        }
    }

    /// The next event, read into `buf`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidXml`] when the parser refuses the file, or the file
    /// ends while an element is open.
    pub(crate) fn next<'b>(
        &mut self,
        buf: &'b mut Vec<u8>,
    ) -> Result<quick_xml::events::Event<'b>, InvalidXml> {
        use quick_xml::events::Event;
        match self.reader.read_event_into(buf) {
            Ok(Event::Start(e)) => {
                self.open.push(e.name().into_inner().to_string());
                Ok(Event::Start(e))
            }
            Ok(Event::End(e)) => {
                // The parser has checked that the end tag closes the
                // innermost open element.
                self.open.pop();
                Ok(Event::End(e))
            }
            Ok(Event::Eof) => match self.open.last() {
                Some(name) => Err(InvalidXml {
                    position: self.reader.buffer_position(),
                    reason: format!("the file ends before </{name}>"),
                }),
                None => Ok(Event::Eof),
            },
            Ok(event) => Ok(event),
            Err(error) => Err(InvalidXml {
                position: self.reader.error_position(),
                reason: error.to_string(),
            }),
        }
    }
}
