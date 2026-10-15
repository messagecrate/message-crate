//! Emptiness, which decides whether a message's extensions are written at all.
//!
//! `Imessage::is_empty` is thirteen clauses and `Source::is_empty` is
//! two, and neither had a test — this crate had no test module. Every clause
//! could be dropped on its own with the whole workspace green, and the effect
//! of dropping one is silent: `into_option` returns `None`, the exporter writes
//! `imessage: null`, and that field is gone from the export with nothing said.
//! A tapback, a send effect, an app balloon — each is one clause.

use super::*;

/// Set exactly one field on an otherwise default `Imessage`, and name it.
fn one_field_set() -> Vec<(&'static str, Imessage)> {
    let json = || Some(serde_json::json!([{"index": 0}]));
    vec![
        (
            "send_effect",
            Imessage {
                send_effect: Some("slam".into()),
                ..Imessage::default()
            },
        ),
        (
            "shared_location",
            Imessage {
                shared_location: Some("shared".into()),
                ..Imessage::default()
            },
        ),
        (
            "announcement",
            Imessage {
                announcement: Some("named the conversation".into()),
                ..Imessage::default()
            },
        ),
        (
            "read_receipt_rfc3339",
            Imessage {
                read_receipt_rfc3339: Some("2015-03-12T18:04:22Z".into()),
                ..Imessage::default()
            },
        ),
        (
            "parts",
            Imessage {
                parts: json(),
                ..Imessage::default()
            },
        ),
        (
            "app",
            Imessage {
                app: json(),
                ..Imessage::default()
            },
        ),
        (
            "balloon_bundle_id",
            Imessage {
                balloon_bundle_id: Some("com.apple.messages.URLBalloonProvider".into()),
                ..Imessage::default()
            },
        ),
        (
            "balloon_kind",
            Imessage {
                balloon_kind: Some("url".into()),
                ..Imessage::default()
            },
        ),
        (
            "associated_guid",
            Imessage {
                associated_guid: Some("assoc-guid".into()),
                ..Imessage::default()
            },
        ),
        (
            "associated_part",
            Imessage {
                associated_part: Some(0),
                ..Imessage::default()
            },
        ),
        (
            "tapback_kind",
            Imessage {
                tapback_kind: Some("loved".into()),
                ..Imessage::default()
            },
        ),
        (
            "tapback_emoji",
            Imessage {
                tapback_emoji: Some("\u{2764}".into()),
                ..Imessage::default()
            },
        ),
        (
            "tapback_action",
            Imessage {
                tapback_action: Some("added".into()),
                ..Imessage::default()
            },
        ),
    ]
}

/// A default `Imessage` carries nothing, so the exporter writes no
/// `imessage` object rather than an object of thirteen nulls.
#[test]
fn an_imessage_with_nothing_set_is_empty() {
    let blank = Imessage::default();
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
        "one case per field on Imessage; add or remove one with the field"
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

/// `Source` is the same shape with two clauses: the Android type code and
/// the bag of vendor fields. Losing either drops the vendor leftovers that let
/// a reader see what the source actually said.
#[test]
fn a_source_is_empty_only_when_it_has_neither_a_type_nor_a_field() {
    let blank = Source::default();
    assert!(blank.is_empty());
    assert!(blank.into_option().is_none());

    let typed = Source {
        android_type: Some(1),
        ..Source::default()
    };
    assert!(
        !typed.is_empty(),
        "an android type is a leftover worth keeping"
    );
    assert!(typed.into_option().is_some());

    let mut fields = serde_json::Map::new();
    fields.insert("read".into(), serde_json::json!("1"));
    let with_fields = Source {
        fields,
        ..Source::default()
    };
    assert!(!with_fields.is_empty(), "a vendor field is worth keeping");
    assert!(with_fields.into_option().is_some());

    // An empty map is not a field.
    let empty_map = Source {
        fields: serde_json::Map::new(),
        ..Source::default()
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
