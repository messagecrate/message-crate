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
use crate::parts::{
    AttachmentBlob, DecodedPartData, MmsPart, decode_part_data, mms_body, part, part_fields,
};
use crate::xml::{attrs, btree, decode_body, get};

pub(crate) const MMS_BOX_SENT: &str = "2";
const MMS_BOX_DRAFT: &str = "3";
const MMS_BOX_OUTBOX: &str = "4";
const MMS_BOX_FAILED: &str = "5";
const MMS_BOX_QUEUED: &str = "6";

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
    if matches!(
        msg_box.as_str(),
        MMS_BOX_DRAFT | MMS_BOX_OUTBOX | MMS_BOX_FAILED | MMS_BOX_QUEUED
    ) {
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
    let is_from_me = msg_box == MMS_BOX_SENT;
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
mod tests {
    use super::*;
    use crate::infer_owner_phones;
    use message_ir::IdentityType;

    #[test]
    fn infers_owner_from_nested_addr() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smses.xml");
        std::fs::write(&path, r#"<smses><mms msg_box="2"><parts/><addrs><addr address="+15555550100" type="137"/></addrs></mms></smses>"#).unwrap();
        assert_eq!(infer_owner_phones(&path).unwrap(), vec!["+15555550100"]);
    }

    #[test]
    fn group_mms_without_from_has_no_sender() {
        let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101~+15555550102~+15555550100"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+15555550101" type="151"/><addr address="+15555550102" type="151"/><addr address="+15555550100" type="151"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), Some(&owners)).unwrap();
        assert_eq!(records[0].conversation_kind, ConversationKind::Group);
        assert!(!records[0].is_from_me);
        assert!(records[0].sender.is_none());
    }

    #[test]
    fn direct_mms_without_from_is_from_the_peer() {
        let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101~+15555550100"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+15555550101" type="151"/><addr address="+15555550100" type="151"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), Some(&owners)).unwrap();
        assert_eq!(records[0].conversation_kind, ConversationKind::Individual);
        assert_eq!(
            records[0].sender.as_ref().map(Handle::key),
            Some("+15555550101")
        );
    }

    #[test]
    fn mms_text_part_without_a_name_keeps_its_text() {
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].text, "hi");
    }

    /// An incoming MMS from +15555550101 with the given parts.
    fn mms_with_parts(parts: &str) -> Record {
        let xml = format!(
            r#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts>{parts}</parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#
        );
        let (mut records, _) = parse_reader(xml.as_bytes(), None).unwrap();
        records.remove(0)
    }

    /// Two text parts the SMIL names in reverse alphabetical order: b.txt
    /// ("zulu"), then a.txt ("alpha"). b.txt is matched by the last segment
    /// of its location.
    const TEXT_PARTS: &str = r#"<part ct="text/plain" name="a.txt" text="alpha"/><part ct="text/plain" cl="parts/b.txt" text="zulu"/>"#;
    const SMIL: &str =
        r#"<smil><body><par><text src="b.txt"/></par><par><text src="a.txt"/></par></body></smil>"#;

    #[test]
    fn mms_text_follows_smil_order() {
        let smil = html_escape::encode_double_quoted_attribute(SMIL);
        let record = mms_with_parts(&format!(
            r#"<part ct="application/smil" text="{smil}"/>{TEXT_PARTS}"#
        ));
        assert_eq!(record.text, "zulu\nalpha");
    }

    #[test]
    fn mms_text_follows_smil_order_from_base64_data() {
        let smil = crate::encode_part_data(SMIL.as_bytes());
        let record = mms_with_parts(&format!(
            r#"<part ct="application/smil" data="{smil}"/>{TEXT_PARTS}"#
        ));
        assert_eq!(record.text, "zulu\nalpha");
    }

    /// A vCard part is written as `ct="text/x-vcard" text="null"`: a text
    /// type with no text, which must not put the word null in the message.
    #[test]
    fn a_text_part_whose_text_is_null_or_empty_adds_nothing_to_the_message() {
        let record = mms_with_parts(
            r#"<part ct="text/plain" text="see the card"/><part ct="text/x-vcard" name="sam.vcf" text="null"/><part ct="text/plain" text=""/>"#,
        );
        assert_eq!(record.text, "see the card");
    }

    /// An incoming MMS to a group of `count` peers, +15555550101 upwards,
    /// listed last to first so the record has to sort them.
    fn group_mms(count: usize) -> Record {
        let address = (1..=count)
            .rev()
            .map(|i| format!("+15555550{:03}", 100 + i))
            .collect::<Vec<_>>()
            .join("~");
        let xml = format!(
            r#"<smses><mms date="1" msg_box="1" address="{address}"><parts><part ct="text/plain" text="hi"/></parts><addrs/></mms></smses>"#
        );
        let (mut records, _) = parse_reader(xml.as_bytes(), None).unwrap();
        records.remove(0)
    }

    #[test]
    fn a_group_is_titled_with_its_first_four_numbers_and_a_count_of_the_rest() {
        assert_eq!(
            group_mms(3).group_title.as_deref(),
            Some("Group: +15555550101, +15555550102, +15555550103")
        );
        assert_eq!(
            group_mms(6).group_title.as_deref(),
            Some("Group: +15555550101, +15555550102, +15555550103, +15555550104, and 2 others")
        );
    }

    /// The key becomes the conversation's file name, and twenty numbers
    /// joined are longer than a file name may be.
    #[test]
    fn a_group_of_twenty_is_keyed_by_a_short_hash_of_its_roster() {
        let key = group_mms(20).chat_key;
        let hash = key.strip_prefix("group-").expect("group- prefix");
        assert_eq!(hash.len(), 16, "key was {key}");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "key was {key}");
        assert_eq!(group_mms(20).chat_key, key, "one roster, one key");
        assert_ne!(group_mms(21).chat_key, key);
    }

    #[test]
    fn attachments_without_smil_keep_the_order_of_their_parts() {
        let names = [
            "zebra.jpg",
            "apple.jpg",
            "mango.jpg",
            "kiwi.jpg",
            "fig.jpg",
            "plum.jpg",
        ];
        let parts: String = names
            .iter()
            .map(|name| {
                let data = crate::encode_part_data(name.as_bytes());
                format!(r#"<part ct="image/jpeg" name="{name}" data="{data}"/>"#)
            })
            .collect();
        let record = mms_with_parts(&parts);
        let order: Vec<&str> = record
            .attachments
            .iter()
            .map(|a| a.original_name.as_deref().unwrap())
            .collect();
        assert_eq!(order, names);
    }

    #[test]
    fn sms_body_is_decoded_and_normalized() {
        let xml = br#"<smses><sms protocol="0" address="+15555550101" date="1" type="1" body="Tom &amp;amp; Jerry&#13;&#10;line two&#13;three"/></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].text, "Tom & Jerry\nline two\nthree");
    }

    #[test]
    fn contact_names_and_subjects_treat_null_as_missing() {
        let sms = |attrs: &str| {
            let xml = format!(
                r#"<smses><sms protocol="0" address="+15555550101" date="1" type="1" body="hi" {attrs}/></smses>"#
            );
            let (mut records, _) = parse_reader(xml.as_bytes(), None).unwrap();
            records.remove(0)
        };
        let named = sms(r#"contact_name="Sam" subject="Plans""#);
        assert_eq!(named.sender_display_name.as_deref(), Some("Sam"));
        assert_eq!(named.contact_name, "Sam");
        assert_eq!(named.subject, "Plans");

        let null = sms(r#"contact_name="null" subject="null""#);
        assert_eq!(null.sender_display_name, None);
        assert_eq!(null.subject, "");

        // The app writes "(Unknown)" where the phone has no contact.
        for unknown in ["(Unknown)", "(unknown)", " (UNKNOWN) "] {
            let record = sms(&format!(r#"contact_name="{unknown}""#));
            assert_eq!(record.sender_display_name, None, "{unknown:?}");
        }

        let fallback = sms(r#"contact_name="" name="Alex" subject="""#);
        assert_eq!(fallback.sender_display_name.as_deref(), Some("Alex"));
        assert_eq!(fallback.contact_name, "Alex");
        assert_eq!(fallback.subject, "");
    }

    #[test]
    fn attachment_extension_comes_from_the_type_then_the_name() {
        let data = crate::encode_part_data(b"payload");
        let ext = |ct: &str, name: &str| {
            let record =
                mms_with_parts(&format!(r#"<part ct="{ct}" name="{name}" data="{data}"/>"#));
            let file = &record.attachments[0].filename;
            file[file.find('.').unwrap()..].to_string()
        };
        assert_eq!(ext("video/3gpp", "clip"), ".3gp");
        assert_eq!(ext("application/x-thing", "notes.PDF"), ".pdf");
        assert_eq!(ext("image/heic", "null"), ".jpg");
        assert_eq!(ext("application/x-thing", "null"), ".bin");
    }

    #[test]
    fn null_part_data_is_no_attachment() {
        // "null" is valid base64 for three junk bytes.
        let record = mms_with_parts(r#"<part ct="image/jpeg" name="pic.jpg" data="null"/>"#);
        assert!(record.attachments.is_empty());
    }

    /// SMS Backup & Restore writes `null` for a part with no name. Two such
    /// parts are two files, not one file named "null".
    #[test]
    fn media_parts_named_null_or_nothing_are_each_an_attachment() {
        let record = mms_with_parts(
            r#"<part ct="image/jpeg" name="null" cl="NULL" fn="" data="aGVsbG8="/><part ct="image/png" name="null" cl="NULL" fn="" data="d29ybGQ="/>"#,
        );
        let data: Vec<&[u8]> = record.attachments.iter().map(|a| a.data.as_ref()).collect();
        assert_eq!(data, vec![b"hello".as_slice(), b"world".as_slice()]);
    }

    #[test]
    fn parses_attachment_and_preserves_fields() {
        let xml = br#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101" extra="x"><parts><part seq="0" ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137" charset="106"/></addrs></mms></smses>"#;
        let (records, stats) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(stats.mms_seen, 1);
        assert_eq!(records[0].attachments[0].data.as_ref(), b"hello");
        let SourceFields::Mms {
            attrs,
            parts,
            addrs,
        } = &records[0].source_fields
        else {
            panic!("mms")
        };
        assert_eq!(attrs.get("extra").map(String::as_str), Some("x"));
        assert!(parts[0].contains_key("data_sha256"));
        assert_eq!(addrs[0].get("charset").map(String::as_str), Some("106"));
    }

    /// The same picture sent twice is two attachments, which share one file
    /// because the file is named by its content.
    #[test]
    fn attachment_filename_is_content_addressed() {
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="first.jpg" data="aGVsbG8="/><part ct="image/jpeg" name="second.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].attachments.len(), 2);
        assert_eq!(
            records[0].attachments[0].filename,
            records[0].attachments[1].filename
        );
        let attachment = &records[0].attachments[0];
        assert!(attachment.filename.starts_with(&attachment.digest_hex));
        assert_eq!(attachment.digest_hex.len(), 64);
    }

    #[test]
    fn parse_file_with_calls_back_per_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smses.xml");
        std::fs::write(
            &path,
            r#"<smses>
            <sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="hi"/>
            <mms date="1400773400000" msg_box="1" address="+15555550101">
                <parts><part seq="0" ct="text/plain" text="mms"/></parts>
                <addrs><addr address="+15555550101" type="137"/></addrs>
            </mms>
        </smses>"#,
        )
        .unwrap();
        let mut n = 0u32;
        let mut stats = ParseStats::default();
        parse_file_with(&path, None, &mut stats, |_| {
            n += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 2);
        assert_eq!(stats.sms_seen, 1);
        assert_eq!(stats.mms_seen, 1);
    }

    #[test]
    fn skipped_unreadable_part_records_decode_error() {
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="pic.jpg" data="@@@not-base64@@@"/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].unreadable_parts, 1);
        assert!(records[0].attachments.is_empty());
        let SourceFields::Mms { parts, .. } = &records[0].source_fields else {
            panic!("mms")
        };
        assert_eq!(
            parts[0].get("data_decode_error").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn smil_src_orders_attachment_from_decoded_payload() {
        let smil = "PHNtaWw+PGJvZHk+PGltZyBzcmM9InBpYy5qcGciLz48L2JvZHk+PC9zbWlsPg==";
        let xml = format!(
            r#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts><part ct="application/smil" data="{smil}"/><part ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#
        );
        let (records, stats) = parse_reader(xml.as_bytes(), None).unwrap();
        assert_eq!(stats.mms_seen, 1);
        assert_eq!(records[0].attachments.len(), 1);
        assert_eq!(records[0].attachments[0].data.as_ref(), b"hello");
    }

    fn sms_with_date(date: &str) -> (Vec<Record>, ParseStats) {
        let xml = format!(
            r#"<smses><sms protocol="0" address="+15555550101" date="{date}" type="1" body="hi"/></smses>"#
        );
        parse_reader(xml.as_bytes(), None).unwrap()
    }

    #[test]
    fn unreadable_dates_are_skipped() {
        for date in [
            "NaN",
            "nan",
            "inf",
            "-inf",
            "infinity",
            "1e400",
            "1.4e12",
            "1400773261000.5",
            "",
            " 1400773261000",
            "-1",
            "abc",
            "99999999999999999999999",
            "9223372036854775807",
        ] {
            let (records, stats) = sms_with_date(date);
            assert!(records.is_empty(), "date {date:?} was accepted");
            assert_eq!(stats.skipped_invalid_date, 1, "date {date:?}");
        }
    }

    #[test]
    fn millisecond_dates_parse_exactly() {
        for (date, secs) in [
            ("0", 0.0),
            ("1", 0.001),
            ("1400773261000", 1_400_773_261.0),
            ("1400773261123", 1_400_773_261_123.0 / 1000.0),
            ("253402300799999", 253_402_300_799_999.0 / 1000.0),
        ] {
            let (records, stats) = sms_with_date(date);
            assert_eq!(stats.skipped_invalid_date, 0, "date {date:?}");
            assert_eq!(records[0].timestamp_secs, secs, "date {date:?}");
            assert_eq!(records[0].date_ms, date);
        }
    }

    #[test]
    fn a_multi_line_body_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.xml");
        let mut writer = crate::SbrBackupWriter::create(&path).unwrap();
        let attrs: BTreeMap<String, String> = [
            ("protocol", "0"),
            ("address", "+15555550101"),
            ("date", "1"),
            ("type", "1"),
            ("body", "line1\nline2\ttab"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        writer
            .write_message(&crate::SbrMessage::sms(attrs))
            .unwrap();
        let path = writer.finish().unwrap();
        let (records, _) = parse_reader(std::fs::read(&path).unwrap().as_slice(), None).unwrap();
        assert_eq!(records[0].text, "line1\nline2\ttab");
    }

    /// The newest backup date noted is written as the root `backup_date`,
    /// which the reader reads back, and a file with none noted has none.
    #[test]
    fn the_backup_date_noted_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let written = |name: &str, dates: &[Option<i64>]| {
            let mut writer = crate::SbrBackupWriter::create(&dir.path().join(name)).unwrap();
            for &date in dates {
                writer.note_backup_date(date);
            }
            let attrs: BTreeMap<String, String> = [
                ("protocol", "0"),
                ("address", "+15555550101"),
                ("date", "1"),
                ("type", "1"),
                ("body", "hi"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
            writer
                .write_message(&crate::SbrMessage::sms(attrs))
                .unwrap();
            let path = writer.finish().unwrap();
            parse_reader(std::fs::read(&path).unwrap().as_slice(), None)
                .unwrap()
                .1
                .backup_date_unix_ms
        };
        assert_eq!(
            written(
                "dated.xml",
                &[Some(1_788_256_800_000), None, Some(1_790_793_912_000)]
            ),
            Some(1_790_793_912_000)
        );
        assert_eq!(written("undated.xml", &[None]), None);
    }

    #[test]
    fn a_literal_line_break_in_an_attribute_is_kept() {
        let xml = b"<smses><sms protocol=\"0\" address=\"+15555550101\" date=\"1\" type=\"1\" body=\"line1\nline2\r\nline3\"/><mms date=\"2\" msg_box=\"1\" address=\"+15555550101\"><parts><part ct=\"text/plain\" text=\"part1\npart2\"/></parts><addrs><addr address=\"+15555550101\" type=\"137\"/></addrs></mms></smses>";
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].text, "line1\nline2\nline3");
        assert_eq!(records[1].text, "part1\npart2");
    }

    #[test]
    fn a_surrogate_pair_reference_is_one_character() {
        let xml = br#"<smses><sms protocol="0" address="+15555550101" date="1" type="1" body="On my way &#55357;&#56832;"/></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].text, "On my way \u{1F600}");
    }

    #[test]
    fn a_reference_that_is_no_character_costs_only_itself() {
        let xml = br#"<smses><sms protocol="0" address="+15555550101" date="1" type="1" body="a&#0;b &#55357;c &#xDE00;d&nbsp;e"/><mms date="2" msg_box="1" address="+15555550101"><parts><part ct="text/plain" text="x&#55357;y"/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].text, "ab c d\u{a0}e");
        assert_eq!(records[1].text, "xy");
        assert_eq!(records[0].dropped_character_references, 3);
        assert_eq!(records[1].dropped_character_references, 1);
    }

    /// Each record counts only the references dropped from its own element
    /// and its parts and addresses, so a note can name the message that lost
    /// them (#1707).
    #[test]
    fn each_record_counts_the_references_it_lost() {
        let xml = br#"<smses><sms address="+15555550101" date="1" type="1" body="a&#0;b"/><mms date="2" msg_box="1" address="+15555550101" sub="&#0;"><parts><part ct="text/plain" text="x&#0;y"/></parts><addrs><addr address="+15555550101" type="137" charset="&#0;"/></addrs></mms><sms address="+15555550101" date="3" type="1" body="clean"></sms><sms address="+15555550101" date="4" type="1" body="&#0;&#0;"></sms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        let dropped: Vec<u64> = records
            .iter()
            .map(|r| r.dropped_character_references)
            .collect();
        assert_eq!(dropped, [1, 3, 0, 2]);
    }

    #[test]
    fn an_email_address_is_not_a_phone_number() {
        let xml = br#"<smses><sms protocol="0" address="john1985@example.com" date="1" type="1" body="hi"/></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].chat_key, "john1985@example.com");
        let sender = records[0].sender.as_ref().unwrap();
        assert_eq!(
            (sender.kind(), sender.key()),
            (IdentityType::Email, "john1985@example.com")
        );
    }

    #[test]
    fn a_number_with_its_country_keeps_it() {
        // +65 5555 0100 is no one's number. The note on the `phone` crate's
        // `mod tests` says why.
        let xml = br#"<smses><sms protocol="0" address="+6555550100" date="1" type="1" body="hi"/></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].chat_key, "+6555550100");
    }

    #[test]
    fn a_sender_name_is_imported_as_an_identity_of_type_other() {
        let xml = br#"<smses><sms protocol="0" address="AMAZON" date="1" type="1" body="Your parcel"/></smses>"#;
        let (records, stats) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(stats.skipped_unknown_address, 0);
        let sender = records[0].sender.as_ref().unwrap();
        assert_eq!(
            (sender.kind(), sender.key()),
            (IdentityType::Other, "AMAZON")
        );
    }

    #[test]
    fn an_owner_outside_the_us_is_inferred_with_its_country() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smses.xml");
        std::fs::write(&path, r#"<smses><mms msg_box="2"><parts/><addrs><addr address="+447700900456" type="137"/></addrs></mms></smses>"#).unwrap();
        assert_eq!(infer_owner_phones(&path).unwrap(), vec!["+447700900456"]);
    }

    /// A group MMS's `contact_name` is the members' names joined by ", ", so
    /// it names the group, not the sender.
    #[test]
    fn a_group_mms_contact_name_does_not_name_its_sender() {
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101~+15555550102" contact_name="Ana, Lee"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+15555550102" type="137"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].conversation_kind, ConversationKind::Group);
        assert_eq!(records[0].sender_display_name, None);
        assert!(
            records[0]
                .participants
                .iter()
                .all(|(_, name)| name.is_none())
        );
    }

    #[test]
    fn a_direct_mms_contact_name_names_its_peer_and_sender() {
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101" contact_name="Sam"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert_eq!(records[0].sender_display_name.as_deref(), Some("Sam"));
        assert_eq!(records[0].participants[0].1.as_deref(), Some("Sam"));
    }

    /// Two pictures with one name are two attachments: parts are told apart
    /// by position, not by name.
    #[test]
    fn two_parts_with_one_name_are_two_attachments() {
        let record = mms_with_parts(
            r#"<part ct="image/jpeg" name="image.jpg" cl="image.jpg" data="aGVsbG8="/><part ct="image/jpeg" name="image.jpg" cl="image.jpg" data="d29ybGQ="/>"#,
        );
        let data: Vec<&[u8]> = record.attachments.iter().map(|a| a.data.as_ref()).collect();
        assert_eq!(data, vec![b"hello".as_slice(), b"world".as_slice()]);
    }

    /// The SMIL may name a text part by its Content-ID.
    #[test]
    fn a_text_part_the_smil_names_by_cid_is_the_text() {
        let smil = html_escape::encode_double_quoted_attribute(
            r#"<smil><body><par><text src="cid:text_0"/></par></body></smil>"#,
        );
        let record = mms_with_parts(&format!(
            r#"<part ct="application/smil" text="{smil}"/><part ct="text/plain" cid="&lt;text_0&gt;" cl="text_0.txt" text="hello there"/>"#
        ));
        assert_eq!(record.text, "hello there");
    }

    /// A text part the SMIL does not name is still the message's text, after
    /// the parts the SMIL names.
    #[test]
    fn a_text_part_the_smil_does_not_name_is_kept_after_the_named_ones() {
        let smil = html_escape::encode_double_quoted_attribute(
            r#"<smil><body><par><text src="b.txt"/></par></body></smil>"#,
        );
        let record = mms_with_parts(&format!(
            r#"<part ct="application/smil" text="{smil}"/><part ct="text/plain" cl="a.txt" text="later"/><part ct="text/plain" cl="b.txt" text="first"/>"#
        ));
        assert_eq!(record.text, "first\nlater");
    }

    /// A contact card is a text type whose content is in `data`; it is an
    /// attachment.
    #[test]
    fn a_contact_card_with_data_is_an_attachment() {
        let data = crate::encode_part_data(b"BEGIN:VCARD\r\nFN:Sam\r\nEND:VCARD\r\n");
        let record = mms_with_parts(&format!(
            r#"<part ct="text/plain" text="card"/><part ct="text/x-vcard" name="sam.vcf" text="null" data="{data}"/>"#
        ));
        assert_eq!(record.text, "card");
        assert_eq!(record.attachments.len(), 1);
        assert_eq!(
            record.attachments[0].mime_type.as_deref(),
            Some("text/x-vcard")
        );
        assert_eq!(
            record.attachments[0].original_name.as_deref(),
            Some("sam.vcf")
        );
    }

    /// Without a SMIL part the text parts are joined in the order they are
    /// written, and a text that repeats another is kept.
    #[test]
    fn text_parts_without_smil_keep_their_order_and_their_repeats() {
        let record = mms_with_parts(
            r#"<part ct="text/plain" text="Tickets attached"/><part ct="text/plain" text="All three are for Friday"/><part ct="text/plain" text="ha"/><part ct="text/plain" text="ha"/>"#,
        );
        assert_eq!(
            record.text,
            "Tickets attached\nAll three are for Friday\nha\nha"
        );
    }

    /// A contact card whose `data` is not base64 is neither text nor an
    /// attachment, and it is counted.
    #[test]
    fn a_part_whose_data_cannot_be_decoded_is_counted() {
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts><part ct="text/x-vcard" name="sam.vcf" data="@@@@"/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), None).unwrap();
        assert!(records[0].attachments.is_empty());
        assert_eq!(records[0].unreadable_parts, 1);
    }
}
