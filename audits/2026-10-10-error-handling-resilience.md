# Error handling and resilience audit

- Audit: error-handling-resilience
- Date: 2026-10-10
- Commit: `530dc1791`
- Scope: the whole repository except `web-next/`, `vendor/`, `target/`, `node_modules/` and `docs/dist`
- Method: read-only. Every claim cites a `path:line` read during the audit. Nothing was built or run.

The sibling audit `exception-flow-analysis` (same day) covers propagation paths, swallowed errors and the `unwrap`/`expect` surface. This report covers the design: the error types, how a failure reaches a person, retries, timeouts, partial failure, recovery, idempotency and cleanup. The design audit `audits/2026-10-09-initial-software-design-analysis.md` is cited by finding id (F2, F6, F11) rather than repeated.

## 1. Summary

The server's error design is strong and consistent: one error type, one mapping, a registry of problem types with generated pages, a request id on every answer, and wrappers that turn every Axum rejection into the same shape. The pipeline's write paths (journal, spool, asset store, multipart uploads) are atomic and swept, and a retried import batch is idempotent. The gaps are at the edges of that design: what happens when code panics inside a request, when the database is busy rather than broken, when one attachment of an export fails, when a long-running child process hangs, and when the web app itself throws.

| Id | Importance | Area | Finding |
|---|---|---|---|
| E1 | 8 | server | A handler panic answers nothing and leaves no line in the server's log |
| E2 | 7 | server | Contention (busy database, exhausted pool, shutdown) answers `500`, not `503` |
| E3 | 7 | export | A failed Export Run is recorded as `cancelled`; `ExportStatus::Failed` is never written |
| E4 | 7 | export | One Asset that cannot be fetched fails the whole Export Run after every other Asset was fetched |
| E5 | 6 | web | No error boundary: a render throw or a failed lazy chunk is a blank page |
| E6 | 6 | server, media | ffmpeg has no wall-clock limit; one hung conversion stops every Thumbnail and Preview until restart |
| E7 | 6 | desktop | The Apple Messages Reader is read with no timeout and Cancel cannot interrupt the read |
| E8 | 5 | pipeline | `with_retries` sleeps through a Cancel and retries unrecognised local errors |
| E9 | 5 | desktop | `ExtractErrorEvent.user_message` is set in one place; the window shows the raw error chain |
| E10 | 4 | server | Two `ERROR` lines per `500`; a `4xx` leaves only an `INFO` line with no `detail` |
| E11 | 4 | desktop | Tauri commands return `Result<_, String>` (82 sites); the window has no kind to branch on |
| E12 | 4 | web | A browser-level `fetch` failure shows the browser's own text ("Failed to fetch", "Load failed") |
| E13 | 3 | web | TanStack Query retries a `4xx` once |
| E14 | 3 | server, media | A Thumbnail or Preview that could not be made is a log line and nothing else |
| E15 | 2 | server | `say!` and `eprintln!` bypass the log files for startup and seed messages |

Counts by severity: 1 high (8 to 10), 8 medium (5 to 7), 6 low (1 to 4).

## 2. The five areas

### 2.1 Consistency

**Server.** One error type, `ApiError` (`crates/server/server/src/server.rs:498-600`), one mapping in `impl IntoResponse` (`server.rs:774`) through `to_problem` (`server.rs:645`), one registry of problem types (`crates/server/server/src/problem.rs`), and a test that fails when the generated error pages drift (`docs/architecture/http-api.md`, "Failures"). Axum's `Query`, `Path` and `Json` rejections are wrapped so that every one answers as a problem document (`crates/server/server/src/extract.rs:25-131`). The plain-text `413` that `RequestBodyLimitLayer` answers on its own is rewritten into the `payload-too-large` problem (`server.rs:982-1000`). Unknown `/v1` paths and wrong methods are problems too (`server.rs:1281-1283`, `server.rs:1301`). Where it is not consistent: `db/` returns `ApiError` (F2), and `From` impls are spread across 14 files (F2).

**Pipeline crates.** `anyhow` everywhere, with `thiserror` for the types a caller must match on: `Cancelled` (`crates/core/message-crate-core/src/process.rs:97-100`), `HttpError` (`crates/libs/http/src/retry.rs:28-34`) and `AuthError` (`crates/libs/http/src/auth_error.rs:21-136`). That is the right split: the typed errors are exactly the ones `classify_retry` and `is_session_refused` downcast (`retry.rs:63-118`). Exporters record non-fatal item failures through the shared counters in `crates/core/message-crate-core/src/counter.rs` (`ItemKind` at `:194-205`, 17 call sites in `crates/exporters/`), so one bad attachment is a run summary line rather than a failed run.

**Desktop app.** Commands return `Result<_, String>` at 82 sites (`grep -rn "Result<.*, String>" src-tauri/src`), and a job's failure travels as `ExtractErrorEvent { detail, user_message }` (`src-tauri/src/commands/events.rs:228-234`). See E9 and E11.

**Web app.** One `ApiError` class built from the problem document (`web/src/lib/api.ts:87-159`), one `endsSession` rule for a `401` that is not `invalid-credentials` (`web/src/lib/routeQuery.ts:54-56`), one query client that logs the person out on it (`routeQuery.ts:68-93`, `web/src/lib/auth.tsx:152-159`). See E5, E12, E13.

### 2.2 Categories

The registry covers `400`, `401`, `403`, `404`, `405`, `406`, `409`, `413`, `415`, `416`, `422`, `429` and `500`, each as a named problem with a reason a client can act on (`server.rs:498-600`; `problem.rs`). Validation and malformed input are told apart on the right side of the line (parsed and broke a rule is `422`; could not be read is `400`), including inside an import batch (`MalformedImportLine` versus `InvalidImportLines`, `server.rs:514-530`, `:838-856`). `429` carries `Retry-After` (`server.rs:789-795`). The rate limiter is bounded by its window: buckets whose newest hit is outside it are forgotten on every call (`crates/server/server/src/credentials.rs:102-107`).

What is missing is `503 Service Unavailable`. Every condition that is the server's state rather than the server's bug (a busy database, an exhausted pool, a shutdown in progress) is answered as `500` with the fixed sentence "internal server error" and an `ERROR` line with the chain. See E2.

### 2.3 Async and background work

Done well:

- The Demo Account build runs in a task of its own and handles every way it can end: success, error, cancellation on shutdown, and a panic, each with the part-built account removed (`crates/server/server/src/server_api.rs:545-607`).
- The media pass runs on its own thread with its own runtime, so a shutdown is not held up by it; a failure of the pass is logged and the pass waits for the next wake (`crates/server/server/src/media_queue.rs:71-100`); one Asset's failure is counted and the pass goes on (`media_queue.rs:292-304`); missing tools are a logged degraded mode, with the Assets kept queued (`media_queue.rs:262-265`).
- Graceful shutdown on Ctrl-C, SIGTERM and parent exit, with ffmpeg told to stop before requests drain (`server.rs:1446-1512`).
- Desktop jobs catch a worker panic so the window never waits forever (`src-tauri/src/commands/jobs.rs:137-152`); the import worker thread's panic is turned into an error at `join` (`crates/libs/import/src/pipeline.rs:498-502`); `[profile.release] panic = "unwind"` is kept on purpose for exactly this (`Cargo.toml:80-82`).
- The desktop app's local server is a state machine that handles exit while starting, exit while ready, a deadline, and a spawn failure (`src-tauri/src/local_server.rs:332-470`).

Not done:

- A panic inside a request handler. There is no `CatchPanicLayer` (tower-http features are `limit`, `cors`, `fs`, `trace`: `crates/server/server/Cargo.toml:41`) and no panic hook (`grep set_hook` finds nothing outside tests; `crates/server/server/src/logging.rs:44-57` installs tracing only). See E1.
- `create_import_batch` runs the import inside `spawn_blocking` with `block_on` (`crates/server/server/src/imports_api/mod.rs:1748-1752`). A blocking task cannot be cancelled, so when the client gives up (its request timeout is 600 s, `crates/libs/import/src/http.rs:273`) the import keeps running and commits. That is safe only because a resent batch is idempotent (2.4), and that fact is not written beside the `spawn_blocking`.
- The web app has no `ErrorBoundary` and no `unhandledrejection` handler (`grep` over `web/src` for `ErrorBoundary`, `componentDidCatch`, `getDerivedStateFromError`, `unhandledrejection` finds nothing). See E5.

### 2.4 Recovery

**Retries.** One classifier and one loop, `classify_retry` and `with_retries` (`crates/libs/http/src/retry.rs:83-148`), used by the import (`crates/libs/import/src/run.rs:194-201`) and the export (`crates/libs/export/src/run.rs:34`, `:370`, `:402`, `:469`, `:851`). The classification is careful: a `2xx` whose body could not be read is permanent because the server did the work (`crates/libs/http/src/response.rs:41-45`, `:57-61`); a `401` mid-run stops the run rather than retrying it (`run.rs:203-213`, `http/auth_error.rs:16-18`); a `413` is caught before sending when the body is over the proxy limit (`import/http.rs:262-266`). Backoff is exponential with jitter, capped at 30 s (`retry.rs:141-144`). Gaps: E8.

**Idempotency of a retried `POST` batch.** Confirmed safe. Staging and promote share one `BEGIN IMMEDIATE` transaction, so a batch either lands whole or not at all (`imports_api/mod.rs:350-375`; `crates/server/server/src/db/write_tx.rs:57-61`). In append mode, promote skips rows production already holds by the guid join (`crates/server/server/src/imports_api/promote.rs:373-376`). A `replace` run wipes its source once, on the first batch, decided by whether the run already has messages (`imports_api/mod.rs:1804-1811`), so a resent first batch cannot wipe what the first attempt inserted. Batches of one account are serialised by a keyed lock (`imports_api/mod.rs:1798`; `crates/server/server/src/keyed_locks.rs`). The run's own state is checked again under the write lock, so a batch for a run discarded meanwhile takes no messages (`mod.rs:360-365`).

**Timeouts.** Every call in the two HTTP clients sets one: 15 s for `HEAD`, 60 s for run start and completion, 600 s for a batch `POST` and an Asset `PUT`, 300 s for an Asset `GET`, 120 s for export pages (`crates/libs/import/src/http.rs:146,202,273,315,348,391,440,473,490`; `crates/libs/export/src/http.rs:54,90,143,189`). The shared `build_client` sets none (`crates/libs/http/src/lib.rs:83-89`), so any future call that forgets `.timeout()` relies on reqwest's documented blocking default. The desktop app's probe of its own server has connect, answer and start deadlines (`src-tauri/src/local_server.rs:79-91`). Two child processes have no deadline at all: ffmpeg (E6) and the Apple Messages Reader (E7). The server has no request timeout layer (`grep TimeoutLayer` finds nothing), which is acceptable for a self-hosted server behind a proxy but worth stating.

**Partial failure.** Import: one conversation's failure is `PrepareOutcome::Failed(String)` and the run goes on (`crates/libs/import/src/prepare.rs:940-951`); a stopped run leaves the conversation for the next Upload (`Stopped`, `:947-950`); a failed batch marks only its conversations (`pipeline.rs:506-520`). Server: a batch with one bad line is refused whole with the line number (`server.rs:838-856`), and nothing of it lands (the transaction above). Export: one Asset fails the run (E4).

**Resumability.** The import journal is appended with `sync_data`, a torn last line is detected and overwritten, unreadable lines are reported and skipped, and compaction writes a temp file and renames it (`crates/libs/journal/src/lib.rs:63-91`, `:104-141`, `:165-181`). The journal is compacted only after a clean run, so a failed run can retry (`crates/libs/import/src/run.rs:350-353`). The window keeps the record of a paused run's earlier parts in the run directory (`web/src/screens/import/runRecord.ts:7-20`), and a running run found at login is offered for resume or discard (`web/src/screens/import/resumeDecision.ts:43`). The export journal records each Asset on disk and compacts at the end; a journal write failure is a progress line, never a failed run (`crates/libs/export/src/run.rs:566-585`, `:633-668`).

**Cleanup.** Spool payloads are written to a temp name and renamed (`crates/libs/staging/src/spool.rs:123`); a scratch directory is removed on `Drop` and swept at app start (`crates/core/message-crate-core/src/scratch.rs:64`, `:137-143`). Multipart upload sessions are swept by age on every `start_upload` (`crates/server/server/src/asset_uploads.rs:216`) and aborted by the client on a part failure (`crates/libs/import/src/http.rs:220-247`). The asset store deletes in two steps through a `.removing/` directory and retries at the next run's end (`crates/server/server/src/asset_store.rs:27-62`). The import batch's temp directory is a `TempDir`, dropped on every path (`imports_api/mod.rs:1730-1753`).

**Degraded modes.** WAL is best effort with a warning (`crates/server/server/src/db/engine.rs:63-70`). Missing ffmpeg or ffprobe leaves Assets queued with a log line (`media_queue.rs:262-265`); the desktop app reports each tool as found, missing, unavailable or downloading and the window polls while a download runs (`src-tauri/src/commands/tools.rs:32-115`; `web/src/lib/useToolsStatus.ts:26-38`). A failed Demo seed starts the server without the Demo Account and says how to add it (`crates/server/server/src/reset_demo.rs:305-310`).

### 2.5 Information

A `500` hides its cause from the client (`about:blank`, fixed sentence) and logs the whole chain once, with the request id in the span (`server.rs:645-665`; `server.rs:1322-1341`). Every problem body repeats the request id, and both clients print it after the server's sentence (`crates/libs/http/src/response.rs:29-37`; `web/src/lib/api.ts:96-105`). The log never holds credentials, message text, or identities; a test enforces it (`docs/architecture/server-log.md`, "What a line never holds"). The run log of the desktop app has the same line format and levels (`docs/architecture/import-run-logs.md`). Gaps: E9, E10, E12, E15.

## 3. Findings

### E1. A handler panic answers nothing and leaves no line in the server's log (8/10)

**Where.** `crates/server/server/Cargo.toml:41` (tower-http features `limit`, `cors`, `fs`, `trace`; no `catch-panic`); `crates/server/server/src/logging.rs:44-57` (`init` installs the tracing subscriber and nothing else); `crates/server/server/src/server.rs:1301-1341` (the layer stack: body limits, CORS, `TraceLayer`, request id; no `CatchPanicLayer`); `grep -rn "set_hook\|catch_unwind" crates/server/server/src` finds nothing outside tests.

**What.** The server has 68 `unwrap`/`expect` sites outside tests (design audit, section 3). A panic in a handler unwinds the connection's task. The client gets a closed connection rather than a problem document with a request id, and the panic text goes through Rust's default hook to stderr, which the server's log files do not read: they are fed by the tracing `file_layer` only (`logging.rs:55`). `docs/architecture/server-log.md` rejected stderr alone because "the owner of either had no way to read it"; a panic is the one failure that still goes there alone.

**Fix.**

```rust
// crates/server/server/src/logging.rs, at the end of `init`
std::panic::set_hook(Box::new(|info| {
    let location = info.location().map(|l| format!("{}:{}", l.file(), l.line()));
    tracing::error!(panic = %info, location = ?location, "The server panicked");
}));
```

```toml
# crates/server/server/Cargo.toml
tower-http = { version = "0.7.1", features = ["limit", "cors", "fs", "trace", "catch-panic"] }
```

```rust
// crates/server/server/src/server.rs, inside the TraceLayer so the span and request id are in scope
.layer(tower_http::catch_panic::CatchPanicLayer::custom(
    |payload: Box<dyn std::any::Any + Send>| {
        let text = payload
            .downcast_ref::<&str>().copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("no message");
        ApiError::Internal(anyhow::anyhow!("handler panicked: {text}")).into_response()
    },
))
```

A test in `crates/server/server/src/server/tests.rs` that routes a panicking handler and asserts a `500` problem body with a `request_id` proves both halves.

### E2. Contention answers `500`, not `503` (7/10)

**Where.** `crates/server/server/src/db/engine.rs:20` (`busy_timeout = 15000`), `:56` (`max_connections(4)`, sqlx's default acquire timeout); `crates/server/server/src/imports_api/mod.rs:1763` (`MAX_CONCURRENT_IMPORTS = 2`), `:1780-1783` (a closed semaphore is `ApiError::Internal("server is shutting down")`), `:1800-1802` (one pooled connection held for a whole batch); `crates/server/server/src/server.rs:889-897` (`From<sqlx::Error>` and `From<anyhow::Error>` both map to `Internal`).

**What.** With four connections, two of which an import can hold for minutes each, a third writer that waits past `busy_timeout`, a reader that waits past the pool's acquire timeout, and a request that arrives during shutdown all answer `500 Internal Server Error` with "internal server error" and an `ERROR` line carrying the chain. None of these is a bug; each is a state the client should wait out. The registry has no `503` type (`problem.rs`; `server.rs:498-600`). The clients retry a `5xx` as transient (`crates/libs/http/src/retry.rs:85-89`), so the behaviour is right by accident, but the owner's log fills with `ERROR` lines for an ordinary busy day, and a browser shows "internal server error" for a request that would succeed a second later.

**Fix.** Add one problem type and map the three conditions to it.

```rust
// problem.rs
/// The server cannot take the request now: the database is busy, every
/// connection is in use, or the server is stopping. Retry after `Retry-After`.
Unavailable,   // status 503, slug "unavailable"

// server.rs, ApiError
/// `503`: the server is busy or stopping; `Retry-After` in seconds.
Unavailable { retry_after_secs: u64 },

// server.rs, replacing `impl From<sqlx::Error> for ApiError`
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::PoolTimedOut => Self::Unavailable { retry_after_secs: 5 },
            sqlx::Error::Database(db) if db.code().as_deref() == Some("5") /* SQLITE_BUSY */ => {
                Self::Unavailable { retry_after_secs: 5 }
            }
            _ => Self::Internal(e.into()),
        }
    }
}
```

Log `Unavailable` at `WARN` without a chain, and set `Retry-After` the way `RateLimited` does (`server.rs:789-795`). The semaphore-closed case at `imports_api/mod.rs:1783` becomes `Unavailable { retry_after_secs: 30 }`.

### E3. A failed Export Run is recorded as `cancelled` (7/10)

**Where.** `crates/libs/api-types/src/lib.rs:333-342` (`ExportStatus::Failed`, "Ended without finishing"); `crates/server/server/src/db/exports.rs:567` (the only status write besides completion is `'cancelled'`; `grep "'failed'"` over `db/exports.rs` and `exports_api.rs` finds nothing); `crates/libs/export/src/http.rs:102-105` (`CloseAction::{Complete, Cancel}`); `crates/libs/export/src/run.rs:200-216` (any error from `export_into_directory` closes the run with `Cancel`).

**What.** `docs/architecture/http-api.md`, "Runs": "a cancelled run marked completed afterwards would lie." A run that failed because an Asset could not be fetched (E4), a file could not be written, or the server answered `500` after retries is recorded as "Closed by the person before it finished". The Audit Trail shows it as cancelled (`crates/server/server/src/db/audit_trail.rs:259`). The import side has the status right: a client reports `failed` through `complete` (`imports_api/mod.rs:796`).

**Fix.** Give the export the same three outcomes the import has, and remove the dead variant or make it live.

```rust
// exports_api.rs: one route, one body, as the import's `complete` takes
#[derive(Deserialize)]
pub(crate) struct CloseExportBody { pub status: ExportStatus /* completed | failed | cancelled */, pub detail: Option<String> }

// crates/libs/export/src/http.rs
pub(crate) enum CloseAction { Complete, Fail { detail: String }, Cancel }

// crates/libs/export/src/run.rs:204-208
let action = match &outcome {
    Ok(_) => CloseAction::Complete,
    Err(e) if e.is::<Cancelled>() => CloseAction::Cancel,
    Err(e) => CloseAction::Fail { detail: format!("{e:#}") },
};
```

### E4. One Asset that cannot be fetched fails the whole Export Run (7/10)

**Where.** `crates/libs/export/src/run.rs:851-868` (`fetch_assets_parallel` collects every worker's result, then `bail!`s on the first `Err` and drops `stats`); `crates/libs/export/src/http.rs:197-204` (a `404` is a permanent `HttpError`); `crates/server/server/src/imports_api/mod.rs:377` (`assets_missing` is a counted, expected import outcome, so a referenced Asset can be absent on the server).

**What.** Every Asset is fetched, in parallel, with retries, and then one `404` throws the run away: the files already fetched are on disk and journaled (`run.rs:566-585`), the conversation files are never written (`export_into_directory` returns before `write_conversations`, `run.rs:420-434`), and the run is recorded as cancelled (E3). The import treats the same class of failure as one row in the summary and carries on (`crates/libs/import/src/prepare.rs:940-951`). The two sides of the same pipeline disagree on what one bad attachment means.

**Fix.** Make an Asset failure a counted outcome, and `bail!` only on cancel.

```rust
// run.rs, AssetCounts
struct AssetCounts { fetched: u64, kept: u64, failed: Vec<(String, String)> /* (sha256, why) */ }

// run.rs:857-866
for ((sha256, _), result) in assets.iter().zip(results) {
    match result {
        Ok(bytes) => { stats.bytes += bytes; stats.fetched += 1; }
        Err(why) if why == Cancelled.to_string() => bail!(Cancelled),
        Err(why) => { stats.failed.push((sha256.clone(), why)); }
    }
}
```

Write the conversation files, list the failed Assets in the progress log and `ExportReport`, and record the run as `completed` with issues, or `failed` when nothing was fetched. A failed Asset must not be journaled as `AssetOk` (`run.rs:566-575` already iterates `assets.keys()`; iterate `to_fetch` minus `failed`).

### E5. The web app has no error boundary (6/10)

**Where.** `web/src/main.tsx:13-21` (the tree is rendered with no boundary); `web/src/App.tsx:23-28` (four screens are `lazy()`), `:84`, `:108` (`Suspense fallback={null}`); `grep` over `web/src` for `ErrorBoundary`, `componentDidCatch`, `getDerivedStateFromError`, `unhandledrejection` finds nothing.

**What.** React unmounts the whole tree when a render throws and nothing catches it. Two cases are not hypothetical: a thrown render error in any screen, and a lazy chunk that fails to load, which happens when a release replaces `static/` under an open tab (a `v*` tag ships the server with new chunk hashes, `CLAUDE.md`, "Pushing a `v*` tag ships a release"). Both end as a blank page with no message, no request id, and no way back but the address bar.

**Fix.**

```tsx
// web/src/components/RouteErrorBoundary.tsx
import { Component, type ReactNode } from "react";

type State = { error: Error | null };

export class RouteErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };
  static getDerivedStateFromError(error: Error): State { return { error }; }
  componentDidCatch(error: Error) {
    // A chunk the server no longer has: a release replaced the app under this tab.
    if (/dynamically imported module|ChunkLoadError/.test(String(error))) window.location.reload();
  }
  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div role="alert" className="p-4">
        <p>Something went wrong in this screen.</p>
        <p className="text-muted">{this.state.error.message}</p>
        <button type="button" onClick={() => window.location.reload()}>Reload</button>
      </div>
    );
  }
}
```

Wrap `<App />` in `main.tsx` and each `<Suspense>` in `App.tsx:84,108` and the Import and Export routes with it.

### E6. ffmpeg has no wall-clock limit on the server (6/10)

**Where.** `crates/libs/media/src/tools.rs:416-468` (`run_ffmpeg_with`: with a `stop` flag it polls the flag, `:434-456`; without one it calls `child.wait()`, `:457`); `crates/server/server/src/media_queue.rs:71-100` (one thread, one Asset at a time), `:102-105` (`ask_to_stop` is called only at shutdown, `server.rs:1430-1437`); `media_queue.rs:267-305` (the loop takes the first queued Asset each time).

**What.** ffmpeg can stall: a malformed container, an encoder that waits on a pipe, a network file system that hangs. On the server there is no deadline and no person to cancel, so one stalled conversion blocks every Thumbnail and Preview for every account until the server restarts, and after the restart the same Asset is first in the queue again (`media_queue::first`, `:268`). The desktop app's Media stage at least has a Cancel (`run_ffmpeg_until`, `tools.rs:386-388`).

**Fix.** Give `run_ffmpeg_with` a deadline and let the pass derive one from what ffprobe already measured.

```rust
// tools.rs
pub(crate) fn run_ffmpeg_within(args: &[String], stop: &AtomicBool, deadline: Duration) -> Result<()> {
    let started = Instant::now();
    run_ffmpeg_poll(args, || stop.load(Ordering::Relaxed) || started.elapsed() > deadline)
}
// The pass: `max(Duration::from_secs(60), probe.duration * 10)`, and on expiry the
// error reads "ffmpeg did not finish within {deadline:?}" so `not_made` names it.
```

`run_ffmpeg_with` already kills and waits when `stopped()` turns true (`tools.rs:443-450`); the change is only what `stopped` reads.

### E7. The Apple Messages Reader is read with no timeout and Cancel cannot interrupt the read (6/10)

**Where.** `crates/libs/ios-backup/src/helper.rs:180-184` (`next_event` blocks on `self.stdout.next()`); `:322-328` (`Drop` kills the child, but only when the `Helper` is dropped); `src-tauri/src/commands/jobs.rs:104-112` (`cancel_running_job` sets a flag and nothing else).

**What.** The exporter checks the cancel flag between events, never inside the blocking read. A helper that hangs (a corrupt `chat.db`, a backup on a disk that went away, a deadlock inside the GPL library the process boundary exists to contain) leaves Staging running and Cancel doing nothing. The job ends only when the app exits, which kills the helper through `exit_with_parent` semantics for the server but not for the helper.

**Fix.** Read on a thread and wait with a timeout, so the loop can see the flag.

```rust
// helper.rs
struct Helper { /* ... */ events: std::sync::mpsc::Receiver<std::io::Result<String>>, child_id: u32 }

// in spawn_at: move `BufReader::new(stdout).lines()` onto a thread that `send`s each line.

pub fn next_event_until(&mut self, cancel: Option<&CancelFlag>) -> Result<Event> {
    loop {
        match self.events.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => { /* the existing match on `line?` */ }
            Err(RecvTimeoutError::Timeout) if is_cancelled(cancel) => {
                let _ = self.child.kill();
                bail!(Cancelled);
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Err(self.exited_early()),
        }
    }
}
```

### E8. `with_retries` sleeps through a Cancel and retries unrecognised local errors (5/10)

**Where.** `crates/libs/http/src/retry.rs:127-148` (no cancel parameter; `thread::sleep` up to 30 s per step, `:144`), `:117` (anything unrecognised is `Transient`); `crates/libs/import/src/run.rs:194-201` (`Session::request` wraps it; `note_refusal` runs only after the loop); `crates/core/message-crate-core/src/process.rs:150-153` (`parallel_for_each` checks cancel before a job starts, never during it); `src-tauri/src/commands/upload.rs:154` (`max_retries: 3`); `crates/libs/import/src/http.rs:273` (a batch `POST` waits up to 600 s).

**What.** After Cancel, every worker finishes its current attempt and, on a transient failure, up to three more, each up to 600 s: in the worst case the Upload stops long after the person pressed Cancel. The default of `Transient` for an error that is not `HttpError`, `AuthError`, `reqwest::Error` or `io::Error` means a local failure raised with `anyhow!` inside the retried closure (for example the digest check in `crates/libs/export/src/http.rs:216-222` is an `HttpError`, but a future `bail!` would not be) is retried three times with backoff for no gain.

**Fix.**

```rust
// retry.rs
pub fn with_retries_until<T>(max_retries: u32, cancel: Option<&CancelFlag>, mut op: impl FnMut() -> Result<T>) -> Result<T> {
    let mut attempt = 0u32;
    loop {
        check_cancel(cancel)?;
        attempt += 1;
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if e.is::<Cancelled>() || attempt > max_retries || classify_retry(&e) == RetryKind::Permanent => return Err(e),
            Err(_) => sleep_in_slices(backoff(attempt), cancel)?, // 100 ms slices, each checking the flag
        }
    }
}
```

Pass `cfg.cancel.as_ref()` from `Session::request` and the export's call sites. Consider making the unrecognised default `Permanent` and listing the transient kinds instead; the test `unrecognized_errors_are_transient` (`retry.rs:289-292`) documents the current choice, so change it deliberately.

### E9. `ExtractErrorEvent.user_message` is set in one place; the window shows the raw chain (5/10)

**Where.** `src-tauri/src/commands/events.rs:228-234` (the two fields), `:237-244` (`From<anyhow::Error>` sets `user_message: None`); `src-tauri/src/commands/jobs.rs:148-151` (the only `Some`, for a panic); `web/src/lib/tauri.ts:693` (`new Error(err.user_message ?? err.detail)`).

**What.** The design is right: a sentence for the person and the chain for the log. But every job failure except a panic shows the chain, so the person reads "stat /Users/…/Backup: No such file or directory (os error 2)" or "Import Run 7 batch failed (HTTP 500 Internal Server Error): internal server error (request id …)". The import run log has the chain already (`docs/architecture/import-run-logs.md`); the window should not.

**Fix.** Map the known families at the job boundary, where the `anyhow::Error` is still typed underneath.

```rust
// events.rs
impl From<anyhow::Error> for ExtractErrorEvent {
    fn from(err: anyhow::Error) -> Self {
        let user_message = if err.is::<Cancelled>() {
            Some("Cancelled.".into())
        } else if message_crate_http::is_session_refused(&err) {
            Some(message_crate_http::SESSION_REFUSED.into())
        } else if let Some(io) = err.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::StorageFull => Some("The disk is full.".into()),
                std::io::ErrorKind::PermissionDenied => Some("Message Crate may not read or write a file it needs.".into()),
                _ => None,
            }
        } else {
            None
        };
        Self { detail: format!("{err:#}"), user_message }
    }
}
```

### E10. Two `ERROR` lines per `500`; a `4xx` leaves only an `INFO` line with no `detail` (4/10)

**Where.** `crates/server/server/src/server.rs:651` (`to_problem` logs the chain at `ERROR`); `server.rs:1322-1337` (`TraceLayer::new_for_http()` with `on_response` at `INFO` and tower-http's default `on_failure`, which logs a `5xx` at `ERROR` as well); `docs/architecture/server-log.md`, "The owner reads it" (the Logs panel opens at warnings and up).

**What.** Every `500` is two `ERROR` lines, one with the chain and one without. Every `4xx` is one `INFO` line with method, path and status, so the owner reading at the default filter sees nothing for a client that is refused a thousand times with `422`, `401` or `429`, and the `detail` that would explain it is in no line at all.

**Fix.** Log once, in `into_response`, with the level set by the status, and turn off the trace layer's own failure line.

```rust
// server.rs, in `impl IntoResponse for ApiError`, before building the response
match problem.status {
    500.. => {}                                   // `to_problem` already logged the chain
    401 | 403 | 429 => tracing::warn!(status = problem.status, kind = %problem.kind, detail = ?problem.detail, "A request was refused"),
    _ => tracing::info!(status = problem.status, kind = %problem.kind, detail = ?problem.detail, "A request was refused"),
}
// and on the TraceLayer: `.on_failure(())`
```

### E11. Tauri commands return `Result<_, String>`; the window has no kind to branch on (4/10)

**Where.** 82 sites (`grep -rn "Result<.*, String>" src-tauri/src --include=*.rs | grep -v tests`); `crates/core/message-crate-core/src/process.rs:93-100` (`Cancelled` displays as `cancelled` so that "callers at a `String` edge can use `.map_err(|e| e.to_string())`"); F11.

**What.** A `String` is the whole contract between the desktop process and the window, so the window can tell a cancel from a failure, or a refused session from a network error, only by reading the text. The `ExtractErrorEvent` has the right two-field shape (E9); commands have none.

**Fix.** One serialised error for every command, built once.

```rust
// src-tauri/src/commands/error.rs
#[derive(serde::Serialize)]
pub struct CommandError { pub kind: CommandErrorKind, pub message: String, pub detail: String }

#[derive(serde::Serialize)] #[serde(rename_all = "snake_case")]
pub enum CommandErrorKind { Cancelled, SessionRefused, NotFound, PermissionDenied, DiskFull, ToolMissing, Other }

impl From<anyhow::Error> for CommandError { /* the same downcasts as E9 */ }
```

Then `#[tauri::command] fn …() -> Result<T, CommandError>`, and `tauri.ts` reads `kind`.

### E12. A browser-level `fetch` failure shows the browser's own text (4/10)

**Where.** `web/src/lib/api.ts:178-187`, `:209-212` (`fetch` is awaited with no `catch`; a network failure is a `TypeError`, not an `ApiError`); `web/src/lib/apiErrorMessage.ts:10-12` (returns `err.message`); `web/src/screens/LoginScreen.tsx:184-190` (only the login card probes reachability first).

**What.** Off the login screen, a server that went away shows "Failed to fetch" (Chromium) or "Load failed" (WebKit, which is the Tauri window on macOS), with no address and no request id. `endsSession` already ignores it (`routeQuery.ts:55` checks `instanceof ApiError`), so nothing else is wrong; only the sentence is.

**Fix.**

```ts
// api.ts, replacing the bare `await fetch(...)` in `request` and `requestRaw`
async function send(input: string, init: RequestInit): Promise<Response> {
  try {
    return await fetch(input, init);
  } catch (e) {
    if (e instanceof DOMException && e.name === "AbortError") throw e;
    throw new ApiError(0, `The server at ${baseUrl || window.location.origin} did not answer.`);
  }
}
```

### E13. TanStack Query retries a `4xx` once (3/10)

**Where.** `web/src/lib/routeQuery.ts:88` (`retry: (failureCount, error) => !endsSession(error) && failureCount < 1`).

**What.** A `404`, `403` or `422` is asked again after the default delay before the screen shows the error, for a second answer that will be the same.

**Fix.**

```ts
retry: (failureCount, error) =>
  failureCount < 1 && !(error instanceof ApiError && error.status > 0 && error.status < 500),
```

### E14. A Thumbnail or Preview that could not be made is a log line and nothing else (3/10)

**Where.** `crates/server/server/src/media_queue.rs:292-304` (the failure is counted, logged at `WARN`, and the Asset is removed from the queue); `crates/server/server/src/process_assets.rs:456-465` (`Outcome.not_made: Option<anyhow::Error>` is never stored); `:218-228` (`record_decision_if_known` is the one thing written back).

**What.** The account sees a missing preview with no reason, the owner finds the reason only by searching the log, and nothing retries until `process-assets` is run by hand. Not a correctness problem; a diagnosis one.

**Fix.** Record the reason on the version row beside the shown-as-is decision (`versions_db::record_shown_as_is`, `process_assets.rs:225`) as `not_made_reason TEXT NULL`, clear it when a version is made, and answer it on the attachment's `/v1` resource so the window can say "No preview: ffmpeg did not finish".

### E15. `say!` and `eprintln!` bypass the log files for startup and seed messages (2/10)

**Where.** `crates/server/server/src/server.rs:47-52` (`say!` writes to stderr directly); `server.rs:1366`, `:1372-1375`, `:1392-1397`, `:1411-1420`, `:1501-1504` (the log location, the database open line, the upload limits, the listening line, the SIGTERM warning); `crates/server/server/src/reset_demo.rs:292-310` (`eprintln!` for the seed's start, success and failure).

**What.** `write_files_in` opens the log before any of these run (`server.rs:1360-1365`) so that "a database that fails to open is in the server's log as well as on stderr", but the lines that say which database was opened, in which journal mode, on which address, and whether the Demo Account could be added, go to stderr only. In Docker and under the desktop app that is the stream nobody reads (`docs/architecture/server-log.md`, "Rejected: stderr alone").

**Fix.**

```rust
macro_rules! say {
    ($($arg:tt)*) => {{ tracing::info!($($arg)*); }};
}
```

and `tracing::warn!`/`tracing::error!` in place of the `eprintln!`s in `reset_demo.rs:292-310`. The subscriber already writes to stderr too (`logging.rs:48-54`), so nothing a terminal shows is lost.

## 4. What is done well

- One error type, one mapping, one registry, one `request_id` on every answer; a `500` hides its cause and logs it once (`server.rs:498-665`; `problem.rs`).
- Every Axum rejection, the body-limit layer's plain `413`, unknown `/v1` paths and wrong methods all answer the same shape (`extract.rs`; `server.rs:982-1000`, `:1281-1283`, `:1301`).
- `BEGIN IMMEDIATE` as the only transaction, with the schema check that fixes #1600 (`db/write_tx.rs`); one transaction per batch; a resent batch is idempotent by guid and `replace` wipes once (`promote.rs:373-376`; `imports_api/mod.rs:1804-1811`).
- Retry classification that knows a `2xx` with an unreadable body is not safe to repeat, and a `401` mid-run is a stop, not a retry (`http/response.rs:41-61`; `import/run.rs:194-213`).
- A journal with `sync_data`, torn-line recovery, atomic compaction, and compaction only after a clean run (`journal/lib.rs`; `import/run.rs:350-353`).
- Two-step deletes and age-based sweeps in the asset store and the upload sessions (`asset_store.rs:27-62`; `asset_uploads.rs:216`); atomic spool writes and a start-up scratch sweep (`spool.rs:123`; `scratch.rs:64`).
- Graceful shutdown that stops ffmpeg first and waits for the Demo build and the media pass (`server.rs:1430-1444`, `:1446-1512`).
- Background tasks that handle success, error, cancel and panic (`server_api.rs:545-607`); a media pass that isolates one Asset's failure and degrades when tools are missing (`media_queue.rs:262-304`).
- Desktop jobs that turn a panic into a failure the window can show, with the Upload paused rather than failed (`jobs.rs:137-165`); a local-server state machine with deadlines and a spawn-failure path (`local_server.rs:332-470`).
- A rate limiter whose memory is bounded by its window (`credentials.rs:102-107`).

## 5. Unable to verify

- **What the client sees on a handler panic (E1).** I read that no layer catches it and no hook logs it. Whether the connection is closed without a response or hyper answers an empty `500` depends on hyper's connection task, which is outside this scope. A test in `crates/server/server/src/server/tests.rs` that routes a panicking handler proves it.
- **reqwest's default timeout for a call that omits `.timeout()` (2.4).** `crates/libs/http/src/lib.rs:83-89` sets none. Every call in `crates/libs/import/src/http.rs` and `crates/libs/export/src/http.rs` sets its own, so today nothing relies on the default. The default is reqwest's documented 30 s for the blocking client; `vendor/` is out of scope, so it was not read.
- **Whether `main` writes a startup failure to the log files (E15).** `server::run` returns the error (`server.rs:1360-1365`); whether `crates/server/server/src/main.rs` prints it with `eprintln!` or `tracing::error!` was not read.
- **Whether the window matches `Cancelled`'s text (E11).** `process.rs:93-96` says a `String` edge relies on `to_string()`; which web code compares against `"cancelled"` was not traced. `grep -rn '"cancelled"' web/src/screens/import` would show it.

## 6. Suggested order of work

1. E1 and E15 together: a panic hook, `CatchPanicLayer`, and `say!` through tracing make the server's log complete in one pull request.
2. E2: one `503` problem type and the `From<sqlx::Error>` mapping, with the `WARN` level that goes with it.
3. E3 and E4 together: the export's failure semantics, since the status fix needs the partial-failure fix to have something to record.
4. E5: the error boundary, small and self-contained.
5. E6 and E7: deadlines for the two child processes.
6. E8, E9, E11: the cancel-aware retry loop and the typed desktop error, which share the downcast list.
7. E10, E12, E13, E14.
