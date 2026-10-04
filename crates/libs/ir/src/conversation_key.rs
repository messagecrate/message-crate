//! What a conversation is keyed by.
//!
//! A conversation's chat id is its key: it is part of every message's
//! `guid`, and the server recognises the conversation on the next import by
//! it. [`ConversationKey`] says what kind of key a conversation has, so that
//! a group's key is never a person's address and a group's members are never
//! read back out of its id.
//!
//! Every key that is not an address starts with a prefix of its own:
//! `group:` for a group, `name:` for a person known only by name, and
//! `nameless:` for the conversation that names nobody. So a name can never
//! take a group's key, an address's key, or the key of the conversation that
//! names nobody.

use crate::IrParticipant;

/// What every group chat id starts with, so a group's key can never equal an
/// address.
pub const GROUP_CHAT_ID_PREFIX: &str = "group:";

/// What every name-only chat id starts with, so a name's key can never equal
/// an address, a group's key, or [`NAMELESS_CHAT_ID`].
pub const NAME_CHAT_ID_PREFIX: &str = "name:";

/// The chat id of the conversation whose rows name nobody: no address and no
/// name. It has a prefix of its own, so a person named "unknown" or
/// "nameless" keeps a conversation apart from it.
pub const NAMELESS_CHAT_ID: &str = "nameless:";

/// The key of one conversation.
#[derive(Debug, Clone)]
pub enum ConversationKey {
    /// A one-to-one conversation, keyed by the other person's address.
    OneToOne(String),
    /// A group, keyed by an id the exporter takes from the source and never
    /// from who wrote in it. The members are data: the key does not change
    /// when they do.
    Group {
        /// The group's id, unique within its source.
        vendor_id: String,
        /// The people in the group other than the account holder.
        members: Vec<IrParticipant>,
    },
    /// A one-to-one conversation with a person the source names and records
    /// no address for, keyed by the name. The name must not be blank: a
    /// conversation that names nobody is keyed [`NAMELESS_CHAT_ID`].
    NameOnly(String),
}

impl ConversationKey {
    /// The conversation's chat id: the address for [`Self::OneToOne`],
    /// `group:` and the vendor id for [`Self::Group`], and `name:` and the
    /// whole trimmed name for [`Self::NameOnly`].
    ///
    /// The name is kept whole rather than made filename-safe, because a
    /// filename-safe stem gives "张伟" and "李娜" one key, and "Ana Lee" and
    /// "Ana.Lee" another. The file name is made from the chat id later, by
    /// [`ConversationDocument::filename_stem`](crate::ConversationDocument::filename_stem).
    pub fn chat_id(&self) -> String {
        match self {
            Self::OneToOne(handle) => handle.clone(),
            Self::Group { vendor_id, .. } => format!("{GROUP_CHAT_ID_PREFIX}{vendor_id}"),
            Self::NameOnly(name) => format!("{NAME_CHAT_ID_PREFIX}{}", name.trim()),
        }
    }

    /// Whether the conversation is a group.
    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group { .. })
    }

    /// Whether the conversation is keyed by a person's name.
    pub fn is_name_only(&self) -> bool {
        matches!(self, Self::NameOnly(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> String {
        ConversationKey::NameOnly(value.into()).chat_id()
    }

    /// Names that a filename-safe stem would merge keep two keys: two names
    /// in a script other than Latin, and two that differ only in a dot.
    #[test]
    fn two_different_names_never_share_a_key() {
        assert_ne!(name("张伟"), name("李娜"));
        assert_ne!(name("Ana Lee"), name("Ana.Lee"));
    }

    /// A name key is the whole trimmed name behind `name:`.
    #[test]
    fn a_name_key_is_the_trimmed_name_behind_its_prefix() {
        assert_eq!(name("  José Ramírez \t"), "name:José Ramírez");
    }

    /// A person named "unknown" or "nameless" is not the conversation that
    /// names nobody.
    #[test]
    fn a_name_key_never_equals_the_key_for_no_name() {
        for value in ["unknown", "nameless", "nameless:", ""] {
            assert_ne!(name(value), NAMELESS_CHAT_ID, "{value:?}");
        }
    }

    /// A name shaped like an address, or like a group's key, keeps a key of
    /// its own.
    #[test]
    fn a_name_key_never_equals_an_address_or_a_group() {
        for value in ["AMAZON", "+15555550111", "ana@example.com"] {
            assert_ne!(
                name(value),
                ConversationKey::OneToOne(value.into()).chat_id()
            );
        }
        let group = ConversationKey::Group {
            vendor_id: "1".into(),
            members: Vec::new(),
        };
        assert_ne!(name("group:1"), group.chat_id());
    }

    /// A group's chat id is in a namespace of its own, so even a vendor id
    /// shaped like an address never gives a group a person's key.
    #[test]
    fn a_group_chat_id_never_equals_an_address() {
        let address = "+15555550111";
        let group = ConversationKey::Group {
            vendor_id: address.to_string(),
            members: Vec::new(),
        };
        assert_ne!(
            group.chat_id(),
            ConversationKey::OneToOne(address.into()).chat_id()
        );
        assert_eq!(group.chat_id(), "group:+15555550111");
    }
}
