//! The reactions, the reply, the deleted mark and the edit an iMazing row
//! records in its `Reactions`, `Replying to`, `Deleted Date` and
//! `Edited Date` cells, read into the shapes the conversation file carries.
//!
//! iMazing writes each as text with no id. A reaction names its reactor by
//! display name and never by address, a reply names the quoted message by
//! its sender and date, a deletion is a date, and an edit is a date with no
//! earlier text. The exporter keeps only part of what the text says: a
//! reaction keeps its emoji alone, a reply is linked by its date within the
//! chat, and an edit is a version with no text (#2030, #2045).

use ir::{Deletion, EarlierVersion, Reaction};

/// What a reaction line writes between the reactor's name and the emoji.
const REACTED_WITH: &str = " reacted with ";

/// What a reaction line writes between the emoji and the time.
const ON: &str = " on ";

/// What iMazing writes in place of the emoji for a sticker reaction.
const A_STICKER: &str = "a sticker";

/// The reactions a `Reactions` cell holds, one per line, in the order
/// written. iMazing writes each line as
/// `<Name> reacted with <emoji> on <M/D/YYYY h:mm:ss AM/PM>`, where the
/// name is a contact name, a phone number, or `You` for the account
/// holder. Only the emoji is kept (#2045): the name and the time are read
/// past, so no reaction names its reactor or is the holder's own. A line
/// saying a reaction was removed
/// (`<Name> removed the reaction <emoji> on <time>`) is no reaction, and
/// neither is a line in any other shape.
pub(crate) fn parse_reactions(cell: &str) -> Vec<Reaction> {
    cell.lines().filter_map(parse_reaction_line).collect()
}

/// One `reacted with` line's reaction, or `None` for any other line. The
/// emoji is the text between `reacted with` and the last ` on `, so a name
/// with ` on ` in it is read past. A sticker reaction is kind `sticker`
/// with no emoji, as Apple Messages writes it, because iMazing writes the
/// words `a sticker` where the emoji goes.
fn parse_reaction_line(line: &str) -> Option<Reaction> {
    let (head, time) = line.trim().rsplit_once(ON)?;
    let (_name, emoji) = head.rsplit_once(REACTED_WITH)?;
    if emoji.is_empty() || time.is_empty() {
        return None;
    }
    let (kind, emoji) = if emoji == A_STICKER {
        ("sticker", None)
    } else {
        ("emoji", Some(emoji.to_string()))
    };
    Some(Reaction {
        part_index: 0,
        kind: kind.into(),
        emoji,
        is_from_me: false,
        reactor_identity: None,
        reactor_display_name: None,
    })
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

    /// Each reaction of `cell` as `(kind, emoji)`, after checking that
    /// none names its reactor or is the account holder's.
    fn kinds_and_emoji(cell: &str) -> Vec<(String, Option<String>)> {
        parse_reactions(cell)
            .into_iter()
            .map(|r| {
                assert_eq!(r.part_index, 0);
                assert!(!r.is_from_me, "{r:?}");
                assert_eq!(r.reactor_identity, None);
                assert_eq!(r.reactor_display_name, None);
                (r.kind, r.emoji)
            })
            .collect()
    }

    /// `(kind "emoji", emoji)` as [`kinds_and_emoji`] lists it.
    fn emoji(emoji: &str) -> (String, Option<String>) {
        ("emoji".into(), Some(emoji.into()))
    }

    /// Each `reacted with` line of a cell is one reaction, its emoji whole
    /// with its variation selector, joiners and skin tone, whoever the line
    /// names: a contact, a phone number, or `You`.
    #[test]
    fn each_reacted_with_line_is_its_emoji() {
        assert_eq!(
            kinds_and_emoji(
                "Bob Sample reacted with \u{2764}\u{fe0f} on 1/1/2020 12:04:00 PM\n\
                 You reacted with \u{1f44d}\u{1f3fb} on 1/1/2020 12:05:00 PM\n\
                 +1 (321) 555-0100 reacted with \u{203c}\u{fe0f} on 12/31/2019 9:00:00 AM\n\
                 Ana Lima-Costa reacted with \u{1f926}\u{200d}\u{2640}\u{fe0f} on 1/2/2020 10:15:30 AM"
            ),
            vec![
                emoji("\u{2764}\u{fe0f}"),
                emoji("\u{1f44d}\u{1f3fb}"),
                emoji("\u{203c}\u{fe0f}"),
                emoji("\u{1f926}\u{200d}\u{2640}\u{fe0f}"),
            ]
        );
    }

    /// A name with ` on ` in it is read past, and the emoji is still the
    /// text before the time.
    #[test]
    fn the_emoji_is_read_from_the_end_of_the_line() {
        assert_eq!(
            kinds_and_emoji("Jo on Main St reacted with \u{1f602} on 1/1/2020 1:00:00 PM"),
            vec![emoji("\u{1f602}")]
        );
    }

    /// A sticker reaction, which iMazing writes as the words `a sticker`,
    /// is kind `sticker` with no emoji.
    #[test]
    fn a_sticker_reaction_has_no_emoji() {
        assert_eq!(
            kinds_and_emoji("Bob Sample reacted with a sticker on 1/1/2020 12:04:00 PM"),
            vec![("sticker".into(), None)]
        );
    }

    /// A removed reaction, a blank line, and a line in neither shape give
    /// no reaction, and the lines around them still do.
    #[test]
    fn a_removed_reaction_or_another_line_is_no_reaction() {
        assert_eq!(
            kinds_and_emoji("You removed the reaction \u{1f44d} on 1/1/2020 12:06:00 PM"),
            vec![]
        );
        assert_eq!(
            kinds_and_emoji(
                "Bob Sample reacted with \u{1f44d} on 1/1/2020 12:04:00 PM\n\
                 \n\
                 Bob Sample \u{1f44d} 1/1/2020 12:05:00 PM\n\
                 Bob Sample reacted with  on 1/1/2020 12:06:00 PM\n\
                 Bob Sample reacted with \u{1f44d} on \n\
                 You reacted with \u{1f602} on 1/1/2020 12:07:00 PM"
            ),
            vec![emoji("\u{1f44d}"), emoji("\u{1f602}")]
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
