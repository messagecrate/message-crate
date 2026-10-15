//! Emptiness, which decides whether a message's extensions are written at all.
//!
//! `ImessageExtensions::is_empty` is thirteen clauses and `VendorFields::is_empty` is
//! two, and neither had a test — this crate had no test module. Every clause
//! could be dropped on its own with the whole workspace green, and the effect
//! of dropping one is silent: `into_option` returns `None`, the exporter writes
//! `imessage: null`, and that field is gone from the export with nothing said.
//! A tapback, a send effect, an app balloon — each is one clause.

use super::*;

/// Set exactly one field on an otherwise default `ImessageExtensions`, and name it.
fn one_field_set() -> Vec<(&'static str, ImessageExtensions)> {
    let json = || Some(serde_json::json!([{"index": 0}]));
    vec![
        (
            "send_effect",
            ImessageExtensions {
                send_effect: Some("slam".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "shared_location",
            ImessageExtensions {
                shared_location: Some("shared".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "announcement",
            ImessageExtensions {
                announcement: Some("named the conversation".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "read_receipt_rfc3339",
            ImessageExtensions {
                read_receipt_rfc3339: Some("2015-03-12T18:04:22Z".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "parts",
            ImessageExtensions {
                parts: json(),
                ..ImessageExtensions::default()
            },
        ),
        (
            "app",
            ImessageExtensions {
                app: json(),
                ..ImessageExtensions::default()
            },
        ),
        (
            "balloon_bundle_id",
            ImessageExtensions {
                balloon_bundle_id: Some("com.apple.messages.URLBalloonProvider".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "balloon_kind",
            ImessageExtensions {
                balloon_kind: Some("url".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "associated_guid",
            ImessageExtensions {
                associated_guid: Some("assoc-guid".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "associated_part",
            ImessageExtensions {
                associated_part: Some(0),
                ..ImessageExtensions::default()
            },
        ),
        (
            "tapback_kind",
            ImessageExtensions {
                tapback_kind: Some("loved".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "tapback_emoji",
            ImessageExtensions {
                tapback_emoji: Some("\u{2764}".into()),
                ..ImessageExtensions::default()
            },
        ),
        (
            "tapback_action",
            ImessageExtensions {
                tapback_action: Some("added".into()),
                ..ImessageExtensions::default()
            },
        ),
    ]
}

/// A default `ImessageExtensions` carries nothing, so the exporter writes no
/// `imessage` object rather than an object of thirteen nulls.
#[test]
fn an_imessage_with_nothing_set_is_empty() {
    let blank = ImessageExtensions::default();
    assert!(blank.is_empty());
    assert!(blank.into_option().is_none());
}

/// Every field on its own is enough to keep the object.
///
/// A dropped clause loses exactly one kind of Apple extension from every
/// export, and nothing else changes — which is why this is one case per field
/// rather than one case with everything set.
#[test]
fn any_one_imessage_field_makes_it_worth_keeping() {
    let cases = one_field_set();
    assert_eq!(
        cases.len(),
        13,
        "one case per field on ImessageExtensions; add or remove one with the field"
    );
    for (field, value) in cases {
        assert!(
            !value.is_empty(),
            "{field} set on its own must not read as empty"
        );
        assert!(
            value.into_option().is_some(),
            "{field} set on its own must survive into_option"
        );
    }
}

/// `VendorFields` is the same shape with two clauses: the Android type code and
/// the bag of vendor fields. Losing either drops the vendor leftovers that let
/// a reader see what the source actually said.
#[test]
fn a_source_is_empty_only_when_it_has_neither_a_type_nor_a_field() {
    let blank = VendorFields::default();
    assert!(blank.is_empty());
    assert!(blank.into_option().is_none());

    let typed = VendorFields {
        android_type: Some(1),
        ..VendorFields::default()
    };
    assert!(
        !typed.is_empty(),
        "an android type is a leftover worth keeping"
    );
    assert!(typed.into_option().is_some());

    let mut fields = serde_json::Map::new();
    fields.insert("read".into(), serde_json::json!("1"));
    let with_fields = VendorFields {
        fields,
        ..VendorFields::default()
    };
    assert!(!with_fields.is_empty(), "a vendor field is worth keeping");
    assert!(with_fields.into_option().is_some());

    // An empty map is not a field.
    let empty_map = VendorFields {
        fields: serde_json::Map::new(),
        ..VendorFields::default()
    };
    assert!(empty_map.is_empty());
}

/// A blank field is no mark, each mark reads back from its own text, and any
/// other text is refused by name.
#[test]
fn a_deletion_field_reads_each_mark_and_refuses_other_text() {
    for deletion in crate::Deletion::ALL {
        assert_eq!(crate::parse_deletion(deletion.as_str()), Ok(Some(deletion)));
    }
    assert_eq!(crate::parse_deletion(" "), Ok(None));
    assert_eq!(
        crate::parse_deletion("deleted").unwrap_err().to_string(),
        r#""deleted" is neither deleted_in_source_app nor unsent"#
    );
}
