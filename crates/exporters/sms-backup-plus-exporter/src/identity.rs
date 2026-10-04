//! The chat a parsed EML message belongs to, and its time in milliseconds.
//!
//! Which copies of a message are one message is decided by the shared
//! projection (`message_ir::one_copy_per_message`), not here.

use crate::types::ParsedMessage;
use message_ir::{ConversationKey, NAMELESS_CHAT_ID};

/// Who this chat is with, as a stable string (the peer's handle key, or
/// `chat-…` for groups).
///
/// When the mail names the other party but records no address, the chat is
/// keyed by that name through [`ConversationKey::NameOnly`], so each person
/// gets their own conversation. Collapsing them all into one chat would
/// merge unrelated people; the server resolves the name against contacts on
/// import. The name key carries a prefix no address has, so a person named
/// "AMAZON" never shares the chat of the sender `AMAZON`. The file name is
/// made from this key later, by `ConversationDocument::filename_stem`.
///
/// A mail that names nobody never gets here: the parser skips it and counts
/// it as a parse error. Should one arrive, it is keyed [`NAMELESS_CHAT_ID`],
/// the key every exporter gives the conversation that names nobody, so it
/// can never take a person's key.
pub(crate) fn chat_id_for(msg: &ParsedMessage) -> String {
    if msg.is_group() {
        format!("chat-{}", msg.chat_key)
    } else if msg.chat_key.is_empty() {
        name_only_key(msg).map_or_else(|| NAMELESS_CHAT_ID.to_string(), |key| key.chat_id())
    } else {
        msg.chat_key.clone()
    }
}

/// The key of a chat with a peer the mail named and recorded no address
/// for. `None` when the mail is a group's, records an address, or has no
/// usable name either.
fn name_only_key(msg: &ParsedMessage) -> Option<ConversationKey> {
    if msg.is_group() || !msg.chat_key.is_empty() {
        return None;
    }
    let name = msg.name_alias.as_deref().map_or("", str::trim);
    if name.is_empty() {
        return None;
    }
    Some(ConversationKey::NameOnly(name.to_string()))
}

/// Message time as milliseconds since 1970.
pub(crate) fn timestamp_ms(timestamp_secs: f64) -> i64 {
    (timestamp_secs * 1000.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_msg(address: &str, ts: f64, is_from_me: bool, text: &str) -> ParsedMessage {
        let peer = phone::Handle::parse(address);
        ParsedMessage {
            chat_key: peer
                .as_ref()
                .map(|p| p.key().to_string())
                .unwrap_or_default(),
            conversation_type: message_ir::IrConversationType::Individual,
            group_title: None,
            participants: peer.iter().cloned().collect(),
            timestamp_secs: ts,
            has_milliseconds: true,
            is_from_me,
            sender: peer.filter(|_| !is_from_me),
            text: text.into(),
            attachments: vec![],
            unreadable_parts: 0,
            name_alias: None,
            smssync_id: None,
            android_type: String::new(),
            eml_path: String::new(),
            owner_not_named: false,
        }
    }

    #[test]
    fn a_mail_that_names_nobody_is_keyed_nameless() {
        let msg = sample_msg("", 1_609_459_200.0, false, "hi");
        assert_eq!(chat_id_for(&msg), NAMELESS_CHAT_ID);
    }

    /// A person named "unknown" is not the chat of the mails that name
    /// nobody.
    #[test]
    fn a_person_named_unknown_has_a_chat_of_their_own() {
        let nobody = sample_msg("", 1.0, false, "hi");
        let mut named = nobody.clone();
        named.name_alias = Some("unknown".into());
        assert_ne!(chat_id_for(&named), chat_id_for(&nobody));
    }

    /// A person named "AMAZON" with no address is not the sender `AMAZON`,
    /// and a person named like a number is not that number.
    #[test]
    fn a_name_never_shares_the_chat_of_an_address_it_spells() {
        for address in ["AMAZON", "+15555550101"] {
            let from_address = sample_msg(address, 1.0, false, "hi");
            let mut named = sample_msg("", 1.0, false, "hi");
            named.name_alias = Some(address.into());
            assert_ne!(chat_id_for(&named), chat_id_for(&from_address), "{address}");
        }
    }

    #[test]
    fn two_people_known_only_by_name_have_two_chats() {
        let mut a = sample_msg("", 1.0, false, "hi");
        a.name_alias = Some("张伟".into());
        let mut b = a.clone();
        b.name_alias = Some("李娜".into());
        assert_ne!(chat_id_for(&a), chat_id_for(&b));
        assert_ne!(chat_id_for(&a), NAMELESS_CHAT_ID);
    }

    #[test]
    fn names_that_differ_only_in_an_accent_have_two_chats() {
        let mut a = sample_msg("", 1.0, false, "hi");
        a.name_alias = Some("José".into());
        let mut b = a.clone();
        b.name_alias = Some("Josè".into());
        assert_ne!(chat_id_for(&a), chat_id_for(&b));
    }

    #[test]
    fn a_chat_known_only_by_name_is_keyed_on_the_trimmed_name() {
        let mut msg = sample_msg("", 1.0, false, "hi");
        msg.name_alias = Some("  José Ramírez \t".into());
        assert_eq!(chat_id_for(&msg), "name:José Ramírez");
    }
}
