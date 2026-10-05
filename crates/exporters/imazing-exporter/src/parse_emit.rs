//! Conversation key, sender, date, and row-classification helpers for the
//! emitter.

use crate::emit::TransportFamily;
use crate::parse::{RawRow, SourceKind};
use chrono::NaiveDateTime;
use message_csv::Zone;
use message_ir::{ConversationKey, HandleType, IrParticipant};
use phone::Handle;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

impl TransportFamily {
    /// The transport family for a source kind.
    pub(super) fn from_kind(kind: SourceKind) -> Self {
        match kind {
            SourceKind::Messages => Self::Messages,
            SourceKind::WhatsApp => Self::WhatsApp,
        }
    }
}

/// Who one chat session is with: its key, and the name the source gives the
/// chat.
#[derive(Debug)]
pub(super) struct Session {
    pub(super) key: ConversationKey,
    /// For a one-to-one conversation with an address, that address as
    /// [`Handle::parse`] classified it; `None` for a group or a name-only
    /// chat.
    pub(super) address: Option<Handle>,
    /// The session name, or empty when the session name is the chat's address.
    pub(super) contact_name: String,
    /// Members a Messages group's session name lists by a name no row pairs
    /// with an address, by that name.
    pub(super) unresolved_roster_labels: Vec<String>,
    /// For a group, the digest of each row ([`row_digest`]), earliest first.
    /// The first one is the group's vendor id; the rest tell apart two groups
    /// whose earliest rows are the same ([`group_vendor_id`]).
    pub(super) row_digests: Vec<[u8; 32]>,
}

/// Work out the key of one chat session from its rows.
///
/// A Messages session named as a roster ("A & B") is a group, and so is any
/// session in which two or more people wrote. Two addresses that the rows
/// give one Sender Name are one person, so one contact writing from a number
/// and an email address is a one-to-one conversation.
///
/// A group's key is a hash of its earliest row ([`group_vendor_id`]), never
/// who wrote, the session name, or the file it came from: iMazing gives a
/// group no id, and every name it does give changes or repeats.
pub(super) fn session_key(kind: SourceKind, session: &str, rows: &[&RawRow]) -> Session {
    let roster = kind == SourceKind::Messages && session.contains(" & ");
    if roster || people_who_wrote(kind, rows) >= 2 {
        let (members, unresolved_roster_labels) = group_members(roster, session, rows);
        let row_digests = row_digests(rows);
        return Session {
            key: ConversationKey::Group {
                vendor_id: group_vendor_id(&row_digests, 1),
                members,
            },
            address: None,
            contact_name: session.trim().to_string(),
            unresolved_roster_labels,
            row_digests,
        };
    }
    let session = session.trim();
    let named_by_address = address(session).is_some();
    let address = one_to_one_address(session, rows);
    let key = match &address {
        Some(handle) => ConversationKey::OneToOne(handle.key().to_string()),
        None => ConversationKey::NameOnly(session.to_string()),
    };
    Session {
        contact_name: if named_by_address {
            String::new()
        } else {
            session.to_string()
        },
        key,
        address,
        unresolved_roster_labels: Vec::new(),
        row_digests: Vec::new(),
    }
}

/// The address a `Sender ID`, a session name or a roster label holds,
/// classified once by [`Handle::parse`]: an email address, or a number keyed
/// as E.164 (the international phone-number format that starts with +) when
/// unambiguous and as its digits otherwise. `None` for a blank value or a
/// name such as `Trip 2024`, which is no address.
fn address(raw: &str) -> Option<Handle> {
    Handle::parse(raw).filter(|handle| handle.kind() != HandleType::Other)
}

/// True for a row someone other than the account holder wrote.
fn written_by_someone_else(row: &RawRow) -> bool {
    !is_outgoing(&row.msg_type) && !is_notification(&row.msg_type)
}

/// The node `node` is joined to in a union-find map.
fn root(parent: &HashMap<String, String>, node: &str) -> String {
    let mut current = node.to_string();
    while let Some(next) = parent.get(&current).filter(|next| **next != current) {
        current.clone_from(next);
    }
    current
}

/// How many people wrote the received rows.
///
/// In Messages a row's address and its Sender Name are one person, so two
/// addresses that share a name are one person: iMazing puts one contact's
/// number and email address, or two numbers, in one chat. Two different
/// people saved under one name are then counted as one. A WhatsApp account
/// has one number, so in WhatsApp two addresses are always two people, and
/// a name joins only a row that has no address.
fn people_who_wrote(kind: SourceKind, rows: &[&RawRow]) -> usize {
    let names_join_addresses = kind == SourceKind::Messages;
    // Each received row's address and lowercased Sender Name.
    let writers: Vec<(Option<String>, Option<String>)> = rows
        .iter()
        .filter(|row| written_by_someone_else(row))
        .map(|row| {
            let name = row.sender_name.trim().to_lowercase();
            (
                address(&row.sender_id).map(Handle::into_key),
                (!name.is_empty()).then_some(name),
            )
        })
        .collect();
    let names_with_an_address: HashSet<&String> = writers
        .iter()
        .filter(|(address, _)| address.is_some())
        .filter_map(|(_, name)| name.as_ref())
        .collect();
    // Union-find over the addresses and the names the rows give.
    let mut parent: HashMap<String, String> = HashMap::new();
    for (address, name) in &writers {
        let name = name
            .as_ref()
            .filter(|name| {
                names_join_addresses || (address.is_none() && !names_with_an_address.contains(name))
            })
            .map(|name| format!("name:{name}"));
        let address = address.as_ref().map(|a| format!("address:{a}"));
        let nodes: Vec<String> = address.into_iter().chain(name).collect();
        for node in &nodes {
            parent.entry(node.clone()).or_insert_with(|| node.clone());
        }
        if let [a, b] = nodes.as_slice() {
            let (ra, rb) = (root(&parent, a), root(&parent, b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
    }
    parent
        .keys()
        .map(|node| root(&parent, node))
        .collect::<HashSet<_>>()
        .len()
}

/// Where a row sorts when looking for the earliest row: by the date as
/// written, a row with no readable date after every dated row, then by its
/// [`RowFields`]. The field order is the comparison order.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct RowOrder<'a> {
    undated: bool,
    date: Option<NaiveDateTime>,
    fields: RowFields<'a>,
}

impl<'a> RowOrder<'a> {
    fn of(row: &'a RawRow) -> Self {
        let date = naive_date(&row.message_date);
        Self {
            undated: date.is_none(),
            date,
            fields: RowFields::of(row),
        }
    }
}

/// The address of a one-to-one chat: the session name when it is an
/// address, else the first number written in it ("Bob (+13215550100)"),
/// else the address of the earliest received row that has one. New messages
/// do not change it; it changes only when the oldest messages are gone from
/// the export. `None` when the source records no address for the person.
fn one_to_one_address(session: &str, rows: &[&RawRow]) -> Option<Handle> {
    address(session)
        .or_else(|| numbers_in_name(session).into_iter().next())
        .or_else(|| {
            rows.iter()
                .filter(|row| written_by_someone_else(row))
                .filter_map(|row| Some((RowOrder::of(row), address(&row.sender_id)?)))
                .min_by(|a, b| a.0.cmp(&b.0))
                .map(|(_, address)| address)
        })
}

/// A group's members: everyone who wrote, every number in the session name,
/// and for a Messages roster ("A & B & C") every name it lists. A listed
/// name no row pairs with an address has no address anywhere in the export,
/// so it is a member with that name and no handle; the server gives such a
/// person an identity of type `other` holding the name.
///
/// Returns the members with an address sorted by handle, then the members
/// with a name only in roster order, and how many listed names had no
/// address.
fn group_members(
    roster: bool,
    session: &str,
    rows: &[&RawRow],
) -> (Vec<IrParticipant>, Vec<String>) {
    let mut members: BTreeMap<String, IrParticipant> = BTreeMap::new();
    let mut add = |handle: Handle, name: &str| {
        let member = members
            .entry(handle.key().to_string())
            .or_insert_with(|| IrParticipant {
                identity_type: Some(handle.kind()),
                identity: Some(handle.into_key()),
                display_name: None,
            });
        let name = name.trim();
        if member.display_name.is_none() && !name.is_empty() {
            member.display_name = Some(name.to_string());
        }
    };

    // The rows themselves pair a sender's name with their address. That is
    // the only name-to-address mapping the source gives.
    let mut handle_by_sender_name: HashMap<String, Handle> = HashMap::new();
    for row in rows.iter().filter(|row| !is_outgoing(&row.msg_type)) {
        let Some(address) = address(&row.sender_id) else {
            continue;
        };
        let name = row.sender_name.trim();
        if !name.is_empty() {
            handle_by_sender_name
                .entry(name.to_lowercase())
                .or_insert_with(|| address.clone());
        }
        add(address, name);
    }
    for number in numbers_in_name(session) {
        add(number, "");
    }

    let mut unresolved = Vec::new();
    let mut named_only: Vec<IrParticipant> = Vec::new();
    if roster {
        for label in session.split(" & ").map(str::trim) {
            if label.is_empty() {
                continue;
            }
            if let Some(handle) = address(label) {
                add(handle, "");
            } else if let Some(handle) = handle_by_sender_name.get(&label.to_lowercase()) {
                add(handle.clone(), label);
            } else {
                // A member who never wrote, shown by name: the export holds
                // no address for them.
                let already = named_only.iter().any(|member| {
                    member
                        .display_name
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(label))
                });
                if !already {
                    unresolved.push(label.to_string());
                    named_only.push(IrParticipant {
                        identity: None,
                        display_name: Some(label.to_string()),
                        identity_type: None,
                    });
                }
            }
        }
    }
    let mut out: Vec<IrParticipant> = members.into_values().collect();
    out.extend(named_only);
    (out, unresolved)
}

/// The columns that identify a row, as the CSV writes them. The field order
/// is the comparison order.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct RowFields<'a> {
    message_date: &'a str,
    msg_type: &'a str,
    sender_id: &'a str,
    text: &'a str,
    attachment: &'a str,
}

impl<'a> RowFields<'a> {
    fn of(row: &'a RawRow) -> Self {
        Self {
            message_date: &row.message_date,
            msg_type: &row.msg_type,
            sender_id: &row.sender_id,
            text: &row.text,
            attachment: &row.attachment,
        }
    }
}

/// SHA-256 of a row's [`RowFields`], joined by the ASCII unit separator,
/// which no CSV cell holds.
fn row_digest(fields: &RowFields<'_>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for (index, field) in [
        fields.message_date,
        fields.msg_type,
        fields.sender_id,
        fields.text,
        fields.attachment,
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            hasher.update([0x1f]);
        }
        hasher.update(field.as_bytes());
    }
    hasher.finalize().into()
}

/// The [`row_digest`] of every row, earliest first ([`RowOrder`]). Where
/// several rows share the earliest time, the smallest of them comes first,
/// so the order does not depend on the order the CSV writes them in.
fn row_digests(rows: &[&RawRow]) -> Vec<[u8; 32]> {
    let mut ordered: Vec<RowOrder<'_>> = rows.iter().map(|row| RowOrder::of(row)).collect();
    ordered.sort();
    ordered
        .iter()
        .map(|order| row_digest(&order.fields))
        .collect()
}

/// A group's vendor id, in lowercase hex. With `depth` 1 it is the
/// [`row_digest`] of the conversation's earliest row. A greater `depth`
/// hashes the digests of the earliest `depth` rows (all of them when there
/// are fewer), which tells apart two groups whose earliest rows are the same.
///
/// Within one export one CSV file is one conversation, and across exports
/// its first message stays the same while new ones arrive, so the id stays
/// the same when someone new writes, a member is renamed, or the session
/// name changes. Where several rows share the earliest time, the smallest of
/// them is taken, so the choice does not depend on row order. The date is
/// read as written, with no time zone, so the id does not depend on the zone
/// the export is converted in.
///
/// The id changes when the oldest messages are gone from the phone or an
/// export covers only a date range: the group then comes in as a second
/// conversation, never merged with another group.
pub(super) fn group_vendor_id(row_digests: &[[u8; 32]], depth: usize) -> String {
    if depth <= 1 {
        return hex::encode(row_digests.first().copied().unwrap_or_else(|| {
            row_digest(&RowFields {
                message_date: "",
                msg_type: "",
                sender_id: "",
                text: "",
                attachment: "",
            })
        }));
    }
    let mut hasher = Sha256::new();
    for digest in row_digests.iter().take(depth) {
        hasher.update(digest);
    }
    hex::encode(hasher.finalize())
}

/// The vendor id of a group whose rows are the same as another group's,
/// which no number of rows tells apart: every row's digest, then the session
/// name. It changes when the group gets a row of its own or is renamed.
pub(super) fn group_vendor_id_with_name(row_digests: &[[u8; 32]], session_name: &str) -> String {
    let mut hasher = Sha256::new();
    for digest in row_digests {
        hasher.update(digest);
    }
    hasher.update([0x1f]);
    hasher.update(session_name.as_bytes());
    hex::encode(hasher.finalize())
}

/// An iMazing date string (`YYYY-MM-DD HH:MM[:SS]`, no zone) as written.
fn naive_date(raw: &str) -> Option<NaiveDateTime> {
    let raw = raw.trim();
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M"))
        .ok()
}

/// Parse an iMazing date string (`YYYY-MM-DD HH:MM[:SS]`, no zone) in `zone`
/// into Unix seconds. iMazing records whole seconds only. [`Zone::instant`]
/// settles a wall clock that a daylight-saving change repeats or skips, so
/// every parsable row is kept.
pub(super) fn parse_message_date(raw: &str, zone: Zone) -> Option<i64> {
    Some(zone.instant(naive_date(raw)?)?.timestamp())
}

/// True for rows the exporter treats as sent (`outgoing`/`sent` types).
pub(super) fn is_outgoing(msg_type: &str) -> bool {
    matches!(
        msg_type.trim().to_ascii_lowercase().as_str(),
        "outgoing" | "sent"
    )
}

/// True for iMazing's `Notification` message type.
pub(super) fn is_notification(msg_type: &str) -> bool {
    msg_type.trim().eq_ignore_ascii_case("notification")
}

/// The fewest digits a number written inside a name has, so `Party +1`
/// names no number.
const MIN_DIGITS_IN_NAME: usize = 4;

/// Every number written inside a name as `+` and its digits, as iMazing
/// writes "Bob (+13215550100)", each classified by [`Handle::parse`], in
/// the order the name gives them and without repeats.
fn numbers_in_name(name: &str) -> Vec<Handle> {
    let mut out: Vec<Handle> = Vec::new();
    for (start, _) in name.match_indices('+') {
        let digits = name[start + 1..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if digits < MIN_DIGITS_IN_NAME {
            continue;
        }
        if let Some(number) = address(&name[start..=start + digits])
            && !out.iter().any(|held| held.key() == number.key())
        {
            out.push(number);
        }
    }
    out
}

/// The sender handle and display name for a row: empty for outgoing, and
/// otherwise the row's own Sender ID and Sender Name.
///
/// Only a one-to-one chat with a number or short code fills a received row
/// that names no sender: it can only be from the chat's one other person. A
/// group's row with no sender has no sender, and a chat keyed by a name has
/// no address to give.
pub(super) fn resolve_sender(
    row: &RawRow,
    is_from_me: bool,
    is_notification: bool,
    session: &Session,
) -> (String, String) {
    if is_from_me {
        return (String::new(), String::new());
    }
    let address = address(&row.sender_id).map(Handle::into_key);
    if is_notification {
        // Keep any available identity from the notification row; often empty.
        return (
            address.unwrap_or_default(),
            row.sender_name.trim().to_string(),
        );
    }
    let handle = address.unwrap_or_else(|| match &session.address {
        Some(peer) if peer.kind() == HandleType::Phone => peer.key().to_string(),
        _ => String::new(),
    });
    let mut display = row.sender_name.trim().to_string();
    if display.is_empty() && !session.key.is_group() {
        display.clone_from(&session.contact_name);
    }
    (handle, display)
}
