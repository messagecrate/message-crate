//! Who is in a chat and what they are called, which files count as
//! messages, and which MIME parts become attachments.
//!
//! Every fixture here is synthetic: Alice is +14075550107, Bob is
//! +14075550108, the owner is +15555550100, and the JPEG is a 22-byte
//! header with no image data.

use crate::emit::{ConvertExportArgs, convert_export};
use message_crate_core::{ExportReport, ExportTransforms, OutputFormat};
use message_ir::ConversationDocument;
use message_ir_format::read_conversation_jsonl;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Convert `input` to JSONL under `output_dir`, with the owner at
/// +15555550100 and attachments copied.
fn convert(input: &Path, output_dir: &Path) -> ExportReport {
    let cache = tempfile::tempdir().unwrap();
    convert_export(ConvertExportArgs {
        inputs: &[input],
        output_dir,
        cache_dir: cache.path(),
        owner_phones: &["+15555550100".into()],
        owner_emails: &["owner@example.com".into()],
        verbose: false,
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Jsonl,
        cancel: None,
        log: None,
        issues: None,
        resume: false,
    })
    .expect("convert")
}

/// Every conversation the run wrote, keyed by chat identifier.
fn documents(output_dir: &Path) -> BTreeMap<String, ConversationDocument> {
    fs::read_dir(output_dir)
        .expect("output dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .map(|p| {
            let doc = read_conversation_jsonl(&p).expect("read jsonl");
            (doc.conversation.chat_identifier.clone(), doc)
        })
        .collect()
}

/// A fresh input directory holding the named fixtures.
fn input_with(root: &Path, names: &[&str]) -> PathBuf {
    let input = root.join("in");
    fs::create_dir_all(&input).expect("input dir");
    for name in names {
        fs::copy(fixtures().join(name), input.join(name)).expect("copy fixture");
    }
    input
}

/// The roster of a one-to-one chat is the peer, named from the subject.
///
/// `peer_handles_from_digits` fills the roster and `normalize_handle` writes
/// the sender; each could return nothing and a message would still be written.
/// `contact_name_from_subject` reads the name; without it the peer is an
/// unnamed number.
#[test]
fn one_to_one_roster_holds_the_peer_named_from_the_subject() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = input_with(tmp.path(), &["flat_received.eml"]);
    let out = tmp.path().join("out");

    let report = convert(&input, &out);
    assert_eq!(report.conversations, 1);

    let docs = documents(&out);
    let alice = &docs["+14075550107"];
    let roster: Vec<(Option<&str>, Option<&str>)> = alice
        .conversation
        .participants
        .iter()
        .map(|p| (p.identity.as_deref(), p.display_name.as_deref()))
        .collect();
    assert_eq!(roster, vec![(Some("+14075550107"), Some("Alice"))]);

    let msg = &alice.messages[0];
    assert_eq!(msg.sender_identity.as_deref(), Some("+14075550107"));
    assert_eq!(msg.sender_display_name.as_deref(), Some("Alice"));
}

/// A group holds every peer, and an incoming message names the peer whose
/// number is in `From`, not the first peer in the address list.
///
/// Bob is second in `X-smssync-address`, so a `group_sender` that returned
/// `None` or fell straight back to the first peer would both name the wrong
/// sender. An empty `peer_handles_from_digits` would leave the group with no
/// roster at all: the one-to-one fallback fills a single peer from the chat
/// id, but a group has nothing to fall back to.
#[test]
fn group_roster_holds_both_peers_and_the_sender_is_the_one_in_from() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = input_with(tmp.path(), &["flat_group_incoming.eml"]);
    let out = tmp.path().join("out");

    convert(&input, &out);

    let docs = documents(&out);
    let (chat_id, group) = docs.iter().next().expect("one conversation");
    assert!(chat_id.starts_with("chat-group-"), "chat id {chat_id}");
    let mut handles: Vec<&str> = group
        .conversation
        .participants
        .iter()
        .filter_map(|p| p.identity.as_deref())
        .collect();
    handles.sort_unstable();
    assert_eq!(handles, vec!["+14075550107", "+14075550108"]);

    let msg = &group.messages[0];
    assert_eq!(msg.text.trim(), "Hello group from Bob");
    assert_eq!(msg.sender_identity.as_deref(), Some("+14075550108"));
}

/// A mail that names the peer but records no address still gets its own
/// conversation, keyed by the name, with a participant that has no handle.
///
/// `name_only_key` is what keeps Alice out of the shared `unknown` chat.
#[test]
fn a_named_peer_with_no_address_gets_a_conversation_of_their_own() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("in");
    fs::create_dir_all(&input).expect("input dir");
    fs::write(
        input.join("named.eml"),
        "From: alice@unknown.email\r\n\
         To: me@example.com\r\n\
         Subject: SMS with Alice\r\n\
         X-smssync-type: 1\r\n\
         X-smssync-address: \r\n\
         X-smssync-date: 1609459200000\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         No number on this one\r\n",
    )
    .expect("write");
    let out = tmp.path().join("out");

    let report = convert(&input, &out);
    assert_eq!(report.extra("unknown_chat_messages"), 1);

    let docs = documents(&out);
    let alice = docs
        .get("name:Alice")
        .unwrap_or_else(|| panic!("keyed by the name: {:?}", docs.keys()));
    let roster: Vec<(Option<&str>, Option<&str>)> = alice
        .conversation
        .participants
        .iter()
        .map(|p| (p.identity.as_deref(), p.display_name.as_deref()))
        .collect();
    assert_eq!(roster, vec![(None, Some("Alice"))]);
}

/// Two people known only by names that share no ASCII letter get two
/// conversations, and each is written to a file of its own even though both
/// names reduce to the same file-name stem.
#[test]
fn people_known_only_by_non_ascii_names_get_a_conversation_each() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("in");
    fs::create_dir_all(&input).expect("input dir");
    for (file, name, text) in [
        ("a.eml", "张伟", "From the first"),
        ("b.eml", "李娜", "From the second"),
    ] {
        fs::write(
            input.join(file),
            format!(
                "From: someone@unknown.email\r\n\
                 To: me@example.com\r\n\
                 Subject: SMS with {name}\r\n\
                 X-smssync-type: 1\r\n\
                 X-smssync-address: \r\n\
                 X-smssync-date: 1609459200000\r\n\
                 Content-Type: text/plain; charset=utf-8\r\n\
                 \r\n\
                 {text}\r\n"
            ),
        )
        .expect("write");
    }
    let out = tmp.path().join("out");

    convert(&input, &out);

    let docs = documents(&out);
    let texts: Vec<(&str, &str)> = docs
        .iter()
        .map(|(id, doc)| (id.as_str(), doc.messages[0].text.trim()))
        .collect();
    assert_eq!(
        texts,
        vec![
            ("name:张伟", "From the first"),
            ("name:李娜", "From the second")
        ]
    );
}

/// Ordinary mail in the same mailbox is not a message, even when its subject
/// happens to read `SMS with Alice`.
///
/// Both fixtures lack `X-smssync-type`, and neither address is at
/// `sms-backup-plus.local`. Every branch of `is_single_sms_eml` replaced by
/// `true` would parse them: the digest has no name to key on and would be
/// counted as a parse failure, and the friend's mail would become a message
/// from Alice.
#[test]
fn ordinary_mail_is_skipped_and_counted_as_not_sms() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = input_with(
        tmp.path(),
        &[
            "flat_received.eml",
            "plain_mail.eml",
            "plain_mail_sms_subject.eml",
        ],
    );
    let out = tmp.path().join("out");

    let report = convert(&input, &out);

    assert_eq!(report.extra("skipped_not_sms_backup_plus"), 2);
    assert_eq!(report.extra("skipped_parse_error"), 0);
    assert_eq!(report.extra("flat_eml"), 1);
    assert_eq!(report.messages, 1);
    let docs = documents(&out);
    assert_eq!(
        docs["+14075550107"].messages[0].text.trim(),
        "Hello from Alice"
    );
}

/// Files under `duplicate/` and `exclude/` are never read.
///
/// Both copies carry text of their own, so an `in_skipped_dir` that let them
/// through would add two messages to Alice's chat.
#[test]
fn files_under_duplicate_and_exclude_are_skipped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = input_with(tmp.path(), &["flat_received.eml"]);
    for (directory, text) in [
        ("duplicate", "Read from duplicate"),
        ("exclude", "Read from exclude"),
    ] {
        let dir = input.join(directory);
        fs::create_dir_all(&dir).expect("skipped dir");
        let eml = fs::read_to_string(fixtures().join("flat_received.eml"))
            .expect("fixture")
            .replace("Hello from Alice", text)
            .replace("1609459200000", "1609459999000");
        fs::write(dir.join("msg.eml"), eml).expect("write");
    }
    let out = tmp.path().join("out");

    let report = convert(&input, &out);

    assert_eq!(
        report.extra("flat_eml"),
        1,
        "only the top-level file is read"
    );
    assert_eq!(report.messages, 1);
    let docs = documents(&out);
    let texts: Vec<&str> = docs["+14075550107"]
        .messages
        .iter()
        .map(|m| m.text.trim())
        .collect();
    assert_eq!(texts, vec!["Hello from Alice"]);
}

/// An input that sits under directories named `Duplicate`, `Exclude` or `.git`
/// is still read, because only the directories below the input are skipped.
///
/// Testing every directory of the absolute path skipped the whole input and
/// failed the run with "no .eml files".
#[test]
fn an_input_under_a_directory_named_duplicate_is_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().join("Duplicate").join("Exclude").join(".git");
    let input = input_with(&parent, &["flat_received.eml"]);
    let out = tmp.path().join("out");

    let report = convert(&input, &out);

    assert_eq!(report.extra("flat_eml"), 1);
    assert_eq!(report.messages, 1);
}

/// A JPEG part is one attachment with a `.jpg` name; the text part and the
/// empty part beside it are not attachments, and a plain SMS has none.
///
/// The JPEG part carries no filename, so the name is generated from the
/// MIME type: `extension_for` returning `""` would drop the `.jpg`.
#[test]
fn a_jpeg_part_is_exactly_one_jpg_attachment_and_a_plain_sms_has_none() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = input_with(tmp.path(), &["flat_received.eml", "flat_mms_jpeg.eml"]);
    let out = tmp.path().join("out");

    let report = convert(&input, &out);
    assert_eq!(report.attachments_saved, 1);

    let docs = documents(&out);
    let alice = &docs["+14075550107"];
    assert_eq!(alice.messages.len(), 2);

    let plain = &alice.messages[0];
    assert_eq!(plain.text.trim(), "Hello from Alice");
    assert!(plain.attachments.is_empty(), "{:?}", plain.attachments);

    let mms = &alice.messages[1];
    assert_eq!(mms.text.trim(), "Picture from Alice");
    assert_eq!(mms.attachments.len(), 1, "{:?}", mms.attachments);
    let att = &mms.attachments[0];
    let name = att.original_name.as_deref().expect("a generated name");
    assert!(name.ends_with(".jpg"), "name {name}");
    assert_eq!(att.mime_type.as_deref(), Some("image/jpeg"));
    let path = att.path.as_deref().expect("staged");
    assert!(path.ends_with(".jpg"), "path {path}");
    assert_eq!(att.missing_reason, None);
    assert!(out.join(path).is_file(), "{path} was written");
}

/// One SMS Backup+ mail: `headers`, then a plain-text `text`.
fn mail(headers: &str, text: &str) -> String {
    format!(
        "{headers}\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\n{text}\n"
    )
}

/// SMS Backup+ names a contact with an email address on the phone by that
/// address in `From` and `To`, and by number when they had none at backup
/// time. A one-to-one mail that gives both says Carol's address is her
/// number, so a group that names her by address and the same group naming
/// her by number are one conversation, and a reply from her address is from
/// her number (#1545).
#[test]
fn a_group_member_named_by_email_address_is_keyed_by_their_number() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("in");
    fs::create_dir_all(&input).expect("input dir");
    let mails = [
        (
            "1.eml",
            mail(
                "From: \"Carol\" <carol@example.com>\nTo: owner@example.com\nSubject: SMS with Carol\nX-smssync-type: 1\nX-smssync-address: 4075550111",
                "Carol alone",
            ),
        ),
        (
            "2.eml",
            mail(
                "From: owner@example.com\nTo: \"Carol\" <carol@example.com>, \"Bob\" <+14075550108@unknown.email>\nSubject: SMS with Bob\nX-smssync-type: 128\nX-smssync-address: 4075550108",
                "to Carol by address",
            ),
        ),
        (
            "3.eml",
            mail(
                "From: owner@example.com\nTo: <+14075550111@unknown.email>, <+14075550108@unknown.email>\nSubject: SMS with Carol\nX-smssync-type: 128\nX-smssync-address: 4075550111",
                "to Carol by number",
            ),
        ),
        (
            "4.eml",
            mail(
                "From: \"Carol\" <carol@example.com>\nTo: <+15555550100@unknown.email>, <+14075550108@unknown.email>\nSubject: SMS with Carol\nX-smssync-type: 132\nX-smssync-address: 4075550108",
                "reply from Carol",
            ),
        ),
    ];
    for (name, body) in &mails {
        fs::write(input.join(name), body).expect("write mail");
    }
    let out = tmp.path().join("out");

    convert(&input, &out);

    let docs = documents(&out);
    let groups: Vec<&ConversationDocument> = docs
        .iter()
        .filter(|(id, _)| id.starts_with("chat-"))
        .map(|(_, doc)| doc)
        .collect();
    assert_eq!(groups.len(), 1, "{:?}", docs.keys().collect::<Vec<_>>());
    let group = groups[0];
    let mut handles: Vec<&str> = group
        .conversation
        .participants
        .iter()
        .filter_map(|p| p.identity.as_deref())
        .collect();
    handles.sort_unstable();
    assert_eq!(handles, ["+14075550108", "+14075550111"]);
    assert_eq!(group.messages.len(), 3);
    let reply = group
        .messages
        .iter()
        .find(|m| m.text.trim() == "reply from Carol")
        .expect("Carol's reply is in the group");
    assert_eq!(reply.sender_identity.as_deref(), Some("+14075550111"));
    assert!(
        docs.contains_key("+14075550111"),
        "Carol's own conversation"
    );
}

/// A group mail that names one person twice, by email address and by
/// number, is that person's one-to-one conversation once the address is
/// keyed by the number (#1545).
#[test]
fn a_group_of_one_person_named_twice_is_their_one_to_one_conversation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("in");
    fs::create_dir_all(&input).expect("input dir");
    let mails = [
        (
            "1.eml",
            mail(
                "From: \"Carol\" <carol@example.com>\nTo: owner@example.com\nSubject: SMS with Carol\nX-smssync-type: 1\nX-smssync-address: 4075550111",
                "Carol alone",
            ),
        ),
        (
            "2.eml",
            mail(
                "From: owner@example.com\nTo: \"Carol\" <carol@example.com>, <+14075550111@unknown.email>\nSubject: SMS with Carol\nX-smssync-type: 128\nX-smssync-address: 4075550111",
                "to Carol twice",
            ),
        ),
    ];
    for (name, body) in &mails {
        fs::write(input.join(name), body).expect("write mail");
    }
    let out = tmp.path().join("out");

    convert(&input, &out);

    let docs = documents(&out);
    assert_eq!(
        docs.keys().collect::<Vec<_>>(),
        ["+14075550111"],
        "one conversation, Carol's"
    );
    assert_eq!(docs["+14075550111"].messages.len(), 2);
}
