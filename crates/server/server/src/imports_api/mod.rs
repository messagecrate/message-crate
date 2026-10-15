//! The `/v1/imports` routes: an Import Run, the batches posted into it, and
//! its end.
//!
//! The handlers are in this module. Its submodules hold the rest of the
//! import:
//!
//! - `dto` holds the routes' request and response types and the checks on
//!   their fields.
//! - `pipeline` imports JSONL into the database without HTTP.
//! - `staging` parses the files into staging rows, the pipeline's first
//!   stage.
//! - `contact_name` links handles to contacts and merges display names,
//!   which `staging` calls as it goes.
//! - `promote` copies the staging rows into the production tables, the
//!   pipeline's second stage.
//! - `records` maps a conversation file's lines to the rows staging writes.
//! - `with_yourself` runs the with-yourself rule again when an account's
//!   identities change.
//! - `failure` says what can stop an import, by kind.
//! - `run_control` holds the import slots, and runs a batch or a run's end
//!   in one.
//! - `batch_bodies` says where a batch's body is written while it is read.

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use message_crate_api_types::{ImportMode, MAX_IMPORT_BATCH_BYTES};
use sqlx::{SqliteConnection, SqlitePool};

use crate::api_error::{ApiError, run_blocking};
use crate::auth::{ImportAccess, resolve_import_account};
use crate::config::validate_source_id;
use crate::extract::{Json, Path as AxumPath, Query};
use crate::paging::{
    DEFAULT_LIST_LIMIT, Page, PageQuery, page_from_rows, page_params, parse_status, sorted_page,
};
use crate::request_body::{
    content_type_base, declared_over, is_jsonl_content_type, stream_body_to_file,
    upload_body_bounded,
};
use crate::server::Created;
use crate::text_caps::{
    MAX_DEVICE_ID_CHARS, MAX_IMPORT_TOOL_CHARS, MAX_PATH_CHARS, capped_path, capped_text,
    too_long_sentences,
};

pub(crate) mod batch_bodies;
pub mod contact_name;
pub(crate) mod dto;
pub mod failure;
pub(crate) mod pipeline;
pub mod promote;
pub mod records;
pub(crate) mod run_control;
pub mod staging;
pub mod with_yourself;

use crate::imports_api::run_control::Importer;
use dto::{
    CompleteImportRequest, CreateImportBatchResponse, CreateImportRequest, CreateImportResponse,
    DiscardImportRequest, ImportIssue, ImportNote, ImportRun, ImportRunSummary, ListImportsQuery,
    OwnerImportRun, OwnerImportRunSummary, UpdateImportRequest, capped_json_string, empty_as_none,
    import_status_refusal, issues_and_notes, optional_json_string, strip_form_credentials,
};
pub use failure::{ImportError, ImportFailure, MISSING_GUID_LINES_NAMED};
use run_control::{
    BatchContext, RunEnd, end_run, run_import_path, server_stopping, take_import_slot,
};

/// The owner's view of one Import Run, read from the run's row, its issue
/// count, its contact tally, and its attachments whose versions were not
/// made.
pub(crate) async fn owner_import_run(
    conn: &mut SqliteConnection,
    row: crate::db::imports::ImportRow,
) -> Result<OwnerImportRun, ApiError> {
    let (issue_count, note_count) = crate::db::imports::issue_and_note_counts(conn, row.id).await?;
    let not_made_attachments =
        crate::db::attachment_versions::not_made_for_import(conn, row.id).await?;
    let written_attachment_count =
        crate::db::attachment_versions::written_by_import(conn, row.id).await?;
    let run = listed_import(conn, row, issue_count, note_count)
        .await
        .map(OwnerImportRunSummary::from)?;
    Ok(OwnerImportRun {
        run,
        written_attachment_count,
        not_made_counts: crate::db::attachment_versions::count_not_made(&not_made_attachments),
        not_made_attachments,
    })
}

/// One run's row as the list would read it, given its issue and note
/// counts: the contact tally is read here.
async fn listed_import(
    conn: &mut SqliteConnection,
    row: crate::db::imports::ImportRow,
    issue_count: u64,
    note_count: u64,
) -> Result<crate::db::imports::ListedImport, ApiError> {
    let contacts = crate::db::import_contacts::counts(conn, row.id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(crate::db::imports::ListedImport {
        row,
        issue_count,
        note_count,
        contacts,
    })
}

/// The account's Import Runs, newest first, as a page. `status=running`
/// finds the one run the desktop app may resume.
#[utoipa::path(
    get,
    path = "/v1/imports",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(
        ("status" = Option<crate::db::imports::ImportStatus>, Query, description = "Only the runs with this status"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000"),
        ("sort" = Option<String>, Query, description = "`started_at` or `-started_at`. Default `-started_at`, newest first.")
    ),
    responses(
        (status = 200, body = Page<ImportRunSummary>),
    )
)]
pub(crate) async fn list_imports(
    State(db): State<SqlitePool>,
    ImportAccess(auth): ImportAccess,
    Query(query): Query<ListImportsQuery>,
) -> Result<Json<Page<ImportRunSummary>>, ApiError> {
    let mut conn = db.acquire().await?;
    let rows = import_rows_page(&mut conn, resolve_import_account(&auth), query).await?;
    Ok(Json(rows.map(ImportRunSummary::from)))
}

/// One account's Import Runs as a page of rows, each with its issue count
/// and contact tally, read in the list's own statements. `GET /v1/imports`
/// answers from it for the credential's account and
/// `GET /v1/accounts/{id}/imports` for the account named, so the two lists
/// cannot drift; each shapes the rows for whoever reads them.
pub(crate) async fn import_rows_page(
    conn: &mut SqliteConnection,
    account: i64,
    query: ListImportsQuery,
) -> Result<Page<crate::db::imports::ListedImport>, ApiError> {
    let (page, order) = sorted_page(
        query.limit,
        query.offset,
        query.sort.as_deref(),
        &crate::db::imports::IMPORT_SORT_KEYS,
        &crate::db::imports::DEFAULT_IMPORT_SORT,
    )?;
    let status = parse_status(
        query.status.as_deref(),
        &crate::db::imports::ImportStatus::ALL.map(crate::db::imports::ImportStatus::as_str),
    )?;

    let (items, total) = crate::db::imports::list_imports_page(
        conn,
        account,
        status,
        &order,
        page.limit as i64,
        i64::try_from(page.offset).map_err(anyhow::Error::from)?,
    )
    .await?;
    Ok(page_from_rows(items, total, page))
}

/// Status, timings, and issues for one Import Run.
#[utoipa::path(
    get,
    path = "/v1/imports/{id}",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(("id" = i64, Path, description = "Import Run id")),
    responses(
        (status = 200, body = ImportRun),
    )
)]
pub(crate) async fn get_import(
    State(db): State<SqlitePool>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
) -> Result<Json<ImportRun>, ApiError> {
    let mut conn = db.acquire().await?;
    full_import_run(&mut conn, auth.account_id, import_id)
        .await
        .map(Json)
}

/// One of an account's Import Runs in full. A run that is another account's
/// is a 404.
pub(crate) async fn full_import_run(
    conn: &mut SqliteConnection,
    account: i64,
    import_id: i64,
) -> Result<ImportRun, ApiError> {
    let row = crate::db::imports::get_owned_import(conn, account, import_id)
        .await
        .map_err(ApiError::from)?;
    import_run(conn, row).await
}

/// Start an Import Run and return its id. Finish the run at
/// POST /v1/imports/{id}/complete.
#[utoipa::path(
    post,
    path = "/v1/imports",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    request_body = CreateImportRequest,
    responses(
        (
            status = 201,
            body = CreateImportResponse,
            headers(("Location" = String, description = "Path of the new import"))
        ),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn create_import(
    State(db): State<SqlitePool>,
    ImportAccess(auth): ImportAccess,
    Json(body): Json<CreateImportRequest>,
) -> Result<Created<CreateImportResponse>, ApiError> {
    // Every rule the body breaks, in the order of its fields, so one
    // refusal names them all.
    let mut errors = Vec::new();
    if body.source.trim().is_empty() {
        // Blank answers as missing does.
        errors.push("source is required".to_string());
    } else if let Err(e) = validate_source_id(&body.source) {
        errors.push(e.to_string());
    }
    let phone_country = match body.phone_country.as_deref() {
        None => None,
        Some(code) => {
            let country = phone::country(code);
            if country.is_none() {
                errors.push(format!(
                    "phone_country {code:?} is not a country GET /v1/phone-countries lists"
                ));
            }
            country
        }
    };
    let tool = body
        .tool
        .as_deref()
        .map(|tool| capped_text("tool", tool, MAX_IMPORT_TOOL_CHARS))
        .transpose();
    let run_dir = body
        .run_dir
        .as_deref()
        .map(|dir| capped_path("run_dir", dir, MAX_PATH_CHARS))
        .transpose();
    let device_id = body
        .device_id
        .as_deref()
        .map(|id| capped_text("device_id", id, MAX_DEVICE_ID_CHARS))
        .transpose();
    errors.extend(too_long_sentences([&tool, &run_dir, &device_id]));
    let account = resolve_import_account(&auth);
    let stage = body.stage.unwrap_or(crate::db::imports::ImportStage::Parse);
    // Credentials never reach the row, whoever the client is.
    let form = body.form.as_ref().map(strip_form_credentials);
    let form_json = capped_json_string(form.as_ref(), "form", &mut errors)?;
    let fingerprint_json = capped_json_string(
        body.source_fingerprint.as_ref(),
        "source_fingerprint",
        &mut errors,
    )?;
    let identities_json = capped_json_string(
        body.source_identities.as_ref(),
        "source_identities",
        &mut errors,
    )?;
    let (Ok(tool), Ok(run_dir), Ok(device_id)) = (tool, run_dir, device_id) else {
        return Err(ApiError::ValidationFailed(errors));
    };
    ApiError::unless_broken(errors)?;
    let (tool, run_dir, device_id) = (
        empty_as_none(tool),
        empty_as_none(run_dir),
        empty_as_none(device_id),
    );

    let mut conn = db.acquire().await?;
    let args = crate::db::imports::StartImportArgs {
        account_id: account,
        source: &body.source,
        mode: body.mode.as_str(),
        phone_country: phone_country.map(|country| country.code),
        tool,
        stage,
        run_dir,
        device_id,
        form_json: form_json.as_deref(),
        source_fingerprint: fingerprint_json.as_deref(),
        source_identities: identities_json.as_deref(),
    };
    // The run and what started it land together, so a run never reads as
    // one the server started.
    let mut tx = crate::db::begin_write(&mut conn).await?;
    crate::auth::require_account_row(&mut tx, account).await?;
    let id = crate::db::imports::start_import(&mut tx, &args, chrono::Utc::now()).await?;
    crate::db::audit_trail::record_run_credential(
        &mut tx,
        crate::db::audit_trail::RunTable::Imports,
        id,
        &auth.credential,
    )
    .await?;
    tx.commit().await?;

    Ok(Created {
        location: format!("/v1/imports/{id}"),
        body: CreateImportResponse { id },
    })
}

/// Record the outcome of an Import Run started with POST /v1/imports. The
/// answer is the run as it now stands.
///
/// Before it records the outcome, the server hides the duplicates of the
/// whole account, within one source and across sources. A batch hides
/// none, so the duplicates a run brings stay shown until the run ends. The
/// time this takes grows with every message the account holds, not with
/// the run's. A dedupe that fails leaves the run running. A server told to
/// stop refuses the request with `503 Service Unavailable`
/// (`server-stopping`), and the run stays running.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/complete",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(("id" = i64, Path, description = "Import Run id")),
    request_body = CompleteImportRequest,
    responses(
        (status = 200, body = ImportRun),
        crate::problem::openapi::StateConflict,
        crate::problem::openapi::ServerStopping,
    )
)]
pub(crate) async fn complete_import(
    State(importer): State<Importer>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Json(body): Json<CompleteImportRequest>,
) -> Result<Json<ImportRun>, ApiError> {
    let account = resolve_import_account(&auth);
    // A status it does not take is named with every issue and note refused.
    let status_errors = import_status_refusal(&body.status).into_iter().collect();
    let (issues, notes) = issues_and_notes(&body.issues, &body.notes, status_errors)?;
    let summary_json =
        match body.summary {
            Some(summary) => Some(serde_json::to_string(&summary).map_err(|e| {
                ApiError::Internal(anyhow::anyhow!("serialize import summary: {e}"))
            })?),
            None => None,
        };
    let args = crate::db::imports::CompleteImportArgs {
        status: body.status,
        message_count: None,
        attachment_count: None,
        bytes_uploaded: body.bytes_uploaded,
        duration_ms: body.duration_ms,
        parse_ms: body.parse_ms,
        attachments_ms: body.attachments_ms,
        prepare_ms: body.prepare_ms,
        upload_ms: body.upload_ms,
        summary_json,
        issues,
        notes,
    };
    let now = chrono::Utc::now();
    let row = end_run(&importer, account, import_id, RunEnd::Complete(args), now).await?;
    let mut conn = importer.db.acquire().await?;
    import_run(&mut conn, row).await.map(Json)
}

/// List the contacts one import run created or changed, with the reason for
/// each, most consequential first.
///
/// The run recorded each reason as it staged (`db::import_contacts`), so the
/// list is what the import decided, not what timestamps suggest. How many of
/// each the run made is on the run's own record, `GET /v1/imports/{id}`: a
/// page can only count its own rows, and the panel states the whole run's
/// tally.
#[utoipa::path(
    get,
    path = "/v1/imports/{id}/contacts",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(
        ("id" = i64, Path, description = "Import Run id"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset")
    ),
    responses(
        (status = 200, body = Page<crate::db::import_contacts::ImportContact>),
    )
)]
pub(crate) async fn list_import_contacts(
    State(db): State<SqlitePool>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<crate::db::import_contacts::ImportContact>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let mut conn = db.acquire().await?;
    crate::db::imports::get_owned_import(&mut conn, auth.account_id, import_id)
        .await
        .map_err(ApiError::from)?;
    let (items, total) =
        crate::db::import_contacts::page(&mut conn, import_id, params.limit, params.offset)
            .await
            .map_err(ApiError::Internal)?;
    Ok(Json(page_from_rows(items, total, params)))
}

/// One Import Run in full, read from its row, its issues, its notes and its
/// contact tally. The caller has already established that the row is the account's.
pub(crate) async fn import_run(
    conn: &mut SqliteConnection,
    row: crate::db::imports::ImportRow,
) -> Result<ImportRun, ApiError> {
    let issues: Vec<ImportIssue> = crate::db::imports::list_import_issues(conn, row.id)
        .await
        .map_err(ApiError::Internal)?
        .into_iter()
        .map(|issue| ImportIssue {
            kind: issue.kind,
            stage: issue.stage,
            item: issue.item,
            reason: issue.reason,
        })
        .collect();
    let notes = crate::db::imports::list_import_notes(conn, row.id)
        .await
        .map_err(ApiError::Internal)?
        .into_iter()
        .map(|note| ImportNote {
            stage: note.stage,
            item: note.item,
            text: note.text,
        })
        .collect::<Vec<_>>();
    let run = listed_import(conn, row, issues.len() as u64, notes.len() as u64)
        .await?
        .into();
    Ok(ImportRun { run, issues, notes })
}

/// Move a running Import Run to another stage.
///
/// A run moves to the next stage, from the Staging Review straight to
/// Upload (the move made when the import has no Media Stage), or to the stage
/// it is already at. Any other move is a state conflict naming both stages,
/// and the run is left as it was.
///
/// The stage is a field of the run, so moving it is a `PATCH` of the run
/// rather than a `POST` to a `stage` sub-resource: a path segment names a
/// resource, and `stage` is not one (`docs/architecture/http-api.md`,
/// "Naming a route"). The answer is the run itself, the same shape
/// `GET /v1/imports/{id}` returns, so a caller reads one record wherever it
/// asks.
#[utoipa::path(
    patch,
    path = "/v1/imports/{id}",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(("id" = i64, Path, description = "Import Run id")),
    request_body = UpdateImportRequest,
    responses(
        (status = 200, body = ImportRun),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn update_import(
    State(db): State<SqlitePool>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Json(body): Json<UpdateImportRequest>,
) -> Result<Json<ImportRun>, ApiError> {
    let account = resolve_import_account(&auth);
    let stage = body.stage;
    let summary_json = optional_json_string(body.summary.as_ref(), "summary")?;
    let mut conn = db.acquire().await?;
    crate::db::imports::set_import_stage(
        &mut conn,
        account,
        import_id,
        stage,
        summary_json.as_deref(),
    )
    .await?;
    full_import_run(&mut conn, account, import_id)
        .await
        .map(Json)
}

/// Discard a running Import Run, freeing the account's single slot. The
/// answer is the run, now `cancelled`, with the issues the request carried.
///
/// The messages the run's batches brought stay in the account, and once it
/// has discarded the run the server hides the duplicates of the whole
/// account, as a completion does. A dedupe that fails does not undo the
/// discard: the next run's end hides them. A server told to stop refuses
/// the request with `503 Service Unavailable` (`server-stopping`), and the
/// run stays running.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/discard",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(("id" = i64, Path, description = "Import Run id")),
    request_body = DiscardImportRequest,
    responses(
        (status = 200, body = ImportRun),
        crate::problem::openapi::StateConflict,
        crate::problem::openapi::ServerStopping,
    )
)]
pub(crate) async fn discard_import(
    State(importer): State<Importer>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Json(body): Json<DiscardImportRequest>,
) -> Result<Json<ImportRun>, ApiError> {
    let account = resolve_import_account(&auth);
    let (issues, notes) = issues_and_notes(&body.issues, &body.notes, Vec::new())?;
    let now = chrono::Utc::now();
    let row = end_run(
        &importer,
        account,
        import_id,
        RunEnd::Discard { issues, notes },
        now,
    )
    .await?;
    let mut conn = importer.db.acquire().await?;
    import_run(&mut conn, row).await.map(Json)
}

/// Import one message-ir JSONL body.
///
/// Every message needs a non-empty `guid`. A batch with a message without
/// one is refused with `422`, naming its lines, and nothing in it is stored.
/// A message whose `guid` the source already holds is skipped, so a batch
/// sent again after its answer was lost stores nothing twice. A batch hides
/// no duplicates: the run's completion or discard hides them, once for the
/// whole run.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/batches",
    params(("id" = i64, Path, description = "Import Run id")),
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    request_body(
        content(
            ("application/x-ndjson"),
            ("application/jsonl")
        ),
        description = "message-ir JSONL as application/x-ndjson or application/jsonl. Attachments are uploaded first by SHA-256 through /v1/assets."
    ),
    responses(
        (status = 200, body = CreateImportBatchResponse),
        crate::problem::openapi::StateConflict,
        crate::problem::openapi::ServerStopping,
    )
)]
pub(crate) async fn create_import_batch(
    State(importer): State<Importer>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<CreateImportBatchResponse>, ApiError> {
    let Some(ct) = content_type_base(&headers) else {
        return Err(ApiError::UnsupportedMediaType(
            "Content-Type required (application/x-ndjson or application/jsonl)".into(),
        ));
    };

    let account = resolve_import_account(&auth);
    // The run's row says what the batch imports under; a run that is not
    // running, or belongs to another account, refuses the batch here before
    // any of the body is read.
    let context = {
        let mut conn = importer.db.acquire().await?;
        let row = crate::db::imports::require_running_import(&mut conn, account, import_id).await?;
        BatchContext::from_row(&row)
    };

    if is_jsonl_content_type(ct) {
        // A body that says it is over the cap is refused before it waits
        // for a slot; one that does not say is cut off as it is read.
        if declared_over(&headers, MAX_IMPORT_BATCH_BYTES) {
            return Err(ApiError::PayloadTooLarge);
        }
        // The slot comes before the body, so a batch waiting behind the
        // running imports holds no disk (#2095). The read gives up once the
        // body stops arriving or takes too long (`upload-stalled`), so a
        // client gone part-way, or one that sends a byte now and then, never
        // holds the slot for good.
        let permit = take_import_slot(&importer.imports).await?;
        let temp = batch_bodies::new_dir(&importer.cfg.paths.data_dir)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("import batch directory: {e}")))?;
        let jsonl_path = temp.path().join("_import.jsonl");
        let body = upload_body_bounded(request.into_body(), importer.upload_time_limits);
        // A server told to stop while the body arrives stops reading it at
        // once and refuses the batch, as it refuses one waiting for a slot,
        // so the stop never waits for the rest of a body (#2878). Dropping
        // the read leaves only the part-written file, which goes with
        // `temp`.
        let n = tokio::select! {
            biased;
            () = importer.imports.stopping.cancelled() => return Err(server_stopping()),
            read = stream_body_to_file(body, &jsonl_path, MAX_IMPORT_BATCH_BYTES) => read?,
        };
        if n == 0 {
            return Err(ImportFailure::Empty.into());
        }
        // A server told to stop as the body's last byte arrived refuses the
        // batch before its import starts, so the stop never waits for a
        // whole import more.
        if importer.imports.stopping.is_cancelled() {
            return Err(server_stopping());
        }
        // The import pipeline does blocking file IO (JSONL parse, asset
        // hashing and copies) — run it off the async workers so a large
        // import cannot stall unrelated requests.
        let handle = tokio::runtime::Handle::current();
        let response = run_blocking("import", move || {
            handle.block_on(run_import_path(importer, permit, context, jsonl_path))
        })
        .await;
        drop(temp);
        return response;
    }

    // Only JSON Lines is an import body. Attachments never travel with it:
    // they are uploaded first, by SHA-256, through `/v1/assets`. A body in
    // another type is the wrong media type, not a malformed request.
    Err(ApiError::UnsupportedMediaType(
        "Content-Type must be application/x-ndjson or application/jsonl".into(),
    ))
}

#[cfg(test)]
mod tests;
