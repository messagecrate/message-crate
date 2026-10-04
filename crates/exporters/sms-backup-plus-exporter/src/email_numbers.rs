//! Group members keyed by number where SMS Backup+ named them by email
//! address (#1545).
//!
//! SMS Backup+ writes a contact who had an email address on the phone as that
//! address in `From` and `To`, and anyone else as their number, so the same
//! person's key in a group would change with the phone's contact book at the
//! moment each mail was backed up. A one-to-one mail gives both the address
//! and the number (`flat_eml::email_and_number`), so the whole archive is
//! read first, and each group member it names by an address is then keyed
//! by the number that address stands for.

use crate::flat_eml::{group_key, names_a_group};
use crate::types::ParsedMessage;
use message_ir::{HandleType, IrConversationType};
use phone::Handle;
use std::collections::{BTreeSet, HashMap, HashSet};

/// An email address and the number one mail says it stands for.
#[derive(Debug, Clone)]
pub(crate) struct EmailNumber {
    /// The email address's handle key.
    pub email: String,
    /// The number in the mail's `X-smssync-address`.
    pub number: Handle,
}

/// The numbers the archive gives each email address.
#[derive(Default)]
pub(crate) struct EmailNumbers {
    seen: HashMap<String, HashMap<String, Handle>>,
}

impl EmailNumbers {
    /// Note one mail that gives an email address as a number.
    pub(crate) fn record(&mut self, pair: EmailNumber) {
        self.seen
            .entry(pair.email)
            .or_default()
            .entry(pair.number.key().to_string())
            .or_insert(pair.number);
    }

    /// The number of each email address the archive gives exactly one
    /// number. An address it gives two or more is left out: one contact card
    /// can hold two people's numbers under one address, such as a family's,
    /// and choosing one would credit one person's messages to the other.
    pub(crate) fn into_numbers(self) -> NumbersByEmail {
        let mut numbers = NumbersByEmail::default();
        for (email, by_key) in self.seen {
            if by_key.len() == 1 {
                let number = by_key.into_values().next().expect("one number");
                numbers.by_email.insert(email, number);
            } else {
                numbers.with_several_numbers.insert(email);
            }
        }
        numbers
    }
}

/// What the archive says about each email address.
#[derive(Default)]
pub(crate) struct NumbersByEmail {
    /// The one number each of these addresses stands for.
    by_email: HashMap<String, Handle>,
    /// Addresses the archive gives two or more numbers.
    with_several_numbers: HashSet<String>,
}

/// The group members that keep their email address as their key.
#[derive(Default)]
pub(crate) struct KeptByEmail {
    /// Addresses the archive gives no number.
    pub without_number: BTreeSet<String>,
    /// Addresses the archive gives two or more numbers.
    pub several_numbers: BTreeSet<String>,
}

/// True when `msg` names a group member by email address: a group's member
/// or sender, or the sender of a group mail that did not name the owner and
/// was filed under them.
pub(crate) fn names_a_member_by_email(msg: &ParsedMessage) -> bool {
    (msg.is_group() || msg.owner_not_named)
        && msg
            .participants
            .iter()
            .chain(&msg.sender)
            .any(|h| h.kind() == HandleType::Email)
}

/// Key every member `msg` names by email address by the one number
/// `numbers` gives that address, and work the conversation's key out again.
/// An address with no number, or with several, keeps its place and is noted
/// in `kept`.
///
/// Two members that turn out to be one person are one member, and a group
/// left with a single member is that person's one-to-one conversation.
pub(crate) fn key_members_by_number(
    msg: &mut ParsedMessage,
    numbers: &NumbersByEmail,
    kept: &mut KeptByEmail,
) {
    let mut by_number = |handle: &Handle| -> Handle {
        if handle.kind() != HandleType::Email {
            return handle.clone();
        }
        if let Some(number) = numbers.by_email.get(handle.key()) {
            return number.clone();
        }
        let key = handle.key().to_string();
        if numbers.with_several_numbers.contains(&key) {
            kept.several_numbers.insert(key);
        } else {
            kept.without_number.insert(key);
        }
        handle.clone()
    };
    let mut seen = HashSet::new();
    let participants: Vec<Handle> = msg
        .participants
        .iter()
        .map(&mut by_number)
        .filter(|h| seen.insert(h.key().to_string()))
        .collect();
    msg.sender = msg.sender.as_ref().map(&mut by_number);
    if msg.is_group() && names_a_group(&participants) {
        let (chat_key, title) = group_key(&participants);
        msg.chat_key = chat_key;
        msg.group_title = Some(title);
    } else if let Some(peer) = participants.first() {
        msg.chat_key = peer.key().to_string();
        msg.conversation_type = IrConversationType::Individual;
        msg.group_title = None;
    }
    msg.participants = participants;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(number: &str) -> EmailNumber {
        EmailNumber {
            email: "smiths@example.com".into(),
            number: Handle::parse(number).unwrap(),
        }
    }

    /// One address on a card two people share is no one's number: it is
    /// never mapped, however many more mails give one of them.
    #[test]
    fn an_address_with_two_numbers_is_not_given_either() {
        let mut numbers = EmailNumbers::default();
        for number in ["+14075550111", "+14075550111", "+14075550112"] {
            numbers.record(pair(number));
        }
        let numbers = numbers.into_numbers();
        assert!(numbers.by_email.is_empty());
        assert!(numbers.with_several_numbers.contains("smiths@example.com"));
    }

    /// Two spellings of one number are one number.
    #[test]
    fn an_address_given_one_number_twice_is_that_number() {
        let mut numbers = EmailNumbers::default();
        numbers.record(pair("+14075550111"));
        numbers.record(pair("4075550111"));
        let numbers = numbers.into_numbers();
        assert_eq!(numbers.by_email["smiths@example.com"].key(), "+14075550111");
    }
}
