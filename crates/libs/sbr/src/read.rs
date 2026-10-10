//! Streaming reader for SMS Backup & Restore XML: it walks the elements and
//! turns each `<sms>` and `<mms>` into a [`Record`].

use anyhow::{Context, Result};
use phone::{Handle, OwnerHandleSet};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;
use std::path::Path;

use crate::addresses::{MmsAddr, addr, address_handle, mms_participants, mms_peers, mms_sender};
use crate::conversations::{ConversationKind, MmsConversation, contact_name};
use crate::mms_box::MmsBox;
use crate::parts::{
    AttachmentBlob, DecodedPartData, MmsPart, decode_part_data, mms_body, part, part_fields,
};
use crate::xml::{attrs, btree, decode_body, get};

/// Serde-tagged raw source bag (`kind: sms|mms`) preserved for write-back.
///
/// `Deserialize` recovers the bag from an IR message's `source.fields` on the
/// write-back path (`sms-backup-restore-exporter`'s SBR writer);
/// `parts`/`addrs` default to empty so a bag written without them still
/// parses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SourceFields {
    /// Raw SMS source bag.
    #[serde(rename = "sms")]
    Sms {
        /// Raw SMS attributes.
        attrs: BTreeMap<String, String>,
    },
    /// Raw MMS source bag.
    #[serde(rename = "mms")]
    Mms {
        /// Raw MMS attributes.
        attrs: BTreeMap<String, String>,
        /// Raw `<part>` attribute maps.
        #[serde(default)]
        parts: Vec<BTreeMap<String, String>>,
        /// Raw `<addr>` attribute maps.
        #[serde(default)]
        addrs: Vec<BTreeMap<String, String>>,
    },
}

/// One parsed SMS/MMS message record.
#[derive(Debug, Clone)]
pub struct Record {
    /// Conversation key: the peer's handle key, or the group key.
    pub chat_key: String,
    /// Individual or group classification.
    pub conversation_kind: ConversationKind,
    /// Generated group title, if group.
    pub group_title: Option<String>,
    /// (Handle, display-name hint) pairs for participants.
    pub participants: Vec<(Handle, Option<String>)>,
    /// Message timestamp in seconds.
    pub timestamp_secs: f64,
    /// Whether the message is outgoing.
    pub is_from_me: bool,
    /// Sender of an incoming message.
    pub sender: Option<Handle>,
    /// Sender display-name hint, when present.
    pub sender_display_name: Option<String>,
    /// Message body text (HTML-entity decoded).
    pub text: String,
    /// Message subject, if any.
    pub subject: String,
    /// Decoded attachment blobs.
    pub attachments: Vec<AttachmentBlob>,
    /// `"sms"` or `"mms"`.
    pub message_kind: &'static str,
    /// Raw `date` attribute in milliseconds.
    pub date_ms: String,
    /// Raw `contact_name` attribute (may be `"null"`).
    pub contact_name: String,
    /// Raw `type` (SMS) or `msg_box` (MMS) attribute string.
    pub android_type: String,
    /// Serde-tagged raw source bag for write-back.
    pub source_fields: SourceFields,
    /// Parts left out of the message because their `data` is not base64.
    pub unreadable_parts: u64,
    /// Character references left out of the message's element, its parts
    /// and its addresses because they are not a character, such as `&#0;`
    /// or a lone surrogate.
    pub dropped_character_references: u64,
}

/// Counters for seen and skipped messages, and when the file's backup was
/// made.
#[derive(Debug, Default, Clone, Copy)]
pub struct ParseStats {
    /// The root `<smses>` element's `backup_date`: when SMS Backup & Restore
    /// made the backup, in Unix milliseconds. `None` when the file has no
    /// such attribute or it is not a number.
    pub backup_date_unix_ms: Option<i64>,
    /// Number of `<sms>` elements encountered.
    pub sms_seen: u64,
    /// Number of `<mms>` elements encountered.
    pub mms_seen: u64,
    /// Records dropped for an unparseable `date`.
    pub skipped_invalid_date: u64,
    /// Records dropped because no usable phone address.
    pub skipped_unknown_address: u64,
    /// SMS records dropped for an unknown `type`.
    pub skipped_unknown_type: u64,
    /// Records dropped as draft/outbox/failed/queued.
    pub skipped_draft_or_outbox: u64,
    /// MMS records dropped with no participants.
    pub skipped_empty_participants: u64,
}

/// The contact name as written: `contact_name`, else `name`.
fn raw_name(attrs: &HashMap<String, String>) -> String {
    let value = get(attrs, "contact_name");
    if value.is_empty() {
        get(attrs, "name").into()
    } else {
        value.into()
    }
}

/// The value trimmed, with `null` treated as empty.
fn non_null(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("null") {
        String::new()
    } else {
        value.into()
    }
}

/// One `<sms>` element as a record, counting rows skipped for a bad date or
/// address. `dropped` is how many character references its attributes lost.
fn parse_sms(
    attrs: &HashMap<String, String>,
    dropped: u64,
    stats: &mut ParseStats,
) -> Option<Record> {
    stats.sms_seen += 1;
    let (date_ms, timestamp_secs) = timestamp_from_date(attrs, stats)?;
    let address = address_handle(get(attrs, "address")).or_else(|| {
        stats.skipped_unknown_address += 1;
        None
    })?;
    let android_type = get(attrs, "type").trim().to_string();
    let (is_from_me, sender) = match android_type.as_str() {
        "1" => (false, Some(address.clone())),
        "2" => (true, None),
        // Draft (3) and outbox (4) SMS carry no delivered content; count them
        // with the descriptive counter used for MMS drafts/outbox/failed/queued
        // instead of the catch-all unknown-type counter.
        "3" | "4" => {
            stats.skipped_draft_or_outbox += 1;
            return None;
        }
        _ => {
            stats.skipped_unknown_type += 1;
            return None;
        }
    };
    let raw_name = raw_name(attrs);
    let hint = contact_name(&raw_name, ConversationKind::Individual).map(String::from);
    Some(Record {
        chat_key: address.key().to_string(),
        conversation_kind: ConversationKind::Individual,
        group_title: None,
        participants: vec![(address, hint.clone())],
        timestamp_secs,
        is_from_me,
        sender,
        sender_display_name: if is_from_me { None } else { hint },
        text: decode_body(get(attrs, "body")),
        subject: non_null(get(attrs, "subject")),
        attachments: Vec::new(),
        message_kind: "sms",
        date_ms,
        contact_name: raw_name,
        android_type,
        source_fields: SourceFields::Sms {
            attrs: btree(attrs),
        },
        unreadable_parts: 0,
        dropped_character_references: dropped,
    })
}

/// The last millisecond of the year 9999, the latest `date` read as real.
const MAX_DATE_MS: i64 = 253_402_300_799_999;

/// Unix seconds from an element's millisecond `date` attribute, with the raw
/// value. Counts and drops an unreadable date: anything but a whole number of
/// milliseconds from 0 to [`MAX_DATE_MS`], so `NaN`, `inf`, and out-of-range
/// values never become a timestamp.
fn timestamp_from_date(
    attrs: &HashMap<String, String>,
    stats: &mut ParseStats,
) -> Option<(String, f64)> {
    let date_ms = get(attrs, "date").to_string();
    let Some(millis) = date_ms
        .parse::<i64>()
        .ok()
        .filter(|ms| (0..=MAX_DATE_MS).contains(ms))
    else {
        stats.skipped_invalid_date += 1;
        return None;
    };
    // Exact: every value up to MAX_DATE_MS fits in an f64 mantissa.
    let millis = millis as f64;
    Some((date_ms, millis / 1000.0))
}

/// One `<mms>` element as a [`Record`], or `None` (counted in `stats`) when it
/// is a draft, has no participants, or names nobody but the owner. `dropped`
/// is how many character references the element, its parts and its
/// addresses lost.
fn parse_mms(
    attrs: &HashMap<String, String>,
    dropped: u64,
    parts: &[MmsPart],
    addrs: &[MmsAddr],
    owners: Option<&OwnerHandleSet>,
    stats: &mut ParseStats,
) -> Option<Record> {
    stats.mms_seen += 1;
    let (date_ms, timestamp_secs) = timestamp_from_date(attrs, stats)?;
    let msg_box = get(attrs, "msg_box").trim().to_string();
    let mms_box = MmsBox::parse(&msg_box);
    if mms_box.is_some_and(MmsBox::is_unsent) {
        stats.skipped_draft_or_outbox += 1;
        return None;
    }
    let participants = mms_participants(attrs, addrs);
    if participants.is_empty() {
        stats.skipped_empty_participants += 1;
        return None;
    }
    let peers = mms_peers(participants, owners);
    if peers.is_empty() {
        stats.skipped_unknown_address += 1;
        return None;
    }
    let is_from_me = mms_box.is_some_and(MmsBox::is_sent);
    let sender = if is_from_me {
        None
    } else {
        mms_sender(addrs, &peers, owners)
    };
    let decoded: Vec<DecodedPartData> = parts.iter().map(|p| decode_part_data(&p.data)).collect();
    let (text, attachments, unreadable_parts) = mms_body(parts, &decoded);
    let raw_name = raw_name(attrs);
    let conversation = MmsConversation::for_peers(peers, &raw_name);
    let hint = contact_name(&raw_name, conversation.kind).map(String::from);
    Some(Record {
        chat_key: conversation.chat_key,
        conversation_kind: conversation.kind,
        group_title: conversation.group_title,
        participants: conversation.participants,
        timestamp_secs,
        is_from_me,
        sender,
        sender_display_name: if is_from_me { None } else { hint },
        text,
        subject: non_null(get(attrs, "sub")),
        attachments,
        message_kind: "mms",
        date_ms,
        contact_name: raw_name,
        android_type: msg_box,
        source_fields: SourceFields::Mms {
            attrs: btree(attrs),
            parts: parts
                .iter()
                .zip(decoded.iter())
                .map(|(p, d)| part_fields(p, d))
                .collect(),
            addrs: addrs.iter().map(|a| a.attrs.clone()).collect(),
        },
        unreadable_parts,
        dropped_character_references: dropped,
    })
}

/// Parse one XML file, calling `on_record` for each message as soon as it is
/// complete.
///
/// The callback owns the record (including decoded attachment bytes). Staging
/// those bytes and dropping the record frees the payload before the next
/// message is parsed.
///
/// `stats` is updated as messages are seen, including when this function later
/// returns an error. Callers that keep records from the callback can merge
/// those counters even if the XML is truncated.
///
/// # Errors
///
/// Returns an error when the file cannot be opened, the XML cannot be parsed,
/// or `on_record` returns an error.
pub fn parse_file_with<F>(
    path: &Path,
    owners: Option<&OwnerHandleSet>,
    stats: &mut ParseStats,
    on_record: F,
) -> Result<()>
where
    F: FnMut(Record) -> Result<()>,
{
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    parse_reader_with(std::io::BufReader::new(file), owners, stats, on_record)
}

/// Stream the XML, calling `on_record` for each SMS or MMS as it completes.
fn parse_reader_with<R, F>(
    reader: R,
    owners: Option<&OwnerHandleSet>,
    stats: &mut ParseStats,
    mut on_record: F,
) -> Result<()>
where
    R: BufRead,
    F: FnMut(Record) -> Result<()>,
{
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let (mut sms, mut mms, mut parts, mut addrs) =
        (HashMap::new(), HashMap::new(), Vec::new(), Vec::new());
    // Character references the open `<sms>` or `<mms>` element, its parts
    // and its addresses have lost so far, so its record can say so.
    let mut dropped = 0;
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref().to_ascii_lowercase().as_str() {
                "smses" => stats.backup_date_unix_ms = backup_date(&attrs(&e, &mut 0)),
                "sms" => {
                    dropped = 0;
                    sms = attrs(&e, &mut dropped);
                }
                "mms" => {
                    dropped = 0;
                    mms = attrs(&e, &mut dropped);
                    parts.clear();
                    addrs.clear();
                }
                "part" => parts.push(part(&attrs(&e, &mut dropped))),
                "addr" => addrs.push(addr(&attrs(&e, &mut dropped))),
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref().to_ascii_lowercase().as_str() {
                "smses" => stats.backup_date_unix_ms = backup_date(&attrs(&e, &mut 0)),
                "sms" => {
                    let mut own = 0;
                    let attrs = attrs(&e, &mut own);
                    if let Some(r) = parse_sms(&attrs, own, stats) {
                        on_record(r)?;
                    }
                }
                "part" => parts.push(part(&attrs(&e, &mut dropped))),
                "addr" => addrs.push(addr(&attrs(&e, &mut dropped))),
                "mms" => {
                    let mut own = 0;
                    let attrs = attrs(&e, &mut own);
                    if let Some(r) = parse_mms(&attrs, own, &[], &[], owners, stats) {
                        on_record(r)?;
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref().to_ascii_lowercase().as_str() {
                "sms" => {
                    if let Some(r) = parse_sms(&sms, dropped, stats) {
                        on_record(r)?;
                    }
                }
                "mms" => {
                    let record = parse_mms(&mms, dropped, &parts, &addrs, owners, stats);
                    // Drop the base64 `data` strings before the callback stages
                    // decoded bytes, so peak RAM is one payload, not payload plus
                    // the still-resident encoding.
                    parts.clear();
                    addrs.clear();
                    if let Some(r) = record {
                        on_record(r)?;
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(error) => return Err(error).context("XML parse error"),
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

/// The root element's `backup_date` in Unix milliseconds, as SMS Backup &
/// Restore writes it, or `None` when it is missing or not a number.
fn backup_date(attrs: &HashMap<String, String>) -> Option<i64> {
    get(attrs, "backup_date").trim().parse().ok()
}

#[cfg(test)]
fn parse_reader<R: BufRead>(
    reader: R,
    owners: Option<&OwnerHandleSet>,
) -> Result<(Vec<Record>, ParseStats)> {
    let mut records = Vec::new();
    let mut stats = ParseStats::default();
    parse_reader_with(reader, owners, &mut stats, |record| {
        records.push(record);
        Ok(())
    })?;
    Ok((records, stats))
}

#[cfg(test)]
mod tests;
