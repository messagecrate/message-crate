use super::*;
use crate::test_support::attachment;
use imessage_database::{
    message_types::{
        edited::{EditStatus, EditedEvent, EditedMessagePart},
        expressives::{BubbleEffect, Expressive},
        text_effects::{style::Style, text_effect::TextEffect},
    },
    tables::messages::models::{AttachmentMeta, AttributedRange, BubbleComponent},
    util::dates::get_offset,
};

/// A message row with every field blank, for tests that set a few.
///
/// `Message` has no `Default` and all of its fields are public, so this
/// is the constructor. A new field in a later `imessage-database`
/// release shows up here as a compile error, which is wanted.
fn message() -> Message {
    Message {
        rowid: 1,
        guid: "guid-1".to_string(),
        text: None,
        service: Some("iMessage".to_string()),
        handle_id: Some(1),
        destination_caller_id: None,
        subject: None,
        date: 0,
        date_read: 0,
        date_delivered: 0,
        is_from_me: false,
        is_read: false,
        item_type: 0,
        other_handle: None,
        share_status: false,
        share_direction: None,
        group_title: None,
        group_action_type: 0,
        associated_message_guid: None,
        associated_message_type: Some(0),
        balloon_bundle_id: None,
        expressive_send_style_id: None,
        thread_originator_guid: None,
        thread_originator_part: None,
        date_edited: 0,
        associated_message_emoji: None,
        chat_id: Some(1),
        num_attachments: 0,
        deleted_from: None,
        num_replies: 0,
        components: Vec::new(),
        edited_parts: None,
        filter_action: None,
        filter_sub_action: None,
    }
}

/// A message whose balloon is the given bundle id.
fn balloon_message(bundle_id: &str) -> Message {
    Message {
        balloon_bundle_id: Some(bundle_id.to_string()),
        ..message()
    }
}

fn app_message<'a>() -> AppMessage<'a> {
    AppMessage {
        image: None,
        url: Some("https://example.com/game"),
        title: Some("A game"),
        subtitle: None,
        caption: Some("Your move"),
        subcaption: None,
        trailing_caption: None,
        trailing_subcaption: None,
        app_name: Some("Games"),
        ldtext: None,
    }
}

/// `{"richLinkMetadata": {"title": ..., "URL": {"URL": ...}}}`, the
/// shape a URL balloon's payload takes once the archive is unwrapped.
fn url_payload(title: &str, url: &str) -> plist::Value {
    let mut inner = plist::Dictionary::new();
    inner.insert("URL".to_string(), plist::Value::String(url.to_string()));
    let mut metadata = plist::Dictionary::new();
    metadata.insert("title".to_string(), plist::Value::String(title.to_string()));
    metadata.insert("URL".to_string(), plist::Value::Dictionary(inner));
    let mut root = plist::Dictionary::new();
    root.insert(
        "richLinkMetadata".to_string(),
        plist::Value::Dictionary(metadata),
    );
    plist::Value::Dictionary(root)
}

#[test]
fn an_expressive_is_its_label_and_no_expressive_is_none() {
    assert_eq!(
        expressive_label(Expressive::Bubble(BubbleEffect::Slam)).as_deref(),
        Some("Sent with Slam")
    );
    assert_eq!(expressive_label(Expressive::None), None);
}

#[test]
fn a_shared_location_is_started_or_stopped() {
    assert_eq!(shared_location_label(SharedLocation::Started), "started");
    assert_eq!(shared_location_label(SharedLocation::Stopped), "stopped");
}

#[test]
fn text_effects_are_labelled() {
    assert_eq!(effect_label(&TextEffect::Default), "default");
    assert_eq!(
        effect_label(&TextEffect::Mention("+1555".to_string())),
        "mention:+1555"
    );
    assert_eq!(
        effect_label(&TextEffect::Link("https://example.com".to_string())),
        "link:https://example.com"
    );
    assert_eq!(effect_label(&TextEffect::OTP), "otp");
    assert!(effect_label(&TextEffect::Styles(vec![Style::Bold])).starts_with("styles:"));
}

/// Every entry of an edited part's history but the last is an earlier
/// version, with the time its entry carries; the last is the message's
/// own text. A part never edited, and an unsent part with no history,
/// give none.
#[test]
fn earlier_versions_are_each_parts_history_but_its_final_text() {
    let event = |date: i64, text: &str| EditedEvent {
        date,
        text: text.to_string(),
        components: Vec::new(),
        guid: None,
    };
    let edited = EditedMessage {
        parts: vec![
            EditedMessagePart {
                status: EditStatus::Original,
                edit_history: Vec::new(),
            },
            EditedMessagePart {
                status: EditStatus::Unsent,
                edit_history: Vec::new(),
            },
            EditedMessagePart {
                status: EditStatus::Edited,
                edit_history: vec![event(0, "first"), event(60, "second"), event(120, "third")],
            },
        ],
    };

    let versions = build_earlier_versions(&edited, get_offset());
    // Apple's epoch, 2001-01-01, in Unix milliseconds.
    let epoch_ms = 978_307_200_000;
    assert_eq!(
        versions,
        vec![
            EarlierVersion {
                part_index: 2,
                text: Some("first".to_string()),
                edited_at_unix_ms: Some(epoch_ms),
            },
            EarlierVersion {
                part_index: 2,
                text: Some("second".to_string()),
                edited_at_unix_ms: Some(epoch_ms + 60_000),
            },
        ]
    );
}

#[test]
fn parts_carry_text_attachments_effects_and_the_emoji_hint() {
    let attachments = vec![attachment(Some("att-1"))];
    let text = "hi \u{FFFC} there";
    let mut msg = Message {
        text: Some(text.to_string()),
        ..message()
    };
    msg.components = vec![
        BubbleComponent::Run(vec![
            AttributedRange::text(0, 3, vec![TextEffect::Default, TextEffect::OTP]),
            AttributedRange::inline_attachment(
                3,
                6,
                AttachmentMeta {
                    guid: Some("att-1".to_string()),
                    ..AttachmentMeta::default()
                },
            ),
            AttributedRange::text(6, text.len(), Vec::new()),
        ]),
        BubbleComponent::App,
        BubbleComponent::Retracted,
    ];

    let parts = build_part_records(&msg, &attachments);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].kind, "run");
    assert_eq!(parts[0].text.as_deref(), Some("hi  there"));
    assert_eq!(parts[0].attachment_indices, vec![0]);
    assert_eq!(parts[0].effects, vec!["otp".to_string()]);
    assert!(parts[0].emoji_image);
    assert_eq!((parts[1].index, parts[1].kind), (1, "app"));
    assert_eq!((parts[2].index, parts[2].kind), (2, "retracted"));

    // A body that never parsed has no parts at all.
    assert!(build_part_records(&message(), &attachments).is_empty());
}

/// Each part carries the rows its own ranges refer to, by GUID or by
/// position, and a range with no row left to point at adds nothing.
#[test]
fn parts_carry_only_the_attachment_rows_that_exist() {
    let attachments = vec![attachment(None), attachment(Some("att-b"))];
    let meta = |guid: Option<&str>| AttachmentMeta {
        guid: guid.map(str::to_string),
        ..AttachmentMeta::default()
    };
    let mut msg = message();
    msg.components = vec![
        BubbleComponent::Run(vec![AttributedRange::attachment(0, 1, meta(Some("att-b")))]),
        BubbleComponent::Run(vec![
            AttributedRange::attachment(1, 2, meta(None)),
            AttributedRange::attachment(2, 3, meta(None)),
        ]),
    ];

    let parts = build_part_records(&msg, &attachments);
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].attachment_indices, vec![1]);
    assert_eq!(
        parts[1].attachment_indices,
        vec![0],
        "the second range in the part has no row left"
    );
}

#[test]
fn a_transcription_is_found_by_attachment_guid() {
    let mut msg = message();
    msg.components = vec![BubbleComponent::Run(vec![AttributedRange::attachment(
        0,
        1,
        AttachmentMeta {
            guid: Some("voice-1".to_string()),
            transcription: Some("call me back".to_string()),
            ..AttachmentMeta::default()
        },
    )])];
    assert_eq!(
        transcription_for_attachment(&msg, &attachment(Some("voice-1"))).as_deref(),
        Some("call me back")
    );
    assert_eq!(
        transcription_for_attachment(&msg, &attachment(Some("other"))),
        None
    );
    assert_eq!(transcription_for_attachment(&msg, &attachment(None)), None);
}

#[test]
fn app_and_url_bubbles_serialize_every_field() {
    let app = app_message_json(&app_message());
    assert_eq!(app["title"], "A game");
    assert_eq!(app["app_name"], "Games");
    assert_eq!(app["image"], Value::Null);

    let url = url_message_json(&URLMessage {
        title: Some("Example"),
        url: Some("https://example.com"),
        images: vec!["https://example.com/a.png"],
        placeholder: true,
        ..URLMessage::default()
    });
    assert_eq!(url["title"], "Example");
    assert_eq!(url["images"][0], "https://example.com/a.png");
    assert_eq!(url["placeholder"], true);
    assert_eq!(url["summary"], Value::Null);
}

#[test]
fn every_balloon_kind_has_a_stable_name() {
    assert_eq!(balloon_kind_name(&CustomBalloon::ApplePay), "apple_pay");
    assert_eq!(balloon_kind_name(&CustomBalloon::Fitness), "fitness");
    assert_eq!(balloon_kind_name(&CustomBalloon::Slideshow), "slideshow");
    assert_eq!(balloon_kind_name(&CustomBalloon::CheckIn), "check_in");
    assert_eq!(balloon_kind_name(&CustomBalloon::FindMy), "find_my");
    assert_eq!(balloon_kind_name(&CustomBalloon::Business), "business");
    assert_eq!(
        balloon_kind_name(&CustomBalloon::Application("com.example")),
        "application"
    );
    assert_eq!(balloon_kind_name(&CustomBalloon::URL), "url");
    assert_eq!(
        balloon_kind_name(&CustomBalloon::Handwriting),
        "handwriting"
    );
    assert_eq!(
        balloon_kind_name(&CustomBalloon::DigitalTouch),
        "digital_touch"
    );
    assert_eq!(balloon_kind_name(&CustomBalloon::Polls), "poll");
}

#[test]
fn a_balloon_without_a_payload_keeps_its_kind_bundle_id_and_text() {
    let msg = Message {
        text: Some("Sent $5".to_string()),
        ..balloon_message("com.apple.PassbookUIService.PeerPaymentMessagesExtension")
    };
    let value = payloadless_value(&CustomBalloon::ApplePay, &msg);
    assert_eq!(value["kind"], "apple_pay");
    assert_eq!(
        value["bundle_id"],
        "com.apple.PassbookUIService.PeerPaymentMessagesExtension"
    );
    assert_eq!(value["text"], "Sent $5");

    // A URL balloon with no payload is only its text.
    let msg = Message {
        text: Some("https://example.com".to_string()),
        ..balloon_message("com.apple.messages.URLBalloonProvider")
    };
    let value = payloadless_value(&CustomBalloon::URL, &msg);
    assert_eq!(
        value,
        json!({ "kind": "url", "text": "https://example.com" })
    );
}

#[test]
fn a_url_override_is_a_normal_rich_link_unless_it_is_something_more() {
    let value = url_override_value(&url_payload("Example", "https://example.com")).unwrap();
    assert_eq!(value["kind"], "url");
    assert_eq!(value["data"]["title"], "Example");
    assert_eq!(value["data"]["url"], "https://example.com");

    // A payload with no metadata is not a rich link.
    assert_eq!(
        url_override_value(&plist::Value::Dictionary(plist::Dictionary::new())),
        None
    );
}

/// A third-party app carries the bundle id twice: as the variant parsed
/// it and as the row stores it, so the app can tell the two apart when
/// Messages wrapped the id in a
/// `com.apple.messages.MSMessageExtensionBalloonPlugin:...:` prefix.
#[test]
fn an_app_bubble_names_the_app_and_carries_its_data() {
    let msg = balloon_message(
        "com.apple.messages.MSMessageExtensionBalloonPlugin:0000000000:com.example.game.MessagesExtension",
    );
    let value = app_bubble_value(
        &CustomBalloon::Application("com.example.game.MessagesExtension"),
        &msg,
        &app_message(),
    );
    assert_eq!(value["kind"], "application");
    assert_eq!(value["bundle_id"], "com.example.game.MessagesExtension");
    assert_eq!(
        value["parsed_bundle_id"],
        "com.example.game.MessagesExtension"
    );
    assert_eq!(value["data"]["caption"], "Your move");

    let msg = balloon_message("com.apple.ActivityMessagesApp.MessagesExtension");
    let value = app_bubble_value(&CustomBalloon::Fitness, &msg, &app_message());
    assert_eq!(value["kind"], "fitness");
    assert_eq!(
        value["bundle_id"],
        "com.apple.ActivityMessagesApp.MessagesExtension"
    );
    assert_eq!(value["data"]["title"], "A game");
}

/// The whole balloon path over an empty database: a plain text is no
/// balloon, handwriting and Digital Touch never read the payload, a
/// poll with no payload is marked unparseable, and an app with no
/// payload blob falls back to the payloadless shape.
#[test]
fn balloon_values_over_a_database_with_no_payloads() {
    let db = Connection::open_in_memory().unwrap();
    let capabilities = Capabilities::determine(&db).unwrap();

    assert_eq!(build_balloon_value(&db, &capabilities, &message()), None);

    let value = build_balloon_value(
        &db,
        &capabilities,
        &balloon_message("com.apple.Handwriting.HandwritingProvider"),
    )
    .unwrap();
    assert_eq!(value["kind"], "handwriting");

    let value = build_balloon_value(
        &db,
        &capabilities,
        &balloon_message("com.apple.DigitalTouchBalloonProvider"),
    )
    .unwrap();
    assert_eq!(value["kind"], "digital_touch");

    let value = build_balloon_value(
        &db,
        &capabilities,
        &balloon_message("com.apple.messages.Polls"),
    )
    .unwrap();
    assert_eq!(value, json!({ "kind": "poll", "error": "unparseable" }));
    assert_eq!(
        poll_value(
            &db,
            &capabilities,
            &balloon_message("com.apple.messages.Polls")
        ),
        json!({ "kind": "poll", "error": "unparseable" })
    );

    let msg = Message {
        text: Some("Your move".to_string()),
        ..balloon_message("com.example.game")
    };
    let value = build_balloon_value(&db, &capabilities, &msg).unwrap();
    assert_eq!(value["kind"], "application");
    assert_eq!(value["bundle_id"], "com.example.game");
    assert_eq!(value["text"], "Your move");
}

#[test]
fn the_balloon_kind_is_read_off_the_value() {
    assert_eq!(
        balloon_kind_label(&json!({ "kind": "url" })).as_deref(),
        Some("url")
    );
    assert_eq!(balloon_kind_label(&json!({ "data": {} })), None);
}

/// The summary prefers the first non-empty field of the data in a fixed
/// order, then the balloon's own text, then the row's text, and only
/// then names the kind.
#[test]
fn a_balloon_summary_prefers_data_then_text_then_the_kind() {
    assert_eq!(
        balloon_summary(
            &json!({ "kind": "url", "data": { "title": "", "url": "https://x" } }),
            Some("ignored")
        ),
        "https://x"
    );
    assert_eq!(
        balloon_summary(&json!({ "kind": "url", "text": "the text" }), None),
        "the text"
    );
    assert_eq!(
        balloon_summary(&json!({ "kind": "fitness" }), Some("  a workout  ")),
        "a workout"
    );
    assert_eq!(
        balloon_summary(&json!({ "kind": "handwriting" }), None),
        "Handwriting"
    );
    assert_eq!(
        balloon_summary(&json!({ "kind": "digital_touch" }), Some("  ")),
        "Digital Touch"
    );
    assert_eq!(balloon_summary(&json!({ "kind": "poll" }), None), "Poll");
    assert_eq!(
        balloon_summary(&json!({ "kind": "apple_pay" }), None),
        "Apple Pay"
    );
    assert_eq!(
        balloon_summary(&json!({ "kind": "check_in" }), None),
        "check in"
    );
    assert_eq!(balloon_summary(&json!({}), None), "App Message");
}

#[test]
fn sticker_extras_are_empty_unless_the_attachment_is_a_sticker() {
    let plain = attachment(None);
    assert_eq!(
        sticker_extras(&plain, &Platform::macOS, Path::new("chat.db"), None),
        (None, None)
    );

    // A sticker whose file is gone still reports its prompt; the effect
    // needs the file and is `None` without it.
    let sticker = Attachment {
        is_sticker: true,
        emoji_description: Some("a cat".to_string()),
        filename: Some("/nowhere/sticker.heic".to_string()),
        ..attachment(Some("sticker-1"))
    };
    let (prompt, effect) = sticker_extras(&sticker, &Platform::macOS, Path::new("chat.db"), None);
    assert_eq!(prompt.as_deref(), Some("a cat"));
    assert_eq!(effect, None);
}

#[test]
fn a_thread_part_is_the_number_before_the_slash() {
    assert_eq!(parse_thread_part("0/1"), Some(0));
    assert_eq!(parse_thread_part("3/ABC-DEF"), Some(3));
    assert_eq!(parse_thread_part("7"), Some(7));
    assert_eq!(parse_thread_part("x/1"), None);
    assert_eq!(parse_thread_part(""), None);
}
