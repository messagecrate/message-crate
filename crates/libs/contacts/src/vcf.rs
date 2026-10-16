//! vCard reader: each card's name, phone numbers and email addresses.

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};

/// A card names a `CHARSET` for a quoted-printable value that the reader
/// cannot read, so the value cannot be read as text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "card {card} writes its {property} in the character set {charset:?}, which Message Crate cannot read"
)]
pub struct UnknownCharset {
    /// The card's position in the file, counted from 1.
    pub card: usize,
    /// The property whose value is in that charset, such as `FN`.
    pub property: String,
    /// The `CHARSET` parameter as the card writes it.
    pub charset: String,
}

/// One parsed vCard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VcfCard {
    /// Unescaped `FN` value.
    pub fn_raw: String,
    /// `N` family (last) name component.
    pub n_family: String,
    /// `N` given (first) name component.
    pub n_given: String,
    /// `N` middle name component.
    pub n_middle: String,
    /// `TEL` values in the order the card lists them.
    pub phones: Vec<String>,
    /// `EMAIL` values in the order the card lists them.
    pub emails: Vec<String>,
}

impl VcfCard {
    /// The name the card gives its person: `FN`, or the given, middle and
    /// family names of `N` joined by spaces when `FN` is blank. Empty when
    /// the card has neither.
    #[must_use]
    pub fn display_name(&self) -> String {
        if !self.fn_raw.is_empty() {
            return self.fn_raw.clone();
        }
        [&self.n_given, &self.n_middle, &self.n_family]
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Parse vCard text into its cards, in the order the text holds them.
///
/// Continuation lines are joined first. A property may carry a group prefix
/// (`item1.TEL`) and parameters (`TEL;TYPE=CELL`); only the property's name
/// is read. Properties other than `FN`, `N`, `TEL` and `EMAIL` are skipped.
///
/// A vCard 2.1 quoted-printable value is decoded in the charset its
/// `CHARSET` parameter names, by its WHATWG label: UTF-8, Shift_JIS, GBK,
/// Big5, EUC-KR, the Windows and ISO-8859 sets, KOI8 and the rest. As in a
/// browser, ISO-8859-1 is read as Windows-1252, which most files labeled
/// ISO-8859-1 are. A value with no `CHARSET` is read as UTF-8, and as
/// Windows-1252 when its bytes are not UTF-8.
///
/// # Errors
///
/// [`UnknownCharset`] when a quoted-printable `FN`, `N`, `TEL` or `EMAIL`
/// names a charset Message Crate cannot read, rather than reading the name
/// wrong: a label WHATWG does not list, or one it maps to its replacement
/// encoding, which would turn every character into U+FFFD.
pub fn parse_vcf(text: &str) -> Result<Vec<VcfCard>, UnknownCharset> {
    let lines = unfold_lines(text);
    let mut cards = Vec::new();
    let mut current: Option<VcfCard> = None;

    for line in lines {
        let line = line.trim_end();
        if line.eq_ignore_ascii_case("BEGIN:VCARD") {
            current = Some(VcfCard::default());
            continue;
        }
        if line.eq_ignore_ascii_case("END:VCARD") {
            if let Some(card) = current.take() {
                cards.push(card);
            }
            continue;
        }
        let Some(card) = current.as_mut() else {
            continue;
        };
        apply_line(card, cards.len() + 1, line)?;
    }

    Ok(cards)
}

/// Join vCard continuation lines (starting with a space or tab) onto the line before.
///
/// A vCard 2.1 value in quoted-printable continues instead on the next line
/// when it ends with a soft line break, an `=`, which is dropped. A card's
/// `BEGIN:VCARD` or `END:VCARD` never continues one, so a malformed last
/// value cannot swallow the end of its card.
fn unfold_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let starts_or_ends_a_card = line.trim_end().eq_ignore_ascii_case("BEGIN:VCARD")
            || line.trim_end().eq_ignore_ascii_case("END:VCARD");
        if let Some(last) = out.last_mut()
            && !starts_or_ends_a_card
            && is_quoted_printable(last)
            && last.ends_with('=')
        {
            last.pop();
            last.push_str(line);
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = out.last_mut() {
                last.push_str(&line[1..]);
            }
            continue;
        }
        out.push(line.to_string());
    }
    out
}

/// Whether a property line's parameters say its value is quoted-printable
/// (`ENCODING=QUOTED-PRINTABLE`, or a bare `QUOTED-PRINTABLE` in vCard 2.1).
fn is_quoted_printable(line: &str) -> bool {
    let Some((name, _)) = line.split_once(':') else {
        return false;
    };
    name.split(';').skip(1).any(|param| {
        let value = param.rsplit_once('=').map_or(param, |(_, value)| value);
        value.eq_ignore_ascii_case("QUOTED-PRINTABLE")
    })
}

/// The `CHARSET` parameter of a property line, if it has one, without the
/// double quotes vCard 3.0 and 4.0 allow around a parameter value.
fn charset_param(name: &str) -> Option<&str> {
    name.split(';').skip(1).find_map(|param| {
        let (key, value) = param.split_once('=')?;
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .unwrap_or(value);
        key.trim().eq_ignore_ascii_case("CHARSET").then_some(value)
    })
}

/// Decode a quoted-printable value: each `=` and two hex digits is one
/// byte. The bytes are read in `encoding`, except that UTF-8 bytes that are
/// not UTF-8 are read as Windows-1252.
fn decode_quoted_printable(value: &str, encoding: &'static Encoding) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[at], hex) {
            (b'=', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    if encoding == UTF_8 {
        return String::from_utf8(out).unwrap_or_else(|e| {
            WINDOWS_1252
                .decode_without_bom_handling(e.as_bytes())
                .0
                .into_owned()
        });
    }
    encoding.decode_without_bom_handling(&out).0.into_owned()
}

/// Apply one `PROP;params:value` line to the card being built, the
/// `card_number`th card of the file.
///
/// # Errors
///
/// [`UnknownCharset`] when the value is quoted-printable in a charset
/// Message Crate cannot read.
fn apply_line(card: &mut VcfCard, card_number: usize, line: &str) -> Result<(), UnknownCharset> {
    let Some((name, value)) = line.split_once(':') else {
        return Ok(());
    };
    let prop = name.split(';').next().unwrap_or(name);
    let prop_upper = prop.to_ascii_uppercase();
    let base = prop_upper
        .rsplit_once('.')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or(prop_upper);
    if !matches!(base.as_str(), "FN" | "N" | "TEL" | "EMAIL") {
        return Ok(());
    }
    let decoded;
    let value =
        if is_quoted_printable(line) {
            let encoding =
                match charset_param(name) {
                    None => UTF_8,
                    Some(label) => Encoding::for_label_no_replacement(label.as_bytes())
                        .ok_or_else(|| UnknownCharset {
                            card: card_number,
                            property: base.clone(),
                            charset: label.to_string(),
                        })?,
                };
            decoded = decode_quoted_printable(value, encoding);
            decoded.as_str()
        } else {
            value
        };

    match base.as_str() {
        "FN" => card.fn_raw = unescape(value),
        "N" => {
            let parts = split_components(value);
            let part = |at: usize| parts.get(at).cloned().unwrap_or_default();
            card.n_family = part(0);
            card.n_given = part(1);
            card.n_middle = part(2);
        }
        "TEL" => push_value(&mut card.phones, without_tel_scheme(&unescape(value))),
        "EMAIL" => push_value(&mut card.emails, unescape(value)),
        _ => {}
    }
    Ok(())
}

/// A number without the `tel:` a vCard 4.0 `TEL;VALUE=uri` value starts
/// with, such as `tel:+1-555-555-0100`.
fn without_tel_scheme(value: &str) -> String {
    match value.get(..4) {
        Some(scheme) if scheme.eq_ignore_ascii_case("tel:") => value[4..].trim().to_string(),
        _ => value.to_string(),
    }
}

/// Add `value` to `list` unless it is blank. A value the card repeats is
/// added again: each `TEL` and `EMAIL` line is one row of the address book,
/// and the load reads one identity listed twice under a contact as one.
fn push_value(list: &mut Vec<String>, value: String) {
    if !value.is_empty() {
        list.push(value);
    }
}

/// Undo vCard escaping, reading the value once from left to right: a
/// backslash and the character after it become one character. `\n` and `\N`
/// are a newline, and any other escaped character is itself, which covers
/// `\,`, `\;`, `\\`, and the `\:` and `\"` Google Contacts writes. A
/// backslash at the very end is kept. The result is trimmed.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(next) => out.push(next),
            None => out.push('\\'),
        }
    }
    out.trim().to_string()
}

/// Split a structured value such as `N` on its unescaped `;`, then unescape
/// each component.
fn split_components(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            cur.push('\\');
            if let Some(next) = chars.next() {
                cur.push(next);
            }
            continue;
        }
        if ch == ';' {
            out.push(unescape(&cur));
            cur.clear();
            continue;
        }
        cur.push(ch);
    }
    out.push(unescape(&cur));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_keeps_the_character_after_an_escaped_backslash() {
        // `C:\\new` is `C:\new`: the escaped backslash is read before the
        // `n` after it, so the `n` is never part of a newline.
        let cards = parse_vcf("BEGIN:VCARD\nFN:C:\\\\new\nEND:VCARD\n").unwrap();
        assert_eq!(cards[0].fn_raw, r"C:\new");
    }

    #[test]
    fn unescape_reads_each_escape_as_one_character() {
        assert_eq!(unescape(r"a\nb\Nc"), "a\nb\nc");
        assert_eq!(unescape(r"Smith\, Jr\; II"), "Smith, Jr; II");
        assert_eq!(
            unescape(r#"https\://example.com/\"x\""#),
            "https://example.com/\"x\""
        );
        assert_eq!(unescape(r"end\"), r"end\");
    }

    #[test]
    fn parse_vcf_reads_every_phone_and_email_in_order() {
        let text = "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Lovelace\nN:Lovelace;Ada;Augusta;;\n\
             TEL;TYPE=CELL:+15555550119\nitem1.TEL:(555) 555-0120\n\
             EMAIL;TYPE=INTERNET:ada@example.com\nitem1.EMAIL;TYPE=INTERNET:ada@example.org\n\
             CATEGORIES:myContacts\nEND:VCARD\n";
        let cards = parse_vcf(text).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].n_middle, "Augusta");
        assert_eq!(cards[0].phones, vec!["+15555550119", "(555) 555-0120"]);
        assert_eq!(cards[0].emails, vec!["ada@example.com", "ada@example.org"]);
    }

    #[test]
    fn parse_vcf_splits_n_on_unescaped_semicolons_only() {
        let cards = parse_vcf("BEGIN:VCARD\nN:Smith\\;Jones;Ada;;;\nEND:VCARD\n").unwrap();
        assert_eq!(cards[0].n_family, "Smith;Jones");
        assert_eq!(cards[0].n_given, "Ada");
    }

    #[test]
    fn parse_vcf_unfolds_lines_continued_with_a_space_or_a_tab() {
        // A continuation line drops its one leading space or tab and joins
        // the line before it.
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Augusta King, Coun\r\n \
             tess of Lovelace\r\nTEL:+1555\r\n\t5550119\r\nEND:VCARD\r\n";
        let cards = parse_vcf(text).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].fn_raw, "Ada Augusta King, Countess of Lovelace");
        assert_eq!(cards[0].phones, vec!["+15555550119"]);
    }

    #[test]
    fn parse_vcf_keeps_every_tel_line_and_skips_an_empty_one() {
        let text = "BEGIN:VCARD\nFN:Ada\nTEL;TYPE=CELL:+15555550119\n\
             TEL;TYPE=HOME:+15555550119\nTEL:\nEND:VCARD\n";
        let cards = parse_vcf(text).unwrap();
        assert_eq!(cards[0].phones, vec!["+15555550119", "+15555550119"]);
    }

    /// A quoted-printable value that ends its card with a stray `=` does not
    /// take the card's `END:VCARD` with it.
    #[test]
    fn parse_vcf_ends_a_card_after_a_quoted_printable_value_ending_in_a_soft_break() {
        let text = "BEGIN:VCARD\nTEL:555-0100\nNOTE;ENCODING=QUOTED-PRINTABLE:a=\nEND:VCARD\n\
             BEGIN:VCARD\nTEL:555-0101\nEND:VCARD\n";
        let cards = parse_vcf(text).unwrap();
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].phones, vec!["555-0100"]);
    }

    /// vCard 4.0 writes a number as a `tel:` URI.
    #[test]
    fn parse_vcf_takes_the_tel_scheme_off_a_number() {
        let text =
            "BEGIN:VCARD\nVERSION:4.0\nTEL;VALUE=uri;TYPE=cell:tel:+1-555-555-0100\nEND:VCARD\n";
        assert_eq!(parse_vcf(text).unwrap()[0].phones, vec!["+1-555-555-0100"]);
    }

    /// Android's contacts export is vCard 2.1, which writes a name outside
    /// ASCII in quoted-printable, continued on the next line after a
    /// trailing `=`.
    #[test]
    fn parse_vcf_decodes_a_quoted_printable_value_across_soft_line_breaks() {
        let text = "BEGIN:VCARD\r\nVERSION:2.1\r\n\
             N;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:Dupr=C3=A9;=C3=89milie;;;\r\n\
             FN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:=C3=89milie Dupr=C3=A9 de la Fo=\r\n\
             nta=C3=AEne\r\nTEL;CELL:555-0100\r\nEND:VCARD\r\n";
        let cards = parse_vcf(text).unwrap();
        assert_eq!(cards[0].fn_raw, "Émilie Dupré de la Fontaîne");
        assert_eq!(cards[0].n_given, "Émilie");
        assert_eq!(cards[0].phones, vec!["555-0100"]);
    }

    /// A Japanese Android export writes a name in Shift_JIS, which the
    /// property's `CHARSET` names.
    #[test]
    fn parse_vcf_decodes_a_quoted_printable_value_in_the_charset_it_names() {
        let text = "BEGIN:VCARD\r\nVERSION:2.1\r\n\
             N;CHARSET=SHIFT_JIS;ENCODING=QUOTED-PRINTABLE:=8E=52=93=63;=91=BE=98=59;;;\r\n\
             FN;CHARSET=SHIFT_JIS;ENCODING=QUOTED-PRINTABLE:=8E=52=93=63=91=BE=98=59\r\n\
             TEL;CELL:555-0100\r\nEND:VCARD\r\n";
        let cards = parse_vcf(text).unwrap();
        assert_eq!(cards[0].fn_raw, "山田太郎");
        assert_eq!(cards[0].n_family, "山田");
        assert_eq!(cards[0].n_given, "太郎");
    }

    /// vCard 3.0 and 4.0 allow a parameter value in double quotes.
    #[test]
    fn parse_vcf_reads_a_charset_written_in_double_quotes() {
        let text = "BEGIN:VCARD\nVERSION:3.0\n\
             N;CHARSET=\"Shift_JIS\";ENCODING=QUOTED-PRINTABLE:=8E=52=93=63;;;;\n\
             FN;CHARSET=\"UTF-8\";ENCODING=QUOTED-PRINTABLE:Dupr=C3=A9\nEND:VCARD\n";
        let cards = parse_vcf(text).unwrap();
        assert_eq!(cards[0].n_family, "山田");
        assert_eq!(cards[0].fn_raw, "Dupré");
    }

    /// Most files labeled ISO-8859-1 are Windows-1252, where `=80` is the
    /// euro sign rather than a control character. Bytes with no `CHARSET`
    /// that are not UTF-8 are read the same way.
    #[test]
    fn parse_vcf_reads_iso_8859_1_and_bytes_that_are_not_utf_8_as_windows_1252() {
        for params in [";CHARSET=ISO-8859-1", ";CHARSET=windows-1252", ""] {
            let text = format!(
                "BEGIN:VCARD\nVERSION:2.1\n\
                 FN{params};ENCODING=QUOTED-PRINTABLE:Caf=E9 =80\nEND:VCARD\n"
            );
            assert_eq!(parse_vcf(&text).unwrap()[0].fn_raw, "Café €", "{params}");
        }
    }

    /// A charset the reader does not know is refused, naming the card and
    /// the charset, rather than read as some other charset. A property the
    /// reader skips is not decoded, so its charset does not matter.
    #[test]
    fn parse_vcf_refuses_a_name_in_a_charset_it_does_not_know() {
        let text = "BEGIN:VCARD\nFN:Ada\nEND:VCARD\n\
             BEGIN:VCARD\nVERSION:2.1\nNOTE;CHARSET=X-MADE-UP;ENCODING=QUOTED-PRINTABLE:=82\n\
             N;CHARSET=X-MADE-UP;ENCODING=QUOTED-PRINTABLE:=82=A0;;;;\nEND:VCARD\n";
        let refused = parse_vcf(text).unwrap_err();
        assert_eq!(
            refused,
            UnknownCharset {
                card: 2,
                property: "N".into(),
                charset: "X-MADE-UP".into(),
            }
        );
        assert_eq!(
            refused.to_string(),
            "card 2 writes its N in the character set \"X-MADE-UP\", which Message Crate cannot read"
        );
        let skipped = "BEGIN:VCARD\nFN:Ada\n\
             NOTE;CHARSET=X-MADE-UP;ENCODING=QUOTED-PRINTABLE:=82\nEND:VCARD\n";
        assert_eq!(parse_vcf(skipped).unwrap()[0].fn_raw, "Ada");
    }

    #[test]
    fn display_name_is_fn_else_n_assembled_else_empty() {
        let card = |fn_raw: &str, given: &str, family: &str| VcfCard {
            fn_raw: fn_raw.into(),
            n_given: given.into(),
            n_family: family.into(),
            ..VcfCard::default()
        };
        assert_eq!(card("Ada L", "Ada", "Lovelace").display_name(), "Ada L");
        assert_eq!(card("", "Ada", "Lovelace").display_name(), "Ada Lovelace");
        assert_eq!(card("", "", "Lovelace").display_name(), "Lovelace");
        assert_eq!(card("", "", "").display_name(), "");
    }
}
