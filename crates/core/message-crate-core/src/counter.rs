//! The counts a run reports, each carrying the words its log line uses.
//!
//! A count is reported by more than one run: an SMS Backup & Restore read
//! counts the same messages whether Convert or an import runs it. Each count
//! is one [`Counter`] value, so every run that reports it words it the same
//! way, and no run can print a counter's key in place of its words (#1700).

/// One count a run reports: a key that tells it apart from the others, and
/// the line that says it in plain words.
///
/// Two counters with one key are the same counter. Each is defined once, as
/// a constant beside the code that counts it, or here when several
/// exporters count it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Counter {
    key: &'static str,
    one: &'static str,
    many: &'static str,
}

impl Counter {
    /// A counter named `key`. `one` is the whole line for a count of 1, such
    /// as `Skipped 1 draft or unsent message`. `many` is the line for any
    /// other count, with `{n}` where the count goes, such as
    /// `Skipped {n} drafts or unsent messages`.
    pub const fn new(key: &'static str, one: &'static str, many: &'static str) -> Self {
        Self { key, one, many }
    }

    /// The counter's key, such as `skipped_draft_or_outbox`.
    pub const fn key(self) -> &'static str {
        self.key
    }

    /// The log line for a count of `n`.
    pub fn line(self, n: u64) -> String {
        if n == 1 {
            self.one.to_string()
        } else {
            self.many.replace("{n}", &n.to_string())
        }
    }
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

/// The log line for one thing a run could not read, `error` being the item
/// and the reason, as [`crate::ExportReport::error`] records them.
pub fn error_line(error: impl std::fmt::Display) -> String {
    format!("error: {error}")
}

/// The log line for one thing a run did that is worth knowing but did not
/// fail, `note` being the item and the text.
pub fn note_line(note: impl std::fmt::Display) -> String {
    format!("note: {note}")
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
