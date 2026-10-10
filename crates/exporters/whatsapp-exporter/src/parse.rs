//! Load KnugiHK WhatsApp-Chat-Exporter single-file JSON (`ChatCollection.to_dict`).

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Top-level JSON: map of JID → chat.
pub(crate) type ChatStoreFile = BTreeMap<String, ChatJson>;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChatJson {
    pub name: Option<String>,
    /// Prefix for relative media `data` paths (iOS often `AppDomainGroup-…/`).
    #[serde(default)]
    pub media_base: Option<String>,
    #[serde(default)]
    pub messages: BTreeMap<String, MessageJson>,
    /// On a group, one entry per person the backup has a member row for,
    /// the owner of the phone included where the backup has one. `null`
    /// on any other chat, and on a group whose member table was absent or
    /// unreadable. An empty list means the table was read and holds no row
    /// for the group.
    #[serde(default)]
    pub members: ForkField<Vec<MemberJson>>,
}

/// One entry of a group's `members`. The fork also writes `lid`, `active`
/// and `admin`, which are not read: an `@lid` id is not a phone number, and
/// the conversation model has no place for the other two.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MemberJson {
    /// As [`MessageJson::sender_jid`].
    #[serde(default)]
    pub jid: Option<String>,
    /// The name the owner gave the member in the address book.
    #[serde(default)]
    pub contact_name: Option<String>,
    /// The name the member typed into their own WhatsApp profile.
    #[serde(default)]
    pub push_name: Option<String>,
}

/// A field only Message Crate's fork of WhatsApp Chat Exporter writes, told
/// apart from one it wrote as `null`. Upstream's JSON lacks the field, and
/// [`load_chat_store`] refuses such a file rather than read it the old way.
#[derive(Debug, Clone, Default)]
pub(crate) enum ForkField<T> {
    /// The JSON has no such field.
    #[default]
    Absent,
    /// The field's value, `None` for `null`.
    Present(Option<T>),
}

impl<T> ForkField<T> {
    /// The value, when the field is there and not `null`.
    pub fn get(&self) -> Option<&T> {
        match self {
            Self::Present(value) => value.as_ref(),
            Self::Absent => None,
        }
    }

    fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ForkField<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(Self::Present)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MessageJson {
    #[serde(default)]
    pub from_me: bool,
    /// Unix seconds (or milliseconds — converted when writing the conversation).
    pub timestamp: Option<f64>,
    pub data: Option<Value>,
    pub sender: Option<String>,
    /// `false` or a media path string.
    #[serde(default)]
    pub media: Value,
    pub mime: Option<String>,
    pub caption: Option<String>,
    #[serde(default)]
    pub sticker: bool,
    pub key_id: Option<Value>,
    /// The whole id the backup stores for the message. On an iPhone
    /// `key_id` is its first 17 characters; on Android the two are equal.
    /// `null` for a message from WhatsApp's own text export. Absent from a
    /// JSON that upstream WhatsApp Chat Exporter wrote: only Message Crate's
    /// fork, `messagecrate/WhatsApp-Chat-Exporter`, writes it, from commit
    /// 96e6b80 on its `main`.
    #[serde(default)]
    pub full_key_id: Option<Value>,
    /// What the message quotes, when it is a reply: the quoted message's
    /// `key_id` as wtsexporter writes it.
    pub reply: Option<Value>,
    /// The whole id of the quoted message, as its `full_key_id`, on a quoted
    /// reply; `null` when the message is not a reply. Absent where
    /// [`Self::full_key_id`] is.
    #[serde(default)]
    pub reply_key_id: Option<Value>,
    #[serde(default)]
    pub reactions: Value,
    /// The sender's phone id (`…@s.whatsapp.net`) whenever the backup can
    /// supply one, else their `@lid` id. `null` unless the message is a
    /// received group message, and on one whose backup names no sender.
    #[serde(default)]
    pub sender_jid: ForkField<String>,
    /// The sender's `@lid` id, when the backup stores the sender under one.
    /// Only its presence is checked: an `@lid` id is not a phone number.
    #[serde(default)]
    pub sender_lid: ForkField<String>,
    /// The name the owner gave the sender in the address book.
    #[serde(default)]
    pub sender_contact_name: ForkField<String>,
    /// The name the sender typed into their own WhatsApp profile.
    #[serde(default)]
    pub sender_push_name: ForkField<String>,
}

impl MessageJson {
    /// True when the message lacks a sender field the fork writes on every
    /// message.
    fn lacks_fork_sender_fields(&self) -> bool {
        self.sender_jid.is_absent()
            || self.sender_lid.is_absent()
            || self.sender_contact_name.is_absent()
            || self.sender_push_name.is_absent()
    }
}

/// Load a wtsexporter `result.json` (one JSON object: JID → chat).
///
/// # Errors
///
/// Returns an error when the file cannot be read or parsed, or when it lacks
/// the sender ids, names and group members that only Message Crate's fork of
/// WhatsApp Chat Exporter writes. Upstream's JSON is refused rather than read
/// without them, because it gives a group message's sender as a name or as
/// digits that may be an internal id, never both.
pub(crate) fn load_chat_store(path: &Path) -> Result<ChatStoreFile> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let store: ChatStoreFile =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    let lacks_fork_fields =
        store
            .iter()
            .filter(|(jid, _)| !jid.starts_with('_'))
            .any(|(_, chat)| {
                chat.members.is_absent()
                    || chat
                        .messages
                        .values()
                        .any(MessageJson::lacks_fork_sender_fields)
            });
    if lacks_fork_fields {
        bail!(
            "{} has no sender ids, names or group members, so Message Crate's fork of \
             WhatsApp Chat Exporter did not write it. Export the backup again with \
             wtsexporter from the messagecrate/WhatsApp-Chat-Exporter release 0.13.0-mc.2 \
             or later.",
            path.display()
        );
    }
    Ok(store)
}

/// True when the message's `media` field is the boolean `true`.
fn has_media_flag(msg: &MessageJson) -> bool {
    matches!(&msg.media, Value::Bool(true))
}

/// True for the text wtsexporter writes when a media file was not in the backup.
fn is_missing_media_placeholder(s: &str) -> bool {
    s.eq_ignore_ascii_case("The media is missing")
}

/// Body text from `data` (string) or caption.
///
/// When `media` is true, wtsexporter stores the file path in `data`, so only
/// `caption` (if any) is treated as message text.
pub(crate) fn message_text(msg: &MessageJson) -> String {
    if has_media_flag(msg) {
        return msg.caption.clone().unwrap_or_default();
    }
    let body = match &msg.data {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    };
    if body.is_empty() {
        msg.caption.clone().unwrap_or_default()
    } else if let Some(cap) = msg.caption.as_deref().filter(|c| !c.is_empty()) {
        if body.contains(cap) {
            body
        } else {
            format!("{body}\n{cap}")
        }
    } else {
        body
    }
}

/// A key field as a string: `None` when it is absent, `null` or blank.
pub(crate) fn key_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => message_ir::trimmed(s).map(str::to_string),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// True when the message quotes another: it names the quoted message by
/// `reply` or by `reply_key_id`.
pub(crate) fn is_reply(msg: &MessageJson) -> bool {
    key_string(msg.reply.as_ref()).is_some() || key_string(msg.reply_key_id.as_ref()).is_some()
}

/// Path hint for an attachment.
///
/// Upstream sets `media: true` and puts the path in `data` (Android/iOS). Older
/// or alternate dumps may put a path string directly in `media`.
pub(crate) fn media_path(msg: &MessageJson) -> Option<&str> {
    match &msg.media {
        Value::String(s) if !s.is_empty() && !is_missing_media_placeholder(s) => Some(s.as_str()),
        Value::Bool(true) => match &msg.data {
            Some(Value::String(s)) if !s.is_empty() && !is_missing_media_placeholder(s) => {
                Some(s.as_str())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Normalize wtsexporter timestamp to Unix milliseconds.
pub(crate) fn timestamp_ms(ts: f64) -> i64 {
    if ts > 9_999_999_999.0 {
        ts as i64
    } else {
        (ts * 1000.0) as i64
    }
}

/// Normalize wtsexporter timestamp to Unix seconds.
pub(crate) fn timestamp_secs(ts: f64) -> i64 {
    timestamp_ms(ts) / 1000
}
