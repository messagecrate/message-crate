//! The router and the state its handlers share: [`AppState`], [`Created`],
//! `http_app`, which assembles every route, and `with_server_layers`, which
//! puts them under the layers in [`crate::middleware`].
//!
//! Domain handlers live in their own modules: `session_api` (logging in and
//! out), `accounts_api` (the accounts collection and their API tokens),
//! `contacts_api`, `conversations_api`, `exports_api` (Export Runs),
//! `imports_api` (JSONL ingest and Import Runs), and `assets_api` (asset bytes
//! and multipart uploads). What they share lives beside this module:
//! [`crate::api_error`] (the failure every route answers with),
//! [`crate::auth`] (the credential and its guards), [`crate::request_body`]
//! (reading a body), [`crate::limits`] (the body caps) and [`crate::startup`]
//! (starting and stopping the server).

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, FromRef, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Serialize;
use sqlx::SqlitePool;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::api_error::ApiError;
use crate::asset_uploads;
use crate::assets_api::media_links::MediaLinkKey;
use crate::config::Config;
use crate::credentials::RefusalWait;
use crate::keyed_locks::KeyedLocks;
use crate::limits::{MAX_AUTH_BODY_BYTES, MAX_JSON_BODY_BYTES};
use crate::media_queue::MediaQueue;
use crate::middleware::{
    BodyFailures, CONTENT_SECURITY_POLICY, answer_panic, build_cors_layer,
    json_body_limit_response, limit_request_body, log_body_failure, log_response, logged_uri,
    mark_routine, no_store_unless_set, require_json_acceptable,
};
use crate::open_db::OpenDb;
use crate::request_body::BodyTimeLimits;

/// The router's state. A handler never takes it whole: it takes
/// `State<SqlitePool>`, `State<Arc<Config>>`, or one of the sub-states the
/// [`FromRef`] implementations below cut from it, so a handler that reads
/// only the database does not depend on the Demo Account build or the media
/// queue (`docs/architecture/rules/http-api.md`, API-133).
#[derive(Debug, Clone)]
pub struct AppState {
    /// Loaded configuration.
    pub cfg: Arc<Config>,
    /// Connection pool for the database file. Handlers acquire
    /// short-lived connections from here.
    pub db: SqlitePool,
    /// What an import holds while it runs: the per-account lock, the slots,
    /// and the stop signal.
    pub(crate) imports: ImportState,
    /// What an attachment upload holds: the per-file lock and the part size.
    pub(crate) assets: AssetState,
    /// How long a request body may go with no byte arriving, and may take in
    /// all, from `[server] body_stall_secs` and `body_total_secs`.
    /// `GET /v1/server` states them, so the Upload sizes its own wait from
    /// what the server holds.
    pub(crate) body_time_limits: BodyTimeLimits,
    /// The wait before the answer to a refused login or a wrong current
    /// password. `serve` sleeps; the test harness swaps in one that counts
    /// each wait and returns at once.
    pub(crate) refusal_wait: RefusalWait,
    /// The Demo Account build and the bundle it imports.
    pub(crate) demo: DemoState,
    /// The key media links are signed with, made when the process starts and
    /// never written anywhere, so a restart ends every media link
    /// (`docs/architecture/rules/http-api.md`, API-75).
    pub(crate) media_link_key: MediaLinkKey,
    /// Wakes the background pass that makes Thumbnails and Previews after
    /// each Import Run. `serve` starts the pass; the test harness does not,
    /// so a test sees the queue as an Import Run leaves it.
    pub(crate) media_queue: MediaQueue,
    /// The ffmpeg and ffprobe the server runs, from [`Config::media_tools`]:
    /// on `PATH`, then in the Tools Directory the desktop app names, as the
    /// app looks (#1053). A test swaps in a fake.
    pub(crate) media_tools: Arc<dyn media::MediaTools>,
    /// The requests being served, so a stop that gives up waiting for them
    /// names each one in the log (`serve_until_shutdown` in
    /// [`crate::startup`]).
    pub(crate) in_flight: crate::in_flight::InFlight,
    /// The move of a Session's expiry running in the background, if any
    /// ([`crate::auth::ExpiryMoves`]).
    pub(crate) expiry_moves: crate::auth::ExpiryMoves,
    /// The error SQLite gave when the server asked for write-ahead logging
    /// as it opened the database.
    pub(crate) wal_error: WalError,
}

/// What an import holds while it runs.
#[derive(Debug, Clone)]
pub(crate) struct ImportState {
    /// Per-account import mutex: same-account imports stay serialized so staging
    /// rows (the temporary import area) for that tenant are not wiped mid-run.
    /// Different accounts may overlap at the lock layer; SQLite write-ahead
    /// logging plus `busy_timeout` serialize writers.
    pub(crate) account_locks: KeyedLocks,
    /// The slots HTTP imports take, from [`crate::imports_api::run_control::import_slots`],
    /// so imports can never drain the connection pool. Held here, not in a
    /// static, so tests in one binary cannot wait on each other's imports.
    pub(crate) slots: Arc<tokio::sync::Semaphore>,
    /// Canceled the moment the server is told to stop
    /// ([`AppState::on_stop_signal`]), so an import batch still reading its
    /// body stops reading and is refused with `server-stopping` at once,
    /// rather than making the stop wait for the rest of the body (#2878).
    pub(crate) stopping: tokio_util::sync::CancellationToken,
    /// Read by each batch once it holds its account's import lock and before
    /// it reads its file, so a test that holds it for writing pauses a batch
    /// at that point rather than racing it.
    #[cfg(test)]
    pub(crate) after_account_lock: Arc<tokio::sync::RwLock<()>>,
}

impl Default for ImportState {
    fn default() -> Self {
        Self {
            account_locks: KeyedLocks::default(),
            slots: crate::imports_api::run_control::import_slots(),
            stopping: tokio_util::sync::CancellationToken::new(),
            #[cfg(test)]
            after_account_lock: Arc::default(),
        }
    }
}

/// What an attachment upload holds.
#[derive(Debug, Clone)]
pub(crate) struct AssetState {
    /// Serialize multipart complete per (account, sha256) so two clients cannot
    /// race `store_verified` on the same SHA-256 fingerprint.
    pub(crate) complete_locks: KeyedLocks,
    /// The largest multipart part, from `[server] asset_part_size`. The part
    /// size a client is told is this or the attachment size limit, whichever
    /// is smaller ([`AssetState::upload_limits`]). The limit is not held here:
    /// it is a Server Setting, read from the database when a request needs it
    /// ([`asset_uploads::asset_max_bytes`]).
    pub(crate) part_size: usize,
}

impl AssetState {
    /// The upload limits in force at this moment: the attachment size limit,
    /// and the part size the server uses and tells a client, which is the
    /// configured one or the limit, whichever is smaller. Worked out on each
    /// call, so a changed limit moves the part size with no restart.
    pub(crate) async fn upload_limits(
        &self,
        db: &SqlitePool,
    ) -> anyhow::Result<asset_uploads::UploadLimits> {
        Ok(asset_uploads::UploadLimits::within(
            self.part_size,
            asset_uploads::asset_max_bytes(db).await?,
        ))
    }
}

/// The Demo Account build the owner started, and what writes its bundle.
#[derive(Debug, Clone)]
pub(crate) struct DemoState {
    /// The build, if one is running or the last one failed. One per server:
    /// a second cannot start while one runs.
    pub(crate) build: crate::server_api::DemoBuild,
    /// Writes the demo bundle a build imports. A test swaps in one that
    /// writes a few conversations.
    pub(crate) bundle_generator: crate::reset_demo::BundleGenerator,
}

impl Default for DemoState {
    fn default() -> Self {
        Self {
            build: crate::server_api::DemoBuild::default(),
            bundle_generator: crate::reset_demo::generate_bundle,
        }
    }
}

/// What SQLite answered when the server asked for write-ahead logging as it
/// opened the database ([`OpenDb::wal_error`]), which
/// `GET /v1/server/settings` reports to the owner: the error SQLite gave, or
/// `None` when it gave none, because it turned write-ahead logging on or kept
/// another mode without an error.
#[derive(Debug, Clone, Default)]
pub(crate) struct WalError(pub(crate) Option<Arc<str>>);

/// Cut one sub-state out of [`AppState`] for a handler's `State<…>`.
macro_rules! sub_state {
    ($($ty:ty => $field:ident),* $(,)?) => {
        $(
            impl FromRef<AppState> for $ty {
                fn from_ref(state: &AppState) -> Self {
                    state.$field.clone()
                }
            }
        )*
    };
}

sub_state!(
    Arc<Config> => cfg,
    SqlitePool => db,
    ImportState => imports,
    AssetState => assets,
    BodyTimeLimits => body_time_limits,
    RefusalWait => refusal_wait,
    DemoState => demo,
    MediaLinkKey => media_link_key,
    WalError => wal_error,
    crate::auth::ExpiryMoves => expiry_moves,
);

impl AppState {
    /// The state every handler shares, over an opened database. `serve` and the
    /// test harness both come through here, so the locks and the slots are
    /// assembled in one place.
    pub fn new(opened: OpenDb, asset_part_size: usize, body_time_limits: BodyTimeLimits) -> Self {
        let media_tools: Arc<dyn media::MediaTools> = Arc::new(opened.cfg.media_tools());
        Self {
            media_tools,
            cfg: Arc::new(opened.cfg),
            db: opened.db,
            imports: ImportState::default(),
            assets: AssetState {
                complete_locks: KeyedLocks::default(),
                part_size: asset_part_size,
            },
            body_time_limits,
            refusal_wait: RefusalWait::default(),
            demo: DemoState::default(),
            media_link_key: MediaLinkKey::random(),
            media_queue: MediaQueue::default(),
            in_flight: crate::in_flight::InFlight::default(),
            expiry_moves: crate::auth::ExpiryMoves::default(),
            wal_error: WalError(opened.wal_error),
        }
    }

    /// What the server does the moment it is told to stop, before it drains
    /// the requests in flight (the `on_signal` of `serve_until_shutdown` in
    /// [`crate::startup`]).
    ///
    /// It stops the conversions: Ctrl-C in a terminal reaches the ffmpeg
    /// they run as well, and a conversion not yet told to stop would read
    /// that ffmpeg's exit as a failure, take the Asset off the queue, and
    /// start the next one (#1729). It closes the import slots, so an import
    /// batch still waiting for one is refused with `server-stopping`, which
    /// the Upload retries, rather than taking a slot when an earlier batch
    /// ends and making the stop wait for a whole batch more (#2203). It
    /// cancels `stopping`, so a batch that holds a slot and is still reading
    /// its body stops reading and is refused the same way at once, rather
    /// than making the stop wait for the rest of the body (#2878). A batch
    /// whose import has started is not refused, and the stop waits for it
    /// up to [`crate::startup::STOP_DRAIN_LIMIT`] (#2122).
    pub(crate) fn on_stop_signal(&self) -> impl FnOnce() + Send + 'static {
        let media_queue = self.media_queue.clone();
        let demo_build = self.demo.build.clone();
        let import_slots = Arc::clone(&self.imports.slots);
        let stopping = self.imports.stopping.clone();
        move || {
            media_queue.ask_to_stop();
            demo_build.stop_conversions();
            import_slots.close();
            stopping.cancel();
        }
    }
}

/// The answer to a request that made one new resource: `201 Created`, a
/// `Location` header naming it, and the JSON body the route documents.
///
/// A create that takes a batch is the exception `docs/architecture/rules/http-api.md` records: it makes no
/// single resource, names no URL, and answers `200 OK` with a summary.
#[derive(Debug)]
pub struct Created<T> {
    /// The new resource's path, `/v1/...`.
    pub location: String,
    pub body: T,
}

impl<T: Serialize> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        (
            StatusCode::CREATED,
            [(header::LOCATION, self.location)],
            Json(self.body),
        )
            .into_response()
    }
}

/// The routes a stranger may call, with a small body limit so password
/// hashing cannot be fed huge requests. `POST /v1/accounts` is among them:
/// the owner's creation shares the route, and a small body is all it needs.
fn limited_auth_router() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    let (router, spec) = crate::openapi::public_openapi().split_for_parts();
    (
        router.layer(RequestBodyLimitLayer::new(MAX_AUTH_BODY_BYTES)),
        spec,
    )
}

/// A `/v1/…` path no route claims. Static files answer everything else.
async fn api_not_found(uri: axum::http::Uri) -> ApiError {
    ApiError::NotFound(format!("no route at {}", uri.path()))
}

/// A route that exists, asked with a method it does not take.
async fn api_method_not_allowed(method: axum::http::Method, uri: axum::http::Uri) -> ApiError {
    ApiError::MethodNotAllowed(format!("{method} is not allowed at {}", uri.path()))
}

/// Assemble the full router: API routes, auth routes, the optional OpenAPI UI, CORS, and the static web app.
pub(crate) fn http_app(state: AppState) -> Router {
    let openapi_ui = state.cfg.server.as_ref().is_some_and(|s| s.openapi_ui);
    let static_dir = state
        .cfg
        .server
        .as_ref()
        .map_or_else(|| "static".into(), |s| s.static_dir.clone());
    let (auth_small, mut spec) = limited_auth_router();
    let (health_router, health) = crate::openapi::health_openapi().split_for_parts();
    spec.merge(health);
    let (doc_router, rest) = crate::openapi::api_openapi().split_for_parts();
    spec.merge(rest);
    crate::openapi::finish(&mut spec);
    let declared_queries =
        std::sync::Arc::new(crate::declared_query::DeclaredQueries::from_spec(&spec));

    let mut api = Router::new()
        .merge(doc_router)
        .merge(auth_small)
        // `/v1/{*rest}` needs at least one character after the slash, so the
        // bare prefix (with or without a trailing slash) needs its own
        // routes to answer the same JSON 404 instead of falling through to
        // the static file server.
        .route("/v1", axum::routing::any(api_not_found))
        .route("/v1/", axum::routing::any(api_not_found))
        .route("/v1/{*rest}", axum::routing::any(api_not_found))
        // `route_layer`, not `layer`: the `Accept` check belongs to the API
        // routes above and never to the static app served by the fallback.
        .route_layer(axum::middleware::from_fn(require_json_acceptable))
        // After the `Accept` check and before the query check: `/health` is
        // outside `/v1`, so a probe sending `Accept: text/plain` gets the
        // plain text it answers, and its query is still checked like every
        // other route's.
        .merge(health_router.route_layer(axum::middleware::map_response(mark_routine)))
        // After routing, so the matched path names the operation whose
        // declared parameters the query is checked against.
        .route_layer(axum::middleware::from_fn_with_state(
            declared_queries,
            crate::declared_query::refuse_undeclared_query,
        ));
    // After both route layers, so the Swagger UI answers a browser's
    // `Accept: text/html` and is not held to a declared query, and before
    // every layer below, so its responses carry the request id, the CORS
    // headers and the log line every other response does.
    if openapi_ui {
        api = api.merge(utoipa_swagger_ui::SwaggerUi::new("/docs").url("/openapi.json", spec));
    }
    let routes = api
        .method_not_allowed_fallback(api_method_not_allowed)
        .fallback_service(
            Router::new()
                .fallback_service(ServeDir::new(static_dir))
                .layer(axum::middleware::map_response(mark_routine)),
        );
    with_server_layers(routes, state)
}

/// `routes` under the layers every answer of the server passes through: the
/// panic catch, the body limits, CORS, `Cache-Control`, the request's log
/// line and its id. `http_app` puts the server's routes under them; a test
/// puts a route of its own there to see how the server answers it.
pub(crate) fn with_server_layers(routes: Router<AppState>, state: AppState) -> Router {
    let cors_origins = state
        .cfg
        .server
        .as_ref()
        .map(|s| s.cors_origins.clone())
        .unwrap_or_default();
    routes
        // Innermost, so a handler that panics is answered like any other
        // internal failure: every layer below then treats the `500` as it
        // treats one, with the CORS headers a browser needs to show it, the
        // request id in its body and on its header, and its log line (#2060).
        .layer(CatchPanicLayer::custom(answer_panic))
        // The cap `extract::Json` reads a body against. The routes that read
        // a body of their own hold it to their own figure instead.
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            limit_request_body,
        ))
        // Rewrite the auth routes' limit layer's plain-text 413 into a problem
        // before CORS sees it, so the response a browser gets is both JSON and
        // CORS-clean.
        .layer(axum::middleware::map_response(json_body_limit_response))
        // Outside the limit layer: every response, including one the limit
        // layer answered itself, carries the CORS headers a browser needs to
        // show it.
        .layer(build_cors_layer(&cors_origins))
        // Outside every layer that answers a `/v1` request itself (the body
        // limits, the `Accept` and query checks, the 405 fallback, and CORS
        // answering a preflight), so their answers are marked as well as the
        // handlers'.
        .layer(axum::middleware::from_fn(no_store_unless_set))
        // Every answer, the website's files and `/v1` alike, so the policy
        // holds whichever response a browser renders (#2073). A header a
        // handler set itself is kept.
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CONTENT_SECURITY_POLICY),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        // Inside the request id layer, so the id it set is on the request
        // this records. Every request is tracked until its answer's body is
        // sent, so a stop that gives up waiting names it (#2122).
        .layer(axum::middleware::from_fn_with_state(
            state.in_flight.clone(),
            crate::in_flight::track,
        ))
        // One line per response (method, path, status, latency), at `debug`
        // for a successful `Routine` answer and at `info` for every other
        // (`log_response`). Nothing more for a 5xx, because
        // `ApiError::to_problem` writes its one `error` line with the
        // error's chain. A body that fails after its status was sent writes
        // an `error` line of its own (`BodyFailures`, `log_body_failure`).
        // A refused request's problem type is logged where the problem is
        // built (`log_refusal`). Runs outside CORS so the status it logs is
        // the one the client receives. The span carries method and path; its
        // level must match the line's or the default `info` filter drops it.
        .layer(
            TraceLayer::new(tower_http::classify::SharedClassifier::new(BodyFailures))
                .make_span_with(|request: &axum::extract::Request| {
                    tracing::info_span!(
                        "request",
                        method = %request.method(),
                        uri = %logged_uri(request.uri()),
                        request_id = request
                            .headers()
                            .get(crate::request_id::HEADER)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                    )
                })
                .on_response(log_response)
                .on_failure(log_body_failure),
        )
        // Outermost of all: the request id is made before the trace span
        // reads it and stays in scope while every problem body is built.
        .layer(axum::middleware::from_fn(crate::request_id::layer))
        .with_state(state)
}

/// How long `/health` waits for a pool connection and its `SELECT 1`. It
/// stays under the time `message-crate-server healthcheck` (`healthcheck.rs`)
/// gives each address it asks, and `SELECT 1` reads no table, so it never
/// waits on a writer's lock. Why a pool that writers fill reads as down, and
/// why that is accepted:
/// `docs/architecture/rules/http-api.md`, API-57.
const HEALTH_DATABASE_LIMIT: std::time::Duration = std::time::Duration::from_millis(1500);

/// Report whether the server can answer.
///
/// Answers `ok` when the server can take a database connection and run a
/// statement on it within 1.5 seconds. A server whose connections are all
/// taken, or whose database cannot be opened, fails the way a `/v1` call it
/// cannot answer fails, with `500 Internal Server Error`, so a health check
/// agrees with those calls.
#[utoipa::path(
    get,
    path = "/health",
    tag = "Health",
    responses((status = 200, description = "The server is up and can reach its database", body = String))
)]
pub(crate) async fn get_health(
    State(db): State<SqlitePool>,
) -> Result<(StatusCode, &'static str), ApiError> {
    let probe = sqlx::query("SELECT 1").execute(&db);
    match tokio::time::timeout(HEALTH_DATABASE_LIMIT, probe).await {
        Ok(Ok(_)) => Ok((StatusCode::OK, "ok\n")),
        Ok(Err(e)) => Err(ApiError::Internal(
            anyhow::Error::new(e).context("run the health check's statement on the database"),
        )),
        Err(_) => Err(ApiError::Internal(anyhow::anyhow!(
            "the health check could not take a database connection and run SELECT 1 within {} ms",
            HEALTH_DATABASE_LIMIT.as_millis()
        ))),
    }
}

/// Build the `AppState` every test in this crate drives: a real `Config`
/// rooted at `data_dir` (with a sibling `messagecrate.db` path that nothing in the
/// test suite reads from disk, since queries go through `pool`), the given pool,
/// and the default part size. Goes through [`AppState::new`], the same
/// assembly `serve` uses. `#[cfg(test)]`-gated so it never ships in a release
/// build; `pub(crate)` so `test_support` and the other test modules in this
/// crate can reach it.
#[cfg(test)]
pub(crate) fn test_app_state(pool: SqlitePool, data_dir: &std::path::Path) -> AppState {
    let cfg = crate::config::Config {
        paths: crate::config::PathsConfig {
            db: data_dir.join("messagecrate.db"),
            data_dir: data_dir.to_path_buf(),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: Some(crate::config::ServerConfig {
            bind: "127.0.0.1:0".into(),
            asset_part_size: 1024 * 1024,
            cors_origins: Vec::new(),
            openapi_ui: false,
            static_dir: "static".into(),
            ..crate::config::ServerConfig::default()
        }),
    };
    let mut state = AppState::new(
        OpenDb {
            cfg,
            db: pool,
            wal_error: None,
        },
        asset_uploads::DEFAULT_PART_SIZE,
        BodyTimeLimits::of(&crate::config::ServerConfig::default()),
    );
    // A refused login counts its wait rather than sleeping it, so no test
    // sleeps and a test can read how many refusals waited.
    state.refusal_wait = crate::credentials::RefusalWait::counted();
    // The tools on `PATH`, the ones `media::testutil::real_ffmpeg` answers,
    // so a test that skips without them runs the programs it checked for.
    state.media_tools = Arc::new(media::testutil::path_ffmpeg());
    state
}

#[cfg(test)]
mod tests;
