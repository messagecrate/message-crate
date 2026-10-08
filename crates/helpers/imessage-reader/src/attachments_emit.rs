//! Attachment records and part-index helpers for the emitter.

use imessage_database::{
    message_types::handwriting::HandwrittenMessage,
    tables::{attachment::Attachment, messages::Message},
};
use imessage_reader_protocol::{Attachment as AttachmentRecord, AttachmentSource};

use crate::{
    attachments::resolved_path,
    body::referenced_attachment_indices,
    error::RuntimeError,
    fields::{PartRecord, build_part_records, sticker_extras, transcription_for_attachment},
    session::MailSession,
};

/// Render handwriting ink as an SVG attachment, if this message is handwriting.
fn try_handwriting_svg(session: &MailSession, message: &Message) -> Option<AttachmentRecord> {
    if !message.is_handwriting() {
        return None;
    }
    let payload = message.raw_payload_data(session.data_source.db())?;
    let hw = HandwrittenMessage::from_payload(&payload).ok()?;
    Some(AttachmentRecord {
        original_name: Some(format!("{}.svg", message.guid)),
        mime_type: Some("image/svg+xml".into()),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        source: AttachmentSource::Inline {
            text: hw.render_svg(),
        },
    })
}

/// Rewrite part attachment indices so they match the kept (referenced) list,
/// not the full attachment list from the database.
fn remap_part_attachment_indices(
    parts: &mut [PartRecord],
    index_by_full: &std::collections::HashMap<usize, usize>,
) {
    for part in parts {
        part.attachment_indices = part
            .attachment_indices
            .iter()
            .filter_map(|full| index_by_full.get(full).copied())
            .collect();
    }
}

/// Body parts and the attachment records the body references, in body order.
///
/// No bytes are read here. Each record names the file Messages resolved (or
/// says there is none), and the app reads or asks for it when it writes.
///
/// # Errors
///
/// Returns an error when attachments cannot be loaded from the database.
pub(super) fn collect_parts_and_attachments(
    session: &MailSession,
    message: &Message,
) -> Result<(Vec<PartRecord>, Vec<AttachmentRecord>), RuntimeError> {
    let attachments = Attachment::from_message(session.data_source.db(), message)?;
    let referenced = referenced_attachment_indices(message, &attachments);
    let index_by_full: std::collections::HashMap<usize, usize> = referenced
        .iter()
        .enumerate()
        .map(|(kept, &full)| (full, kept))
        .collect();

    let mut parts = build_part_records(message, &attachments);
    remap_part_attachment_indices(&mut parts, &index_by_full);

    let mut records = Vec::new();
    for &idx in &referenced {
        let attachment = &attachments[idx];
        let transcription = transcription_for_attachment(message, attachment);
        let (_prompt, sticker_effect) = sticker_extras(
            attachment,
            &session.options.platform,
            session.options.db_path.as_path(),
            session.options.attachment_root.as_deref(),
        );
        let size_hint = (attachment.total_bytes > 0).then_some(attachment.total_bytes as u64);
        let source = match resolved_path(session, attachment) {
            Some(path) => AttachmentSource::Path { path, size_hint },
            None => AttachmentSource::Missing,
        };
        records.push(AttachmentRecord {
            original_name: attachment.transfer_name.clone(),
            mime_type: attachment.mime_type.clone(),
            is_sticker: attachment.is_sticker,
            transcription,
            sticker_effect,
            source,
        });
    }

    if let Some(svg) = try_handwriting_svg(session, message) {
        records.push(svg);
    }

    Ok((parts, records))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FixtureDb;
    use chat_db_fixture::PHOTO_BYTES;

    /// The photo message has no parsed body, so every join row is kept, and
    /// the record names the file on disk with its size. The text message
    /// has one run part and no attachments.
    #[test]
    fn the_fixture_photo_is_one_attachment_record() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let messages = FixtureDb::messages(&session);

        let (parts, records) = collect_parts_and_attachments(&session, &messages[0]).unwrap();
        assert!(parts.is_empty(), "no attributedBody, so no parts");
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.original_name.as_deref(), Some("photo.jpg"));
        assert_eq!(record.mime_type.as_deref(), Some("image/jpeg"));
        assert!(!record.is_sticker);
        assert_eq!(record.transcription, None);
        assert_eq!(record.sticker_effect, None);
        let AttachmentSource::Path { path, size_hint } = &record.source else {
            panic!("a file on disk: {:?}", record.source);
        };
        assert_eq!(path, &fixture.dir.path().join("photo.jpg"));
        assert_eq!(*size_hint, Some(PHOTO_BYTES.len() as u64));

        let (parts, records) = collect_parts_and_attachments(&session, &messages[1]).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].text.as_deref(), Some("Nice"));
        assert!(records.is_empty());
    }

    /// Part indices point into the full attachment list from the database;
    /// the emitted list keeps only the referenced rows, so the indices are
    /// rewritten to that shorter list and an unreferenced index is dropped.
    #[test]
    fn part_indices_are_rewritten_to_the_kept_list() {
        let mut parts = vec![
            PartRecord {
                index: 0,
                kind: "run",
                text: None,
                attachment_indices: vec![2, 0, 5],
                effects: Vec::new(),
                emoji_image: false,
            },
            PartRecord {
                index: 1,
                kind: "run",
                text: None,
                attachment_indices: vec![1],
                effects: Vec::new(),
                emoji_image: false,
            },
        ];
        let index_by_full = std::collections::HashMap::from([(0, 0), (2, 1)]);
        remap_part_attachment_indices(&mut parts, &index_by_full);
        assert_eq!(parts[0].attachment_indices, vec![1, 0]);
        assert!(parts[1].attachment_indices.is_empty());
    }

    /// A plain row is not handwriting.
    #[test]
    fn a_plain_row_renders_no_handwriting() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let messages = FixtureDb::messages(&session);
        assert!(try_handwriting_svg(&session, &messages[1]).is_none());
    }

    /// The `payload_data` of a handwritten "hello", from the
    /// `imessage-database` crate (see `tests/fixtures/handwriting/SOURCE.md`).
    const HELLO_PAYLOAD: &[u8] = include_bytes!("../tests/fixtures/handwriting/hello.bin");
    /// What `imessage-database` renders from [`HELLO_PAYLOAD`].
    const HELLO_SVG: &str = include_str!("../tests/fixtures/handwriting/hello.svg");
    /// The balloon id Messages stores on a handwritten message.
    const HANDWRITING_BALLOON: &str = "com.apple.messages.MSMessageExtensionBalloonPlugin:0000000000:com.apple.Handwriting.HandwritingProvider";

    /// The ROWID of the row [`insert_hello_row`] adds, well past the
    /// fixture's own ROWIDs.
    const HELLO_ROWID: i64 = 100;
    /// When that row was sent, in seconds since 2001: after the fixture's
    /// last message.
    const HELLO_SECONDS_SINCE_2001: i64 = 600_001_000;
    /// Apple's own link preview balloon: a balloon id with no plugin prefix.
    const URL_BALLOON: &str = "com.apple.messages.URLBalloonProvider";
    /// A third-party app's extension under the same
    /// `MSMessageExtensionBalloonPlugin` prefix as handwriting.
    const OTHER_APP_BALLOON: &str = "com.apple.messages.MSMessageExtensionBalloonPlugin:ABCDE12345:com.example.SomeApp.MessagesExtension";

    /// Add a message row to the fixture's first chat that carries the
    /// "hello" handwriting payload under the given balloon id, and return
    /// it as the reader loads it.
    fn insert_hello_row(
        fixture: &FixtureDb,
        session: &MailSession,
        guid: &str,
        balloon_bundle_id: Option<&str>,
    ) -> Message {
        let db = rusqlite::Connection::open(&fixture.db_path).expect("open the fixture");
        db.execute(
            "INSERT INTO message (ROWID, guid, text, service, handle_id, date, is_from_me,
                 item_type, associated_message_type, balloon_bundle_id, payload_data)
             VALUES (?1, ?2, NULL, 'iMessage', 1, ?3, 0, 0, 0, ?4, ?5)",
            rusqlite::params![
                HELLO_ROWID,
                guid,
                chat_db_fixture::apple_nanos(HELLO_SECONDS_SINCE_2001),
                balloon_bundle_id,
                HELLO_PAYLOAD
            ],
        )
        .expect("insert the message row");
        db.execute(
            "INSERT INTO chat_message_join VALUES (1, ?1, 1)",
            [HELLO_ROWID],
        )
        .expect("join the message to a chat");
        FixtureDb::messages(session)
            .into_iter()
            .find(|message| message.guid == guid)
            .expect("the inserted row")
    }

    /// A handwritten message has no attachment rows, so its one attachment
    /// is the SVG rendered from its ink, named after the message.
    #[test]
    fn a_handwritten_message_is_one_svg_attachment() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let message = insert_hello_row(&fixture, &session, "hw-guid", Some(HANDWRITING_BALLOON));

        let (_parts, records) = collect_parts_and_attachments(&session, &message).unwrap();
        assert_eq!(records.len(), 1, "one attachment: {records:?}");
        let record = &records[0];
        assert_eq!(record.original_name.as_deref(), Some("hw-guid.svg"));
        assert_eq!(record.mime_type.as_deref(), Some("image/svg+xml"));
        assert!(!record.is_sticker);
        let AttachmentSource::Inline { text } = &record.source else {
            panic!("the SVG travels inline: {:?}", record.source);
        };
        // The upstream crate's own test pins `render_svg` of this payload to
        // `hello.svg` byte for byte, so the attachment text matches it too.
        assert_eq!(text, HELLO_SVG);
    }

    /// The same payload under another balloon id is not handwriting, so no
    /// SVG is made from it. One id is a built-in balloon provider; the other
    /// is another app's extension under the same
    /// `MSMessageExtensionBalloonPlugin` prefix as handwriting.
    #[test]
    fn a_payload_that_is_not_handwriting_renders_no_svg() {
        for balloon in [URL_BALLOON, OTHER_APP_BALLOON] {
            let fixture = FixtureDb::write();
            let session = fixture.session();
            let message = insert_hello_row(&fixture, &session, "not-hw-guid", Some(balloon));

            assert!(
                try_handwriting_svg(&session, &message).is_none(),
                "{balloon} is not handwriting"
            );
            let (_parts, records) = collect_parts_and_attachments(&session, &message).unwrap();
            assert!(records.is_empty(), "{balloon}: no attachments: {records:?}");
        }
    }
}
