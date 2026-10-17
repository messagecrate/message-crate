//! The `msg_box` value of an `<mms>` element: the phone box the message sat
//! in. The reader treats a sent message apart from a received one, and skips
//! a message that was never sent.

/// The phone box of an `<mms>` element, read from its `msg_box` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MmsBox {
    /// A message the owner received.
    Inbox,
    /// A message the owner sent.
    Sent,
    /// A draft the owner had not sent.
    Draft,
    /// A message waiting in the outbox.
    Outbox,
    /// A message the phone failed to send.
    Failed,
    /// A message queued to be sent.
    Queued,
}

impl MmsBox {
    /// Reads a `msg_box` attribute, ignoring surrounding whitespace. A value
    /// outside the phone's boxes gives `None`.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "1" => Some(Self::Inbox),
            "2" => Some(Self::Sent),
            "3" => Some(Self::Draft),
            "4" => Some(Self::Outbox),
            "5" => Some(Self::Failed),
            "6" => Some(Self::Queued),
            _ => None,
        }
    }

    /// Whether the owner sent the message.
    pub(crate) fn is_sent(self) -> bool {
        self == Self::Sent
    }

    /// Whether the message was never sent: a draft, or a message in the
    /// outbox, failed, or queued. The reader skips these.
    pub(crate) fn is_never_sent(self) -> bool {
        matches!(
            self,
            Self::Draft | Self::Outbox | Self::Failed | Self::Queued
        )
    }
}

#[cfg(test)]
mod tests {
    use super::MmsBox;

    #[test]
    fn parse_reads_each_box_and_refuses_anything_else() {
        assert_eq!(MmsBox::parse(" 2 "), Some(MmsBox::Sent));
        assert_eq!(MmsBox::parse("1"), Some(MmsBox::Inbox));
        assert_eq!(MmsBox::parse("6"), Some(MmsBox::Queued));
        assert_eq!(MmsBox::parse(""), None);
        assert_eq!(MmsBox::parse("7"), None);
    }

    #[test]
    fn only_draft_outbox_failed_and_queued_are_never_sent() {
        let never_sent: Vec<&str> = ["1", "2", "3", "4", "5", "6"]
            .into_iter()
            .filter(|v| MmsBox::parse(v).is_some_and(MmsBox::is_never_sent))
            .collect();
        assert_eq!(never_sent, ["3", "4", "5", "6"]);
    }
}
