use super::*;
use mailparse::MailHeaderMap;

fn base_sms() -> MailMessage {
    MailMessage {
        chat_identifier: "+15555550101".into(),
        conversation_type: "individual".into(),
        group_title: None,
        participants: vec![Participant {
            identity: "+15555550101".into(),
            display_name: Some("Sam".into()),
        }],
        owner_identity: "+15555550100".into(),
        owner_display_name: None,
        export_source: "sms-backup-restore".into(),
        export_tool: "SMS Backup & Restore".into(),
        export_tool_version: "10.26.003".into(),
        filename_suffix: None,
        message: IrMessage {
            guid: "aabbccddeeff00112233445566778899".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: message_ir::IrService::Sms,
            message_kind: message_ir::IrMessageKind::Sms,
            sender_identity: Some("+15555550101".into()),
            sender_display_name: Some("Sam".into()),
            owner_identity: None,
            subject: None,
            text: "hello from sms".into(),
            attachments: Vec::new(),
            reactions: Vec::new(),
            deletion: None,
            edits: Vec::new(),
            imessage: None,
            source: Some(message_ir::IrSource {
                android_type: Some(1),
                fields: serde_json::from_str(r#"{"address":"+15555550101"}"#).unwrap(),
            }),
        },
        attachments: vec![],
    }
}

/// The extension bag, creating it when the base message has none.
fn im_mut(msg: &mut MailMessage) -> &mut message_ir::IrImessage {
    msg.message.imessage.get_or_insert_with(Default::default)
}

#[test]
fn writes_individual_sms_text_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = write_conversation(tmp.path(), &[base_sms()]).unwrap();
    assert_eq!(dir.file_name().unwrap(), "+15555550101");

    let mut emls: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("eml"))
        .collect();
    emls.sort();
    assert_eq!(emls.len(), 1);
    assert!(
        emls[0]
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("000001_")
    );

    let bytes = fs::read(&emls[0]).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    assert_eq!(
        headers.get_first_value("X-ME-Chat-Identifier").as_deref(),
        Some("+15555550101")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Direction").as_deref(),
        Some("incoming")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Message-Kind").as_deref(),
        Some("sms")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Guid").as_deref(),
        Some("aabbccddeeff00112233445566778899")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Export-Source").as_deref(),
        Some("sms-backup-restore")
    );
    let mid = headers.get_first_value("Message-ID").unwrap();
    assert!(mid.contains("aabbccddeeff00112233445566778899@message-crate.local"));
    assert!(headers.get_first_value("In-Reply-To").is_none());
    let from = headers.get_first_value("From").unwrap();
    assert!(from.contains("Sam"), "From was {from}");
    assert!(from.contains("+15555550101@sms.local"), "From was {from}");
    let to = headers.get_first_value("To").unwrap();
    assert!(to.contains("Me"), "To was {to}");
    assert!(to.contains("+15555550100@sms.local"), "To was {to}");
    assert_eq!(
        headers.get_first_value("Subject").as_deref(),
        Some("Message with Sam")
    );
    let body = mail.get_body().unwrap();
    assert!(body.contains("hello from sms"));
    assert!(!mail.ctype.mimetype.starts_with("multipart/"));
}

#[test]
fn writes_group_mms_with_image_part() {
    let mut msg = base_sms();
    msg.chat_identifier = "chat-group1".into();
    msg.conversation_type = "group".into();
    msg.group_title = Some("Family".into());
    msg.message.message_kind = message_ir::IrMessageKind::Mms;
    msg.participants = vec![
        Participant {
            identity: "+15555550101".into(),
            display_name: Some("Sam".into()),
        },
        Participant {
            identity: "+15555550102".into(),
            display_name: Some("Alex".into()),
        },
    ];
    msg.attachments = vec![MailAttachment {
        bytes: b"\xff\xd8\xfffakejpeg".to_vec(),
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some("photo.jpg".into()),
            mime_type: Some("image/jpeg".into()),
            digest_sha256: Some("deadbeef".into()),
            size_bytes: None,
            missing_reason: None,
        },
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
    }];

    let tmp = tempfile::tempdir().unwrap();
    let dir = write_conversation(tmp.path(), &[msg]).unwrap();
    assert_eq!(dir.file_name().unwrap(), "Family");

    let eml = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    let bytes = fs::read(&eml).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    assert_eq!(
        headers.get_first_value("X-ME-Conversation-Type").as_deref(),
        Some("group")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Group-Title").as_deref(),
        Some("Family")
    );
    assert_eq!(
        headers.get_first_value("Subject").as_deref(),
        Some("Message with Family")
    );
    let to = headers.get_first_value("To").unwrap();
    assert!(to.contains("Family"), "To was {to}");
    assert!(to.contains("chat-group1@chat.local"), "To was {to}");
    assert!(
        !to.contains("+15555550102"),
        "group To should be the chat, not the full roster: {to}"
    );
    let participants = headers.get_first_value("X-ME-Participants").unwrap();
    assert!(participants.contains("+15555550101"));
    assert!(participants.contains("+15555550102"));
    let meta = headers.get_first_value("X-ME-Attachment-Meta").unwrap();
    assert!(meta.contains("photo.jpg"));
    assert!(meta.contains("deadbeef"));
    assert!(mail.ctype.mimetype.starts_with("multipart/"));

    let mut found_image = false;
    fn walk(m: &mailparse::ParsedMail<'_>, found: &mut bool) {
        if m.ctype.mimetype == "image/jpeg" {
            *found = true;
        }
        for sub in &m.subparts {
            walk(sub, found);
        }
    }
    walk(&mail, &mut found_image);
    assert!(found_image, "expected image/jpeg MIME part");
}

#[test]
fn encodes_email_handles_and_imessage_message_id() {
    let mut msg = base_sms();
    msg.chat_identifier = "friend@example.com".into();
    msg.participants = vec![Participant {
        identity: "friend@example.com".into(),
        display_name: Some("Friend".into()),
    }];
    msg.message.sender_identity = Some("friend@example.com".into());
    msg.owner_identity = "me@example.com".into();
    msg.message.service = message_ir::IrService::IMessage;
    msg.message.message_kind = message_ir::IrMessageKind::IMessage;
    msg.message.guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".into();
    msg.export_source = "imessage".into();

    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    let from = headers.get_first_value("From").unwrap();
    assert!(
        from.contains("friend=example.com@identity.local"),
        "From was {from}"
    );
    let to = headers.get_first_value("To").unwrap();
    assert!(to.contains("me=example.com@identity.local"), "To was {to}");
    let mid = headers.get_first_value("Message-ID").unwrap();
    assert!(
        mid.contains("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE@imessage.local"),
        "Message-ID was {mid}"
    );
}

#[test]
fn outgoing_uses_me_and_stable_subject() {
    let mut msg = base_sms();
    msg.message.direction = IrDirection::Outgoing;
    msg.message.sender_identity = Some("+15555550100".into());
    msg.message.sender_display_name = Some("Me".into());
    msg.message.text = "body must not become subject".into();

    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    let from = headers.get_first_value("From").unwrap();
    assert!(from.contains("Me"), "From was {from}");
    let to = headers.get_first_value("To").unwrap();
    assert!(to.contains("Sam"), "To was {to}");
    assert_eq!(
        headers.get_first_value("Subject").as_deref(),
        Some("Message with Sam")
    );
    assert!(
        !headers
            .get_first_value("Subject")
            .unwrap()
            .contains("body must not")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Sender-Identity").as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Owner-Identity").as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        headers
            .get_first_value("X-ME-Owner-Display-Name")
            .as_deref(),
        None // unset on base_sms unless set
    );
}

fn subject_and_to(msg: &MailMessage) -> (String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    (
        headers.get_first_value("Subject").unwrap(),
        headers.get_first_value("To").unwrap(),
    )
}

/// The sender of an outgoing message is the owner, so the sender's name is
/// not the peer's. With no name in the roster the peer is known by handle.
#[test]
fn outgoing_to_a_peer_with_no_name_is_titled_with_the_peer_handle() {
    let mut msg = base_sms();
    msg.participants[0].display_name = None;
    msg.message.direction = IrDirection::Outgoing;
    msg.message.sender_identity = Some("+15555550100".into());
    msg.message.sender_display_name = Some("Me".into());

    let (subject, to) = subject_and_to(&msg);
    assert_eq!(subject, "Message with +15555550101");
    assert!(!to.contains("Me"), "To was {to}");
}

#[test]
fn a_roster_that_lists_the_owner_first_still_addresses_the_other_person() {
    let mut msg = base_sms();
    msg.participants.insert(
        0,
        Participant {
            identity: "+15555550100".into(),
            display_name: Some("Owner".into()),
        },
    );
    msg.message.direction = IrDirection::Outgoing;
    msg.message.sender_identity = Some("+15555550100".into());
    msg.message.sender_display_name = Some("Me".into());

    let (subject, to) = subject_and_to(&msg);
    assert_eq!(subject, "Message with Sam");
    assert!(to.contains("Sam"), "To was {to}");
}

/// With no roster and no sender on the message, the chat identifier is the
/// only place the other person is named.
#[test]
fn an_empty_roster_takes_the_peer_from_the_chat_identifier() {
    let mut msg = base_sms();
    msg.participants.clear();
    msg.message.sender_identity = None;

    let tmp = tempfile::tempdir().unwrap();
    let path = write_conversation_mbox(tmp.path(), &[msg]).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.lines().next(),
        Some("From +15555550101@sms.local Thu May 22 15:41:01 2014")
    );
}

#[test]
fn caller_id_owner_display_and_imessage_extension_headers() {
    let mut msg = base_sms();
    msg.message.direction = IrDirection::Outgoing;
    msg.message.sender_identity = Some("+15555550100".into());
    msg.message.sender_display_name = Some("+15555550100".into());
    msg.owner_display_name = Some("+15555550100".into());
    msg.export_source = "imessage".into();
    msg.message.message_kind = message_ir::IrMessageKind::IMessage;
    im_mut(&mut msg).is_reply = true;
    im_mut(&mut msg).in_reply_to_guid = Some("parent-guid-1111".into());
    im_mut(&mut msg).thread_originator_part = Some(0);
    im_mut(&mut msg).num_replies = Some(2);
    im_mut(&mut msg).send_effect = Some("Sent with Balloons".into());
    msg.message.text = "hello\n\nSent with Balloons".into();
    im_mut(&mut msg).parts =
        serde_json::from_str(r#"[{"index":0,"kind":"run","text":"hello"}]"#).ok();
    im_mut(&mut msg).announcement = None;

    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    let from = headers.get_first_value("From").unwrap();
    assert!(from.contains("+15555550100"), "From was {from}");
    assert!(!from.contains("Me <"), "From was {from}");
    assert_eq!(
        headers.get_first_value("X-ME-Sender-Identity").as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        headers
            .get_first_value("X-ME-Owner-Display-Name")
            .as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Is-Reply").as_deref(),
        Some("true")
    );
    assert_eq!(
        headers
            .get_first_value("X-ME-Thread-Originator-Guid")
            .as_deref(),
        Some("parent-guid-1111")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Send-Effect").as_deref(),
        Some("Sent with Balloons")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Num-Replies").as_deref(),
        Some("2")
    );
    let irt = headers.get_first_value("In-Reply-To").unwrap();
    assert!(irt.contains("parent-guid-1111@imessage.local"), "{irt}");
    assert!(mail.get_body().unwrap().contains("Sent with Balloons"));
}

#[test]
fn tapback_and_handwriting_svg_headers() {
    let mut msg = base_sms();
    msg.export_source = "imessage".into();
    msg.message.message_kind = message_ir::IrMessageKind::Tapback;
    im_mut(&mut msg).associated_guid = Some("parent-guid".into());
    im_mut(&mut msg).associated_part = Some(0);
    im_mut(&mut msg).tapback_kind = Some("loved".into());
    im_mut(&mut msg).tapback_action = Some("add".into());
    im_mut(&mut msg).in_reply_to_guid = Some("parent-guid".into());
    msg.message.text = "Loved a message".into();
    msg.attachments = vec![MailAttachment {
        bytes: b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some("handwriting.svg".into()),
            mime_type: Some("image/svg+xml".into()),
            digest_sha256: None,
            size_bytes: None,
            missing_reason: None,
        },
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
    }];
    // Handwriting would normally be message_kind=balloon; this only checks MIME.
    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    assert_eq!(
        headers.get_first_value("X-ME-Tapback-Kind").as_deref(),
        Some("loved")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Associated-Guid").as_deref(),
        Some("parent-guid")
    );
    let mut found_svg = false;
    fn walk(m: &mailparse::ParsedMail<'_>, found: &mut bool) {
        if m.ctype.mimetype == "image/svg+xml" {
            *found = true;
        }
        for sub in &m.subparts {
            walk(sub, found);
        }
    }
    walk(&mail, &mut found_svg);
    assert!(found_svg, "expected image/svg+xml MIME part");
}

#[test]
fn escape_mboxrd_from_lines() {
    assert_eq!(escape_mboxrd_line("Hello"), "Hello");
    assert_eq!(escape_mboxrd_line("From me"), ">From me");
    assert_eq!(escape_mboxrd_line(">From me"), ">>From me");
    assert_eq!(escape_mboxrd_line("Fromage"), "Fromage");
}

/// Other mail clients show the `From_` line's sender and date, so both must
/// be right even though reading the mbox back does not use them.
#[test]
fn each_mbox_record_starts_with_its_sender_and_utc_date() {
    let incoming = base_sms(); // 2014-05-22 15:41:01 UTC, from +15555550101
    let mut outgoing = base_sms();
    outgoing.message.guid = "bbccddeeff00112233445566778899aa".into();
    outgoing.message.direction = IrDirection::Outgoing;
    outgoing.message.timestamp_unix_ms = 1_401_700_000_000; // 2014-06-02 09:06:40 UTC
    let mut from_email = base_sms();
    from_email.message.guid = "ccddeeff00112233445566778899aabb".into();
    from_email.message.sender_identity = Some("sam@example.com".into());
    from_email.message.timestamp_unix_ms = 1_401_700_001_000;

    let tmp = tempfile::tempdir().unwrap();
    let path = write_conversation_mbox(tmp.path(), &[incoming, outgoing, from_email]).unwrap();

    let text = fs::read_to_string(&path).unwrap();
    let from_lines: Vec<&str> = text.lines().filter(|l| l.starts_with("From ")).collect();
    assert_eq!(
        from_lines,
        [
            "From +15555550101@sms.local Thu May 22 15:41:01 2014",
            // The owner sent it. asctime pads a one-digit day with a space.
            "From +15555550100@sms.local Mon Jun  2 09:06:40 2014",
            // An address may not hold a second `@`, so it becomes `=`.
            "From sam=example.com@identity.local Mon Jun  2 09:06:41 2014",
        ]
    );
}

#[test]
fn writes_conversation_mboxrd() {
    let mut a = base_sms();
    // The body is one quoted-printable line, so only a text that starts
    // with `From ` puts a `From ` at the start of an mbox line.
    a.message.text = "From spoofed\nfirst\nlast".into();
    a.message.timestamp_unix_ms = 1_400_773_261_000;
    let mut b = base_sms();
    b.message.guid = "bbccddeeff00112233445566778899aa".into();
    b.message.text = "second".into();
    b.message.timestamp_unix_ms = 1_400_773_361_000;

    let tmp = tempfile::tempdir().unwrap();
    let path = write_conversation_mbox(tmp.path(), &[b, a]).unwrap();
    assert_eq!(path.file_name().unwrap(), "+15555550101.mbox");

    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("From "));
    assert!(text.contains(">From spoofed"));
    // Chronological: first then second
    let first_pos = text.find("first").unwrap();
    let second_pos = text.find("second").unwrap();
    assert!(first_pos < second_pos);
    assert_eq!(text.matches("\nFrom ").count(), 1); // one additional From_ between records
    assert!(text.contains("X-ME-Guid: aabbccddeeff00112233445566778899"));
    assert!(text.contains("X-ME-Guid: bbccddeeff00112233445566778899aa"));

    // The escape comes back off on read and the text is whole.
    let parsed = mail_messages_from_mbox(&path).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].message.text, "From spoofed\nfirst\nlast");
    assert_eq!(parsed[1].message.text, "second");
}

#[test]
fn a_messages_own_owner_survives_an_mbox_round_trip() {
    let mut msg = base_sms();
    msg.message.direction = IrDirection::Outgoing;
    msg.message.owner_identity = Some("me@example.com".into());
    let tmp = tempfile::tempdir().unwrap();
    let path = write_conversation_mbox(tmp.path(), &[msg]).unwrap();
    let parsed = mail_messages_from_mbox(&path).unwrap();
    assert_eq!(
        parsed[0].message.owner_identity.as_deref(),
        Some("me@example.com")
    );
    // The conversation's owner stays where it was.
    assert_eq!(parsed[0].owner_identity, "+15555550100");
}

#[test]
fn an_mbox_keeps_the_bytes_of_a_text_attachment() {
    let card = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Sam\r\nEND:VCARD\r\n".to_vec();
    let mut msg = base_sms();
    msg.message.message_kind = message_ir::IrMessageKind::Mms;
    msg.attachments = vec![MailAttachment {
        bytes: card.clone(),
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some("sam.vcf".into()),
            mime_type: Some("text/vcard".into()),
            digest_sha256: None,
            size_bytes: None,
            missing_reason: None,
        },
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
    }];
    let tmp = tempfile::tempdir().unwrap();
    let path = write_conversation_mbox(tmp.path(), &[msg]).unwrap();
    let parsed = mail_messages_from_mbox(&path).unwrap();
    assert_eq!(parsed[0].attachments[0].bytes, card);
}

/// A mail an earlier Message Crate wrote names each address a handle
/// (`X-ME-Sender-Handle`, `[{"handle": …}]`). Read as it is, it would come back
/// with no sender and no one in the conversation, so it is refused instead.
#[test]
fn a_mail_that_names_addresses_handles_is_refused() {
    let eml = concat!(
        "From: sam=example.com@handle.local\r\n",
        "X-ME-Chat-Identifier: sam@example.com\r\n",
        "X-ME-Guid: g1\r\n",
        "X-ME-Timestamp-Unix-Ms: 1400773261000\r\n",
        "X-ME-Participants: [{\"handle\":\"sam@example.com\"}]\r\n",
        "X-ME-Sender-Handle: sam@example.com\r\n",
        "\r\n",
        "hello\r\n",
    );
    let err = crate::mail_message_from_eml_bytes(eml.as_bytes()).unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        "This mail was written by an earlier Message Crate, which named each address a \
         handle (X-ME-Sender-Handle). Export the backup again"
    );
}

/// An earlier mail with no owner and no sender address carries none of the
/// `X-ME-*-Handle` headers, but its participants still say `handle`.
/// Participants that do not read are refused rather than read as nobody.
#[test]
fn a_mail_whose_participants_do_not_read_is_refused() {
    assert_json_header_is_refused(
        "X-ME-Participants",
        r#"[{"handle":"+15555550101"},{"handle":"+15555550102"}]"#,
        "participants",
    );
}

/// A display name that looks like an RFC 2047 encoded word is the person's
/// name, not an encoding: a mail header reader would decode it, and the
/// decoded `"` would break the roster's JSON. It reads back as it was written.
#[test]
fn a_name_that_looks_like_an_encoded_word_reads_back_as_written() {
    let mut msg = base_sms();
    msg.participants[0].display_name = Some("=?utf-8?Q?=22?= x".into());
    let eml = build_eml(&msg).unwrap();
    let back = crate::mail_message_from_eml_bytes(&eml).unwrap();
    assert_eq!(
        back.participants[0].display_name.as_deref(),
        Some("=?utf-8?Q?=22?= x")
    );
}

/// An earlier Message Crate kept a message's reactions in `X-ME-Tapbacks`,
/// which no reader looks for now, so the mail is refused rather than read
/// with its reactions gone.
#[test]
fn a_mail_that_keeps_reactions_in_x_me_tapbacks_is_refused() {
    let eml = concat!(
        "X-ME-Chat-Identifier: +15555550101\r\n",
        "X-ME-Guid: g1\r\n",
        "X-ME-Timestamp-Unix-Ms: 1400773261000\r\n",
        "X-ME-Tapbacks: [{\"part_index\":0,\"kind\":\"loved\",\"is_from_me\":false,\"reactor_identity\":\"+15555550101\"}]\r\n",
        "\r\n",
        "hello\r\n",
    );
    let err = crate::mail_message_from_eml_bytes(eml.as_bytes()).unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        "This mail was written by an earlier Message Crate, which kept reactions in \
         X-ME-Tapbacks. Export the backup again"
    );
}

/// A reactions header that does not read is refused rather than read as no
/// reactions.
#[test]
fn a_mail_whose_reactions_do_not_read_is_refused() {
    assert_json_header_is_refused("X-ME-Reactions", r#"[{"part_index":0}]"#, "reactions");
}

/// Each mark a message carries is written in `X-ME-Deletion` and read back
/// as the same mark; a message with none writes no header.
#[test]
fn a_deletion_mark_reads_back_as_written() {
    for deletion in message_ir::Deletion::ALL {
        let mut msg = base_sms();
        msg.message.deletion = Some(deletion);
        let eml = build_eml(&msg).unwrap();
        let back = crate::mail_message_from_eml_bytes(&eml).unwrap();
        assert_eq!(back.message.deletion, Some(deletion));
    }
    let eml = build_eml(&base_sms()).unwrap();
    assert!(
        !String::from_utf8_lossy(&eml).contains("X-ME-Deletion"),
        "a message with no mark writes no header"
    );
    assert_eq!(
        crate::mail_message_from_eml_bytes(&eml)
            .unwrap()
            .message
            .deletion,
        None
    );
}

/// An earlier Message Crate kept the Apple Messages deleted mark in
/// `X-ME-Is-Deleted`, which no reader looks for now, so the mail is refused
/// rather than read with its mark gone.
#[test]
fn a_mail_that_keeps_the_deleted_mark_in_x_me_is_deleted_is_refused() {
    let eml = concat!(
        "X-ME-Chat-Identifier: +15555550101\r\n",
        "X-ME-Guid: g1\r\n",
        "X-ME-Timestamp-Unix-Ms: 1400773261000\r\n",
        "X-ME-Is-Deleted: true\r\n",
        "\r\n",
        "hello\r\n",
    );
    let err = crate::mail_message_from_eml_bytes(eml.as_bytes()).unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        "This mail was written by an earlier Message Crate, which kept the deleted mark in \
         X-ME-Is-Deleted. Export the backup again"
    );
}

/// An earlier Message Crate kept the Apple Messages edit history in
/// `X-ME-Edits`, which no reader looks for now, so the mail is refused
/// rather than read with its earlier versions gone.
#[test]
fn a_mail_that_keeps_the_edit_history_in_x_me_edits_is_refused() {
    let eml = concat!(
        "X-ME-Chat-Identifier: +15555550101\r\n",
        "X-ME-Guid: g1\r\n",
        "X-ME-Timestamp-Unix-Ms: 1400773261000\r\n",
        "X-ME-Edits: [{\"part_index\":0,\"status\":\"edited\",\"text\":\"helo\"}]\r\n",
        "\r\n",
        "hello\r\n",
    );
    let err = crate::mail_message_from_eml_bytes(eml.as_bytes()).unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        "This mail was written by an earlier Message Crate, which kept the edit history in \
         X-ME-Edits. Export the backup again"
    );
}

/// Earlier versions that do not read are refused rather than read as none.
#[test]
fn a_mail_whose_earlier_versions_do_not_read_is_refused() {
    assert_json_header_is_refused(
        "X-ME-Earlier-Versions",
        r#"[{"part_index":0}]"#,
        "earlier versions",
    );
}

/// A header value that is not JSON at all.
const BROKEN_JSON: &str = r#"[{"broken""#;

/// Read a mail whose JSON header `header` carries `value`, which does not
/// read, and assert it is refused with the message that names `what` and the
/// header.
fn assert_json_header_is_refused(header: &str, value: &str, what: &str) {
    let eml = format!(
        "X-ME-Chat-Identifier: +15555550101\r\n\
         X-ME-Guid: g1\r\n\
         X-ME-Timestamp-Unix-Ms: 1400773261000\r\n\
         X-ME-Service: imessage\r\n\
         {header}: {value}\r\n\
         \r\n\
         hello\r\n"
    );
    let err = crate::mail_message_from_eml_bytes(eml.as_bytes()).unwrap_err();
    assert!(
        format!("{err:#}").starts_with(&format!(
            "This mail's {what} ({header}) cannot be read. It may have been written by an \
             earlier Message Crate, so export the backup again"
        )),
        "{err:#}"
    );
}

/// Source fields that do not read are refused rather than read as none (#1716).
#[test]
fn a_mail_whose_source_fields_do_not_read_is_refused() {
    assert_json_header_is_refused("X-ME-Source-Fields", BROKEN_JSON, "source fields");
}

/// Attachment metadata that does not read is refused rather than read as
/// none, which would lose every attachment's name, type and fingerprint on
/// Convert (#1716).
#[test]
fn a_mail_whose_attachment_metadata_does_not_read_is_refused() {
    assert_json_header_is_refused("X-ME-Attachment-Meta", BROKEN_JSON, "attachment metadata");
}

/// Message parts that do not read are refused rather than read as none (#1716).
#[test]
fn a_mail_whose_message_parts_do_not_read_is_refused() {
    assert_json_header_is_refused("X-ME-Parts", BROKEN_JSON, "message parts");
}

/// An app message that does not read is refused rather than read as none
/// (#1716).
#[test]
fn a_mail_whose_app_message_does_not_read_is_refused() {
    assert_json_header_is_refused("X-ME-App", BROKEN_JSON, "app message");
}

/// A mark that names neither Deleted in the source app nor Unsent is
/// refused rather than read as no mark.
#[test]
fn a_mail_whose_deletion_names_no_mark_is_refused() {
    let eml = concat!(
        "X-ME-Chat-Identifier: +15555550101\r\n",
        "X-ME-Guid: g1\r\n",
        "X-ME-Timestamp-Unix-Ms: 1400773261000\r\n",
        "X-ME-Deletion: trashed\r\n",
        "\r\n",
        "hello\r\n",
    );
    let err = crate::mail_message_from_eml_bytes(eml.as_bytes()).unwrap_err();
    assert_eq!(
        format!("{err:#}"),
        "This mail's X-ME-Deletion header: \"trashed\" is neither deleted_in_source_app nor unsent"
    );
}

/// A copy of [`base_sms`] whose every free-text `X-ME-*` value is `value`:
/// each plain-text header, and a string inside each JSON header. Headers
/// whose value is a number, a flag or a name from a fixed list are read
/// trimmed (`typed_header`), so they keep their written values.
///
/// The values that also fill a standard header take `value` too: the guid
/// and the reply guid (`Message-ID`, `In-Reply-To`, `References`), the
/// sender, owner and participant identities (`From`, `To` and the mbox
/// `From_` line) and the attachment's type (its part's `Content-Type`).
fn with_every_x_me_text(value: &str, direction: IrDirection) -> MailMessage {
    let text = || Some(value.to_string());
    let mut msg = base_sms();
    msg.message.direction = direction;
    msg.chat_identifier = value.into();
    msg.conversation_type = "group".into();
    msg.group_title = text();
    msg.participants[0].display_name = text();
    msg.owner_display_name = text();
    msg.export_source = value.into();
    msg.export_tool = value.into();
    msg.export_tool_version = value.into();
    msg.participants[0].identity = value.into();
    msg.owner_identity = value.into();
    msg.message.guid = value.into();
    msg.message.sender_identity = text();
    msg.message.sender_display_name = text();
    msg.message.owner_identity = text();
    msg.message.subject = text();
    msg.message.reactions = vec![message_ir::Reaction {
        part_index: 0,
        kind: value.into(),
        emoji: text(),
        is_from_me: false,
        reactor_identity: text(),
        reactor_display_name: text(),
    }];
    msg.message.edits = vec![message_ir::EarlierVersion {
        part_index: 0,
        text: value.into(),
        edited_at_unix_ms: None,
    }];
    msg.message.source.as_mut().unwrap().fields =
        serde_json::from_value(serde_json::json!({ "note": value })).unwrap();
    let im = im_mut(&mut msg);
    im.send_effect = text();
    im.shared_location = text();
    im.announcement = text();
    im.parts = Some(serde_json::json!([{ "index": 0, "text": value }]));
    im.app = Some(serde_json::json!({ "title": value }));
    im.balloon_bundle_id = text();
    im.associated_guid = text();
    im.tapback_emoji = text();
    im.in_reply_to_guid = text();
    msg.attachments = vec![MailAttachment {
        bytes: b"x".to_vec(),
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: text(),
            mime_type: text(),
            digest_sha256: None,
            size_bytes: Some(1),
            missing_reason: None,
        },
        is_sticker: false,
        transcription: text(),
        sticker_effect: text(),
    }];
    msg
}

/// Everything of `msg` the `X-ME-*` headers carry, as JSON, so a written
/// message and the one read back compare whole.
fn x_me_values(msg: &MailMessage) -> serde_json::Value {
    serde_json::json!({
        "chat_identifier": msg.chat_identifier,
        "owner_identity": msg.owner_identity,
        "group_title": msg.group_title,
        "participants": msg.participants,
        "owner_display_name": msg.owner_display_name,
        "export_source": msg.export_source,
        "export_tool": msg.export_tool,
        "export_tool_version": msg.export_tool_version,
        "message": msg.message,
        "attachment_names": msg.attachments.iter().map(|a| &a.meta.original_name).collect::<Vec<_>>(),
        "attachment_types": msg.attachments.iter().map(|a| &a.meta.mime_type).collect::<Vec<_>>(),
        "transcriptions": msg.attachments.iter().map(|a| &a.transcription).collect::<Vec<_>>(),
        "sticker_effects": msg.attachments.iter().map(|a| &a.sticker_effect).collect::<Vec<_>>(),
    })
}

/// Assert that every value of `values`, written into every free-text
/// `X-ME-*` value of a message, reads back from an EML file and from an mbox
/// exactly as written, in both directions. A failure counts the values that
/// changed or did not read at all, and names the first few.
fn assert_every_value_reads_back(values: &[String]) {
    let tmp = tempfile::tempdir().unwrap();
    let mut changed = Vec::new();
    for (i, value) in values.iter().enumerate() {
        let same = [IrDirection::Outgoing, IrDirection::Incoming]
            .into_iter()
            .enumerate()
            .all(|(d, direction)| {
                let msg = with_every_x_me_text(value, direction);
                let written = x_me_values(&msg);
                let from_eml = crate::mail_message_from_eml_bytes(&build_eml(&msg).unwrap());
                let dir = tmp.path().join(format!("{i}-{d}"));
                let mbox = write_mail_package(&dir, MailPackage::Mbox, &[msg]).unwrap();
                let from_mbox = crate::mail_messages_from_mbox(&mbox);
                from_eml.is_ok_and(|m| x_me_values(&m) == written)
                    && from_mbox.is_ok_and(|m| m.len() == 1 && x_me_values(&m[0]) == written)
            });
        if !same {
            changed.push(value);
        }
    }
    assert!(
        changed.is_empty(),
        "{} of {} values did not read back as written, such as {:?}",
        changed.len(),
        values.len(),
        &changed[..changed.len().min(5)]
    );
}

/// A run of spaces next to the characters RFC 2047 encoded words are made
/// of (`=?`, `?=`, a quote, non-ASCII) reads back from every `X-ME-*`
/// header byte for byte. mail-builder folds a long header value at a run of
/// spaces, and mailparse's unfolding turned the run into one space (#1812).
#[test]
fn runs_of_spaces_next_to_encoded_word_characters_read_back_as_written() {
    let pieces = [
        "bcdefghij     ?=<",
        "é     é",
        "a     b",
        "x  \"  y",
        "=?     ?=",
        "  leading and trailing  ",
    ];
    let mut values = Vec::new();
    for piece in pieces {
        for pad in 0..60 {
            let z = "z".repeat(pad);
            values.push(format!("{z}{piece}"));
            values.push(format!("{z}{piece}{z}"));
        }
    }
    assert_every_value_reads_back(&values);
}

/// Values made from the characters a mail header reader treats specially,
/// in every order and length up to 120 characters, read back from every
/// `X-ME-*` header byte for byte (#1812).
#[test]
fn generated_values_read_back_as_written() {
    const ALPHABET: [&str; 16] = [
        " ", " ", " ", "a", "z", "é", "😀", "=", "?", "_", "\"", "\\", "<", "\t", "\r\n", "=?",
    ];
    // xorshift64, seeded, so a failure names values that fail again.
    let mut state: u64 = 0x1812_1812_1812_1812;
    let mut next = move |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % bound as u64) as usize
    };
    // Each value is written and read four times (two directions, EML and
    // mbox), so 1000 values keep the test to a few seconds.
    let values: Vec<String> = (0..1000)
        .map(|_| {
            let len = 1 + next(120);
            (0..len).map(|_| ALPHABET[next(ALPHABET.len())]).collect()
        })
        .collect();
    assert_every_value_reads_back(&values);
}

/// A number, a flag or a name from a fixed list that ends in a space, as a
/// mail rewritten by hand or by another tool can carry, reads as the value
/// it names. Read untrimmed, `outgoing ` was read as incoming and the
/// timestamp was refused.
#[test]
fn a_typed_header_that_ends_in_a_space_reads_as_its_value() {
    let eml = concat!(
        "X-ME-Chat-Identifier: +15555550101\r\n",
        "X-ME-Conversation-Type: group \r\n",
        "X-ME-Guid: g1\r\n",
        "X-ME-Timestamp-Unix-Ms: 1400773261000 \r\n",
        "X-ME-Direction: outgoing \r\n",
        "X-ME-Service: imessage \r\n",
        "X-ME-Message-Kind: imessage \r\n",
        "X-ME-Android-Type: 2 \r\n",
        "X-ME-Deletion: unsent \r\n",
        "X-ME-Is-Reply: true \r\n",
        "X-ME-Num-Replies: 3 \r\n",
        "X-ME-Read-Receipt: 2014-05-22T15:41:01Z \r\n",
        "X-ME-Balloon-Kind: url \r\n",
        "X-ME-Tapback-Kind: loved \r\n",
        "X-ME-Tapback-Action: add \r\n",
        "\r\n",
        "hello\r\n",
    );
    let msg = crate::mail_message_from_eml_bytes(eml.as_bytes()).unwrap();
    assert_eq!(msg.conversation_type, "group");
    assert_eq!(msg.message.timestamp_unix_ms, 1_400_773_261_000);
    assert_eq!(msg.message.direction, IrDirection::Outgoing);
    assert_eq!(msg.message.service, message_ir::IrService::IMessage);
    assert_eq!(
        msg.message.message_kind,
        message_ir::IrMessageKind::IMessage
    );
    assert_eq!(msg.message.source.unwrap().android_type, Some(2));
    assert_eq!(msg.message.deletion, Some(message_ir::Deletion::Unsent));
    let im = msg.message.imessage.unwrap();
    assert!(im.is_reply);
    assert_eq!(im.num_replies, Some(3));
    assert_eq!(
        im.read_receipt_rfc3339.as_deref(),
        Some("2014-05-22T15:41:01Z")
    );
    assert_eq!(im.balloon_kind.as_deref(), Some("url"));
    assert_eq!(im.tapback_kind.as_deref(), Some("loved"));
    assert_eq!(im.tapback_action.as_deref(), Some("add"));
}

/// `msg` as a mail written to an EML file and to an mbox, each read back.
fn written_and_read_back(msg: &MailMessage) -> (MailMessage, MailMessage) {
    let from_eml = crate::mail_message_from_eml_bytes(&build_eml(msg).unwrap()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mbox =
        write_mail_package(tmp.path(), MailPackage::Mbox, std::slice::from_ref(msg)).unwrap();
    let mut from_mbox = crate::mail_messages_from_mbox(&mbox).unwrap();
    assert_eq!(from_mbox.len(), 1, "the mbox holds one mail");
    (from_eml, from_mbox.remove(0))
}

/// The value of header `name` in the mail `msg` is written as, checked to
/// be one line.
fn standard_header(msg: &MailMessage, name: &str) -> String {
    let eml = build_eml(msg).unwrap();
    let (headers, _) = mailparse::parse_headers(&eml).unwrap();
    let header = headers
        .get_first_header(name)
        .unwrap_or_else(|| panic!("no {name} header"));
    let raw = header.get_value_raw();
    assert!(
        !raw.contains(&b'\r') && !raw.contains(&b'\n'),
        "{name} spans lines: {:?}",
        String::from_utf8_lossy(raw)
    );
    header.get_value()
}

/// A guid holding a line break, written into `Message-ID` as it was, ended
/// the mail's headers there: every `X-ME-*` header after it was read as the
/// body (#1816). The guid and the reply guid now read back exactly, and the
/// reply's `In-Reply-To` still names its parent's `Message-ID`.
#[test]
fn a_guid_with_a_line_break_keeps_the_mail_headers_whole() {
    let mut parent = base_sms();
    parent.message.guid = "c\r\n\r\nd".into();
    let mut msg = base_sms();
    msg.message.guid = "a\r\n\r\nb".into();
    im_mut(&mut msg).in_reply_to_guid = Some("c\r\n\r\nd".into());

    for read in <[MailMessage; 2]>::from(written_and_read_back(&msg)) {
        assert_eq!(x_me_values(&read), x_me_values(&msg));
    }
    let message_id = standard_header(&parent, "Message-ID");
    assert!(
        message_id.starts_with('<')
            && message_id.ends_with("@message-crate.local>")
            && !message_id.contains(char::is_whitespace),
        "Message-ID was {message_id:?}"
    );
    assert_eq!(standard_header(&msg, "In-Reply-To"), message_id);
    assert_eq!(standard_header(&msg, "References"), message_id);
    assert_ne!(standard_header(&msg, "Message-ID"), message_id);
}

/// Two guids that differ only in characters a `Message-ID` cannot hold still
/// name their mails apart, because a mail client threads replies by it.
#[test]
fn guids_that_differ_in_unsafe_characters_keep_different_message_ids() {
    let ids: Vec<String> = [
        "a b", "a\tb", "a\r\nb", "a%20b", "a@b", "a.b", "a..b", ".ab", "",
    ]
    .into_iter()
    .map(|guid| {
        let mut msg = base_sms();
        msg.message.guid = guid.into();
        standard_header(&msg, "Message-ID")
    })
    .collect();
    let distinct: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(distinct.len(), ids.len(), "Message-IDs were {ids:?}");
}

/// An identity an address can hold is written as it is, and identities
/// that differ only in characters an address cannot hold still have
/// different addresses, because a mail client groups mails by address.
#[test]
fn identities_keep_their_own_addresses() {
    let address = |identity: &str| {
        let mut msg = base_sms();
        msg.message.direction = IrDirection::Outgoing;
        msg.owner_identity = identity.into();
        let from = standard_header(&msg, "From");
        let [mailparse::MailAddr::Single(addr)] = &mailparse::addrparse(&from).unwrap()[..] else {
            panic!("From was {from:?}");
        };
        addr.addr.clone()
    };
    assert_eq!(
        address("o'brien/x@example.com"),
        "o'brien/x=example.com@identity.local"
    );
    assert_eq!(address("+15555550101"), "+15555550101@sms.local");
    let addresses: Vec<String> = ["a b", "a/b", "a_b", "a\r\nb", "a%20b", "josé", "jos_"]
        .into_iter()
        .map(address)
        .collect();
    let distinct: std::collections::BTreeSet<&String> = addresses.iter().collect();
    assert_eq!(
        distinct.len(),
        addresses.len(),
        "addresses were {addresses:?}"
    );
}

/// An identity holding a line break, written into the `From` or `To`
/// address as it was, made the mail unreadable (mailparse: "Header cannot
/// start with a space") or broke the mbox `From_` line (#1816). Every
/// identity now reads back exactly, from an incoming sender, the owner and a
/// 1:1 peer, in a group and a 1:1 conversation, in both directions.
#[test]
fn an_identity_with_a_line_break_keeps_the_address_headers_whole() {
    for conversation_type in ["group", "individual"] {
        for direction in [IrDirection::Incoming, IrDirection::Outgoing] {
            for (sender, owner, peer) in [
                ("a\r\n b", "c\r\n d", "e\r\n f"),
                (
                    "a\r\n b@example.com",
                    "c\r\n d@example.com",
                    "e\r\n f@example.com",
                ),
            ] {
                let mut msg = base_sms();
                msg.conversation_type = conversation_type.into();
                msg.message.direction = direction;
                msg.message.sender_identity = Some(sender.into());
                msg.owner_identity = owner.into();
                msg.participants[0].identity = peer.into();
                let case = format!("{conversation_type} {direction:?} {sender:?}");

                let (from_eml, from_mbox) = written_and_read_back(&msg);
                for read in [&from_eml, &from_mbox] {
                    assert_eq!(x_me_values(read), x_me_values(&msg), "{case}");
                }
                for name in ["From", "To"] {
                    let value = standard_header(&msg, name);
                    let addrs = mailparse::addrparse(&value).unwrap();
                    let [mailparse::MailAddr::Single(addr)] = &addrs[..] else {
                        panic!("{case}: {name} was {value:?}");
                    };
                    assert!(
                        !addr.addr.contains(char::is_whitespace),
                        "{case}: {name} was {value:?}"
                    );
                }

                let tmp = tempfile::tempdir().unwrap();
                let mbox = write_mail_package(tmp.path(), MailPackage::Mbox, &[msg]).unwrap();
                let text = fs::read_to_string(&mbox).unwrap();
                let envelope = text.lines().next().unwrap();
                let fields: Vec<&str> = envelope.split(' ').collect();
                assert_eq!(fields.len(), 7, "{case}: From_ line was {envelope:?}");
                assert_eq!(text.lines().filter(|l| l.starts_with("From ")).count(), 1);
            }
        }
    }
}

/// An attachment type holding a line break, written into its part's
/// `Content-Type` as it was, ended the part's headers there. The part is now
/// written as `application/octet-stream`, and the type, the bytes and the
/// other attachments read back exactly.
#[test]
fn an_attachment_type_with_a_line_break_keeps_its_part_whole() {
    let attachment = |mime: &str, bytes: &[u8]| MailAttachment {
        bytes: bytes.to_vec(),
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some("a.txt".into()),
            mime_type: Some(mime.into()),
            digest_sha256: None,
            size_bytes: Some(bytes.len() as u64),
            missing_reason: None,
        },
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
    };
    let mut msg = base_sms();
    msg.attachments = vec![
        attachment("text/plain\r\n\r\nX-Injected: 1", b"first"),
        attachment("image/png", b"second"),
    ];

    let types_and_bytes = |m: &MailMessage| -> Vec<(Option<String>, Vec<u8>)> {
        m.attachments
            .iter()
            .map(|a| (a.meta.mime_type.clone(), a.bytes.clone()))
            .collect()
    };
    for read in <[MailMessage; 2]>::from(written_and_read_back(&msg)) {
        assert_eq!(types_and_bytes(&read), types_and_bytes(&msg));
    }
    let eml = String::from_utf8(build_eml(&msg).unwrap()).unwrap();
    assert!(!eml.contains("\r\nX-Injected"), "{eml}");
    assert!(
        eml.contains("Content-Type: application/octet-stream"),
        "{eml}"
    );
}
