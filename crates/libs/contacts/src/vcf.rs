//! vCard 3.0 reader: each card's name, phone numbers and email addresses.

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
    /// `TEL` values in the order the card lists them, each once.
    pub phones: Vec<String>,
    /// `EMAIL` values in the order the card lists them, each once.
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
#[must_use]
pub fn parse_vcf(text: &str) -> Vec<VcfCard> {
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
        apply_line(card, line);
    }

    cards
}

/// Join vCard continuation lines (starting with a space or tab) onto the line before.
fn unfold_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
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

/// Apply one `PROP;params:value` line to the card being built.
fn apply_line(card: &mut VcfCard, line: &str) {
    let Some((name, value)) = line.split_once(':') else {
        return;
    };
    let prop = name.split(';').next().unwrap_or(name);
    let prop_upper = prop.to_ascii_uppercase();
    let base = prop_upper
        .rsplit_once('.')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or(prop_upper);

    match base.as_str() {
        "FN" => card.fn_raw = unescape(value),
        "N" => {
            let parts = split_components(value);
            let part = |at: usize| parts.get(at).cloned().unwrap_or_default();
            card.n_family = part(0);
            card.n_given = part(1);
            card.n_middle = part(2);
        }
        "TEL" => push_once(&mut card.phones, unescape(value)),
        "EMAIL" => push_once(&mut card.emails, unescape(value)),
        _ => {}
    }
}

/// Add `value` to `list` unless it is blank or already there.
fn push_once(list: &mut Vec<String>, value: String) {
    if !value.is_empty() && !list.contains(&value) {
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
        let cards = parse_vcf("BEGIN:VCARD\nFN:C:\\\\new\nEND:VCARD\n");
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
        let cards = parse_vcf(text);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].n_middle, "Augusta");
        assert_eq!(cards[0].phones, vec!["+15555550119", "(555) 555-0120"]);
        assert_eq!(cards[0].emails, vec!["ada@example.com", "ada@example.org"]);
    }

    #[test]
    fn parse_vcf_splits_n_on_unescaped_semicolons_only() {
        let cards = parse_vcf("BEGIN:VCARD\nN:Smith\\;Jones;Ada;;;\nEND:VCARD\n");
        assert_eq!(cards[0].n_family, "Smith;Jones");
        assert_eq!(cards[0].n_given, "Ada");
    }

    #[test]
    fn parse_vcf_unfolds_lines_continued_with_a_space_or_a_tab() {
        // A continuation line drops its one leading space or tab and joins
        // the line before it.
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Augusta King, Coun\r\n \
             tess of Lovelace\r\nTEL:+1555\r\n\t5550119\r\nEND:VCARD\r\n";
        let cards = parse_vcf(text);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].fn_raw, "Ada Augusta King, Countess of Lovelace");
        assert_eq!(cards[0].phones, vec!["+15555550119"]);
    }

    #[test]
    fn parse_vcf_keeps_one_phone_for_a_repeated_or_empty_tel() {
        let text = "BEGIN:VCARD\nFN:Ada\nTEL;TYPE=CELL:+15555550119\n\
             TEL;TYPE=HOME:+15555550119\nTEL:\nEND:VCARD\n";
        let cards = parse_vcf(text);
        assert_eq!(cards[0].phones, vec!["+15555550119"]);
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
