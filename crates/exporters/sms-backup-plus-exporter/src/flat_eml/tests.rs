use super::*;
use message_ir::IdentityType;

#[test]
fn parses_flat_received() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("msg.eml");
    std::fs::write(
        &path,
        b"From: alice@unknown.email\r\n\
To: me@example.com\r\n\
Subject: SMS with Alice\r\n\
X-smssync-type: 1\r\n\
X-smssync-address: 4075550107\r\n\
X-smssync-date: 1609459200000\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello from Alice\r\n",
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = MailHeaders::from_mail(&mail);
    let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
    let owner = Owner::new(owners, &[]).in_country(phone::country("US"));
    let message = parse_flat_eml_mail(&path, &mail, &headers, &owner).unwrap();
    assert!(!message.is_from_me);
    assert_eq!(message.text.trim(), "Hello from Alice");
    assert_eq!(message.chat_key, "+14075550107");
    assert!((message.timestamp_secs - 1_609_459_200.0).abs() < 0.001);
}

#[test]
fn individual_chat_uses_first_non_owner_address() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("msg.eml");
    std::fs::write(
        &path,
        b"From: me@example.com\r\n\
To: alice@unknown.email\r\n\
Subject: SMS with Alice\r\n\
X-smssync-type: 2\r\n\
X-smssync-address: 5555550100~4075550107\r\n\
X-smssync-date: 1609459200000\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello\r\n",
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = MailHeaders::from_mail(&mail);
    let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
    let message = parse_flat_eml_mail(
        &path,
        &mail,
        &headers,
        &Owner::new(owners, &["me@example.com".into()]).in_country(phone::country("US")),
    )
    .unwrap();
    assert_eq!(message.chat_key, "+14075550107");
    assert!(message.is_from_me);
}

#[test]
fn early_2000s_ms_dates_are_not_treated_as_seconds() {
    // 2001-01-01T00:00:00Z as epoch ms is < 1e12; the old >1e12 cutoff
    // would have treated this as seconds (~year 32995).
    let ms = 978_307_200_000_i64;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("msg.eml");
    std::fs::write(
        &path,
        format!(
            "From: alice@unknown.email\r\n\
To: me@example.com\r\n\
Subject: SMS with Alice\r\n\
X-smssync-type: 1\r\n\
X-smssync-address: 4075550107\r\n\
X-smssync-date: {ms}\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
old message\r\n"
        ),
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = MailHeaders::from_mail(&mail);
    let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
    let message = parse_flat_eml_mail(&path, &mail, &headers, &Owner::new(owners, &[])).unwrap();
    assert!((message.timestamp_secs - 978_307_200.0).abs() < 0.001);
}

#[test]
fn sent_detection_uses_exact_owner_email() {
    fn headers_with_from(from: &str) -> MailHeaders {
        MailHeaders {
            smssync_datatype: Datatype::Missing,
            smssync_type: String::new(),
            smssync_address: String::new(),
            smssync_date: String::new(),
            smssync_id: String::new(),
            subject: String::new(),
            from: from.into(),
            to: String::new(),
            date: String::new(),
        }
    }
    // `ce@example.com` is a substring of `alice@example.com`; substring
    // matching would misclassify this received message as sent.
    let owner = Owner::new(
        OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap(),
        &["ce@example.com".into()],
    );
    assert!(!is_sent(&headers_with_from("alice@example.com"), &owner));
    // Exact addr-spec matches still detect sent mail, including the
    // `Name <addr>` form.
    let owner = Owner::new(
        OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap(),
        &["Alice@Example.com ".into()],
    );
    assert!(is_sent(&headers_with_from("alice@example.com"), &owner));
    assert!(is_sent(
        &headers_with_from("Alice <alice@example.com>"),
        &owner
    ));
}

/// SMS Backup+ titles a thread "SMS with <who>". For a saved contact
/// that is their name; for an unsaved one it is the number, which beside
/// an address is that address and never a display name. With no address,
/// the name is taken as it is (#1593).
#[test]
fn a_subject_naming_a_number_gives_no_contact_name_beside_an_address() {
    assert_eq!(
        contact_name_from_subject("SMS with +15555550101", true),
        None
    );
    assert_eq!(contact_name_from_subject("SMS with 5550101", true), None);
    assert_eq!(
        contact_name_from_subject("SMS with Sam", true).as_deref(),
        Some("Sam")
    );
    assert_eq!(
        contact_name_from_subject("SMS with +1 555 0101", false).as_deref(),
        Some("+1 555 0101")
    );
}

/// A mail with no `X-smssync-date` header takes its time from `Date`.
#[test]
fn a_mail_without_the_smssync_date_is_timed_by_its_date_header() {
    let headers = MailHeaders {
        smssync_datatype: Datatype::Missing,
        smssync_type: "1".into(),
        smssync_address: "4075550107".into(),
        smssync_date: String::new(),
        smssync_id: String::new(),
        subject: "SMS with Alice".into(),
        from: String::new(),
        to: String::new(),
        date: "Fri, 01 Jan 2021 00:00:00 +0000".into(),
    };
    assert_eq!(timestamp_seconds(&headers), Some((1_609_459_200.0, false)));
}

/// The headers of a received mail of `datatype` whose `X-smssync-date` is
/// `smssync_date` and whose `Date` header is `date`.
fn dated(datatype: &str, smssync_date: &str, date: &str) -> MailHeaders {
    MailHeaders {
        smssync_datatype: Datatype::from_header(datatype),
        smssync_type: "1".into(),
        smssync_address: "4075550107".into(),
        smssync_date: smssync_date.into(),
        smssync_id: String::new(),
        subject: "SMS with Alice".into(),
        from: String::new(),
        to: String::new(),
        date: date.into(),
    }
}

/// 2021-01-01T00:00:00Z, as SMS Backup+ writes it in `Date`.
const NEW_YEAR_2021: &str = "Fri, 01 Jan 2021 00:00:00 +0000";

/// An SMS's, a call's and a WhatsApp message's date are milliseconds.
#[test]
fn an_sms_date_is_read_in_milliseconds() {
    for datatype in ["SMS", "sms", "CALLLOG", "WHATSAPP"] {
        assert_eq!(
            timestamp_seconds(&dated(datatype, "1609459200123", "")),
            Some((1_609_459_200.123, true)),
            "{datatype}"
        );
    }
}

/// An MMS's date is Android's seconds, however small or large the number.
#[test]
fn an_mms_date_is_read_in_seconds() {
    for datatype in ["MMS", "mms"] {
        assert_eq!(
            timestamp_seconds(&dated(datatype, "1609459200", "")),
            Some((1_609_459_200.0, false)),
            "{datatype}"
        );
    }
    // 1973, which the old size guess read as milliseconds.
    assert_eq!(
        timestamp_seconds(&dated("MMS", "100000000", "")),
        Some((100_000_000.0, false))
    );
}

/// A date past the year 9999 in its unit is unreadable, and the mail takes
/// its time from `Date`, or has none.
#[test]
fn a_date_past_the_bound_is_not_read() {
    // The millisecond date of a 2021 MMS is past the bound when read in
    // seconds, as is a millisecond date past 9999.
    for (datatype, date) in [("MMS", "1609459200000"), ("SMS", "253402300800000")] {
        assert_eq!(
            timestamp_seconds(&dated(datatype, date, "")),
            None,
            "{datatype}"
        );
        assert_eq!(
            timestamp_seconds(&dated(datatype, date, NEW_YEAR_2021)),
            Some((1_609_459_200.0, false)),
            "{datatype}"
        );
    }
}

/// A mail with no datatype comes from an SMS Backup+ older than the header,
/// and may be an SMS or an MMS: its date is read in the unit that agrees
/// with `Date`, and in milliseconds when it has no `Date`.
#[test]
fn a_mail_without_a_datatype_is_dated_in_the_unit_its_date_header_agrees_with() {
    // An SMS of October 2010, and an MMS of the same second.
    let sent = "Mon, 25 Oct 2010 09:46:40 +0000";
    assert_eq!(
        timestamp_seconds(&dated("", "1288000000123", sent)),
        Some((1_288_000_000.123, true))
    );
    assert_eq!(
        timestamp_seconds(&dated("", "1288000000", sent)),
        Some((1_288_000_000.0, false))
    );
    // With no `Date`, milliseconds.
    assert_eq!(
        timestamp_seconds(&dated("", "1609459200123", "")),
        Some((1_609_459_200.123, true))
    );
    // Agreeing with `Date` in neither unit, the time is `Date`'s.
    assert_eq!(
        timestamp_seconds(&dated("", "1609459260123", NEW_YEAR_2021)),
        Some((1_609_459_200.0, false))
    );
}

/// A datatype SMS Backup+ never writes names no unit: the time comes from
/// `Date`, or the mail has none.
#[test]
fn a_mail_of_an_unknown_datatype_is_timed_by_its_date_header() {
    assert_eq!(
        timestamp_seconds(&dated("RCS", "1609459260123", NEW_YEAR_2021)),
        Some((1_609_459_200.0, false))
    );
    assert_eq!(timestamp_seconds(&dated("RCS", "1609459260123", "")), None);
}

/// The committed MMS fixture carries its date in seconds, as SMS Backup+
/// writes an MMS's.
#[test]
fn the_mms_fixture_is_dated_in_seconds() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/flat_mms_seconds.eml");
    let bytes = std::fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = MailHeaders::from_mail(&mail);
    let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
    let owner = Owner::new(owners, &["me@example.com".to_string()]);
    let message = parse_flat_eml_mail(&path, &mail, &headers, &owner).unwrap();
    assert_eq!(message.timestamp_secs, 1_609_459_380.0);
    assert!(!message.has_milliseconds);
    assert_eq!(message.text.trim(), "An MMS dated in seconds");
}

#[test]
fn group_chat_id_does_not_collide_on_digit_split() {
    let (k1, _) = phone::group_chat_id("group-", &["12".to_string(), "34".to_string()]);
    let (k2, _) = phone::group_chat_id("group-", &["123".to_string(), "4".to_string()]);
    assert_ne!(k1, k2);
    assert_eq!(k1, "group-2:12_2:34");
    assert_eq!(k2, "group-3:123_1:4");
}

/// Parse `eml` (written with `\n` line ends) as one flat EML.
fn parse(eml: &str, owners: &[&str]) -> Option<ParsedMessage> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("msg.eml");
    std::fs::write(&path, eml.replace('\n', "\r\n")).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = MailHeaders::from_mail(&mail);
    let owners: Vec<String> = owners.iter().map(|s| s.to_string()).collect();
    let owners = OwnerHandleSet::from_phones(&owners).unwrap();
    // The fixtures are from a US phone, as the import form states.
    let owner =
        Owner::new(owners, &["me@example.com".to_string()]).in_country(phone::country("US"));
    parse_flat_eml_mail(&path, &mail, &headers, &owner)
}

/// An incoming MMS from 4075550107 whose MIME parts are `body`.
fn mms_mail(body: &str) -> ParsedMessage {
    parse(
        &format!(
            "From: x@unknown.email\nTo: me@example.com\nSubject: SMS with X\nX-smssync-type: 1\nX-smssync-address: 4075550107\nX-smssync-date: 1609459200000\nMIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=\"b\"\n\n{body}--b--\n"
        ),
        &["5555550100"],
    )
    .unwrap()
}

/// Every `text/plain` part is the message's text, in the order of the
/// parts, and nothing is removed for repeating another.
#[test]
fn every_text_part_is_kept_in_order() {
    let message = mms_mail(
        "--b\nContent-Type: text/plain; charset=utf-8\n\nTickets attached\n--b\nContent-Type: text/plain; charset=utf-8\n\nAll three are for Friday\n--b\nContent-Type: text/plain; charset=utf-8\n\nTickets attached\n",
    );
    assert_eq!(
        message.text,
        "Tickets attached\nAll three are for Friday\nTickets attached"
    );
}

/// A contact card is a text type, and an attachment all the same.
#[test]
fn a_contact_card_is_an_attachment() {
    let message = mms_mail(
        "--b\nContent-Type: text/plain; charset=utf-8\n\ncard\n--b\nContent-Type: text/x-vcard; name=\"sam.vcf\"\n\nBEGIN:VCARD\nFN:Sam\nEND:VCARD\n",
    );
    assert_eq!(message.text, "card");
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(
        message.attachments[0].original_name.as_deref(),
        Some("sam.vcf")
    );
    assert!(message.attachments[0].data.starts_with(b"BEGIN:VCARD"));
}

/// A part whose bytes cannot be decoded is neither text nor an
/// attachment, and it is counted.
#[test]
fn a_part_that_cannot_be_decoded_is_counted() {
    let message = mms_mail(
        "--b\nContent-Type: text/plain; charset=utf-8\n\nhi\n--b\nContent-Type: image/jpeg\nContent-Transfer-Encoding: base64\n\n@@@@\n",
    );
    assert_eq!(message.text, "hi");
    assert!(message.attachments.is_empty());
    assert_eq!(message.unreadable_parts, 1);
}

/// SMS Backup+ writes `X-smssync-type` on a call-log mail too, holding
/// the call's type, so only `X-smssync-datatype` tells a call from a text.
#[test]
fn a_call_log_mail_is_not_a_text_message() {
    let message = parse(
        "From: x@unknown.email\nTo: me@example.com\nSubject: Call with Alice\nX-smssync-datatype: CALLLOG\nX-smssync-type: 1\nX-smssync-address: 4075550107\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\n123s (00:02:03)\n4075550107 (incoming call)\n",
        &["5555550100"],
    );
    assert!(message.is_none(), "{:?}", message.map(|m| m.text));
}

/// An MMS whose address list names the owner in national form
/// (`07700900123` for `+447700900123`) is one-to-one with the other number.
#[test]
fn the_owner_in_national_form_is_not_a_peer() {
    let message = parse(
        "From: x@unknown.email\nTo: me@example.com\nSubject: MMS with X\nX-smssync-type: 132\nX-smssync-address: 07700900123~+447700900456\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
        &["+447700900123"],
    )
    .unwrap();
    assert_eq!(message.conversation_type, IrConversationType::Individual);
}

fn received_from(address: &str) -> ParsedMessage {
    parse(
        &format!("From: x@unknown.email\nTo: me@example.com\nSubject: SMS with X\nX-smssync-type: 1\nX-smssync-address: {address}\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n"),
        &["5555550100"],
    )
    .unwrap()
}

#[test]
fn an_international_number_keeps_its_country() {
    // +65 5555 0100 is no one's number. The note on the `phone` crate's `mod
    // tests` says why.
    let message = received_from("+6555550100");
    assert_eq!(crate::identity::chat_id_for(&message), "+6555550100");
}

#[test]
fn an_email_address_and_a_sender_name_are_not_numbers() {
    let from_an_address = received_from("john1985@example.com");
    assert_eq!(
        crate::identity::chat_id_for(&from_an_address),
        "john1985@example.com"
    );
    let sender = from_an_address.sender.unwrap();
    assert_eq!(sender.kind(), IdentityType::Email);
    let from_a_name = received_from("AMAZON");
    assert_eq!(crate::identity::chat_id_for(&from_a_name), "AMAZON");
    assert_eq!(from_a_name.sender.unwrap().kind(), IdentityType::Other);
}

/// The `From` of a group message names its sender inside the address,
/// `"Bob" <+447700900789@unknown.email>`.
#[test]
fn a_group_sender_is_read_from_the_from_address() {
    let message = parse(
        "From: \"Bob\" <+447700900789@unknown.email>\nTo: me@example.com\nSubject: MMS with X\nX-smssync-type: 132\nX-smssync-address: +447700900456~+447700900789\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
        &["+447700900123"],
    )
    .unwrap();
    assert_eq!(message.sender.unwrap().key(), "+447700900789");
}

/// A digit in the display name of `From` is not part of the number:
/// `"Mom 2" <+14075550108@unknown.email>` is from 4075550108.
#[test]
fn a_name_with_a_digit_does_not_move_the_sender() {
    let message = parse(
        "From: \"Mom 2\" <+14075550108@unknown.email>\nTo: me@example.com\nSubject: SMS with group\nX-smssync-type: 132\nX-smssync-address: 4075550150~4075550108~5555550100\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
        &["5555550100"],
    )
    .unwrap();
    assert_eq!(message.conversation_type, IrConversationType::Group);
    assert_eq!(message.sender.unwrap().key(), "+14075550108");
}

/// A received group message whose `From` names nobody in the group has
/// no sender: the first address of the list would be a guess.
#[test]
fn a_group_message_from_nobody_in_the_group_has_no_sender() {
    for from in [
        "Bob <bob@example.com>",
        "\"Carol\" <+14075550152@unknown.email>",
    ] {
        let message = parse(
            &format!("From: {from}\nTo: me@example.com\nSubject: SMS with group\nX-smssync-type: 132\nX-smssync-address: 4075550150~4075550108\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n"),
            &["5555550100"],
        )
        .unwrap();
        assert_eq!(
            message.conversation_type,
            IrConversationType::Group,
            "{from}"
        );
        assert!(message.sender.is_none(), "{from}: {:?}", message.sender);
    }
}

/// SMS Backup+ names one address in `X-smssync-address` for a sent MMS
/// and every recipient in `To`, so `To` is what makes it a group.
#[test]
fn a_sent_mms_to_three_recipients_is_a_group_of_three() {
    let message = parse(
        "From: me@example.com\nTo: \"Alice\" <+14075550150@unknown.email>, Bob <4075550151@unknown.email>,\n \"Carol\" <carol@example.org>\nSubject: SMS with Alice\nX-smssync-type: 128\nX-smssync-address: 4075550150\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi all\n",
        &["5555550100"],
    )
    .unwrap();
    assert!(message.is_from_me);
    assert_eq!(message.conversation_type, IrConversationType::Group);
    let mut keys: Vec<&str> = message.participants.iter().map(Handle::key).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["+14075550150", "+14075550151", "carol@example.org"]);
    assert!(message.sender.is_none());
}

/// SMS Backup+ names the sender of a received MMS in `From` and every
/// other recipient, the owner included, in `To`. A reply in a group lands
/// in the conversation the owner's sent MMS to that group made, with its
/// sender read from `From`.
#[test]
fn a_received_group_mms_joins_the_group_the_owner_sent_to() {
    let sent = parse(
        "From: me@example.com\nTo: \"Alice\" <+14075550150@unknown.email>, \"Carol\" <carol@example.org>\nSubject: SMS with Alice\nX-smssync-type: 128\nX-smssync-address: 4075550150\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi all\n",
        &["5555550100"],
    )
    .unwrap();
    let received = parse(
        "From: \"Carol\" <carol@example.org>\nTo: <+15555550100@unknown.email>, \"Alice\" <+14075550150@unknown.email>\nSubject: SMS with Carol\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459260000\nContent-Type: text/plain; charset=utf-8\n\nhello\n",
        &["5555550100"],
    )
    .unwrap();
    assert_eq!(received.conversation_type, IrConversationType::Group);
    assert_eq!(received.chat_key, sent.chat_key);
    assert_eq!(received.sender.unwrap().key(), "carol@example.org");
}

/// A received group MMS names the owner in `To` under the address on
/// their own contact card. When that is no number or email address the
/// owner gave, the owner would count as a participant, so the message is
/// not filed as a group. `From` names who wrote it, so it goes to that
/// person's one-to-one conversation, and neither the `X-smssync-address`
/// peer nor the subject's name is taken for its sender.
#[test]
fn a_received_group_mms_that_does_not_name_the_owner_is_filed_under_its_from() {
    let message = parse(
        "From: carol@example.org\nTo: <me@icloud.example>, <+14075550150@unknown.email>\nSubject: SMS with Alice\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459260000\nContent-Type: text/plain; charset=utf-8\n\nhello\n",
        &["5555550100"],
    )
    .unwrap();
    assert_eq!(message.conversation_type, IrConversationType::Individual);
    assert_eq!(message.chat_key, "carol@example.org");
    assert_eq!(
        message.sender.map(Handle::into_key).as_deref(),
        Some("carol@example.org")
    );
    assert_eq!(message.name_alias, None);
    assert!(message.owner_not_named);
}

/// When a received group MMS names neither the owner nor, in `From`, its
/// sender, it stays in the `X-smssync-address` conversation with no
/// sender: the peer there is not known to have written it.
#[test]
fn a_received_group_mms_naming_neither_the_owner_nor_a_sender_has_no_sender() {
    let message = parse(
        "From: \nTo: <me@icloud.example>, <+14075550150@unknown.email>\nSubject: SMS with Alice\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459260000\nContent-Type: text/plain; charset=utf-8\n\nhello\n",
        &["5555550100"],
    )
    .unwrap();
    assert_eq!(message.conversation_type, IrConversationType::Individual);
    assert_eq!(message.chat_key, "+14075550150");
    assert!(message.sender.is_none(), "{:?}", message.sender);
    assert_eq!(message.name_alias, None);
    assert!(message.owner_not_named);
}

/// A received MMS from one person names only the owner in `To`, so it
/// stays one-to-one with the number in `X-smssync-address`.
#[test]
fn a_received_mms_from_one_person_stays_one_to_one() {
    let message = parse(
        "From: \"Alice\" <alice@example.org>\nTo: <+15555550100@unknown.email>\nSubject: SMS with Alice\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
        &["5555550100"],
    )
    .unwrap();
    assert_eq!(message.conversation_type, IrConversationType::Individual);
    assert_eq!(message.chat_key, "+14075550150");
}

/// A sent message with one recipient in `To` stays one-to-one with the
/// number in `X-smssync-address`, even when `To` gives that person's
/// email address instead of the number.
#[test]
fn a_sent_message_to_one_recipient_stays_one_to_one() {
    let message = parse(
        "From: me@example.com\nTo: \"Alice\" <alice@example.org>\nSubject: SMS with Alice\nX-smssync-type: 2\nX-smssync-address: 4075550150\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
        &["5555550100"],
    )
    .unwrap();
    assert_eq!(message.conversation_type, IrConversationType::Individual);
    assert_eq!(message.chat_key, "+14075550150");
}

/// A received group MMS that does not name the owner is filed under the
/// sender in `From`, and the sender's name is the display name `From` gives,
/// never the subject's, which names the `X-smssync-address` contact (#1547).
#[test]
fn a_group_mms_not_naming_the_owner_names_its_sender_from_the_from_header() {
    let message = parse(
        "From: \"Carol\" <carol@example.com>\nTo: <me@icloud.example>, <+14075550150@unknown.email>\nSubject: SMS with Alice\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459260000\nContent-Type: text/plain; charset=utf-8\n\nhello\n",
        &["5555550100"],
    )
    .unwrap();
    assert_eq!(message.chat_key, "carol@example.com");
    assert_eq!(
        message.sender.map(Handle::into_key).as_deref(),
        Some("carol@example.com")
    );
    assert_eq!(message.name_alias.as_deref(), Some("Carol"));
}

/// SMS Backup+ gives a sender it has no name for their number as the
/// display name, written as Android stored it. That is no name.
#[test]
fn a_from_display_name_written_like_a_number_is_no_name() {
    for name in [
        "+14075550111",
        "4075550111",
        "407-555-0111",
        "(407) 555-0111",
    ] {
        let message = parse(
            &format!("From: \"{name}\" <carol@example.com>\nTo: <me@icloud.example>, <+14075550150@unknown.email>\nSubject: SMS with Alice\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459260000\nContent-Type: text/plain; charset=utf-8\n\nhello\n"),
            &["5555550100"],
        )
        .unwrap();
        assert_eq!(message.name_alias, None, "{name}");
    }
}

/// A mail with no address is keyed by the name in its subject, even a name
/// written like a number: with no address beside it, the name cannot be
/// mistaken for one (#1593).
#[test]
fn a_mail_with_no_address_is_keyed_by_a_name_written_like_a_number() {
    for name in ["+1 555 0101", "5550101"] {
        let message = parse(
            &format!("From: me@example.com\nTo: x@unknown.email\nSubject: SMS with {name}\nX-smssync-type: 2\nX-smssync-address: \nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n"),
            &["5555550100"],
        )
        .unwrap_or_else(|| panic!("{name}: skipped"));
        assert_eq!(
            crate::identity::chat_id_for(&message),
            format!("name:{name}")
        );
    }
}

/// A mail that names nobody, with no address and a subject that gives no
/// name, is the conversation that names nobody: no participant, no sender
/// (#1591).
#[test]
fn a_mail_that_names_nobody_is_kept_in_the_conversation_that_names_nobody() {
    for (typ, from, to) in [
        ("2", "me@example.com", ""),
        ("1", "unknown@unknown.email", "me@example.com"),
    ] {
        let message = parse(
            &format!("From: {from}\nTo: {to}\nSubject: SMS\nX-smssync-type: {typ}\nX-smssync-address: \nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n"),
            &["5555550100"],
        )
        .unwrap_or_else(|| panic!("type {typ}: skipped"));
        assert_eq!(
            crate::identity::chat_id_for(&message),
            message_ir::NAMELESS_CHAT_ID
        );
        assert!(
            message.participants.is_empty(),
            "{:?}",
            message.participants
        );
        assert!(message.sender.is_none(), "{:?}", message.sender);
        assert_eq!(message.name_alias, None);
    }
}
