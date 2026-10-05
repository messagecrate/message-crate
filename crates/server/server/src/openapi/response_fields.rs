//! Every field of a success answer is required in the reference
//! (`docs/architecture/http-api.md`, "Fields").
//!
//! The server sends every field of a success answer, as `null` when it has no
//! value, so every property of a schema a success answer holds is required.
//! utoipa makes an `Option` field optional, because serde lets a reader do
//! without it, so this marks the rest required once the document is
//! assembled. Whether the server keeps to it is what `document_rules` checks,
//! by calling the routes and reading the keys of what they answer.
//!
//! Which component schemas a success answer holds is read the other way
//! round: every one that no request and no failure names. utoipa writes a
//! page's item type into `Page_X.items` rather than naming it, so a type
//! answered only in pages, such as `AuditEntry`, is named by no answer, and
//! the web app still reads it as a component.
//!
//! A schema a request also names keeps what the derive gave it, because a
//! request may leave out a field it has no value for. `ExportScope` is the one
//! such schema with properties: it is the body of `POST /v1/exports` and is
//! answered back on the run. A failure's `Problem` keeps RFC 7807's members.

use std::collections::BTreeSet;

use serde_json::Value;
use utoipa::openapi::schema::{AdditionalProperties, ArrayItems, Schema};
use utoipa::openapi::{OpenApi, RefOr};

/// Mark every property of every schema a success answer holds required.
pub(crate) fn require_every_field(spec: &mut OpenApi) {
    let doc = serde_json::to_value(&*spec).expect("the OpenAPI document serializes to JSON");
    let answers = answer_schemas(&doc);
    if let Some(components) = spec.components.as_mut() {
        for name in &answers {
            if let Some(schema) = components.schemas.get_mut(name) {
                require_all(schema);
            }
        }
    }
    for item in spec.paths.paths.values_mut() {
        for op in super::shared_parts::operations_mut(item) {
            for (status, response) in &mut op.responses.responses {
                let RefOr::T(response) = response else {
                    continue;
                };
                if !status.starts_with('2') {
                    continue;
                }
                for (media_type, content) in &mut response.content {
                    if let (true, RefOr::T(content)) = (is_json(media_type), content)
                        && let Some(schema) = content.schema.as_mut()
                    {
                        require_all(schema);
                    }
                }
            }
        }
    }
}

/// Add every property of each object in `schema` to its `required`, keeping
/// the order the derive wrote and putting the rest after it. A `$ref` is left
/// alone: the schema it names is marked on its own.
fn require_all(schema: &mut RefOr<Schema>) {
    let RefOr::T(schema) = schema else {
        return;
    };
    match schema {
        Schema::Object(object) => {
            let missing: Vec<String> = object
                .properties
                .keys()
                .filter(|name| !object.required.contains(name))
                .cloned()
                .collect();
            object.required.extend(missing);
            object.properties.values_mut().for_each(require_all);
            if let Some(AdditionalProperties::RefOr(values)) =
                object.additional_properties.as_deref_mut()
            {
                require_all(values);
            }
        }
        Schema::Array(array) => {
            if let ArrayItems::RefOrSchema(items) = &mut array.items {
                require_all(items);
            }
        }
        Schema::OneOf(one_of) => one_of.items.iter_mut().for_each(require_all),
        Schema::AllOf(all_of) => all_of.items.iter_mut().for_each(require_all),
        Schema::AnyOf(any_of) => any_of.items.iter_mut().for_each(require_all),
        _ => {}
    }
}

/// Whether a media type is JSON: `application/json` and the `+json` types.
fn is_json(media_type: &str) -> bool {
    media_type == "application/json" || media_type.ends_with("+json")
}

/// The component schemas a success answer holds: every one that no request
/// body, parameter or failure names, directly or through another schema.
pub(crate) fn answer_schemas(doc: &Value) -> BTreeSet<String> {
    let mut others = BTreeSet::new();
    for op in operations(doc) {
        name_refs(doc, &op["requestBody"], &mut others);
        name_refs(doc, &op["parameters"], &mut others);
        for (status, response) in op["responses"].as_object().into_iter().flatten() {
            if !status.starts_with('2') {
                name_refs(doc, response, &mut others);
            }
        }
    }
    doc["components"]["schemas"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name)
        .filter(|name| !others.contains(*name))
        .cloned()
        .collect()
}

/// Every operation of the document.
fn operations(doc: &Value) -> impl Iterator<Item = &Value> {
    doc["paths"]
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, item)| item.as_object().into_iter().flatten())
        .map(|(_, op)| op)
}

/// Add to `names` every component schema `node` names, and the ones those
/// name.
fn name_refs(doc: &Value, node: &Value, names: &mut BTreeSet<String>) {
    match node {
        Value::Object(map) => {
            if let Some(name) = schema_named(node)
                && names.insert(name.to_string())
            {
                name_refs(doc, &doc["components"]["schemas"][name], names);
            }
            for value in map.values() {
                name_refs(doc, value, names);
            }
        }
        Value::Array(items) => {
            for value in items {
                name_refs(doc, value, names);
            }
        }
        _ => {}
    }
}

/// The component schema a `$ref` names.
pub(crate) fn schema_named(reference: &Value) -> Option<&str> {
    reference["$ref"]
        .as_str()
        .and_then(|r| r.strip_prefix("#/components/schemas/"))
}
