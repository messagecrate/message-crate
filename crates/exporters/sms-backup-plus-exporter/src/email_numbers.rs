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

use crate::flat_eml::group_key;
use crate::types::ParsedMessage;
use message_ir::{HandleType, IrConversationType};
use phone::Handle;
use std::collections::{BTreeSet, HashMap, HashSet};

/// The numbers the archive gives each email address, and how many mails
/// give each.
#[derive(Default)]
pub(crate) struct EmailNumbers {
    seen: HashMap<String, HashMap<String, (Handle, u64)>>,
}

impl EmailNumbers {
    /// Count one mail that gives `email` as `number`.
    pub(crate) fn record(&mut self, email: String, number: Handle) {
        let numbers = self.seen.entry(email).or_default();
        numbers
            .entry(number.key().to_string())
            .or_insert((number, 0))
            .1 += 1;
    }

    /// Each email address with the number most mails give it. Of two numbers
    /// given equally often, the smaller key wins, so the choice never
    /// depends on the order the files were read in.
    pub(crate) fn into_map(self) -> HashMap<String, Handle> {
        self.seen
            .into_iter()
            .filter_map(|(email, numbers)| {
                numbers
                    .into_iter()
                    .max_by(|(a_key, (_, a)), (b_key, (_, b))| a.cmp(b).then(b_key.cmp(a_key)))
                    .map(|(_, (number, _))| (email, number))
            })
            .collect()
    }
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

/// Key every member `msg` names by email address by the number `numbers`
/// gives that address, and work the conversation's key out again. An address
/// with no number keeps its place and is added to `without_number`.
///
/// Two members that turn out to be one person are one member, and a group
/// left with a single member is that person's one-to-one conversation.
pub(crate) fn key_members_by_number(
    msg: &mut ParsedMessage,
    numbers: &HashMap<String, Handle>,
    without_number: &mut BTreeSet<String>,
) {
    let mut by_number = |handle: &Handle| -> Handle {
        if handle.kind() != HandleType::Email {
            return handle.clone();
        }
        if let Some(number) = numbers.get(handle.key()) {
            return number.clone();
        }
        without_number.insert(handle.key().to_string());
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
    if msg.is_group() && participants.len() >= 2 {
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

    fn handle(raw: &str) -> Handle {
        Handle::parse(raw).unwrap()
    }

    /// A person with two numbers is keyed by the one more of their mails
    /// give, and on a tie by the smaller, whatever order the mails came in.
    #[test]
    fn an_address_with_two_numbers_takes_the_one_most_mails_give() {
        for order in [
            ["+14075550111", "+14075550112", "+14075550112"],
            ["+14075550112", "+14075550111", "+14075550112"],
        ] {
            let mut numbers = EmailNumbers::default();
            for number in order {
                numbers.record("carol@example.com".into(), handle(number));
            }
            let map = numbers.into_map();
            assert_eq!(map["carol@example.com"].key(), "+14075550112");
        }
        for order in [
            ["+14075550112", "+14075550111"],
            ["+14075550111", "+14075550112"],
        ] {
            let mut numbers = EmailNumbers::default();
            for number in order {
                numbers.record("carol@example.com".into(), handle(number));
            }
            let map = numbers.into_map();
            assert_eq!(map["carol@example.com"].key(), "+14075550111");
        }
    }
}
