//! Every rule of `docs/architecture/http-api.md` that can be checked by
//! walking the OpenAPI document, checked over every operation in it
//! ("The reference": a rule checked one route at a time is checked on the
//! routes someone remembered).
//!
//! Part of it reads the document: the page shape and paging parameters of every
//! list, every property of a success answer required, a `Location` on every
//! `201`, a `404` on every path with an id, no body
//! on the success of a `HEAD`, one-sentence summaries, declared tags,
//! kebab-case paths and the nesting depth. The rest calls every operation
//! through the router, on the credential matrix's fixture: with no credential
//! it must answer `401`, with a query parameter it does not declare `422`, with
//! a body that has no `Content-Type` or one the route does not take `415`, with
//! a JSON body that is not JSON `400`, and a list with `limit`, or an `offset`
//! past the ceiling its description states, out of range `422`. Each answer
//! must be a problem document carrying its `request_id`, of a status and type
//! the operation's document lists. An `Accept` that names nothing JSON must
//! answer `406` exactly where the document lists it; a `GET` that succeeds must
//! answer a media type its document declares; a success must carry every field
//! its document declares, `null` where it has no value; and a `201` must name
//! in its `Location` a resource the same credential can `GET`.
//!
//! The failures an operation's shape brings are written into the document
//! by `shared_parts`, so a check that reads them back from the document
//! passes whatever `shared_parts` does. Calling the operation is what shows
//! the document says what the server answers. Like the matrix, it walks the
//! in-process document, so a new route is covered the moment it is
//! registered.

use std::collections::BTreeSet;

use axum::http::StatusCode;
use serde_json::Value;

use super::credential_matrix::{self, Operation, Shared, World};
use super::dump_openapi_json;
use super::response_fields::{self, schema_named};
use super::shared_parts::{PROBLEM_TYPES, split_first_sentence};
use crate::paging::MAX_LIST_OFFSET;
use crate::problem::{Problem, ProblemType};

/// The one route nested three deep (`docs/architecture/http-api.md`,
/// "Naming a route").
const MULTIPART_PART: &str = "/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}";

/// The four keys of every page.
const PAGE_KEYS: [&str; 4] = ["items", "total", "limit", "offset"];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_operation_keeps_the_rules_the_document_can_show() {
    let doc: Value = serde_json::from_str(&dump_openapi_json()).unwrap();
    let operations = credential_matrix::operations();
    let mut broken: Vec<String> = Vec::new();
    for op in &operations {
        let spec = &doc["paths"][&op.path][&op.method];
        for rule in read_rules(&doc, op, spec) {
            broken.push(format!("{}: {rule}", op.label()));
        }
    }
    // Every component a success answer holds, including the ones a page
    // holds without naming them ("Fields").
    let mut optional = BTreeSet::new();
    for name in response_fields::answer_schemas(&doc) {
        optional_fields(&doc["components"]["schemas"][&name], &name, &mut optional);
    }
    broken.extend(
        optional
            .into_iter()
            .map(|field| format!("{field} is optional in a success answer")),
    );

    // Each operation gets accounts and rows of its own, so a call the server
    // wrongly accepts (a delete, a logout) cannot change what the next
    // operation sees.
    let shared = Shared::build().await;
    let mut refuse_the_demo_account = BTreeSet::new();
    for (n, op) in operations.iter().enumerate() {
        let world = World::build(&shared, n).await;
        let spec = &doc["paths"][&op.path][&op.method];
        for rule in called_rules(&doc, &world, op, spec).await {
            broken.push(format!("{}: {rule}", op.label()));
        }
        match demo_account_rule(&world, op, spec).await {
            DemoAnswer::RefusedAsDocumented => {
                refuse_the_demo_account.insert(op.label());
            }
            DemoAnswer::RefusedBreaking(rule) => {
                refuse_the_demo_account.insert(op.label());
                broken.push(format!("{}: {rule}", op.label()));
            }
            DemoAnswer::Other => {}
        }
    }
    // The check above is only as good as the routes that reach it: an import
    // start, a delete for good and an address book load must each refuse the
    // Demo Account by its id.
    for label in ["POST /v1/imports", "DELETE /v1/trash", "POST /v1/contacts"] {
        if !refuse_the_demo_account.contains(label) {
            broken.push(format!(
                "{label}: the Demo Account was not refused by its id"
            ));
        }
    }

    assert!(
        operations.len() >= 80,
        "walked only {} operations; is the document whole?",
        operations.len()
    );
    assert!(
        broken.is_empty(),
        "{} rules broken:\n{}",
        broken.len(),
        broken.join("\n")
    );
}

/// The rules one operation breaks on paper.
fn read_rules(doc: &Value, op: &Operation, spec: &Value) -> Vec<String> {
    let mut broken = Vec::new();

    let segments: Vec<&str> = op.path.trim_start_matches('/').split('/').collect();
    for segment in &segments {
        if !segment.starts_with('{') && !is_kebab(segment) {
            broken.push(format!("path segment {segment} is not kebab-case"));
        }
    }
    let depth = segments.iter().filter(|s| s.starts_with('{')).count();
    if depth > 2 && op.path != MULTIPART_PART {
        broken.push(format!("nested {depth} deep; the rule is at most two"));
    }

    let summary = spec["summary"].as_str().unwrap_or_default();
    if summary.is_empty() {
        broken.push("no summary".to_string());
    } else if !split_first_sentence(summary).1.is_empty() {
        broken.push(format!("summary is more than one sentence: {summary}"));
    }
    let declared_tags: BTreeSet<&str> = doc["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for tag in spec["tags"].as_array().into_iter().flatten() {
        let tag = tag.as_str().unwrap_or_default();
        if !declared_tags.contains(tag) {
            broken.push(format!("tag {tag} is not declared"));
        }
    }

    let responses = spec["responses"].as_object().cloned().unwrap_or_default();
    // A failure keeps its problem document's media type, as every failure
    // does, and the HEAD answer carries the header without the body.
    if op.method == "head" {
        for (status, response) in &responses {
            if status.starts_with('2') && !response["content"].is_null() {
                broken.push(format!(
                    "a HEAD answer has no body, and its {status} declares one"
                ));
            }
        }
    }
    if responses.contains_key("201") && spec["responses"]["201"]["headers"]["Location"].is_null() {
        broken.push("201 without a Location header".to_string());
    }
    // `shared_parts` gives `404` to an operation that declares a path
    // parameter, so a handler that leaves its id out of `params(...)` gets
    // none, though the server answers `404` for an id that is not there.
    // This reads the path itself, which `shared_parts` does not.
    if op.path.contains('{') && !responses.contains_key("404") {
        broken.push("no 404, which an id in the path brings".to_string());
    }

    // Every field of a success answer is sent, `null` when it has no value
    // ("Fields"), so the document requires every one. A schema it names is
    // checked once, over the whole document.
    let mut optional = BTreeSet::new();
    for schema in success_schemas(spec) {
        optional_fields(schema, "answer", &mut optional);
    }
    broken.extend(
        optional
            .into_iter()
            .map(|field| format!("{field} is optional in a success answer")),
    );

    for page in page_schemas(doc, spec) {
        let required: BTreeSet<&str> = page["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for key in PAGE_KEYS {
            if !required.contains(key) {
                broken.push(format!("the page it answers has no required {key}"));
            }
        }
        // A POST that reads the rows its body names answers the whole body
        // as one page, and takes no paging ("Lists").
        if op.method == "get" {
            let query = query_parameters(spec);
            for key in ["limit", "offset"] {
                if !query.contains(key) {
                    broken.push(format!("a list that does not take {key}"));
                }
            }
        }
    }
    broken
}

/// The rules one operation breaks when called. Each call puts the operation
/// in a condition the shared parts claim a failure for, and the status and
/// problem type the server answers must be ones the document lists for the
/// operation. Reading the document alone cannot show that, because the
/// document and any reading of it come from the same code in `shared_parts`.
async fn called_rules(doc: &Value, world: &World<'_>, op: &Operation, spec: &Value) -> Vec<String> {
    let mut broken = Vec::new();
    if !op.path.starts_with("/v1/") {
        return broken;
    }
    let path = world.path_for(op);
    let with = |query: &str| {
        let joiner = if path.contains('?') { '&' } else { '?' };
        format!("{path}{joiner}{query}")
    };
    let fixture_body = credential_matrix::body_for(op, 0);
    let sent = || {
        fixture_body
            .clone()
            .map(|(content_type, body)| (Some(content_type), body))
    };

    let answer = call(world, op, &with("no_such_parameter=1"), None, sent()).await;
    broken.extend(answer.problem_rule(
        op,
        spec,
        ProblemType::ValidationFailed,
        "an unknown parameter",
    ));

    if takes_a_credential_only(op) {
        let answer = call(world, op, &path, None, sent()).await;
        broken.extend(answer.problem_rule(
            op,
            spec,
            ProblemType::AuthenticationRequired,
            "no credential",
        ));
    }

    // Every call below carries a credential the operation admits, so the
    // guard lets it through to the body and the query.
    let token = op.admitted().map(|c| world.token(c).to_string());
    let token = token.as_deref();
    if !spec["requestBody"].is_null() {
        let Some((content_type, body)) = &fixture_body else {
            broken.push("takes a body, and credential_matrix::body_for has none for it".into());
            return broken;
        };
        // An asset's bytes are sent with the asset's own media type, so only
        // a missing one is wrong there. Elsewhere `text/plain` is wrong, and
        // so is a missing type, unless the body is optional: there a body
        // with no type is read as no body.
        let wrong_types = if *content_type == "application/octet-stream" {
            vec![None]
        } else if spec["requestBody"]["required"] == true {
            vec![None, Some("text/plain")]
        } else {
            vec![Some("text/plain")]
        };
        for wrong in wrong_types {
            let answer = call(world, op, &path, token, Some((wrong, body.clone()))).await;
            let asked = match wrong {
                None => "a body with no Content-Type".to_string(),
                Some(wrong) => format!("a body sent as {wrong}"),
            };
            broken.extend(answer.problem_rule(op, spec, ProblemType::UnsupportedMediaType, &asked));
        }
        // A JSON body, or an import's JSON Lines, that is not JSON.
        if ["application/json", "application/x-ndjson"].contains(content_type) {
            let not_json = (Some(*content_type), b"not json\n".to_vec());
            let answer = call(world, op, &path, token, Some(not_json)).await;
            broken.extend(answer.problem_rule(
                op,
                spec,
                ProblemType::MalformedBody,
                "a body that is not JSON",
            ));
        }
    }

    // An `Accept` that names nothing JSON. A route that answers JSON refuses
    // it with `406`, which its document must list; a route that answers
    // bytes ignores it, and its document must not claim a `406` it never
    // gives.
    let answer = call_with(world, op, &path, token, sent(), &[("accept", "text/html")]).await;
    if answer.status == StatusCode::NOT_ACCEPTABLE || !spec["responses"]["406"].is_null() {
        broken.extend(answer.problem_rule(
            op,
            spec,
            ProblemType::NotAcceptable,
            "Accept: text/html",
        ));
    }

    // A read that succeeds answers in a media type its document declares.
    if op.method == "get" {
        let answer = call(world, op, &path, token, None).await;
        if answer.status == StatusCode::OK {
            broken.extend(answer.declared_media_type_rule(spec));
        }
    }

    if op.method == "get" && !page_schemas(doc, spec).is_empty() {
        let mut out_of_range = vec!["limit=0", "limit=501"];
        // A browse list says its offset ceiling in the parameter's own
        // description, and must keep to it.
        if offset_description(spec).contains(&MAX_LIST_OFFSET.to_string()) {
            out_of_range.push("offset=50001");
        }
        for query in out_of_range {
            let answer = call(world, op, &with(query), token, None).await;
            broken.extend(answer.problem_rule(op, spec, ProblemType::ValidationFailed, query));
        }
    }

    // Last, because it may make, change or remove something: a success
    // answers every field its document declares, `null` when it has no value
    // ("Fields"), and a creation names the new resource in `Location`, where
    // the credential that made it can read it ("Status codes"). An operation
    // that answers no JSON and makes nothing is not called, so the owner's
    // `DELETE /v1/session` leaves the shared Session alone.
    let answers_json = success_schemas(spec).next().is_some();
    let creates = !spec["responses"]["201"].is_null() && token.is_some();
    if op.method == "head" || !(answers_json || creates) {
        return broken;
    }
    let answer = call(world, op, &path, token, sent()).await;
    if answer.status.is_success() && answer.content_type.starts_with("application/json") {
        let status = answer.status.as_u16().to_string();
        let schema = &spec["responses"][&status]["content"]["application/json"]["schema"];
        match serde_json::from_str::<Value>(&answer.text) {
            Err(e) => broken.push(format!(
                "a {status} answered JSON that does not parse ({e})"
            )),
            Ok(body) => {
                let mut missing = BTreeSet::new();
                left_out(doc, schema, &body, "answer", &mut missing);
                broken.extend(
                    missing
                        .into_iter()
                        .map(|field| format!("a {status} left out {field}, which it declares")),
                );
            }
        }
    }
    if creates && answer.status == StatusCode::CREATED {
        match &answer.location {
            None => broken.push("a 201 with no Location".to_string()),
            Some(location) => {
                let read = Operation {
                    method: "get".to_string(),
                    path: location.clone(),
                    security: None,
                };
                let followed = call(world, &read, location, token, None).await;
                if followed.status != StatusCode::OK {
                    broken.push(format!(
                        "the 201's Location {location} answered GET with {}: {}",
                        followed.status.as_u16(),
                        followed.text
                    ));
                }
            }
        }
    }
    broken
}

/// What the Demo Account was answered.
enum DemoAnswer {
    /// `demo-account-protected`, as the document lists it.
    RefusedAsDocumented,
    /// `demo-account-protected`, breaking the rule named.
    RefusedBreaking(String),
    /// Anything else.
    Other,
}

/// Call the operation as the Demo Account, whose row grants every
/// permission, so a refusal comes from its id alone (ADR 0016). Wherever the
/// server answers `demo-account-protected`, the document must list it. A
/// `HEAD` answer has no body to tell the problem type by, so it is skipped.
async fn demo_account_rule(world: &World<'_>, op: &Operation, spec: &Value) -> DemoAnswer {
    if !op.path.starts_with("/v1/") || op.method == "head" || !takes_a_credential_only(op) {
        return DemoAnswer::Other;
    }
    let path = world.path_for(op);
    let body =
        credential_matrix::body_for(op, 0).map(|(content_type, body)| (Some(content_type), body));
    let token = world.demo_session().await;
    let answer = call(world, op, &path, Some(&token), body).await;
    let refused = serde_json::from_str::<Problem>(&answer.text)
        .is_ok_and(|problem| problem.kind == ProblemType::DemoAccountProtected.url());
    if !refused {
        return DemoAnswer::Other;
    }
    match answer.problem_rule(
        op,
        spec,
        ProblemType::DemoAccountProtected,
        "the Demo Account",
    ) {
        Some(rule) => DemoAnswer::RefusedBreaking(rule),
        None => DemoAnswer::RefusedAsDocumented,
    }
}

/// A response, read whole.
struct Answer {
    status: StatusCode,
    content_type: String,
    location: Option<String>,
    text: String,
}

impl Answer {
    /// What is wrong with this answer as the problem `kind`, if anything:
    /// another status or type, a body that is not a problem document, or a
    /// status and type the operation's document does not list.
    fn problem_rule(
        &self,
        op: &Operation,
        spec: &Value,
        kind: ProblemType,
        asked: &str,
    ) -> Option<String> {
        if self.status != kind.status() {
            return Some(format!(
                "{asked} answered {}, not {} {}: {}",
                self.status.as_u16(),
                kind.status().as_u16(),
                kind.slug(),
                self.text
            ));
        }
        // A HEAD answer has no body to be a problem document.
        if op.method != "head" {
            if self.content_type != Problem::CONTENT_TYPE {
                return Some(format!("{asked} answered as {}", self.content_type));
            }
            match serde_json::from_str::<Problem>(&self.text) {
                Err(e) => {
                    return Some(format!(
                        "{asked} answered a body that is not a problem ({e})"
                    ));
                }
                Ok(problem) if problem.kind != kind.url() => {
                    return Some(format!("{asked} answered the type {}", problem.kind));
                }
                Ok(problem) if problem.request_id.is_none() => {
                    return Some(format!("{asked} answered a problem with no request_id"));
                }
                Ok(_) => {}
            }
        }
        let status = kind.status().as_u16().to_string();
        let response = &spec["responses"][&status];
        if response.is_null() {
            return Some(format!(
                "{asked} answered {status} {}, a status the document does not list",
                kind.slug()
            ));
        }
        let listed = response[PROBLEM_TYPES]
            .as_array()
            .is_some_and(|types| types.iter().any(|t| *t == kind.url()));
        if !listed {
            return Some(format!(
                "{asked} answered {status} {}, a type the document does not list under {status}",
                kind.slug()
            ));
        }
        None
    }

    /// What is wrong with a `200` answer's media type, if anything: one the
    /// operation's document does not declare for its `200`. A declared
    /// range (`*/*`, `image/*`) covers every type in it.
    fn declared_media_type_rule(&self, spec: &Value) -> Option<String> {
        let answered = self
            .content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let declared: Vec<&String> = spec["responses"]["200"]["content"]
            .as_object()
            .map(|content| content.keys().collect())
            .unwrap_or_default();
        let covered = declared.iter().any(|range| match range.split_once('/') {
            Some(("*", "*")) => true,
            Some((kind, "*")) => answered.split('/').next() == Some(kind),
            _ => **range == answered,
        });
        (!covered).then(|| {
            format!("a 200 answered as {answered}, and the document declares {declared:?}")
        })
    }
}

/// Call `op` at `path` with `token`, or no credential, sending `body` with
/// its `Content-Type`, or with none, or no body.
async fn call(
    world: &World<'_>,
    op: &Operation,
    path: &str,
    token: Option<&str>,
    body: Option<(Option<&str>, Vec<u8>)>,
) -> Answer {
    call_with(world, op, path, token, body, &[]).await
}

/// [`call`], with `headers` added to the request.
async fn call_with(
    world: &World<'_>,
    op: &Operation,
    path: &str,
    token: Option<&str>,
    body: Option<(Option<&str>, Vec<u8>)>,
    headers: &[(&str, &str)],
) -> Answer {
    let method = reqwest::Method::from_bytes(op.method.to_uppercase().as_bytes()).unwrap();
    let mut request = reqwest::Client::new().request(method, world.url(path));
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some((content_type, body)) = body {
        if let Some(content_type) = content_type {
            request = request.header(reqwest::header::CONTENT_TYPE, content_type);
        }
        request = request.body(body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let text = response.text().await.unwrap_or_default();
    Answer {
        status,
        content_type,
        location,
        text,
    }
}

/// Every field's description sits on the field, where openapi-typescript
/// reads it into the web app's generated types (`field_descriptions`). An
/// optional field typed by a schema, a `oneOf` of one `$ref` and `null`, must
/// carry no description inside the `$ref`'s branch. A choice between schemas
/// keeps a description on each branch, and is not such a field. Walks the
/// whole document, since a schema no operation answers is still generated.
#[test]
fn every_field_description_sits_on_the_field() {
    let doc: Value = serde_json::from_str(&dump_openapi_json()).unwrap();
    let mut broken = BTreeSet::new();
    for_each_property(&doc, "#", &mut |at, schema| {
        if description_inside_branch(schema) {
            broken.insert(at.to_string());
        }
    });
    assert!(
        broken.is_empty(),
        "these fields keep their description inside a `oneOf` branch: {broken:#?}"
    );
}

/// Every field in the reference has a description, written from the doc
/// comment on its Rust field, so the reference and the web app's generated
/// types say what each one holds. Walks the whole document, as the rule
/// above does.
#[test]
fn every_field_has_a_description() {
    let doc: Value = serde_json::from_str(&dump_openapi_json()).unwrap();
    let mut undescribed = BTreeSet::new();
    for_each_property(&doc, "#", &mut |at, schema| {
        if schema.get("description").is_none() {
            undescribed.insert(at.to_string());
        }
    });
    assert!(
        undescribed.is_empty(),
        "these fields have no description; give each Rust field a doc comment: {undescribed:#?}"
    );
}

/// Call `f` with the JSON pointer and the schema of each property of every
/// schema under `value`. A `properties` map is read as fields only where a
/// schema holds it: the walk goes into each field's schema, never into the
/// map as if it were a schema, so a field named `properties` is one field.
/// An `example` or `examples` value is data, not a schema, and is skipped.
fn for_each_property(value: &Value, at: &str, f: &mut impl FnMut(&str, &Value)) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let at = format!("{at}/{key}");
                match key.as_str() {
                    "example" | "examples" => {}
                    "properties" => {
                        for (field, schema) in child.as_object().into_iter().flatten() {
                            let at = format!("{at}/{field}");
                            f(&at, schema);
                            for_each_property(schema, &at, f);
                        }
                    }
                    _ => for_each_property(child, &at, f),
                }
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                for_each_property(child, &format!("{at}/{i}"), f);
            }
        }
        _ => {}
    }
}

/// Whether `field` is a `oneOf` of one `$ref` and `null`, with no description
/// of its own, whose `$ref` branch carries a description: what
/// `field_descriptions` lifts.
fn description_inside_branch(field: &Value) -> bool {
    let branches = field["oneOf"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let (refs, others): (Vec<&Value>, Vec<&Value>) = branches
        .iter()
        .partition(|branch| branch.get("$ref").is_some());
    refs.len() == 1
        && !others.is_empty()
        && others.iter().all(|branch| branch["type"] == "null")
        && field.get("description").is_none()
        && refs[0].get("description").is_some()
}

/// A field named `properties` is one field, and an example holding a
/// `properties` object is data: neither is read as a list of fields.
#[test]
fn a_field_named_properties_and_an_example_are_not_fields_lists() {
    let doc = serde_json::json!({ "components": { "schemas": { "S": {
        "type": "object",
        "example": { "properties": { "x": {} } },
        "properties": {
            "properties": { "type": "string", "format": "uri", "description": "One field." }
        }
    } } } });
    let mut seen = Vec::new();
    for_each_property(&doc, "#", &mut |at, _| seen.push(at.to_string()));
    assert_eq!(seen, ["#/components/schemas/S/properties/properties"]);
}

/// Add to `out` each property of `schema` that it does not require, as
/// `at.field`, looking into the objects it holds but not into a schema it
/// names, which is checked on its own.
fn optional_fields(schema: &Value, at: &str, out: &mut BTreeSet<String>) {
    let required: BTreeSet<&str> = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    for (field, field_schema) in schema["properties"].as_object().into_iter().flatten() {
        let at = format!("{at}.{field}");
        if !required.contains(field.as_str()) {
            out.insert(at.clone());
        }
        optional_fields(field_schema, &at, out);
    }
    for key in ["items", "additionalProperties"] {
        if schema[key].is_object() {
            optional_fields(&schema[key], at, out);
        }
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        for branch in schema[key].as_array().into_iter().flatten() {
            optional_fields(branch, at, out);
        }
    }
}

/// The schema `schema` names, followed through every `$ref`.
fn resolved<'d>(doc: &'d Value, schema: &'d Value) -> &'d Value {
    match schema_named(schema) {
        Some(name) => resolved(doc, &doc["components"]["schemas"][name]),
        None => schema,
    }
}

/// Add to `out` each property `schema` declares that `value` does not carry,
/// as `answer.field.field`, looking into every object `value` holds. Of a
/// choice (`oneOf`, `anyOf`), the branches `value` fits are tried, and a
/// value that carries every property of one of them leaves nothing out. A
/// branch fits only a value of its own shape ([`fits`]), so a page of
/// `ExportRun` is never read as a page of `OwnerExportRun`.
fn left_out(doc: &Value, schema: &Value, value: &Value, at: &str, out: &mut BTreeSet<String>) {
    let schema = resolved(doc, schema);
    if value.is_null() {
        return;
    }
    for branch in schema["allOf"].as_array().into_iter().flatten() {
        left_out(doc, branch, value, at, out);
    }
    for key in ["oneOf", "anyOf"] {
        let Some(branches) = schema[key].as_array() else {
            continue;
        };
        let tried: Vec<BTreeSet<String>> = branches
            .iter()
            .map(|branch| resolved(doc, branch))
            .filter(|branch| fits(doc, branch, value))
            .map(|branch| {
                let mut missing = BTreeSet::new();
                left_out(doc, branch, value, at, &mut missing);
                missing
            })
            .collect();
        match tried.into_iter().min_by_key(BTreeSet::len) {
            Some(fewest) => {
                out.extend(fewest);
            }
            None => {
                out.insert(format!(
                    "{at}, which has the shape of no branch of its {key}"
                ));
            }
        }
    }
    let properties = schema["properties"].as_object();
    if let (Some(properties), Some(object)) = (properties, value.as_object()) {
        for (field, field_schema) in properties {
            let at = format!("{at}.{field}");
            match object.get(field) {
                None => {
                    out.insert(at);
                }
                Some(field_value) => left_out(doc, field_schema, field_value, &at, out),
            }
        }
    }
    if schema["additionalProperties"].is_object()
        && let Some(object) = value.as_object()
    {
        for (key, entry) in object {
            if properties.is_none_or(|p| !p.contains_key(key)) {
                let at = format!("{at}.{key}");
                left_out(doc, &schema["additionalProperties"], entry, &at, out);
            }
        }
    }
    if let Some(items) = value.as_array() {
        let at = format!("{at}[]");
        for item in items {
            left_out(doc, &schema["items"], item, &at, out);
        }
    }
}

/// Whether `value` has the shape of `schema`, which is how the branches of a
/// choice are told apart: its JSON type is one the schema allows, it holds one
/// of the values each `enum` property allows, it carries no key the schema
/// does not declare (unless it takes any), and each object or list it holds
/// has the shape of its own schema in turn. Missing keys do not count against
/// it: those are what [`left_out`] reports.
fn fits(doc: &Value, schema: &Value, value: &Value) -> bool {
    let schema = resolved(doc, schema);
    if value.is_null() {
        return true;
    }
    let is = |kind: &str| match kind {
        "null" => value.is_null(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        _ => true,
    };
    let type_fits = match &schema["type"] {
        Value::String(kind) => is(kind),
        Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).any(is),
        _ => true,
    };
    if !type_fits {
        return false;
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(branches) = schema[key].as_array()
            && !branches.iter().any(|branch| fits(doc, branch, value))
        {
            return false;
        }
    }
    if let Some(items) = value.as_array() {
        return !schema["items"].is_object()
            || items.iter().all(|item| fits(doc, &schema["items"], item));
    }
    let Some(object) = value.as_object() else {
        return true;
    };
    let mut properties = serde_json::Map::new();
    declared_properties(doc, schema, &mut properties);
    if properties.is_empty() {
        return true;
    }
    object.iter().all(|(key, held)| match properties.get(key) {
        None => takes_any_key(doc, schema),
        Some(field_schema) => {
            let tag_fits = resolved(doc, field_schema)["enum"]
                .as_array()
                .is_none_or(|allowed| allowed.contains(held));
            tag_fits && fits(doc, field_schema, held)
        }
    })
}

/// The properties `schema` declares, with those of each `allOf` part.
fn declared_properties(doc: &Value, schema: &Value, out: &mut serde_json::Map<String, Value>) {
    let schema = resolved(doc, schema);
    for (field, field_schema) in schema["properties"].as_object().into_iter().flatten() {
        out.insert(field.clone(), field_schema.clone());
    }
    for part in schema["allOf"].as_array().into_iter().flatten() {
        declared_properties(doc, part, out);
    }
}

/// Whether `schema`, or an `allOf` part of it, takes keys it does not name.
fn takes_any_key(doc: &Value, schema: &Value) -> bool {
    let schema = resolved(doc, schema);
    !schema["additionalProperties"].is_null() && schema["additionalProperties"] != false
        || schema["allOf"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|part| takes_any_key(doc, part))
}

/// Whether the operation takes a credential and admits no request without
/// one. `POST /v1/accounts` admits a stranger, so it has no such refusal.
fn takes_a_credential_only(op: &Operation) -> bool {
    op.security.as_ref().is_some_and(|requirements| {
        !requirements.is_empty()
            && requirements
                .iter()
                .all(|r| r.as_object().is_some_and(|r| !r.is_empty()))
    })
}

/// Lowercase words of letters and digits joined by single hyphens.
fn is_kebab(segment: &str) -> bool {
    !segment.is_empty()
        && segment.split('-').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// The page schemas a `200` answers: the page it names, or each page of a
/// choice between pages, as an account's history answers the account in
/// full and the owner without content. Empty when it answers no page.
fn page_schemas<'d>(doc: &'d Value, spec: &Value) -> Vec<&'d Value> {
    let schemas = &doc["components"]["schemas"];
    let Some(name) =
        schema_named(&spec["responses"]["200"]["content"]["application/json"]["schema"])
    else {
        return Vec::new();
    };
    if name.starts_with("Page_") {
        return vec![&schemas[name]];
    }
    let choices: Vec<&str> = schemas[name]["oneOf"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(schema_named)
        .collect();
    if !choices.is_empty() && choices.iter().all(|c| c.starts_with("Page_")) {
        choices.into_iter().map(|c| &schemas[c]).collect()
    } else {
        Vec::new()
    }
}

/// What the operation says about its `offset`.
fn offset_description(spec: &Value) -> &str {
    spec["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["in"] == "query" && p["name"] == "offset")
        .and_then(|p| p["description"].as_str())
        .unwrap_or_default()
}

/// The names of the query parameters an operation declares.
fn query_parameters(spec: &Value) -> BTreeSet<&str> {
    spec["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["in"] == "query")
        .filter_map(|p| p["name"].as_str())
        .collect()
}

/// The schema of each success answer in JSON the operation declares.
fn success_schemas(op: &Value) -> impl Iterator<Item = &Value> {
    op["responses"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(status, _)| status.starts_with('2'))
        .flat_map(|(_, response)| response["content"].as_object().into_iter().flatten())
        .filter(|(media_type, _)| *media_type == "application/json")
        .map(|(_, content)| &content["schema"])
}
