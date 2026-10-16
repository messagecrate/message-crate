use super::*;
use crate::write::SbrBackupSession;
use ir::Direction;
use message_crate_core::{CancelFlag, LogSink, ProgressSink};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

/// Sinks that drop what they get, and a cancel flag nobody sets, for
/// reader options a helper returns.
static NO_LOG: LazyLock<LogSink> = LazyLock::new(LogSink::none);
static NO_PROGRESS: LazyLock<ProgressSink> = LazyLock::new(ProgressSink::none);
static NO_CANCEL: LazyLock<CancelFlag> = LazyLock::new(CancelFlag::default);

fn opts<'a>(
    owner_phones: &'a [String],
    attachments_dir: Option<&'a Path>,
    spool: Option<&'a AttachmentSpool>,
) -> ReadOptions<'a> {
    ReadOptions {
        owner_phones,
        attachments_dir,
        spool,
        files: if spool.is_some() {
            AttachmentFiles::Originals
        } else {
            AttachmentFiles::Skip
        },
        sinks: Sinks {
            log: &NO_LOG,
            progress: &NO_PROGRESS,
            cancel: &NO_CANCEL,
        },
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
    let (mut documents, _) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
    stage_read_attachments(&mut documents, &opts(&[], Some(&stage), Some(&spool))).unwrap();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 1, "one file staged");
    let staged: Vec<_> = fs::read_dir(&stage)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(staged.len(), 1);
    assert_eq!(fs::metadata(&staged[0]).unwrap().len(), 5);
    assert_eq!(
        documents[0].export.owner_identity.as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        documents[0].messages[0].attachments[0].size_bytes,
        Some(5),
        "decoded aGVsbG8= is five bytes; size_bytes lets the import skip re-hashing"
    );
    assert_eq!(
        documents[0].messages[0].source.as_ref().unwrap().fields["attrs"]["extra"],
        "yes"
    );
    let mut writer = SbrBackupSession::create(&output).unwrap();
    writer.append_document(&documents[0]).unwrap();
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
    let (mut documents, _) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
    stage_read_attachments(&mut documents, &opts(&[], Some(&stage), Some(&spool))).unwrap();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 1, "one file staged");
    let mut writer = SbrBackupSession::create(&output).unwrap();
    writer.append_document(&documents[0]).unwrap();
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
    let (mut documents, _) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
    stage_read_attachments(&mut documents, &opts(&[], Some(&stage), Some(&spool))).unwrap();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 1, "one file staged");
    assert_eq!(documents[0].messages[0].text, "card");
    let mut writer = SbrBackupSession::create(&output).unwrap();
    writer.append_document(&documents[0]).unwrap();
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
    let (documents, report) = read_backup(&fixture, opts(&[], None, None)).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(documents.len(), 1, "every message lands in the same group");
    let document = &documents[0];
    assert_eq!(
        document.conversation.chat_identifier,
        "chat-group-12:+15555550101_12:+15555550102_12:+15555550103"
    );
    assert_eq!(
        document.export.owner_identity.as_deref(),
        Some("+15555550100")
    );
    let mut participants: Vec<_> = document
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
    let seen: Vec<(&str, Direction, Option<&str>)> = document
        .messages
        .iter()
        .map(|m| (m.text.as_str(), m.direction, m.sender_identity.as_deref()))
        .collect();
    assert_eq!(
        seen,
        [
            ("from lee", Direction::Incoming, Some("+15555550103")),
            ("no from", Direction::Incoming, None),
            ("sent by me", Direction::Outgoing, Some("+15555550100")),
            ("from ana", Direction::Incoming, Some("+15555550102")),
        ]
    );
}

/// An Import Run reads one backup, and SMS Backup & Restore writes each
/// backup as one file, so a directory is refused rather than walked.
#[test]
fn a_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("sms-1.xml"),
        r#"<smses><sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="hi"/></smses>"#,
    )
    .unwrap();
    let owner = vec!["+15555550100".to_string()];
    let err = read_backup(dir.path(), opts(&owner, None, None)).unwrap_err();
    assert!(err.to_string().contains("not a directory"), "{err:#}");
}

/// A backup cut off partway through a message, as a copy that stopped early
/// leaves it.
fn cut_off_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cut_off_backup.xml")
}

/// A file that is not valid XML is refused whole, not read in part, whether
/// the owner's phone is given or inferred, and both passes say why in the
/// same words (#2028).
#[test]
fn a_file_cut_off_partway_is_refused_whole() {
    let input = cut_off_fixture();
    let owner = ["+15555550100".to_string()];
    for owner_phones in [&owner[..], &[]] {
        let error = read_backup(&input, opts(owner_phones, None, None)).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains(&format!("{} is not valid XML", input.display())),
            "{message}"
        );
        assert!(
            message.ends_with("the file ends before </addrs>"),
            "{message}"
        );
    }
}

/// A message that cannot be kept in a file that is valid XML is skipped and
/// counted, and the read goes on to the rest of the file.
#[test]
fn a_message_that_cannot_be_kept_does_not_stop_the_read() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(
        &input,
        r#"<smses><sms protocol="0" address="+15555550101" date="not a date" type="1" body="lost"/><sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="kept"/></smses>"#,
    )
    .unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (documents, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].messages.len(), 1);
    assert_eq!(documents[0].messages[0].text, "kept");
    assert_eq!(report.skipped_invalid_date, 1);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
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

    let (documents, _) = read_backup(&input, opts(&owner, Some(&stage), Some(&spool))).unwrap();

    let attachments: Vec<_> = documents[0]
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

/// A run that does not copy attachments keeps no payload anywhere.
#[test]
fn without_a_spool_no_payload_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="a.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#).unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (documents, _) = read_backup(&input, opts(&owner, None, None)).unwrap();
    let attachment = &documents[0].messages[0].attachments[0];
    assert!(attachment.bytes.is_none() && attachment.path.is_none());
    assert_eq!(attachment.size_bytes, Some(5), "the size is still recorded");
}

/// Read `messages` wrapped in `<smses>` with +15555550100 as the owner.
fn read_xml(messages: &str) -> Vec<ConversationDocument> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, format!("<smses>{messages}</smses>")).unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (documents, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    documents
}

fn roster(document: &ConversationDocument) -> Vec<(&str, Option<&str>)> {
    document
        .conversation
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
    let (documents, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
    let texts: Vec<_> = documents[0]
        .messages
        .iter()
        .map(|m| m.text.as_str())
        .collect();
    assert_eq!(texts, ["hi", "hey"]);
    assert_eq!(report.sms_seen, 3);
    assert_eq!(report.duplicates_dropped, 1);
}

/// Two MMS alike in everything but the attachment are two messages: the
/// attachment's digest is part of what the dedupe step compares.
#[test]
fn two_messages_that_differ_only_in_an_attachment_are_both_kept() {
    let mms = |data: &str| {
        format!(
            r#"<mms date="1400773261000" msg_box="1" address="+15555550101"><parts><part ct="text/plain" text="look"/><part ct="image/jpeg" name="pic.jpg" data="{data}"/></parts><addrs><addr address="+15555550101" type="137"/><addr address="+15555550100" type="151"/></addrs></mms>"#
        )
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(
        &input,
        format!("<smses>{}{}</smses>", mms("aGVsbG8="), mms("d29ybGQ=")),
    )
    .unwrap();
    let owner = vec!["+15555550100".to_string()];
    let (documents, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
    assert_eq!(report.duplicates_dropped, 0);
    let messages = &documents[0].messages;
    assert_eq!(messages.len(), 2);
    assert_ne!(messages[0].guid, messages[1].guid);
}

#[test]
fn a_contact_name_names_the_peer_of_a_one_to_one_conversation() {
    // Only sent messages, which name the peer but carry no sender.
    let documents = read_xml(
        r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="2" body="hi" contact_name="Sam"/>"#,
    );
    assert_eq!(roster(&documents[0]), [("+15555550101", Some("Sam"))]);
}

#[test]
fn a_placeholder_contact_name_names_nobody() {
    for placeholder in ["null", "(Unknown)"] {
        let documents = read_xml(&format!(
            r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="2" body="hi" contact_name="{placeholder}"/><sms protocol="0" address="+15555550101" date="1400773262000" type="1" body="hey" contact_name="{placeholder}"/>"#,
        ));
        assert_eq!(
            roster(&documents[0]),
            [("+15555550101", None)],
            "{placeholder}"
        );
        assert_eq!(documents[0].messages[1].sender_display_name, None);
    }
}

/// A group MMS's `contact_name` is the members' names joined by ", ",
/// which names the group, so no participant takes it as a name.
#[test]
fn a_group_contact_name_names_no_sender() {
    let documents = read_xml(
        r#"<mms date="1400773400000" msg_box="1" address="+15555550101~+15555550102~+15555550100" contact_name="Ana, Lee"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+15555550102" type="137"/><addr address="+15555550101" type="151"/><addr address="+15555550100" type="151"/></addrs></mms>"#,
    );
    assert_eq!(
        roster(&documents[0]),
        [("+15555550101", None), ("+15555550102", None)]
    );
    assert_eq!(documents[0].messages[0].sender_display_name, None);
}

#[test]
fn a_subject_is_kept_and_a_null_one_dropped() {
    let documents = read_xml(
        r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="one" subject="Plans"/><sms protocol="0" address="+15555550101" date="1400773262000" type="1" body="two" subject="null"/>"#,
    );
    let subjects: Vec<_> = documents[0]
        .messages
        .iter()
        .map(|m| m.subject.as_deref())
        .collect();
    assert_eq!(subjects, [Some("Plans"), None]);
}

#[test]
fn an_attachment_carries_its_payload_digest() {
    let documents = read_xml(
        r#"<mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms>"#,
    );
    assert_eq!(
        documents[0].messages[0].attachments[0]
            .digest_sha256
            .as_deref(),
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
    let (documents, _) = read_backup(&input, opts(&owner, None, None)).unwrap();
    assert_eq!(
        documents[0].conversation.participants.len(),
        1,
        "{:?}",
        documents[0].conversation.participants
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
    let (documents, _) = read_backup(&input, opts(&owners, None, None)).unwrap();
    assert_eq!(
        documents[0].export.owner_identity.as_deref(),
        Some("+15555550109")
    );
}

#[test]
fn an_international_number_keeps_its_country() {
    // +65 5555 0100 is no one's number. The note on the `phone` crate's `mod
    // tests` says why.
    let documents = read_xml(
        r#"<sms protocol="0" address="+6555550100" date="1400773261000" type="1" body="hi"/><sms protocol="0" address="+447700900123" date="1400773261000" type="1" body="hi"/>"#,
    );
    let ids: Vec<_> = documents
        .iter()
        .map(|d| d.conversation.chat_identifier.as_str())
        .collect();
    assert_eq!(ids, ["+447700900123", "+6555550100"]);
}

#[test]
fn an_email_sender_is_an_email_identity() {
    let documents = read_xml(
        r#"<sms protocol="0" address="john1985@example.com" date="1400773261000" type="1" body="hi"/>"#,
    );
    assert_eq!(
        documents[0].conversation.chat_identifier,
        "john1985@example.com"
    );
    let participant = &documents[0].conversation.participants[0];
    assert_eq!(
        participant.identity.as_deref(),
        Some("john1985@example.com")
    );
    assert_eq!(
        documents[0].messages[0].sender_identity.as_deref(),
        Some("john1985@example.com")
    );
}

#[test]
fn a_sender_name_is_kept_as_its_identity() {
    let documents = read_xml(
        r#"<sms protocol="0" address="AMAZON" date="1400773261000" type="1" body="Your parcel"/>"#,
    );
    let participant = &documents[0].conversation.participants[0];
    assert_eq!(participant.identity.as_deref(), Some("AMAZON"));
}

/// With no owner on the form, the owner comes from the sent MMS, and a
/// UK owner keeps its country, as the account's identity does.
#[test]
fn an_inferred_owner_outside_the_us_keeps_its_country() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="2" address="+447700900123"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+447700900456" type="137"/><addr address="+447700900123" type="151"/></addrs></mms></smses>"#).unwrap();
    let (documents, _) = read_backup(&input, opts(&[], None, None)).unwrap();
    assert_eq!(
        documents[0].export.owner_identity.as_deref(),
        Some("+447700900456")
    );
    assert_eq!(documents[0].conversation.chat_identifier, "+447700900123");
}

#[test]
fn verify_e1_4_two_group_senders_in_one_second_get_two_guids() {
    let documents = read_xml(
        r#"<mms date="1400773400100" msg_box="1" address="+15555550101~+15555550102~+15555550100"><parts><part ct="text/plain" text="lol"/></parts><addrs><addr address="+15555550101" type="137"/><addr address="+15555550102" type="151"/><addr address="+15555550100" type="151"/></addrs></mms><mms date="1400773400200" msg_box="1" address="+15555550101~+15555550102~+15555550100"><parts><part ct="text/plain" text="lol"/></parts><addrs><addr address="+15555550102" type="137"/><addr address="+15555550101" type="151"/><addr address="+15555550100" type="151"/></addrs></mms>"#,
    );
    assert_eq!(documents[0].messages.len(), 2, "the exporter keeps both");
    assert_ne!(documents[0].messages[0].guid, documents[0].messages[1].guid);
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
    let (mut documents, _) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
    // A second attachment whose record gives a size, with no file spooled.
    let mut unspooled = documents[0].messages[0].attachments[0].clone();
    unspooled.digest_sha256 = Some("0".repeat(64));
    unspooled.size_bytes = Some(700);
    documents[0].messages[0].attachments.push(unspooled);
    let (progress, totals) = attachment_totals();

    let base = opts(&[], Some(&stage), Some(&spool));
    stage_read_attachments(
        &mut documents,
        &ReadOptions {
            sinks: Sinks {
                progress: &progress,
                ..base.sinks
            },
            ..base
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

/// The fixture's root says when SMS Backup & Restore made the backup.
fn dated_fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dated_backup.xml")
}

/// The Backup Dates the messages of each conversation read from `input`
/// carry, each date once, by chat id.
fn backup_dates(input: &Path) -> Vec<(String, Vec<Option<i64>>)> {
    let owners = ["+15555550100".to_string()];
    let (documents, _) = read_backup(input, opts(&owners, None, None)).unwrap();
    documents
        .into_iter()
        .map(|document| {
            let mut dates: Vec<Option<i64>> = document
                .messages
                .iter()
                .map(|m| m.backup_taken_at_unix_ms)
                .collect();
            dates.dedup();
            (document.conversation.chat_identifier, dates)
        })
        .collect()
}

/// The root element's `backup_date` decides, not when the file was copied.
#[test]
fn the_backup_date_is_the_files_backup_date_attribute() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("dated_backup.xml");
    fs::copy(dated_fixture(), &input).unwrap();
    message_crate_core::testutil::set_modified_unix_ms(&input, 1_000_000_000_000);
    assert_eq!(
        backup_dates(&input),
        [
            ("+15555550101".to_string(), vec![Some(1_790_793_912_000)]),
            ("+15555550102".to_string(), vec![Some(1_790_793_912_000)]),
        ]
    );
}

/// A file without `backup_date` is dated by when it was last changed.
#[test]
fn a_file_without_a_backup_date_is_dated_by_its_modification_time() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("undated.xml");
    fs::write(
        &input,
        r#"<smses count="1"><sms address="+15555550101" date="1400773261000" type="1" body="hello" /></smses>"#,
    )
    .unwrap();
    message_crate_core::testutil::set_modified_unix_ms(&input, 1_500_000_000_000);
    assert_eq!(
        backup_dates(&input),
        [("+15555550101".to_string(), vec![Some(1_500_000_000_000)])]
    );
}
