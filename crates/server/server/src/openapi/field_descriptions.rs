//! A field's description sits on the field (`docs/architecture/http-api.md`,
//! "The reference").
//!
//! utoipa writes the doc comment of a field typed by a schema of its own
//! beside that schema's `$ref`. For a required field the `$ref` is the
//! field's whole schema, so the description is on the property. For an
//! `Option` field the property is a `oneOf` of the `$ref` and `null`, and the
//! description lands inside the `$ref`'s branch. openapi-typescript reads no
//! description there, so the web app's generated type for that field carried
//! no doc. This moves the description up onto the `oneOf`.
//!
//! A `oneOf` that is a schema of its own, such as `AccountImportRun`, keeps
//! each branch's description, because there it describes the branch, not a
//! field.

use utoipa::openapi::schema::{Object, Schema, SchemaType, Type};
use utoipa::openapi::{OpenApi, RefOr};

use super::shared_parts::{for_each_object, operations_mut};

/// Move the description of every optional field typed by a schema from
/// inside its `oneOf` onto the field.
pub(crate) fn lift(spec: &mut OpenApi) {
    let lift_fields = &mut |object: &mut Object| {
        object.properties.values_mut().for_each(lift_field);
    };
    if let Some(components) = spec.components.as_mut() {
        for schema in components.schemas.values_mut() {
            for_each_object(schema, lift_fields);
        }
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
                    for_each_object(schema, lift_fields);
                }
            }
        }
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
}
