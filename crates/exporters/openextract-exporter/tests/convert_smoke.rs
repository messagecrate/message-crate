use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_crate_core::testutil::{assert_csv_row, assert_jsonl_resumes};
use message_crate_core::{ExportReport, ExportTransforms, OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};

fn convert(input: &Path, output: &Path) -> Result<ExportReport> {
    convert_export(ConvertExportArgs {
        input,
        output,
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
        issues: None,
    })
}

#[test]
fn output_equals_input_dir_bails_before_cleaning() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let csv = fixture.join("all_conversations.csv");
    assert!(csv.is_file(), "missing {}", csv.display());
    // Output = fixture dir that holds the source CSV — open_prepared would
    // delete every *.csv before discovery.
    let err = convert(&fixture, &fixture).expect_err("output == input must fail");
    assert!(
        err.to_string()
            .contains("must not be the same as, or contain"),
        "unexpected error: {err}"
    );
    assert!(
        csv.is_file(),
        "source CSV must survive the refused run: {}",
        csv.display()
    );
}

#[test]
fn convert_all_conversations_keys_the_chat_by_its_number() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let csv = fixture.join("all_conversations.csv");
    assert!(csv.is_file(), "missing {}", csv.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let report = convert(&csv, tmp.path()).expect("convert");

    assert_eq!(report.conversations, 1);
    assert_eq!(report.messages, 2);
    // The source gave a number, so nothing is name-only.
    assert_eq!(report.extra.get("name_only_chat").copied().unwrap_or(0), 0);

    let out = tmp.path().join("+15555550122.csv");
    let body = fs::read_to_string(&out).expect("read csv");
    assert!(body.contains("openextract"));
    assert!(body.contains("all-conversations"));

    // Both messages the fixture carries, read back by column. The two
    // assertions above are all satisfied by the header line and the export
    // metadata, so before this the crate parsed no content in any test: an
    // exporter that dropped every row still passed.
    assert_csv_row(
        &out,
        &[
            ("text", "Hello from Sam"),
            ("direction", "incoming"),
            ("sender_identity", "+15555550122"),
            // 2020-01-01T17:00:00+00:00 in the source.
            ("timestamp_unix_ms", "1577898000000"),
        ],
    );
    // "Is From Me" is True on this row, and the direction column is where that
    // decision lands.
    assert_csv_row(
        &out,
        &[
            ("text", "Hi Sam"),
            ("direction", "outgoing"),
            ("timestamp_unix_ms", "1577898060000"),
        ],
    );
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let csv =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/all_conversations.csv");
    let tmp = tempfile::tempdir().expect("tempdir");
    assert_jsonl_resumes(tmp.path(), |resume| {
        convert_export(ConvertExportArgs {
            input: &csv,
            output: tmp.path(),
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
            issues: None,
        })
    });
}

/// Convert the CSV files `files` (name, body) as one OpenExtract export to
/// JSON, and read each conversation back, keyed by chat identifier.
fn convert_to_documents(
    files: &[(&str, &str)],
) -> (
    ExportReport,
    std::collections::BTreeMap<String, message_ir::ConversationDocument>,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("in");
    fs::create_dir(&input).unwrap();
    for (name, body) in files {
        let path = input.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    let out = dir.path().join("out");
    let report = convert_export(ConvertExportArgs {
        input: &input,
        output: &out,
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
        issues: None,
    })
    .expect("convert");
    let documents = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| message_ir_format::read_conversation_json(&path).expect("read back"))
        .map(|doc| (doc.conversation.chat_identifier.clone(), doc))
        .collect();
    (report, documents)
}

/// The phone handles on a conversation's roster.
fn roster(doc: &message_ir::ConversationDocument) -> Vec<&str> {
    doc.conversation
        .participants
        .iter()
        .filter_map(|p| p.identity.as_deref())
        .collect()
}

/// A chat labelled with a name is keyed by the number its incoming rows
/// carry, and that number is the roster. The owner's own number on a sent
/// row comes first here and must not become the chat's key.
#[test]
fn a_named_chat_is_keyed_by_its_peers_number() {
    let (_, documents) = convert_to_documents(&[(
        "all_conversations.csv",
        "Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T17:00:00+00:00,Sam Example,Sent,+15555550100,Hi Sam,True,False\n\
2020-01-01T17:01:00+00:00,Sam Example,Received,+15555550122,Hello from Sam,False,False\n",
    )]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["+15555550122"]);
    let doc = &documents["+15555550122"];
    assert_eq!(doc.messages.len(), 2);
    assert_eq!(roster(doc), vec!["+15555550122"]);
}

/// A row with no conversation named, or one named only `Me`, belongs to
/// its incoming sender. An outgoing row with nothing to say who it went to
/// is one of the account holder's orphaned messages, and lands in "Unknown
/// recipient", which has no roster.
#[test]
fn a_row_without_a_conversation_belongs_to_its_sender_or_to_no_one() {
    let (_, documents) = convert_to_documents(&[(
        "all_conversations.csv",
        "Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T17:00:00+00:00,,Received,+15555550133,Hey,False,False\n\
2020-01-01T17:01:00+00:00,,Sent,me,To whom,True,False\n\
2020-01-01T17:02:00+00:00,Me,Received,Cathy Arp,Still here,False,False\n",
    )]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["+15555550133", "name:Cathy Arp", "orphaned:"]);
    assert_eq!(roster(&documents["+15555550133"]), vec!["+15555550133"]);
    assert!(documents["orphaned:"].conversation.participants.is_empty());
}

/// A person named "unknown" has a chat of their own, apart from the sent
/// rows that name no recipient.
#[test]
fn a_person_named_unknown_is_not_the_chat_that_names_nobody() {
    let (_, documents) = convert_to_documents(&[(
        "all_conversations.csv",
        "Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T17:01:00+00:00,,Sent,me,To whom,True,False\n\
2020-01-01T17:02:00+00:00,,Received,unknown,Still here,False,False\n",
    )]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["name:unknown", "orphaned:"]);
    assert_eq!(documents["name:unknown"].messages.len(), 1);
    assert_eq!(documents["orphaned:"].messages.len(), 1);
}

/// In a per-chat file every row is the one chat, whatever its sender says:
/// a sent row carrying the owner's number, and a `Me` row whose
/// "Is From Me" column was left blank.
#[test]
fn every_row_of_a_per_chat_file_is_the_one_chat() {
    let (report, documents) = convert_to_documents(&[
        (
            "conversation_1.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,+15555550122,Hello,False,False\n\
2020-01-01T12:01:00+00:00,+15555550100,Hi,True,False\n",
        ),
        (
            "conversation_2.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,Me,Are you there,,False\n\
2020-01-01T12:01:00+00:00,Cathy Arp,Yes,False,False\n",
        ),
    ]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["+15555550122", "name:Cathy Arp"]);
    assert_eq!(documents["+15555550122"].messages.len(), 2);
    assert_eq!(documents["name:Cathy Arp"].messages.len(), 2);
    assert_eq!(report.conversations, 2);
}

/// OpenExtract writes each chat's attachments to a CSV of their own. That
/// file is not a conversation, so it is skipped rather than read and
/// reported as a file that failed to parse.
#[test]
fn an_attachments_csv_is_not_read_as_a_conversation() {
    let (report, documents) = convert_to_documents(&[
        (
            "conversation_1.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,+15555550122,Hello,False,True\n",
        ),
        (
            "conversation_1_attachments.csv",
            "Date,Filename,Mime Type\n2020-01-01T12:00:00+00:00,photo.jpg,image/jpeg\n",
        ),
    ]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(documents.len(), 1);

    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("conversation_1_attachments.csv");
    fs::write(&attachments, "Date,Filename\n").unwrap();
    let err = convert(&attachments, &dir.path().join("out")).unwrap_err();
    assert!(
        err.to_string()
            .contains("not an OpenExtract conversation CSV"),
        "{err}"
    );
}

/// The sender handle of each message in a conversation, in order; `None` for
/// a sent message.
fn senders(doc: &message_ir::ConversationDocument) -> Vec<Option<&str>> {
    doc.messages
        .iter()
        .map(|m| m.sender_identity.as_deref().filter(|h| !h.is_empty()))
        .collect()
}

/// A per-chat file is one conversation whoever wrote each row. Three people
/// wrote in this one, so it is one group with all three in it, and every
/// message is credited to the person who sent it. Before, each sender's rows
/// became that person's one-to-one conversation.
#[test]
fn a_per_chat_file_with_three_senders_is_one_group() {
    let (report, documents) = convert_to_documents(&[(
        "conversation_7.csv",
        "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,+15555550122,Who is in,False,False\n\
2020-01-01T12:01:00+00:00,+15555550133,Me,False,False\n\
2020-01-01T12:02:00+00:00,Cathy Arp,And me,False,False\n\
2020-01-01T12:03:00+00:00,Me,Great,True,False\n",
    )]);
    assert_eq!(report.conversations, 1);
    let doc = documents.values().next().unwrap();
    assert!(doc.conversation.chat_identifier.starts_with("group:"));
    assert_eq!(
        doc.conversation.conversation_type,
        message_ir::IrConversationType::Group
    );
    assert_eq!(roster(doc), vec!["+15555550122", "+15555550133"]);
    let named: Vec<_> = doc
        .conversation
        .participants
        .iter()
        .filter_map(|p| p.display_name.as_deref())
        .collect();
    assert_eq!(named, vec!["Cathy Arp"]);
    assert_eq!(
        senders(doc),
        vec![Some("+15555550122"), Some("+15555550133"), None, None]
    );
    assert_eq!(
        doc.messages[2].sender_display_name.as_deref(),
        Some("Cathy Arp")
    );
}

/// The account holder's messages that name no recipient: the guid of each.
fn guids(doc: &message_ir::ConversationDocument) -> std::collections::BTreeSet<&str> {
    doc.messages.iter().map(|m| m.guid.as_str()).collect()
}

/// A per-chat file in which only the owner sent records no recipient, so
/// its messages are the account holder's orphaned messages and go to
/// "Unknown recipient": the conversation of type `orphaned` keyed
/// `orphaned:`, with no participants. Two such files share it, and the same
/// text sent in both in the same second is two messages, never collapsed as
/// duplicates (#1095).
#[test]
fn files_of_only_sent_messages_go_to_unknown_recipient() {
    let sent_only = "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T00:00:00+00:00,Me,Happy new year,True,False\n";
    let (report, documents) = convert_to_documents(&[
        ("conversation_1.csv", sent_only),
        ("conversation_2.csv", sent_only),
    ]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["orphaned:"]);
    let doc = &documents["orphaned:"];
    assert_eq!(doc.conversation.conversation_type.as_str(), "orphaned");
    assert!(doc.conversation.participants.is_empty());
    assert_eq!(report.duplicates_dropped, 0);
    assert_eq!(guids(doc).len(), 2);
}

/// Sent rows of the all-conversations CSV that name no recipient go to
/// "Unknown recipient" too. The same text sent in the same second to three
/// people is three messages, never collapsed as duplicates, and a second
/// export of the same rows gives them the same ids, so importing it again
/// adds nothing (#1095).
#[test]
fn sent_rows_that_name_no_recipient_are_never_collapsed() {
    let csv = "Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T00:00:00+00:00,,Sent,me,Happy new year,True,False\n\
2020-01-01T00:00:00+00:00,,Sent,me,Happy new year,True,False\n\
2020-01-01T00:00:00+00:00,,Sent,me,Happy new year,True,False\n";
    let (report, documents) = convert_to_documents(&[("all_conversations.csv", csv)]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["orphaned:"]);
    let doc = &documents["orphaned:"];
    assert_eq!(doc.conversation.conversation_type.as_str(), "orphaned");
    assert!(doc.conversation.participants.is_empty());
    assert_eq!(report.duplicates_dropped, 0);
    assert_eq!(guids(doc).len(), 3);

    let (_, again) = convert_to_documents(&[("all_conversations.csv", csv)]);
    assert_eq!(guids(&again["orphaned:"]), guids(doc));
}

/// In the all-conversations CSV a `Conversation` value two people wrote in
/// is one group, apart from either one's one-to-one conversation, and each
/// message is credited to its own sender. Before, the group was filed under
/// its first sender's number, merged into that person's one-to-one
/// conversation, and every message in it was credited to them.
#[test]
fn an_all_conversations_group_is_apart_from_its_senders() {
    let (_, documents) = convert_to_documents(&[(
        "all_conversations.csv",
        "Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T17:00:00+00:00,Trip,Received,+15555550122,Packed?,False,False\n\
2020-01-01T17:01:00+00:00,Trip,Received,+15555550133,Almost,False,False\n\
2020-01-01T17:02:00+00:00,Trip,Sent,me,Leaving now,True,False\n\
2020-01-01T18:00:00+00:00,Sam Example,Received,+15555550122,Just us,False,False\n",
    )]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["+15555550122", "group:Trip"]);

    let trip = &documents["group:Trip"];
    // The value is kept as data, not as a title: it may be a list of names.
    assert_eq!(trip.conversation.group_title, None);
    let source = trip.messages[0].source.as_ref().unwrap();
    assert_eq!(source.fields["conversation"], "Trip");
    assert_eq!(
        trip.conversation.conversation_type,
        message_ir::IrConversationType::Group
    );
    assert_eq!(roster(trip), vec!["+15555550122", "+15555550133"]);
    assert_eq!(
        senders(trip),
        vec![Some("+15555550122"), Some("+15555550133"), None]
    );

    let sam = &documents["+15555550122"];
    assert_eq!(sam.messages.len(), 1);
    assert_eq!(roster(sam), vec!["+15555550122"]);
}

/// OpenExtract numbers its files, so `conversation_7.csv` in two exports is
/// two conversations, and the same file is the same conversation wherever
/// the export is put.
#[test]
fn a_per_chat_groups_key_comes_from_its_rows_and_name_not_its_folder() {
    let group = |text: &str| {
        format!(
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,+15555550122,{text},False,False\n\
2020-01-01T12:01:00+00:00,+15555550133,Yes,False,False\n"
        )
    };
    let trip = group("Trip?");
    let party = group("Party?");
    let (_, two_exports) = convert_to_documents(&[
        ("phone_a/conversation_7.csv", &trip),
        ("phone_b/conversation_7.csv", &party),
    ]);
    assert_eq!(two_exports.len(), 2);

    let (_, moved) = convert_to_documents(&[("elsewhere/conversation_7.csv", &trip)]);
    let trip_key = |documents: &std::collections::BTreeMap<String, _>| {
        documents
            .iter()
            .find(|(_, doc): &(_, &message_ir::ConversationDocument)| {
                doc.messages[0].text == "Trip?"
            })
            .map(|(key, _)| key.clone())
            .unwrap()
    };
    assert_eq!(trip_key(&two_exports), trip_key(&moved));
}

/// A number and an email address are two people. They may be one person's
/// phone and Apple ID, but the export does not say, and taking them for one
/// would put the second person's messages in the first one's one-to-one
/// conversation. The email address is an address, typed email.
#[test]
fn a_number_and_an_email_are_two_people() {
    let (_, documents) = convert_to_documents(&[(
        "conversation_3.csv",
        "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T17:00:00+00:00,+15555550122,Hi from Jo,False,False\n\
2020-01-01T17:01:00+00:00,Pat@Example.com,Hi from Pat,False,False\n\
2020-01-01T17:02:00+00:00,5555550122,Jo again,False,False\n",
    )]);
    assert_eq!(documents.len(), 1);
    let doc = documents.values().next().unwrap();
    assert!(doc.conversation.chat_identifier.starts_with("group:"));
    let members: Vec<_> = doc
        .conversation
        .participants
        .iter()
        .map(|p| (p.identity.as_deref(), p.identity_type))
        .collect();
    assert_eq!(
        members,
        vec![
            (Some("+15555550122"), Some(message_ir::HandleType::Phone)),
            (Some("pat@example.com"), Some(message_ir::HandleType::Email)),
        ]
    );
    assert_eq!(
        senders(doc),
        vec![
            Some("+15555550122"),
            Some("pat@example.com"),
            Some("+15555550122")
        ]
    );
}

/// The key that keeps one per-chat file's sent messages apart from
/// another's depends on that file alone. A file of one sent message gives
/// it the same id imported alone as beside another file with the same rows,
/// and two such files in one export give two ids.
#[test]
fn a_files_key_does_not_depend_on_the_other_files() {
    let sent_only = "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T00:00:00+00:00,Me,Happy new year,True,False\n";
    let (_, alone) = convert_to_documents(&[("conversation_1.csv", sent_only)]);
    let (_, together) = convert_to_documents(&[
        ("conversation_1.csv", sent_only),
        ("conversation_2.csv", sent_only),
    ]);
    let together = guids(&together["orphaned:"]);
    assert_eq!(together.len(), 2);
    let alone = guids(&alone["orphaned:"]);
    assert!(alone.is_subset(&together), "{alone:?} not in {together:?}");
}
