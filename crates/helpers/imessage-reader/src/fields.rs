//! Structured helpers for message record fields (parts, edits, balloons).
//!

use imessage_database::{
    message_types::{
        app::AppMessage,
        edited::EditedMessage,
        expressives::Expressive,
        text_effects::text_effect::TextEffect,
        url::URLMessage,
        variants::{BalloonProvider, CustomBalloon, URLOverride, Variant},
    },
    tables::{
        attachment::Attachment,
        messages::{
            Message,
            models::{BubbleComponent, SharedLocation},
        },
    },
    util::{
        bundle_id::parse_balloon_bundle_id, dates::get_local_time, platform::Platform,
        plist::parse_ns_keyed_archiver,
    },
};
use imessage_reader_protocol::EarlierVersion;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

use crate::body::{AttachmentResolver, resolve_run};

/// One logical message body part.
#[derive(Serialize)]
pub(crate) struct PartRecord {
    pub index: usize,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachment_indices: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub emoji_image: bool,
}

/// Screen-effect label (slam, loud, invisible ink, and similar), if any.
pub(crate) fn expressive_label(expressive: Expressive<'_>) -> Option<String> {
    let label = expressive.to_string();
    if label.is_empty() { None } else { Some(label) }
}

/// `"started"` or `"stopped"` for a live-location share.
pub(crate) fn shared_location_label(kind: SharedLocation) -> &'static str {
    match kind {
        SharedLocation::Started => "started",
        SharedLocation::Stopped => "stopped",
    }
}

/// Compact label for a typed-stream text effect.
fn effect_label(effect: &TextEffect) -> String {
    match effect {
        TextEffect::Default => "default".to_string(),
        TextEffect::Mention(id) => format!("mention:{id}"),
        TextEffect::Link(url) => format!("link:{url}"),
        TextEffect::OTP => "otp".to_string(),
        TextEffect::Address(_) => "address".to_string(),
        TextEffect::Styles(styles) => format!("styles:{styles:?}"),
        TextEffect::Animated(anim) => format!("animated:{anim:?}"),
        TextEffect::Conversion(_) => "conversion".to_string(),
        TextEffect::Currency(_) => "currency".to_string(),
        TextEffect::Tracking(_) => "tracking".to_string(),
        TextEffect::Flight(_) => "flight".to_string(),
    }
}

/// The earlier versions of an edited message from its parsed
/// [`EditedMessage`] metadata: every entry of each part's edit history but
/// the last, which is the text the message holds now. Each version's time is
/// the date its history entry carries, the send time for the first.
/// An unsent part has no history and so no earlier version; the message's
/// `deletion` says what was unsent.
pub(crate) fn build_earlier_versions(edited: &EditedMessage, offset: i64) -> Vec<EarlierVersion> {
    let mut out = Vec::new();
    for (part_index, part) in edited.parts.iter().enumerate() {
        let Ok(part_index) = u32::try_from(part_index) else {
            continue;
        };
        let Some((_final, earlier)) = part.edit_history.split_last() else {
            continue;
        };
        for event in earlier {
            out.push(EarlierVersion {
                part_index,
                text: Some(event.text.clone()),
                edited_at_unix_ms: get_local_time(event.date, offset)
                    .ok()
                    .map(|date| date.timestamp_millis()),
            });
        }
    }
    out
}

/// Build multipart `parts` from the message body components.
pub(crate) fn build_part_records(message: &Message, attachments: &[Attachment]) -> Vec<PartRecord> {
    if message.components.is_empty() {
        return Vec::new();
    }

    let mut resolver = AttachmentResolver::new(attachments);
    let mut parts = Vec::new();

    for (index, component) in message.components.iter().enumerate() {
        match component {
            BubbleComponent::Run(ranges) => {
                let resolved = resolve_run(ranges, &mut resolver);
                let mut texts = Vec::new();
                let mut attachment_indices = Vec::new();
                let mut effects = Vec::new();
                let mut emoji_image = false;

                for (range, att_idx) in resolved {
                    if let Some(idx) = att_idx {
                        if idx < attachments.len() {
                            attachment_indices.push(idx);
                        }
                    } else if let Some(text) = message.text.as_deref() {
                        let start = range.start.min(text.len());
                        let end = range.end.min(text.len());
                        if start < end {
                            texts.push(text[start..end].to_string());
                        }
                    }
                    for effect in &range.effects {
                        let label = effect_label(effect);
                        if label != "default" {
                            effects.push(label);
                        }
                    }
                    if range.emoji_image {
                        emoji_image = true;
                    }
                }

                let joined = texts.concat();
                parts.push(PartRecord {
                    index,
                    kind: "run",
                    text: if joined.is_empty() {
                        None
                    } else {
                        Some(joined)
                    },
                    attachment_indices,
                    effects,
                    emoji_image,
                });
            }
            BubbleComponent::App => parts.push(PartRecord {
                index,
                kind: "app",
                text: None,
                attachment_indices: Vec::new(),
                effects: Vec::new(),
                emoji_image: false,
            }),
            BubbleComponent::Retracted => parts.push(PartRecord {
                index,
                kind: "retracted",
                text: None,
                attachment_indices: Vec::new(),
                effects: Vec::new(),
                emoji_image: false,
            }),
        }
    }

    parts
}

/// Collect transcription text from body ranges matching an attachment GUID.
pub(crate) fn transcription_for_attachment(
    message: &Message,
    attachment: &Attachment,
) -> Option<String> {
    let guid = attachment.guid.as_deref()?;
    for component in &message.components {
        let BubbleComponent::Run(ranges) = component else {
            continue;
        };
        for range in ranges {
            if let Some(meta) = &range.attachment
                && meta.guid.as_deref() == Some(guid)
                && let Some(t) = &meta.transcription
                && !t.is_empty()
            {
                return Some(t.clone());
            }
        }
    }
    None
}

/// JSON object for an iMessage app bubble payload.
fn app_message_json(bubble: &AppMessage<'_>) -> Value {
    json!({
        "image": bubble.image,
        "url": bubble.url,
        "title": bubble.title,
        "subtitle": bubble.subtitle,
        "caption": bubble.caption,
        "subcaption": bubble.subcaption,
        "trailing_caption": bubble.trailing_caption,
        "trailing_subcaption": bubble.trailing_subcaption,
        "app_name": bubble.app_name,
        "ldtext": bubble.ldtext,
    })
}

/// JSON object for a rich-link (URL) bubble payload.
fn url_message_json(bubble: &URLMessage<'_>) -> Value {
    json!({
        "title": bubble.title,
        "summary": bubble.summary,
        "url": bubble.url,
        "original_url": bubble.original_url,
        "item_type": bubble.item_type,
        "images": bubble.images,
        "icons": bubble.icons,
        "site_name": bubble.site_name,
        "placeholder": bubble.placeholder,
    })
}

/// Best-effort structured balloon/app payload for archival.
pub(crate) fn build_balloon_value(db: &Connection, message: &Message) -> Option<Value> {
    let Variant::App(balloon) = message.variant() else {
        return None;
    };
    if message.is_handwriting() {
        return Some(json!({
            "kind": "handwriting",
            "bundle_id": message.balloon_bundle_id,
        }));
    }
    if message.is_digital_touch() {
        return Some(json!({
            "kind": "digital_touch",
            "bundle_id": message.balloon_bundle_id,
        }));
    }
    if message.is_poll() {
        return Some(poll_value(db, message));
    }
    let Some(payload) = message.payload_data(db) else {
        return Some(payloadless_value(&balloon, message));
    };
    let parsed = parse_ns_keyed_archiver(&payload).ok()?;
    if message.is_url() {
        return url_override_value(&parsed);
    }
    let bubble = AppMessage::from_map(&parsed).ok()?;
    Some(app_bubble_value(&balloon, message, &bubble))
}

/// The stable name of a balloon kind, as written in the `kind` field.
fn balloon_kind_name(balloon: &CustomBalloon) -> &'static str {
    match balloon {
        CustomBalloon::ApplePay => "apple_pay",
        CustomBalloon::Fitness => "fitness",
        CustomBalloon::Slideshow => "slideshow",
        CustomBalloon::CheckIn => "check_in",
        CustomBalloon::FindMy => "find_my",
        CustomBalloon::Business => "business",
        CustomBalloon::Application(_) => "application",
        CustomBalloon::URL => "url",
        CustomBalloon::Handwriting => "handwriting",
        CustomBalloon::DigitalTouch => "digital_touch",
        CustomBalloon::Polls => "poll",
    }
}

/// A poll's options and votes in display order, or an `unparseable` marker.
fn poll_value(db: &Connection, message: &Message) -> Value {
    let Ok(Some(poll)) = message.as_poll(db) else {
        return json!({ "kind": "poll", "error": "unparseable" });
    };
    let options: Vec<Value> = poll
        .order
        .iter()
        .filter_map(|id| {
            let opt = poll.options.get(id)?;
            Some(json!({
                "id": id,
                "text": opt.text,
                "creator": opt.creator,
                "votes": opt.votes.iter().map(|v| json!({
                    "voter": v.voter,
                    "option_id": v.option_id,
                })).collect::<Vec<_>>(),
            }))
        })
        .collect();
    json!({ "kind": "poll", "options": options })
}

/// A balloon with no payload blob: only its kind, bundle id, and text are known.
fn payloadless_value(balloon: &CustomBalloon, message: &Message) -> Value {
    if message.is_url() {
        return json!({ "kind": "url", "text": message.text });
    }
    json!({
        "kind": balloon_kind_name(balloon),
        "bundle_id": message.balloon_bundle_id,
        "text": message.text,
    })
}

/// A URL balloon's rich preview, by the override Messages stored for it.
fn url_override_value(parsed: &plist::Value) -> Option<Value> {
    let override_msg = URLMessage::get_url_message_override(parsed).ok()?;
    Some(match override_msg {
        URLOverride::Normal(b) => json!({ "kind": "url", "data": url_message_json(&b) }),
        URLOverride::AppleMusic(b) => json!({
            "kind": "apple_music",
            "data": {
                "url": b.url,
                "preview": b.preview,
                "artist": b.artist,
                "album": b.album,
                "track_name": b.track_name,
                "lyrics": b.lyrics,
            }
        }),
        URLOverride::Collaboration(b) => json!({
            "kind": "collaboration",
            "data": {
                "url": b.url,
                "title": b.title,
                "bundle_id": b.bundle_id,
            }
        }),
        URLOverride::AppStore(b) => json!({
            "kind": "app_store",
            "data": {
                "url": b.url,
                "original_url": b.original_url,
                "app_name": b.app_name,
                "description": b.description,
                "platform": b.platform,
                "genre": b.genre,
            }
        }),
        URLOverride::SharedPlacemark(b) => json!({
            "kind": "placemark",
            "data": {
                "url": b.url,
                "name": b.placemark.name,
                "address": b.placemark.address,
                "city": b.placemark.city,
                "state": b.placemark.state,
                "country": b.placemark.country,
                "postal_code": b.placemark.postal_code,
                "street": b.placemark.street,
            }
        }),
    })
}

/// An app balloon's parsed bubble. Third-party apps also carry the bundle id
/// as parsed from the balloon's own field.
fn app_bubble_value(balloon: &CustomBalloon, message: &Message, bubble: &AppMessage) -> Value {
    match balloon {
        CustomBalloon::Application(bundle_id) => json!({
            "kind": "application",
            "bundle_id": bundle_id,
            "parsed_bundle_id": parse_balloon_bundle_id(message.balloon_bundle_id.as_deref()),
            "data": app_message_json(bubble),
        }),
        other => json!({
            "kind": balloon_kind_name(other),
            "bundle_id": message.balloon_bundle_id,
            "data": app_message_json(bubble),
        }),
    }
}

/// Balloon kind string for `X-ME-Balloon-Kind` (from `X-ME-App` JSON or variant).
pub(crate) fn balloon_kind_label(app: &Value) -> Option<String> {
    app.get("kind").and_then(|v| v.as_str()).map(str::to_string)
}

/// Plain-text summary for balloon messages.
pub(crate) fn balloon_summary(app: &Value, fallback_text: Option<&str>) -> String {
    if let Some(data) = app.get("data") {
        for key in ["title", "caption", "app_name", "url", "track_name", "name"] {
            if let Some(s) = data
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                return s.to_string();
            }
        }
    }
    if let Some(t) = app
        .get("text")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return t.to_string();
    }
    if let Some(t) = fallback_text.map(str::trim).filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    match app.get("kind").and_then(|v| v.as_str()) {
        Some("handwriting") => "Handwriting".into(),
        Some("digital_touch") => "Digital Touch".into(),
        Some("poll") => "Poll".into(),
        Some("apple_pay") => "Apple Pay".into(),
        Some(other) => other.replace('_', " "),
        None => "App Message".into(),
    }
}

/// Sticker presentation extras when available from the attachment row.
pub(crate) fn sticker_extras(
    attachment: &Attachment,
    platform: &Platform,
    db_path: &Path,
    attachment_root: Option<&str>,
) -> (Option<String>, Option<String>) {
    if !attachment.is_sticker {
        return (None, None);
    }
    let prompt = attachment.emoji_description.clone();
    let effect = attachment
        .get_sticker_effect(platform, db_path, attachment_root)
        .ok()
        .flatten()
        .map(|e| format!("{e:?}"));
    (prompt, effect)
}

/// Parse the part index from a thread identifier like `"0/1"`.
pub(crate) fn parse_thread_part(part: &str) -> Option<u32> {
    part.split('/').next()?.parse().ok()
}

#[cfg(test)]
mod tests;
