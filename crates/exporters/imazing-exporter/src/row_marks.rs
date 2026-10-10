//! The reactions, the reply, the deleted mark and the edit an iMazing row
//! records in its `Reactions`, `Replying to`, `Deleted Date` and
//! `Edited Date` cells, read into the shapes the conversation file carries.
//!
//! iMazing writes each as text with no id. A reaction names its reactor by
//! display name and never by address, a reply names the quoted message by
//! its sender and date, a deletion is a date, and an edit is a date with no
//! earlier text. The exporter reads what the text says and no more: a
//! reactor is kept by name, a reply is linked by its date within the chat,
//! and an edit is a version with no text (#2030).

use message_ir::{Deletion, EarlierVersion, Reaction};

/// What iMazing names the account holder in a reaction line, as its own
/// window does. The export carries no other name for them: outgoing rows
/// name no sender.
const OWN_REACTOR_NAME: &str = "Me";

/// What a reaction line may put between its name, its emoji and its time.
const SEPARATORS: &[char] = &[',', ':', ';', '-', '\u{2013}', '\u{2014}', '\u{b7}'];

/// The reactions a `Reactions` cell holds, one per line, in the order
/// written. A line names the reactor, the emoji, and when they reacted, in
/// the US `M/D/YYYY` form. The name is the text before the emoji, or after
/// it when nothing comes before, or before the time when the line has no
/// emoji, or the whole line when it has neither. The time is read past: a
/// reaction has no time of its own in the conversation file.
pub(crate) fn parse_reactions(cell: &str) -> Vec<Reaction> {
    cell.lines().filter_map(parse_reaction_line).collect()
}

/// One reaction line, or `None` for a blank one.
fn parse_reaction_line(line: &str) -> Option<Reaction> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (name, emoji) = match emoji_run(line) {
        Some((start, end)) => {
            let before = clean_name(&line[..start]);
            let name = if before.is_empty() {
                clean_name(before_us_date(&line[end..]))
            } else {
                before
            };
            (name, Some(line[start..end].to_string()))
        }
        None => (clean_name(before_us_date(line)), None),
    };
    Some(Reaction {
        part_index: 0,
        kind: "emoji".into(),
        emoji,
        is_from_me: name.eq_ignore_ascii_case(OWN_REACTOR_NAME),
        reactor_identity: None,
        reactor_display_name: (!name.is_empty()).then(|| name.to_string()),
    })
}

/// `text` without the whitespace and separators around it.
fn clean_name(text: &str) -> &str {
    text.trim()
        .trim_matches(|c: char| c.is_whitespace() || SEPARATORS.contains(&c))
}

/// `text` up to the first US date in it (`M/D/YYYY`), or all of it.
fn before_us_date(text: &str) -> &str {
    match us_date_at(text) {
        Some(at) => &text[..at],
        None => text,
    }
}

/// The byte offset of the first `M/D/YYYY` in `text`, with one or two
/// digits for the month and the day.
fn us_date_at(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    (0..bytes.len())
        .filter(|&i| i == 0 || !bytes[i - 1].is_ascii_digit())
        .find(|&i| {
            let mut at = i;
            for (min, max) in [(1, 2), (1, 2), (4, 4)] {
                let digits = bytes[at..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                if digits < min || digits > max {
                    return false;
                }
                at += digits;
                if max == 4 {
                    return true;
                }
                if bytes.get(at) != Some(&b'/') {
                    return false;
                }
                at += 1;
            }
            false
        })
}

/// The byte range of the first run of emoji-like characters in `line`:
/// the emoji with its joiners, variation selectors and skin tones.
fn emoji_run(line: &str) -> Option<(usize, usize)> {
    let start = line.char_indices().find(|&(_, c)| is_emoji_like(c))?.0;
    let end = line[start..]
        .char_indices()
        .find(|&(_, c)| !is_emoji_like(c))
        .map_or(line.len(), |(at, _)| start + at);
    Some((start, end))
}

/// Whether `c` can be part of an emoji rather than of a name or a time: a
/// symbol or pictograph, or a character that joins or varies one. Letters
/// of every script, digits and punctuation are not.
fn is_emoji_like(c: char) -> bool {
    match u32::from(c) {
        // Zero-width joiner, keycap, variation selectors and skin tones.
        0x200D | 0x20E3 | 0xFE00..=0xFE0F | 0x1F3FB..=0x1F3FF => true,
        // General and CJK punctuation.
        0x2000..=0x206F | 0x3000..=0x303F => false,
        cp => cp >= 0x2190 && !c.is_alphanumeric(),
    }
}

/// The date a `Replying to` cell quotes, as written: the first
/// `YYYY-MM-DD HH:MM` in it, with its seconds when it has them. iMazing
/// writes the cell as `↩ <sender>, <date>: « <snippet> »`, and leaves the
/// sender out on some rows, so the date alone names the quoted message.
/// `None` when the cell holds no date.
pub(crate) fn quoted_date(cell: &str) -> Option<&str> {
    const DATE: &[u8] = b"dddd-dd-dd dd:dd";
    const SECONDS: &[u8] = b":dd";
    let bytes = cell.as_bytes();
    let start = (0..bytes.len().checked_sub(DATE.len())? + 1)
        .find(|&at| matches_shape(&bytes[at..], DATE))?;
    let mut end = start + DATE.len();
    if matches_shape(&bytes[end..], SECONDS) {
        end += SECONDS.len();
    }
    Some(&cell[start..end])
}

/// Whether `bytes` starts with `shape`, where `d` stands for any digit and
/// every other byte for itself.
fn matches_shape(bytes: &[u8], shape: &[u8]) -> bool {
    bytes.len() >= shape.len()
        && shape.iter().zip(bytes).all(|(&want, &have)| {
            if want == b'd' {
                have.is_ascii_digit()
            } else {
                have == want
            }
        })
}

/// The mark a `Deleted Date` cell gives: a row with a date was deleted in
/// Messages before the export, and iMazing never says a message was
/// unsent, so the mark is never Unsent.
pub(crate) fn deletion(deleted_date: &str) -> Option<Deletion> {
    (!deleted_date.trim().is_empty()).then_some(Deletion::DeletedInSourceApp)
}

/// The earlier version an `Edited Date` cell gives: one version of the
/// first part, with no text, because iMazing writes the final text only,
/// and with the edit's time when the cell's date parsed
/// (`edited_at_unix_ms`).
pub(crate) fn textless_edit(edited_at_unix_ms: Option<i64>) -> EarlierVersion {
    EarlierVersion {
        part_index: 0,
        text: None,
        edited_at_unix_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each reaction of `cell` as `(reactor's name, emoji, is from me)`.
    fn names_and_emoji(cell: &str) -> Vec<(Option<String>, Option<String>, bool)> {
        parse_reactions(cell)
            .into_iter()
            .map(|r| (r.reactor_display_name, r.emoji, r.is_from_me))
            .collect()
    }

    /// `(name, emoji, from me)` as [`names_and_emoji`] lists it.
    fn one(
        name: Option<&str>,
        emoji: Option<&str>,
        from_me: bool,
    ) -> (Option<String>, Option<String>, bool) {
        (name.map(str::to_string), emoji.map(str::to_string), from_me)
    }

    /// Each line is one reaction: the name before the emoji, the emoji with
    /// its selectors and joiners whole, the time read past, and `Me` the
    /// account holder.
    #[test]
    fn a_line_is_a_name_an_emoji_and_a_time() {
        assert_eq!(
            names_and_emoji(
                "Bob Sample \u{2764}\u{fe0f} 1/1/2020 12:04:00 PM\n\
                 Me \u{1f44d}\u{1f3fb} 1/1/2020 12:05:00 PM\n\
                 \n\
                 Ana Lima-Costa, \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} - 12/31/2019 9:00:00 AM"
            ),
            vec![
                one(Some("Bob Sample"), Some("\u{2764}\u{fe0f}"), false),
                one(Some("Me"), Some("\u{1f44d}\u{1f3fb}"), true),
                one(
                    Some("Ana Lima-Costa"),
                    Some("\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}"),
                    false
                ),
            ]
        );
        let reaction = &parse_reactions("Bob \u{1f602} 1/1/2020 1:00:00 PM")[0];
        assert_eq!(reaction.part_index, 0);
        assert_eq!(reaction.kind, "emoji");
        assert_eq!(reaction.reactor_identity, None);
    }

    /// A name after the emoji, a name in another script, and a line with no
    /// emoji each keep the name without the time.
    #[test]
    fn the_name_is_found_either_side_of_the_emoji_or_before_the_time() {
        assert_eq!(
            names_and_emoji(
                "\u{1f602} Bob Sample 1/1/2020 1:00:00 PM\n\
                 \u{5c71}\u{7530} \u{1f389} 1/1/2020 1:01:00 PM\n\
                 Bob Sample 1/1/2020 1:02:00 PM\n\
                 Bob Sample"
            ),
            vec![
                one(Some("Bob Sample"), Some("\u{1f602}"), false),
                one(Some("\u{5c71}\u{7530}"), Some("\u{1f389}"), false),
                one(Some("Bob Sample"), None, false),
                one(Some("Bob Sample"), None, false),
            ]
        );
        assert_eq!(parse_reactions(""), vec![]);
        assert_eq!(parse_reactions(" \n "), vec![]);
    }

    /// The date is found with or without a sender before it, with or
    /// without seconds, and a cell without one names nothing.
    #[test]
    fn the_quoted_date_is_the_first_date_in_the_cell() {
        assert_eq!(
            quoted_date("\u{21a9} Bob Sample, 2020-01-01 12:03:00: \u{ab} Lunch tomorrow? \u{bb}"),
            Some("2020-01-01 12:03:00")
        );
        assert_eq!(
            quoted_date("\u{21a9} 2019-12-31 09:00: \u{ab} 2020-01-01 10:00:00 \u{bb}"),
            Some("2019-12-31 09:00")
        );
        assert_eq!(quoted_date("\u{21a9} Bob: \u{ab} no date \u{bb}"), None);
        assert_eq!(quoted_date(""), None);
    }

    /// A deleted date marks the row Deleted in the source app and an edited
    /// date gives one version with no text.
    #[test]
    fn a_date_marks_a_deletion_and_a_date_is_an_edit_with_no_text() {
        assert_eq!(
            deletion("2020-01-02 08:00:00"),
            Some(Deletion::DeletedInSourceApp)
        );
        assert_eq!(deletion(" "), None);
        assert_eq!(
            textless_edit(Some(1_577_880_600_000)),
            EarlierVersion {
                part_index: 0,
                text: None,
                edited_at_unix_ms: Some(1_577_880_600_000),
            }
        );
        assert_eq!(textless_edit(None).text, None);
    }
}
