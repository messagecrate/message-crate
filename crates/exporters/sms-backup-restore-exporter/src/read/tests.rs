use super::*;
use crate::write::SbrBackupSession;
use std::fs;

fn opts<'a>(
    owner_phones: &'a [String],
    attachments_dir: Option<&'a Path>,
    spool: Option<&'a AttachmentSpool>,
) -> ReadOptions<'a> {
    ReadOptions {
        owner_phones,
        attachments_dir,
        spool,
        exclude_dir: None,
        media: if spool.is_some() {
            MediaMode::Clone
        } else {
            MediaMode::Disabled
        },
        compress: CompressOptions::default(),
        log: None,
        progress: None,
        cancel: None,
    }
}

#[test]
fn reads_then_writes_source_fields_and_attachment() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101" extra="yes"><parts><part seq="0" ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550100" type="137" charset="106"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#).unwrap();
    let output = dir.path().join("output");
    let stage = output.join("attachments");
    let spool = AttachmentSpool::new(dir.path());
    let (mut docs, _) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
    stage_read_attachments(&mut docs, &opts(&[], Some(&stage), Some(&spool))).unwrap();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 1, "one file staged");
    let staged: Vec<_> = fs::read_dir(&stage)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(staged.len(), 1);
    assert_eq!(fs::metadata(&staged[0]).unwrap().len(), 5);
    assert_eq!(
        docs[0].export.owner_identity.as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        docs[0].messages[0].attachments[0].size_bytes,
        Some(5),
        "decoded aGVsbG8= is five bytes; size_bytes lets message-crate-push skip re-hashing"
    );
    assert_eq!(
        docs[0].messages[0].source.as_ref().unwrap().fields["attrs"]["extra"],
        "yes"
    );
    let mut writer = SbrBackupSession::create(&output).unwrap();
    writer.append_document(&docs[0]).unwrap();
    let xml = fs::read_to_string(writer.finish().unwrap()).unwrap();
    assert!(xml.contains(r#"extra="yes""#));
    assert!(xml.contains(r#"data="aGVsbG8=""#));
    assert!(xml.contains(r#"charset="106""#));
}

#[test]
fn write_back_matches_parts_to_attachments_by_digest() {
    // An empty-data part must not consume the next part's attachment, and
    // identical payloads dedupe into one staged file that both parts share.
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(
        &input,
        r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101"><parts><part seq="0" ct="image/jpeg" name="empty.jpg" data=""/><part seq="1" ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/><part seq="2" ct="image/jpeg" name="pic-copy.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550100" type="137" charset="106"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#,
    )
    .unwrap();
    let output = dir.path().join("output");
    let stage = output.join("attachments");
    let spool = AttachmentSpool::new(dir.path());
    let (mut docs, _) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
    stage_read_attachments(&mut docs, &opts(&[], Some(&stage), Some(&spool))).unwrap();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 1, "one file staged");
    let mut writer = SbrBackupSession::create(&output).unwrap();
    writer.append_document(&docs[0]).unwrap();
    let xml = fs::read_to_string(writer.finish().unwrap()).unwrap();
    // Both payload parts carry the decoded bytes; the empty part does not.
    assert_eq!(xml.match_indices(r#"data="aGVsbG8=""#).count(), 2);
}

/// A contact card is a text type with its content in `data`. It is read
/// as an attachment and written back into its part.
#[test]
fn a_contact_card_is_read_as_an_attachment_and_written_back() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(
        &input,
        r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101"><parts><part seq="0" ct="text/plain" text="card"/><part seq="1" ct="text/x-vcard" name="sam.vcf" text="null" data="QkVHSU46VkNBUkQ="/></parts><addrs><addr address="+15555550100" type="137" charset="106"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#,
    )
    .unwrap();
    let output = dir.path().join("output");
    let stage = output.join("attachments");
    let spool = AttachmentSpool::new(dir.path());
    let (mut docs, _) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
    stage_read_attachments(&mut docs, &opts(&[], Some(&stage), Some(&spool))).unwrap();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 1, "one file staged");
    assert_eq!(docs[0].messages[0].text, "card");
    let mut writer = SbrBackupSession::create(&output).unwrap();
    writer.append_document(&docs[0]).unwrap();
    let xml = fs::read_to_string(writer.finish().unwrap()).unwrap();
    assert!(xml.contains(r#"data="QkVHSU46VkNBUkQ=""#), "{xml}");
}

/// A group MMS is credited to the `type="137"` addr, whichever position
/// that number holds in the `address` list, and to nobody when the
/// backup names no sender. Every form of the owner's number counts as
/// the owner, so all four messages share one conversation.
#[test]
fn group_mms_sender_direction_and_conversation() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/group_mms_sender.xml");
    let (docs, report) = read_backup(&fixture, opts(&[], None, None)).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(docs.len(), 1, "every message lands in the same group");
    let doc = &docs[0];
    assert_eq!(
        doc.conversation.chat_identifier,
        "chat-group-+15555550101_+15555550102_+15555550103"
    );
    assert_eq!(doc.export.owner_identity.as_deref(), Some("+15555550100"));
    let mut participants: Vec<_> = doc
        .conversation
        .participants
        .iter()
        .filter_map(|p| p.identity.as_deref())
        .collect();
    participants.sort_unstable();
    assert_eq!(
        participants,
        ["+15555550101", "+15555550102", "+15555550103"],
        "the owner is not a participant, in any spelling"
    );
    let seen: Vec<(&str, IrDirection, Option<&str>)> = doc
        .messages
        .iter()
        .map(|m| (m.text.as_str(), m.direction, m.sender_identity.as_deref()))
        .collect();
    assert_eq!(
        seen,
        [
            ("from lee", IrDirection::Incoming, Some("+15555550103")),
            ("no from", IrDirection::Incoming, None),
            ("sent by me", IrDirection::Outgoing, Some("+15555550100")),
            ("from ana", IrDirection::Incoming, Some("+15555550102")),
        ]
    );
}

#[test]
fn owner_inference_tolerates_malformed_files() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("ok.xml"),
        r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101"><parts/><addrs><addr address="+15555550100" type="137"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#,
    )
    .unwrap();
    fs::write(input.join("broken.xml"), "<smses><mms date=").unwrap();
    let (docs, report) = read_backup(&input, opts(&[], None, None)).unwrap();
    assert_eq!(
        docs[0].export.owner_identity.as_deref(),
        Some("+15555550100")
    );
    assert_eq!(report.errors.len(), 1);
}

#[test]
fn truncated_xml_keeps_messages_parsed_before_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(
        &input,
        r#"<smses><sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="kept"/><sms date=""#,
    )
    .unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (docs, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].messages.len(), 1);
    assert_eq!(docs[0].messages[0].text, "kept");
    assert_eq!(
        report.sms_seen, 1,
        "stats from the completed message must survive the XML error"
    );
    assert_eq!(report.errors.len(), 1);
}

/// Every payload is on disk in the spool once the read returns, and no
/// document holds a byte of one, so a backup full of video is never in
/// memory at once (issue #1128).
#[test]
fn reading_a_backup_spools_every_payload_and_holds_none() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="a.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms><mms date="1400773500000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="b.jpg" data="d29ybGQ="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#).unwrap();
    let stage = dir.path().join("output").join("attachments");
    let spool = AttachmentSpool::new(dir.path());
    let owner = vec!["+15555550100".to_string()];

    let (docs, _) = read_backup(&input, opts(&owner, Some(&stage), Some(&spool))).unwrap();

    let attachments: Vec<_> = docs[0]
        .messages
        .iter()
        .flat_map(|m| &m.attachments)
        .collect();
    let held: usize = attachments
        .iter()
        .filter_map(|a| a.bytes.as_ref())
        .map(Vec::len)
        .sum();
    assert_eq!(held, 0, "no payload stays on a document");
    let spooled: Vec<Vec<u8>> = attachments
        .iter()
        .map(|a| fs::read(spool.path(a.digest_sha256.as_deref().unwrap()).unwrap()).unwrap())
        .collect();
    assert_eq!(spooled, [b"hello".to_vec(), b"world".to_vec()]);
    assert_eq!(attachments[0].size_bytes, Some(5));
    assert!(
        attachments.iter().all(|a| a.path.is_none()),
        "nothing was staged, so nothing to point at"
    );
    assert!(!stage.exists(), "no attachment files were written");
}

/// An `.xml` file under `exclude_dir` is not read, so a backup Convert wrote
/// into an output inside the input's directory is never read back in.
#[test]
fn a_backup_under_the_excluded_directory_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let sms = r#"<smses><sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="kept"/></smses>"#;
    fs::write(dir.path().join("smses.xml"), sms).unwrap();
    let output = dir.path().join("converted");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("smses.xml"), sms.replace("kept", "skipped")).unwrap();

    assert_eq!(
        collect_xml_paths(dir.path(), Some(&output)).unwrap(),
        [dir.path().join("smses.xml")]
    );
    assert_eq!(collect_xml_paths(dir.path(), None).unwrap().len(), 2);
}

/// A run that does not copy attachments keeps no payload anywhere.
#[test]
fn without_a_spool_no_payload_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="a.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#).unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (docs, _) = read_backup(&input, opts(&owner, None, None)).unwrap();
    let att = &docs[0].messages[0].attachments[0];
    assert!(att.bytes.is_none() && att.path.is_none());
    assert_eq!(att.size_bytes, Some(5), "the size is still recorded");
}

/// Read `messages` wrapped in `<smses>` with +15555550100 as the owner.
fn read_xml(messages: &str) -> Vec<ConversationDocument> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, format!("<smses>{messages}</smses>")).unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (docs, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    docs
}

fn roster(doc: &ConversationDocument) -> Vec<(&str, Option<&str>)> {
    doc.conversation
        .participants
        .iter()
        .map(|p| (p.identity.as_deref().unwrap(), p.display_name.as_deref()))
        .collect()
}

/// A message the backup holds twice is kept once, and the copy dropped is
/// counted, so the run summary can say so. The reader takes only
/// millisecond dates, so the two copies carry the same one.
#[test]
fn a_repeated_message_is_kept_once_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    let copy =
        r#"<sms protocol="0" address="+15555550101" date="1400773261123" type="1" body="hi"/>"#;
    fs::write(
        &input,
        format!(
            r#"<smses>{copy}{copy}<sms protocol="0" address="+15555550101" date="1400773321000" type="2" body="hey"/></smses>"#
        ),
    )
    .unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (docs, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
    let texts: Vec<_> = docs[0].messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["hi", "hey"]);
    assert_eq!(report.sms_seen, 3);
    assert_eq!(report.duplicates_dropped, 1);
}

#[test]
fn a_contact_name_names_the_peer_of_a_direct_conversation() {
    // Only sent messages, which name the peer but carry no sender.
    let docs = read_xml(
        r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="2" body="hi" contact_name="Sam"/>"#,
    );
    assert_eq!(roster(&docs[0]), [("+15555550101", Some("Sam"))]);
}

#[test]
fn a_placeholder_contact_name_names_nobody() {
    for placeholder in ["null", "(Unknown)"] {
        let docs = read_xml(&format!(
            r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="2" body="hi" contact_name="{placeholder}"/><sms protocol="0" address="+15555550101" date="1400773262000" type="1" body="hey" contact_name="{placeholder}"/>"#,
        ));
        assert_eq!(roster(&docs[0]), [("+15555550101", None)], "{placeholder}");
        assert_eq!(docs[0].messages[1].sender_display_name, None);
    }
}

/// A group MMS's `contact_name` is the members' names joined by ", ",
/// which names the group, so no participant takes it as a name.
#[test]
fn a_group_contact_name_names_no_sender() {
    let docs = read_xml(
        r#"<mms date="1400773400000" msg_box="1" address="+15555550101~+15555550102~+15555550100" contact_name="Ana, Lee"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+15555550102" type="137"/><addr address="+15555550101" type="151"/><addr address="+15555550100" type="151"/></addrs></mms>"#,
    );
    assert_eq!(
        roster(&docs[0]),
        [("+15555550101", None), ("+15555550102", None)]
    );
    assert_eq!(docs[0].messages[0].sender_display_name, None);
}

#[test]
fn a_subject_is_kept_and_a_null_one_dropped() {
    let docs = read_xml(
        r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="one" subject="Plans"/><sms protocol="0" address="+15555550101" date="1400773262000" type="1" body="two" subject="null"/>"#,
    );
    let subjects: Vec<_> = docs[0]
        .messages
        .iter()
        .map(|m| m.subject.as_deref())
        .collect();
    assert_eq!(subjects, [Some("Plans"), None]);
}

#[test]
fn an_attachment_carries_its_payload_digest() {
    let docs = read_xml(
        r#"<mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms>"#,
    );
    assert_eq!(
        docs[0].messages[0].attachments[0].digest_sha256.as_deref(),
        // SHA-256 of "hello", the decoded payload.
        Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    );
}

/// A carrier often lists the owner's number in national form in an MMS,
/// so `07700900123` is the owner `+447700900123`, and the MMS is the
/// one-to-one conversation with the other person, not a group.
#[test]
fn the_owner_in_national_form_is_not_a_participant() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="1" address="+447700900456~07700900123"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+447700900456" type="137"/><addr address="07700900123" type="151"/></addrs></mms></smses>"#).unwrap();
    let owner = vec!["+447700900123".to_string()];
    let (docs, _) = read_backup(&input, opts(&owner, None, None)).unwrap();
    assert_eq!(
        docs[0].conversation.participants.len(),
        1,
        "{:?}",
        docs[0].conversation.participants
    );
}

/// With more than one owner number, a sent message is credited to the
/// first one given, in every run.
#[test]
fn the_first_owner_number_given_is_the_owner_handle() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><sms protocol="0" address="+15555550101" date="1400773261000" type="2" body="hi"/></smses>"#).unwrap();
    // Nine numbers, so a set's order would pick the first one given only
    // by chance; the first given is not the smallest either.
    let owners: Vec<String> = [9, 0, 2, 3, 4, 5, 6, 7, 8]
        .iter()
        .map(|i| format!("+1555555010{i}"))
        .collect();
    let (docs, _) = read_backup(&input, opts(&owners, None, None)).unwrap();
    assert_eq!(
        docs[0].export.owner_identity.as_deref(),
        Some("+15555550109")
    );
}

#[test]
fn an_international_number_keeps_its_country() {
    // +65 5555 0100 is no one's number. The note on the `phone` crate's `mod
    // tests` says why.
    let docs = read_xml(
        r#"<sms protocol="0" address="+6555550100" date="1400773261000" type="1" body="hi"/><sms protocol="0" address="+447700900123" date="1400773261000" type="1" body="hi"/>"#,
    );
    let ids: Vec<_> = docs
        .iter()
        .map(|d| d.conversation.chat_identifier.as_str())
        .collect();
    assert_eq!(ids, ["+447700900123", "+6555550100"]);
}

#[test]
fn an_email_sender_is_an_email_identity() {
    let docs = read_xml(
        r#"<sms protocol="0" address="john1985@example.com" date="1400773261000" type="1" body="hi"/>"#,
    );
    assert_eq!(docs[0].conversation.chat_identifier, "john1985@example.com");
    let participant = &docs[0].conversation.participants[0];
    assert_eq!(
        participant.identity.as_deref(),
        Some("john1985@example.com")
    );
    assert_eq!(participant.identity_type, Some(HandleType::Email));
    assert_eq!(
        docs[0].messages[0].sender_identity.as_deref(),
        Some("john1985@example.com")
    );
}

#[test]
fn a_sender_name_is_an_identity_of_type_other() {
    let docs = read_xml(
        r#"<sms protocol="0" address="AMAZON" date="1400773261000" type="1" body="Your parcel"/>"#,
    );
    let participant = &docs[0].conversation.participants[0];
    assert_eq!(participant.identity.as_deref(), Some("AMAZON"));
    assert_eq!(participant.identity_type, Some(HandleType::Other));
}

/// With no owner on the form, the owner comes from the sent MMS, and a
/// UK owner keeps its country, as the account's identity does.
#[test]
fn an_inferred_owner_outside_the_us_keeps_its_country() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="2" address="+447700900123"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+447700900456" type="137"/><addr address="+447700900123" type="151"/></addrs></mms></smses>"#).unwrap();
    let (docs, _) = read_backup(&input, opts(&[], None, None)).unwrap();
    assert_eq!(
        docs[0].export.owner_identity.as_deref(),
        Some("+447700900456")
    );
    assert_eq!(docs[0].conversation.chat_identifier, "+447700900123");
}

#[test]
fn verify_e1_4_two_group_senders_in_one_second_get_two_guids() {
    let docs = read_xml(
        r#"<mms date="1400773400100" msg_box="1" address="+15555550101~+15555550102~+15555550100"><parts><part ct="text/plain" text="lol"/></parts><addrs><addr address="+15555550101" type="137"/><addr address="+15555550102" type="151"/><addr address="+15555550100" type="151"/></addrs></mms><mms date="1400773400200" msg_box="1" address="+15555550101~+15555550102~+15555550100"><parts><part ct="text/plain" text="lol"/></parts><addrs><addr address="+15555550102" type="137"/><addr address="+15555550101" type="151"/><addr address="+15555550100" type="151"/></addrs></mms>"#,
    );
    assert_eq!(docs[0].messages.len(), 2, "the exporter keeps both");
    assert_ne!(docs[0].messages[0].guid, docs[0].messages[1].guid);
}

/// Staging a backup's attachments knows before it starts which ones the
/// spool holds no file for. Its byte total leaves them out from the first
/// event and never drops mid-run (#1727).
#[test]
fn the_byte_total_stays_the_same_when_the_spool_holds_no_file() {
    use message_crate_core::testutil::attachment_totals;

    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101"><parts><part seq="0" ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550100" type="137" charset="106"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#).unwrap();
    let stage = dir.path().join("output").join("attachments");
    let spool = AttachmentSpool::new(dir.path());
    let (mut docs, _) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
    // A second attachment whose record gives a size, with no file spooled.
    let mut unspooled = docs[0].messages[0].attachments[0].clone();
    unspooled.digest_sha256 = Some("0".repeat(64));
    unspooled.size_bytes = Some(700);
    docs[0].messages[0].attachments.push(unspooled);
    let (progress, totals) = attachment_totals();

    stage_read_attachments(
        &mut docs,
        &ReadOptions {
            progress: Some(&progress),
            ..opts(&[], Some(&stage), Some(&spool))
        },
    )
    .unwrap();

    let totals = totals.lock().unwrap().clone();
    assert!(totals.len() > 1, "{totals:?}");
    assert!(
        totals.iter().all(|&(_, _, bytes_total)| bytes_total == 5),
        "every total: {totals:?}"
    );
    assert_eq!(
        totals
            .last()
            .map(|&(done, bytes_done, _)| (done, bytes_done)),
        Some((2, 5))
    );
}

/// A message kept with a part or character references left out is named by
/// its file, its time and its `address` attribute as the file writes it, so
/// the run can say which one it is (#1707). A repeated copy is named once,
/// and a message the read skips is not named at all.
#[test]
fn a_message_kept_with_something_left_out_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    let copy = r#"<sms address="(555) 555-0101" date="1400773261000" type="1" body="a&#0;b"/>"#;
    fs::write(
        &input,
        format!(
            r#"<smses>{copy}{copy}<sms address="+15555550101" date="1400773300000" type="3" body="&#0;"/><mms date="1400773900000" msg_box="1" address="+15555550101~+15555550102"><parts><part ct="text/plain" text="photo&#0;"/><part ct="image/jpeg" name="pic.jpg" data="%%%"/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#
        ),
    )
    .unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (_, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
    let file = input.display().to_string();
    assert_eq!(
        report.left_out,
        [
            LeftOut {
                file: file.clone(),
                message: "message of 2014-05-22T15:41:01Z with (555) 555-0101".into(),
                counts: LeftOutCounts {
                    unreadable_parts: 0,
                    dropped_character_references: 1,
                },
            },
            LeftOut {
                file,
                message: "message of 2014-05-22T15:51:40Z with +15555550101~+15555550102".into(),
                counts: LeftOutCounts {
                    unreadable_parts: 1,
                    dropped_character_references: 1,
                },
            },
        ]
    );
    assert_eq!(report.skipped_unreadable_part(), 1);
    assert_eq!(report.dropped_character_references(), 2);
}
