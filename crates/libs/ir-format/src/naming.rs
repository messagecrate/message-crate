//! How Message Crate names the files it writes for a conversation: the
//! stem every format shares (`<stem>.jsonl`, `<stem>.csv`, `<stem>/` of EML
//! files, `<stem>.mbox`), and the suffix that keeps two conversations from
//! sharing one file.

use message_ir::{ConversationDocument, trimmed};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// The file name stem of `doc`, without an extension, used for CSV, JSON,
/// JSON Lines, and mail files and directories.
pub fn filename_stem(doc: &ConversationDocument) -> String {
    let handles: Vec<String> = doc
        .conversation
        .participants
        .iter()
        .filter_map(|p| p.identity.clone())
        .collect();
    conversation_stem(
        doc.conversation.conversation_type.as_str(),
        &doc.conversation.chat_identifier,
        doc.conversation.group_title.as_deref(),
        &handles,
        doc.packaging_stem_suffix.as_deref(),
    )
}

/// Give every document a file name no other document in `docs` has.
///
/// Two conversations can reduce to one [`filename_stem`]:
/// two groups with the same title, two untitled groups with the same people,
/// or two names that differ only in case on a disk that ignores case. Written
/// as they are, the second file would replace the first and lose its
/// messages. Each document in such a clash gets a short suffix taken from its
/// chat identifier, so the names are the same on every run over the same
/// backup and a resumed run finds its files.
///
/// # Errors
///
/// Names the file when two documents still share one after that (the same
/// chat identifier twice), so the caller can refuse rather than write one
/// over the other.
pub fn give_each_document_its_own_file(
    docs: &mut [&mut ConversationDocument],
) -> Result<(), String> {
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, doc) in docs.iter().enumerate() {
        by_name
            .entry(filename_stem(doc).to_lowercase())
            .or_default()
            .push(i);
    }
    for clash in by_name.values().filter(|indexes| indexes.len() > 1) {
        for &i in clash {
            let doc = &mut *docs[i];
            let digest = hex::encode(Sha256::digest(doc.conversation.chat_identifier.as_bytes()));
            let suffix = doc.packaging_stem_suffix.take().unwrap_or_default();
            doc.packaging_stem_suffix = Some(format!("{suffix}__{}", &digest[..8]));
        }
    }
    let mut names = HashSet::new();
    for doc in docs.iter() {
        let stem = filename_stem(doc);
        if !names.insert(stem.to_lowercase()) {
            return Err(format!("two conversations would both be written to {stem}"));
        }
    }
    Ok(())
}

/// Max peer phones included in an untitled group filename stem.
const GROUP_FILENAME_MAX_PHONES: usize = 10;

/// A file stem with anything but letters, digits, `-`, `_`, and `+` replaced by `_`.
fn sanitize_stem(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '+' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// True for a handle that is a phone number: an optional `+` then digits.
fn is_phone_handle(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if let Some(rest) = value.strip_prefix('+') {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
    } else {
        value.chars().all(|c| c.is_ascii_digit())
    }
}

/// The stem with `suffix` appended when there is one.
fn with_suffix(stem: &str, suffix: Option<&str>) -> String {
    match suffix {
        Some(s) if !s.is_empty() => format!("{stem}{s}"),
        _ => stem.to_string(),
    }
}

/// Standard per-conversation filename stem (no extension — callers append
/// `.csv`, `.jsonl`, …).
///
/// - Individual → `sanitize_stem(chat_id)` (+ optional suffix); `unknown`
///   when the chat id is empty, so no file is named only by its extension
/// - Group with a real `group_title` → sanitized title
/// - Untitled group → `group_+A_+B_…` (sorted unique E.164, max 10);
///   if more than 10 peers, append `_<16 hex>` of SHA-256 over the full roster
/// - Untitled group with empty roster → `group_unknown` (or hash of `chat_id`)
fn conversation_stem(
    conversation_type: &str,
    chat_id: &str,
    group_title: Option<&str>,
    participant_e164s: &[String],
    suffix: Option<&str>,
) -> String {
    let is_group = conversation_type.eq_ignore_ascii_case("group");
    if !is_group {
        let mut stem = sanitize_stem(chat_id);
        if stem.is_empty() {
            stem = "unknown".to_string();
        }
        return with_suffix(&stem, suffix);
    }

    if let Some(title) = group_title.and_then(trimmed) {
        let stem = sanitize_stem(title);
        if !stem.is_empty() && !stem.chars().all(|c| c == '_') {
            return with_suffix(&stem, suffix);
        }
    }

    let phones = unique_sorted_phone_handles(participant_e164s);

    if phones.is_empty() {
        let stem = if chat_id.trim().is_empty() {
            "group_unknown".to_string()
        } else {
            let digest = hex::encode(Sha256::digest(chat_id.as_bytes()));
            format!("group_{}", &digest[..16])
        };
        return with_suffix(&stem, suffix);
    }

    let mut stem = "group".to_string();
    for phone in phones.iter().take(GROUP_FILENAME_MAX_PHONES) {
        stem.push('_');
        stem.push_str(phone);
    }
    if phones.len() > GROUP_FILENAME_MAX_PHONES {
        let joined = phones.join("|");
        let digest = hex::encode(Sha256::digest(joined.as_bytes()));
        stem.push('_');
        stem.push_str(&digest[..16]);
    }
    with_suffix(&stem, suffix)
}

/// Trim, keep phone-looking handles, sort, and drop duplicates.
fn unique_sorted_phone_handles(participant_e164s: &[String]) -> Vec<String> {
    let mut phones: Vec<String> = participant_e164s
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| is_phone_handle(p))
        .collect();
    phones.sort();
    phones.dedup();
    phones
}

#[cfg(test)]
mod tests {
    use super::conversation_stem;

    #[test]
    fn individual_uses_chat_id() {
        assert_eq!(
            conversation_stem("individual", "+15550112", None, &[], None),
            "+15550112"
        );
    }

    #[test]
    fn individual_with_an_empty_chat_id_is_unknown() {
        assert_eq!(
            conversation_stem("individual", "", None, &[], None),
            "unknown"
        );
        assert_eq!(conversation_stem("individual", "  ", None, &[], None), "__");
        assert_eq!(
            conversation_stem("individual", "", None, &[], Some("__whatsapp")),
            "unknown__whatsapp"
        );
    }

    #[test]
    fn group_with_title_uses_title() {
        assert_eq!(
            conversation_stem("group", "chat-x", Some("Family Chat"), &[], None),
            "Family_Chat"
        );
    }

    #[test]
    fn group_with_a_one_word_title_uses_it_and_not_the_phones() {
        let peers = vec!["+15555550100".into()];
        assert_eq!(
            conversation_stem("group", "chat-x", Some("Family"), &peers, None),
            "Family"
        );
    }

    #[test]
    fn untitled_group_lists_sorted_phones() {
        let peers = vec!["+18285550100".into(), "+14075550100".into()];
        assert_eq!(
            conversation_stem("group", "chat-group-x", None, &peers, None),
            "group_+14075550100_+18285550100"
        );
    }

    #[test]
    fn untitled_group_over_ten_appends_hash() {
        let peers: Vec<String> = (100..=112).map(|i| format!("+1555555{i:04}")).collect();
        let stem = conversation_stem("group", "chat-x", None, &peers, None);
        assert!(stem.starts_with("group_+15555550100_"));
        assert!(stem.contains("+15555550109_"));
        assert!(!stem.contains("+15555550110"));
        let hash = stem.rsplit('_').next().unwrap();
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            stem,
            conversation_stem("group", "other-id", None, &peers, None)
        );
    }

    #[test]
    fn whatsapp_suffix() {
        let peers = vec!["+15555550100".into()];
        assert_eq!(
            conversation_stem("group", "x", None, &peers, Some("__whatsapp")),
            "group_+15555550100__whatsapp"
        );
    }

    #[test]
    fn none_title_uses_phones_not_synthetic() {
        let peers = vec!["+15555550100".into()];
        assert_eq!(
            conversation_stem("group", "chat-group-x", None, &peers, None),
            "group_+15555550100"
        );
    }
}
