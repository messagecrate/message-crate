//! A field's description sits on the field (`docs/architecture/http-api.md`,
//! "The reference").
//!
//! utoipa writes the doc comment of a field typed by a schema of its own
//! beside that schema's `$ref`. For a required field the `$ref` is the
//! field's whole schema, so the description is on the property. For an
//! `Option` field the property is a `oneOf` of the `$ref` and `null`, and the
//! description lands inside the `$ref`'s branch. openapi-typescript reads no
//! description there, so the web app's generated type for that field carried
//! no doc. [`lift`] moves the description up onto the `oneOf`.
//!
//! A `oneOf` that is a schema of its own, such as `AccountImportRun`, keeps
//! each branch's description, because there it describes the branch, not a
//! field.
//!
//! The tag of an internally tagged enum (`#[serde(tag = "kind")]`), such as
//! `ExportScope.kind`, is a field no Rust code declares, so it has no doc
//! comment to carry. [`describe_tags`] writes its description from the tag's
//! values and each form's own doc comment, the variant's.

use utoipa::openapi::schema::{Object, Schema, SchemaType, Type};
use utoipa::openapi::{OpenApi, RefOr};

use super::shared_parts::{for_each_object, for_each_schema, operations_mut, split_first_sentence};

/// Move the description of every optional field typed by a schema from
/// inside its `oneOf` onto the field.
pub(crate) fn lift(spec: &mut OpenApi) {
    let lift_fields = &mut |object: &mut Object| {
        object.properties.values_mut().for_each(lift_field);
    };
    each_root_schema(spec, &mut |schema| for_each_object(schema, lift_fields));
}

/// Describe the tag of every internally tagged enum: in each form, the tag
/// names that form, says what it means, and names the others.
pub(crate) fn describe_tags(spec: &mut OpenApi) {
    each_root_schema(spec, &mut |schema| {
        for_each_schema(schema, &mut describe_tag)
    });
}

/// Call `f` on every schema the document holds at its root: each component
/// schema, and each request and response body schema an operation writes
/// inline.
fn each_root_schema(spec: &mut OpenApi, f: &mut impl FnMut(&mut RefOr<Schema>)) {
    if let Some(components) = spec.components.as_mut() {
        components.schemas.values_mut().for_each(&mut *f);
    }
    for item in spec.paths.paths.values_mut() {
        for op in operations_mut(item) {
            let request = op.request_body.iter_mut().flat_map(|body| match body {
                RefOr::T(body) => Some(body.content.values_mut()),
                RefOr::Ref(_) => None,
            });
            let responses =
                op.responses
                    .responses
                    .values_mut()
                    .flat_map(|response| match response {
                        RefOr::T(response) => Some(response.content.values_mut()),
                        RefOr::Ref(_) => None,
                    });
            for content in request.flatten().chain(responses.flatten()) {
                if let RefOr::T(content) = content
                    && let Some(schema) = content.schema.as_mut()
                {
                    f(schema);
                }
            }
        }
    }
}

/// When `schema` is a `oneOf` of two or more objects that each require one
/// property, the tag, holding a single string that differs in every form,
/// give each form's tag that has no description one that names the form,
/// says what it means from the first sentence of the form's own description,
/// and names the others.
fn describe_tag(schema: &mut Schema) {
    let Schema::OneOf(one_of) = schema else {
        return;
    };
    let branches = one_of.items.len();
    let forms: Vec<&mut Object> = one_of
        .items
        .iter_mut()
        .filter_map(|branch| match branch {
            RefOr::T(Schema::Object(object)) => Some(object),
            _ => None,
        })
        .collect();
    if forms.len() < 2 || forms.len() != branches {
        return;
    }
    let Some(tag) = forms[0]
        .required
        .iter()
        .find(|name| forms.iter().all(|form| tag_value(form, name).is_some()))
        .cloned()
    else {
        return;
    };
    let values: Vec<String> = forms
        .iter()
        .filter_map(|form| tag_value(form, &tag))
        .collect();
    let mut distinct = values.clone();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != values.len() {
        return;
    }
    for (form, value) in forms.into_iter().zip(&values) {
        let others: Vec<&str> = values
            .iter()
            .filter(|other| *other != value)
            .map(String::as_str)
            .collect();
        let meaning = form.description.as_deref().map(form_meaning);
        if let Some(RefOr::T(Schema::Object(field))) = form.properties.get_mut(&tag)
            && field.description.is_none()
        {
            let names = match meaning {
                Some(meaning) => format!("Names this form, `{value}`: {meaning}"),
                None => format!("Names this form, `{value}`."),
            };
            field.description = Some(format!(
                "{names} The other forms are {}.",
                code_list(&others)
            ));
        }
    }
}

/// The first sentence of a form's own description, its doc comment, to
/// follow a colon: on one line, with its first letter lower case unless the
/// word is an acronym such as `SMS`.
fn form_meaning(description: &str) -> String {
    let flat = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let (first, _) = split_first_sentence(&flat);
    let mut chars = first.chars();
    match (chars.next(), chars.next()) {
        (Some(initial), Some(next)) if !next.is_uppercase() => {
            format!("{}{}", initial.to_lowercase(), &first[initial.len_utf8()..])
        }
        _ => first.to_string(),
    }
}

/// The one string `form` requires its property `name` to hold, when `name` is
/// a required property whose schema is an `enum` of exactly one string.
fn tag_value(form: &Object, name: &str) -> Option<String> {
    if !form.required.iter().any(|required| required == name) {
        return None;
    }
    let Some(RefOr::T(Schema::Object(field))) = form.properties.get(name) else {
        return None;
    };
    match field.enum_values.as_deref() {
        Some([serde_json::Value::String(value)]) => Some(value.clone()),
        _ => None,
    }
}

/// `values` in backticks, joined as prose: "`a`", "`a` and `b`", "`a`, `b`
/// and `c`".
fn code_list(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| format!("`{value}`")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// When `field` is a `oneOf` of one described `$ref` and `null`, with no
/// description of its own, move the `$ref`'s description onto the `oneOf`.
fn lift_field(field: &mut RefOr<Schema>) {
    let RefOr::T(Schema::OneOf(one_of)) = field else {
        return;
    };
    if one_of.description.is_some() {
        return;
    }
    let (refs, others): (Vec<_>, Vec<_>) = one_of
        .items
        .iter_mut()
        .partition(|branch| matches!(branch, RefOr::Ref(_)));
    let [RefOr::Ref(reference)] = refs.as_slice() else {
        return;
    };
    if reference.description.is_empty() || !others.iter().all(|branch| is_null(branch)) {
        return;
    }
    let description = reference.description.clone();
    for branch in &mut one_of.items {
        if let RefOr::Ref(reference) = branch {
            reference.description.clear();
        }
    }
    one_of.description = Some(description);
}

/// Whether `branch` is the `null` of an `Option`.
fn is_null(branch: &RefOr<Schema>) -> bool {
    matches!(
        branch,
        RefOr::T(Schema::Object(object)) if object.schema_type == SchemaType::Type(Type::Null)
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    /// A document of one schema, `Request`, whose `properties` are `fields`.
    fn document(fields: Value) -> OpenApi {
        serde_json::from_value(json!({
            "openapi": "3.1.0",
            "info": { "title": "t", "version": "1" },
            "paths": {},
            "components": { "schemas": {
                "Service": { "type": "string", "enum": ["sms", "whatsapp"] },
                "Request": { "type": "object", "properties": fields }
            } }
        }))
        .unwrap()
    }

    fn lifted(fields: Value) -> Value {
        let mut spec = document(fields);
        lift(&mut spec);
        serde_json::to_value(&spec).unwrap()["components"]["schemas"]["Request"]["properties"]
            .clone()
    }

    /// The bug: openapi-typescript dropped the description of an optional
    /// field typed by a schema, because utoipa wrote it beside the `$ref`.
    #[test]
    fn an_optional_field_typed_by_a_schema_carries_its_description() {
        let fields = lifted(json!({
            "service": { "oneOf": [
                { "$ref": "#/components/schemas/Service", "description": "The service." },
                { "type": "null" }
            ] }
        }));
        assert_eq!(
            fields["service"],
            json!({
                "oneOf": [{ "$ref": "#/components/schemas/Service" }, { "type": "null" }],
                "description": "The service."
            })
        );
    }

    #[test]
    fn a_field_nested_in_an_array_of_objects_is_lifted_too() {
        let fields = lifted(json!({
            "items": { "type": "array", "items": { "type": "object", "properties": {
                "service": { "oneOf": [
                    { "$ref": "#/components/schemas/Service", "description": "The service." },
                    { "type": "null" }
                ] }
            } } }
        }));
        assert_eq!(
            fields["items"]["items"]["properties"]["service"]["description"],
            "The service."
        );
    }

    /// A choice between two schemas describes each branch, not the field.
    #[test]
    fn a_choice_between_schemas_keeps_each_branch_description() {
        let choice = json!({ "oneOf": [
            { "$ref": "#/components/schemas/Service", "description": "One." },
            { "$ref": "#/components/schemas/Request", "description": "Other." }
        ] });
        assert_eq!(
            lifted(json!({ "either": choice.clone() }))["either"],
            choice
        );
    }

    #[test]
    fn a_field_that_has_its_own_description_keeps_both() {
        let field = json!({
            "oneOf": [
                { "$ref": "#/components/schemas/Service", "description": "Inside." },
                { "type": "null" }
            ],
            "description": "Outside."
        });
        assert_eq!(
            lifted(json!({ "service": field.clone() }))["service"],
            field
        );
    }

    /// A document whose one schema, `Scope`, is `forms`, and describe its tags.
    fn tags_described(forms: Value) -> Value {
        let mut spec: OpenApi = serde_json::from_value(json!({
            "openapi": "3.1.0",
            "info": { "title": "t", "version": "1" },
            "paths": {},
            "components": { "schemas": { "Scope": { "oneOf": forms } } }
        }))
        .unwrap();
        describe_tags(&mut spec);
        serde_json::to_value(&spec).unwrap()["components"]["schemas"]["Scope"]["oneOf"].clone()
    }

    /// A form of an internally tagged enum, `kind` holding `value`.
    fn form(value: &str) -> Value {
        json!({
            "type": "object",
            "required": ["kind"],
            "properties": { "kind": { "type": "string", "enum": [value] } }
        })
    }

    /// A form as `form` makes it, with `description`, its variant's doc
    /// comment.
    fn described_form(value: &str, description: &str) -> Value {
        let mut form = form(value);
        form["description"] = json!(description);
        form
    }

    /// The bug: the tag of `ExportScope` had no description, because no Rust
    /// field declares it.
    #[test]
    fn each_form_of_a_tagged_enum_says_what_it_means_and_names_the_others() {
        let forms = tags_described(json!([
            described_form("everything", "Every message the account holds."),
            described_form("query", "What a query finds.\nOn one of two lists. More."),
            described_form("sms", "SMS messages only.")
        ]));
        assert_eq!(
            forms[0]["properties"]["kind"]["description"],
            "Names this form, `everything`: every message the account holds. \
             The other forms are `query` and `sms`."
        );
        assert_eq!(
            forms[1]["properties"]["kind"]["description"],
            "Names this form, `query`: what a query finds. \
             The other forms are `everything` and `sms`."
        );
        assert_eq!(
            forms[2]["properties"]["kind"]["description"],
            "Names this form, `sms`: SMS messages only. \
             The other forms are `everything` and `query`."
        );
    }

    #[test]
    fn a_form_with_no_description_is_named_alone() {
        let forms = tags_described(json!([form("one"), form("two")]));
        assert_eq!(
            forms[0]["properties"]["kind"]["description"],
            "Names this form, `one`. The other forms are `two`."
        );
    }

    #[test]
    fn a_tag_that_has_a_description_keeps_it() {
        let mut described = form("one");
        described["properties"]["kind"]["description"] = json!("Mine.");
        let forms = tags_described(json!([described, form("two")]));
        assert_eq!(forms[0]["properties"]["kind"]["description"], "Mine.");
        assert_eq!(
            forms[1]["properties"]["kind"]["description"],
            "Names this form, `two`. The other forms are `one`."
        );
    }

    /// Two forms holding the same value are not told apart by it, so it is
    /// not a tag.
    #[test]
    fn a_property_with_the_same_value_in_two_forms_is_not_a_tag() {
        let forms = tags_described(json!([form("same"), form("same")]));
        assert!(forms[0]["properties"]["kind"].get("description").is_none());
    }
}
