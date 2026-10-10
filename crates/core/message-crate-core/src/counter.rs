//! The counts a run reports, each carrying the words its log line uses.
//!
//! A count is reported by more than one run: an SMS Backup & Restore read
//! counts the same messages whether Convert or an import runs it. Each count
//! is one [`Counter`] value, so every run that reports it words it the same
//! way, and no run can print a counter's key in place of its words (#1700).

/// One count a run reports, in its report's summary or in its log: a key
/// that tells it apart from the others, and the line that says it in plain
/// words. A count only a log line gives, such as the conversation files a
/// run is about to write, is a counter as well, so its line is singular for
/// one like every other.
///
/// Two counters with one key are the same counter: they compare and hash by
/// the key alone, so a report counts them as one and prints one line. Each
/// is defined once, as a constant beside the code that counts it, or here
/// when several exporters count it.
#[derive(Debug, Clone, Copy)]
pub struct Counter {
    key: &'static str,
    one: &'static str,
    many: &'static str,
}

impl Counter {
    /// A counter named `key`. `one` is the whole line for a count of 1, such
    /// as `Skipped 1 draft or message never sent`. `many` is the line for any
    /// other count, with `{n}` where the count goes, such as
    /// `Skipped {n} drafts or messages never sent`.
    pub const fn new(key: &'static str, one: &'static str, many: &'static str) -> Self {
        Self { key, one, many }
    }

    /// The counter's key, such as `skipped_never_sent`.
    pub const fn key(self) -> &'static str {
        self.key
    }

    /// The log line for a count of `n`.
    pub fn line(self, n: u64) -> String {
        words(n, self.one, self.many)
    }
}

impl PartialEq for Counter {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Counter {}

impl std::hash::Hash for Counter {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

/// `one` for a count of 1, else `many` with `n` in place of `{n}`.
fn words(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.replace("{n}", &n.to_string())
    }
}

/// `n` of a thing, singular for one: `count_of(1, "message", "messages")` is
/// `1 message` and `count_of(3, "message", "messages")` is `3 messages`. It
/// words a count inside a longer log line, where a [`Counter`]'s whole line
/// does not fit.
pub fn count_of(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// `n` files, singular for one, such as `1 file` or `3 files`.
pub fn count_of_files(n: u64) -> String {
    count_of(n, "file", "files")
}

/// Repeated copies of a message dropped, one copy of each kept.
pub const DUPLICATES_DROPPED: Counter = Counter::new(
    "duplicates_dropped",
    "Dropped 1 repeated copy of a message",
    "Dropped {n} repeated copies of messages",
);

/// Messages skipped because their date could not be read.
pub const SKIPPED_INVALID_DATE: Counter = Counter::new(
    "skipped_invalid_date",
    "Skipped 1 message with an invalid date",
    "Skipped {n} messages with an invalid date",
);

/// Messages skipped because they give no address that a phone number, an
/// email address or a name can be read from.
pub const SKIPPED_UNKNOWN_ADDRESS: Counter = Counter::new(
    "skipped_unknown_address",
    "Skipped 1 message with no usable address",
    "Skipped {n} messages with no usable address",
);

/// Messages skipped because their type is none the reader knows.
pub const SKIPPED_UNKNOWN_TYPE: Counter = Counter::new(
    "skipped_unknown_type",
    "Skipped 1 message of an unknown type",
    "Skipped {n} messages of an unknown type",
);

/// Conversations a resumed run found already written and did not write
/// again.
pub const CONVERSATIONS_RESUMED: Counter = Counter::new(
    "conversations_resumed",
    "Resumed past 1 conversation that was already written",
    "Resumed past {n} conversations that were already written",
);

/// The conversation files a run is about to write, logged as it starts.
pub const CONVERSATION_FILES_PREPARING: Counter = Counter::new(
    "conversation_files_preparing",
    "Preparing 1 conversation file...",
    "Preparing {n} conversation files...",
);

/// Attachment files saved to the output.
pub const ATTACHMENTS_SAVED: Counter = Counter::new(
    "attachments_saved",
    "Saved 1 attachment",
    "Saved {n} attachments",
);

/// Conversations whose identities, names and text were obfuscated.
pub const CONVERSATIONS_OBFUSCATED: Counter = Counter::new(
    "conversations_obfuscated",
    "Obfuscated 1 conversation",
    "Obfuscated {n} conversations",
);

/// Notification rows, such as an iMazing notification, kept as received
/// messages.
pub const NOTIFICATIONS: Counter = Counter::new(
    "notifications",
    "Kept 1 notification as a received message",
    "Kept {n} notifications as received messages",
);

/// Chats that name their person with no phone number or email address, each
/// kept under the name alone and sent as a
/// [`crate::NAME_ONLY_CHAT_NOTE`].
pub const NAME_ONLY_CHAT: Counter = Counter::new(
    "name_only_chat",
    "Kept 1 chat under a name alone, with no phone number or email address",
    "Kept {n} chats under a name alone, with no phone number or email address",
);

/// Parts left out of kept messages because they could not be read, each
/// message sent as an [`crate::unreadable_parts_note`].
pub const SKIPPED_UNREADABLE_PART: Counter = Counter::new(
    "skipped_unreadable_part",
    "Left out 1 message part that could not be read",
    "Left out {n} message parts that could not be read",
);

/// Messages an export left out because its format holds only SMS and MMS:
/// an iMessage or a WhatsApp message written as an SMS would come back from
/// a re-import as an SMS under a new id (ADR 0021).
pub const NOT_SMS_OR_MMS_LEFT_OUT: Counter = Counter::new(
    "messages_not_sms_or_mms_left_out",
    "Left out 1 message that is not SMS or MMS",
    "Left out {n} messages that are not SMS or MMS",
);

/// Attachments left out because their file was gone.
pub const ATTACHMENTS_MISSING: Counter = Counter::new(
    "attachments_missing",
    "Left out 1 attachment whose file is missing",
    "Left out {n} attachments whose files are missing",
);

/// The heading the run summary gives its Import Errors.
const IMPORT_ERRORS_HEADING: &str = "Import Errors";

/// The heading the run summary gives its notes.
const NOTES_HEADING: &str = "Notes";

/// The kind of backup item that an Import Error or a note names. Its
/// noun goes before the item in an [`item_line`] and after "This" in an
/// [`item_reason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A backup file, such as an XML or a mail.
    File,
    /// A CSV of a backup.
    Csv,
    /// An MMS, named by the file that holds it.
    Mms,
    /// A picture an attachment row names.
    Picture,
    /// An attachment, named by its path in the backup.
    Attachment,
}

impl ItemKind {
    /// The noun a sentence calls the item by, such as `file`.
    pub const fn noun(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Csv => "CSV",
            Self::Mms => "MMS",
            Self::Picture => "picture",
            Self::Attachment => "attachment",
        }
    }
}

/// The sentence the run summary gives one item of the backup: `kind` says
/// what the item is, and `what_happened` says what happened to it, worded
/// to follow the item, such as `could not be read and was left out`.
/// `item_line(ItemKind::File, "a.xml", "could not be read")` is
/// `The file a.xml could not be read`.
pub fn item_line(kind: ItemKind, item: &str, what_happened: &str) -> String {
    format!("The {} {item} {what_happened}", kind.noun())
}

/// The reason the Import Run lists beside one item of the backup, the same
/// words as its [`item_line`] said of "this" item, because the Import Run
/// names the item in a column of its own.
/// `item_reason(ItemKind::File, "could not be read")` is
/// `This file could not be read`.
pub fn item_reason(kind: ItemKind, what_happened: &str) -> String {
    format!("This {} {what_happened}", kind.noun())
}

/// The run summary's Import Errors under their heading, then its notes
/// under theirs, each line a sentence such as an [`item_line`], set in by
/// two spaces beneath its heading. A heading with nothing under it is left
/// out, so a run with neither gives no line.
pub fn error_and_note_lines(
    errors: &[impl std::fmt::Display],
    notes: &[impl std::fmt::Display],
) -> Vec<String> {
    let mut lines = import_error_lines(errors);
    push_under_heading(&mut lines, NOTES_HEADING, notes);
    lines
}

/// The Import Errors heading with `errors` beneath it, as
/// [`error_and_note_lines`] gives them, for a log that has no notes. No
/// line when `errors` is empty.
pub fn import_error_lines(errors: &[impl std::fmt::Display]) -> Vec<String> {
    let mut lines = Vec::new();
    push_under_heading(&mut lines, IMPORT_ERRORS_HEADING, errors);
    lines
}

/// Append `heading`, then each of `items` set in by two spaces, to `lines`;
/// nothing when `items` is empty.
fn push_under_heading(lines: &mut Vec<String>, heading: &str, items: &[impl std::fmt::Display]) {
    if items.is_empty() {
        return;
    }
    lines.push(heading.to_string());
    lines.extend(items.iter().map(|item| format!("  {item}")));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One is singular with its 1 written in, and every other count, 0
    /// included, is plural with the count in place of `{n}`.
    #[test]
    fn a_line_is_singular_for_one_and_plural_for_every_other_count() {
        assert_eq!(
            DUPLICATES_DROPPED.line(1),
            "Dropped 1 repeated copy of a message"
        );
        assert_eq!(
            DUPLICATES_DROPPED.line(3),
            "Dropped 3 repeated copies of messages"
        );
        assert_eq!(
            DUPLICATES_DROPPED.line(0),
            "Dropped 0 repeated copies of messages"
        );
    }

    /// A count inside a line is singular for one and plural for every other
    /// count, 0 included.
    #[test]
    fn a_count_is_singular_for_one_and_plural_for_every_other_count() {
        assert_eq!(count_of(1, "message", "messages"), "1 message");
        assert_eq!(count_of(3, "message", "messages"), "3 messages");
        assert_eq!(count_of(0, "message", "messages"), "0 messages");
        assert_eq!(count_of_files(1), "1 file");
        assert_eq!(count_of_files(2), "2 files");
    }

    /// Two counters with one key are one counter, whatever their words, so
    /// a report never splits one count into two lines.
    #[test]
    fn counters_with_one_key_are_equal() {
        let other = Counter::new("duplicates_dropped", "Dropped 1 copy", "Dropped {n} copies");
        assert_eq!(DUPLICATES_DROPPED, other);
        assert_ne!(DUPLICATES_DROPPED, SKIPPED_INVALID_DATE);
    }

    /// The Import Errors and the notes each go under their heading, each
    /// line a sentence beneath it with no `error:` or `note:` in front, and
    /// a heading with nothing under it is left out (#1920).
    #[test]
    fn import_errors_and_notes_go_under_their_headings() {
        let errors = ["The file a.xml could not be read and was left out: cut off".to_string()];
        let notes = ["The picture b.jpg is named by 2 rows".to_string()];
        let none: [String; 0] = [];
        assert_eq!(
            error_and_note_lines(&errors, &notes),
            [
                "Import Errors",
                "  The file a.xml could not be read and was left out: cut off",
                "Notes",
                "  The picture b.jpg is named by 2 rows",
            ]
        );
        assert_eq!(
            error_and_note_lines(&errors, &none),
            [
                "Import Errors",
                "  The file a.xml could not be read and was left out: cut off",
            ]
        );
        assert_eq!(
            error_and_note_lines(&none, &notes),
            ["Notes", "  The picture b.jpg is named by 2 rows"]
        );
        assert!(error_and_note_lines(&none, &none).is_empty());
    }

    /// A line names its item after the kind of thing it is, and the Import
    /// Run's reason says the same of "this" item, so both read as sentences.
    #[test]
    fn an_item_line_and_its_reason_are_sentences() {
        assert_eq!(
            item_line(
                ItemKind::File,
                "a.xml",
                "could not be read and was left out: cut off"
            ),
            "The file a.xml could not be read and was left out: cut off"
        );
        assert_eq!(
            item_reason(
                ItemKind::File,
                "could not be read and was left out: cut off"
            ),
            "This file could not be read and was left out: cut off"
        );
    }

    /// Every counter here says its count in words: its lines carry the count
    /// and no underscore, so none of them is a key printed in place of words.
    #[test]
    fn every_shared_counter_says_its_count_in_words() {
        for counter in [
            DUPLICATES_DROPPED,
            SKIPPED_INVALID_DATE,
            SKIPPED_UNKNOWN_ADDRESS,
            SKIPPED_UNKNOWN_TYPE,
            CONVERSATIONS_RESUMED,
            CONVERSATION_FILES_PREPARING,
            ATTACHMENTS_SAVED,
            CONVERSATIONS_OBFUSCATED,
            NOTIFICATIONS,
            NAME_ONLY_CHAT,
            SKIPPED_UNREADABLE_PART,
            NOT_SMS_OR_MMS_LEFT_OUT,
            ATTACHMENTS_MISSING,
        ] {
            assert!(counter.line(1).contains(" 1 "), "{}", counter.key());
            assert!(counter.line(7).contains(" 7 "), "{}", counter.key());
            assert!(!counter.line(7).contains('_'), "{}", counter.key());
        }
    }
}
