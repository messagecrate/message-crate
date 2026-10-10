//! Which conversation a message lands in: its kind, key, title, and the
//! name its contact name gives.

use phone::Handle;

/// Individual or group conversation classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConversationKind {
    /// One-to-one conversation (default).
    #[default]
    Individual,
    /// Group conversation with multiple participants.
    Group,
}

/// The person a raw `contact_name` names, or `None` when it names nobody.
///
/// SMS Backup & Restore writes `null` or `(Unknown)` where the phone has no
/// contact for the address, so an empty value and those two, compared without
/// case, give no name. In a group the value is the members' names joined by
/// `, `, which names the group rather than the sender, so a group gives no name.
pub fn contact_name(raw: &str, kind: ConversationKind) -> Option<&str> {
    if kind == ConversationKind::Group {
        return None;
    }
    let value = raw.trim();
    let placeholder = value.is_empty()
        || value.eq_ignore_ascii_case("null")
        || value.eq_ignore_ascii_case("(Unknown)");
    (!placeholder).then_some(value)
}

/// Where an MMS lands: a one-to-one conversation keyed by the peer's handle
/// key, or a group keyed by the sorted peer set.
pub(crate) struct MmsConversation {
    /// The peer's handle key, or the group key.
    pub(crate) chat_key: String,
    /// Individual or group.
    pub(crate) kind: ConversationKind,
    /// The generated title of a group; `None` for an individual.
    pub(crate) group_title: Option<String>,
    /// Each peer with the name the element gives it, if any.
    pub(crate) participants: Vec<(Handle, Option<String>)>,
}

impl MmsConversation {
    /// `peers` is sorted and non-empty; `raw_name` is the element's contact
    /// name as written, which names the one peer of an individual
    /// conversation and nobody in a group.
    pub(crate) fn for_peers(mut peers: Vec<Handle>, raw_name: &str) -> Self {
        if peers.len() == 1 {
            let peer = peers.remove(0);
            let name = contact_name(raw_name, ConversationKind::Individual).map(String::from);
            return Self {
                chat_key: peer.key().to_string(),
                kind: ConversationKind::Individual,
                group_title: None,
                participants: vec![(peer, name)],
            };
        }
        // Group chats are keyed by the participant set because the format
        // has no stable thread ID. When the roster changes (someone is added
        // or removed), messages before and after the change land in
        // different conversations, an inherent limitation of the source,
        // documented at
        // https://messagecrate.app/docs/developer/formats/sms-backup-restore/mapping/.
        let keys: Vec<String> = peers.iter().map(|p| p.key().to_string()).collect();
        let (chat_key, title) = phone::group_chat_id("group-", &keys);
        Self {
            chat_key,
            kind: ConversationKind::Group,
            group_title: Some(title),
            participants: peers.into_iter().map(|p| (p, None)).collect(),
        }
    }
}
