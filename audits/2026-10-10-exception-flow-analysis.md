# Exception flow analysis

Date: 2026-10-10. Commit: `530dc1791`. Scope: whole repository less `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`. Read-only; no build or test was run.

Sibling audit: `error-handling-resilience` (same day) covers the design of error types, mapping, retries, timeouts and recovery. This report covers the flow: where an error starts, how it travels (`?`, `map_err`, `Context`, `From`), where it is swallowed, where it turns into a panic, where a panic lands, and whether context is lost or leaked on the way to a log line or a person. Timeouts are named only where they change where a flow ends.

Design-audit findings are cited by id from `audits/2026-10-09-initial-software-design-analysis.md` (F2, F6, F11, F15, F19) and not repeated.

## 1. Summary

The Rust side propagates well: every crate returns `anyhow::Error` or a `thiserror` enum, the server's `ApiError` (`crates/server/server/src/server.rs:498`) is the single mapping point, and the one `500` log line carries the whole chain under a span with the request id (`server.rs:651`, `server.rs:1323-1340`). The import path from a malformed JSONL line to the RFC 7807 answer with `line` is typed end to end and loses nothing it should keep (section 3.1).

The weak points are at the seams between processes and between Rust and TypeScript:

- A panic in an exporter thread is caught (`src-tauri/src/commands/jobs.rs:145`) but its message then goes nowhere a person can find it: not into the Import Run's log, and dropped by `web/src/lib/tauri.ts:693` because `user_message` wins over `detail` (3.3).
- Cancellation is a string contract: Rust `Cancelled` displays as `cancelled`, `ExtractErrorEvent.detail` is `{err:#}`, and the web app compares `msg === "cancelled"` (3.7, F19 confirmed).
- The server's media pass thread dies silently on a panic and is never restarted; the only log line comes at shutdown (3.5, 3.6).
- A panic inside an Axum handler produces no problem document and no line in the server's log file: there is no `CatchPanicLayer` and no panic hook that routes to `tracing` (3.6).
- `ApiError.requestId` is captured in the web client (`web/src/lib/api.ts:104`) and rendered nowhere, so the one thing that would let a person point an operator at the log line is lost (3.2).
- The web app has no `ErrorBoundary`, no `unhandledrejection` listener, and calls two Import actions as `void approve()` / `void cancelRun()` whose bodies are `try/finally` without `catch` (3.7).

Counts (section 4): the design audit's `.unwrap()/.expect()` figures stand as computed, but 49 of the server's 68 sites, 23 of core's 26, 155 of libs' 190 and all 29 of helpers' are in test-only modules or fixture crates. Production sites: server 19, src-tauri 1, core 3, libs 35, exporters 16, `imessage-reader` 0. None is a confirmed panic on user input; every one is listed with a verdict in 4.1.

Findings by importance: 7/10 x1, 6/10 x3, 5/10 x3, 4/10 x2, 3/10 x3, 2/10 x1.

## 2. How errors travel, by layer

| Layer | Error type out | Transformation | Final handler | Logged | Shown |
|---|---|---|---|---|---|
| `crates/libs/*`, `crates/exporters/*`, core | `anyhow::Error`, a few `thiserror` enums (`ReadRecordsError`, `Cancelled`, `HttpError`) | `?`, `.context()`, `.with_context()` | caller | no | no |
| server `db/` | `Result<_, ApiError>` or `sqlx::Error` (F2) | `From<sqlx::Error> for ApiError` (`server.rs:889`), no context added | handler `?` | at 500 only | problem body |
| server handlers | `ApiError` | `From<ImportError>`, `From<AssetError>`, `From<ImportLookupError>` (`server.rs:812-887`) | `IntoResponse` (`server.rs:774`) | `tracing::error!` for `Internal` (`server.rs:651`) | RFC 7807 with `request_id` |
| server background | `anyhow` | none | `tracing::warn!` x22 sites, `tracing::error!` x5 | yes | no |
| `imessage-reader` process | `Event::Error{message}` on stdout, `exit(1)` (`crates/helpers/imessage-reader/src/main.rs:49-54`) | parent `anyhow!(message)` (`crates/libs/ios-backup/src/helper.rs:193`) | exporter | run log | yes |
| Tauri job thread | `Result<String, ExtractErrorEvent>` | `From<anyhow::Error>` gives `detail = {err:#}` (`src-tauri/src/commands/events.rs:230-236`); panic gives `detail + user_message` (`jobs.rs:148-152`) | `events::emit(ERROR)` (`jobs.rs:128`) | `run_log.error` only at two `inspect_err` sites (`extract.rs:228,230`) | `user_message ?? detail` |
| Tauri plain command | `Result<_, String>` at 53 sites (F11) | 43 `format!("… {e}")`, 9 `{e:#}`, 7 `.to_string()` | Tauri rejects the invoke promise | `eprintln!` x3 in the whole crate | the string |
| web `apiClient` | `ApiError{status, message, problem, requestId}` (`web/src/lib/api.ts:87-104`) | `problemFromBody` (`api.ts:143`) | TanStack `QueryCache.onError` for 401 (`web/src/lib/routeQuery.ts:66-83`); otherwise `query.error` per screen | `console.error` x2 in the app | `message`; `requestId` never |
| web desktop calls | `Error(string)` | `tauri.ts:693`, `tauri.ts:697` | `useImportJob.ts:1678` catch | no | issue row `reason` |

The one place an internal error's wording is deliberately hidden from a client is `ApiError::Internal` (`server.rs:644-664`): the body says `internal server error` and `about:blank`, the log gets `error_chain(err)` (`server.rs:808`). This is correct and matches `docs/architecture/http-api.md` "Failures".

## 3. The traced flows

### 3.1 A malformed JSONL line in `POST /v1/imports/{id}/batches`

```
create_import_batch  crates/server/server/src/imports_api/mod.rs:1707
 ├ no/unknown Content-Type ─────────────► ApiError::UnsupportedMediaType (415)        mod.rs:1715, 1759
 ├ require_running_import ──ImportLookupError──► From (server.rs:812): NotFound 404 | InvalidRun 409 | Db 500
 ├ stream_body_to_file ?  ─────────────► PayloadTooLarge 413 | Internal 500            mod.rs:1735-1741
 ├ n == 0 ─────────────────────────────► ImportFailure::Empty.into() → validation 422 (no line)   mod.rs:1744
 └ spawn_blocking(run_import_path)                                                      mod.rs:1747
     JoinError (task panicked) ────────► Internal("import task failed: …") 500         mod.rs:1751
     run_import_path → import_jsonl_files_on_conn (mod.rs:314) → stage_all_files (mod.rs:448)
       → staging::import_file_to_staging → jsonl::read_records(path)                  jsonl.rs:52
           File::open Err ─────────────► ReadRecordsError::Unreadable{line: None}
           line not UTF-8 ─────────────► Rejected(NotJson{line, "not valid UTF-8"})     jsonl.rs:70-76
           other io Err ───────────────► Unreadable{line: Some(n)}                      jsonl.rs:77
           models::parse_ir_lines ─────► Rejected(NotJson | Invalid | SchemaVersion | …) jsonl.rs:84
       → From<ReadRecordsError> for StagingError  staging.rs:47: Rejected→Rejected, Unreadable→Internal
       → ImportError::staging(path, err)         failure.rs:196: Rejected{failure, file} | Internal
     ◄ import_result? (mod.rs:1834) → From<ImportError> for ApiError (server.rs:879)
         Rejected → From<ImportFailure> (server.rs:860):
             NotJson + line ───────────► MalformedImportLine{detail, line}  400 malformed-body
             other + line ─────────────► InvalidImportLines{errors, line}   422 validation-failed
             no line ──────────────────► validation(detail)                 422
         Run → ImportLookupError → 404 | 409
         Internal → ApiError::Internal → 500 + tracing::error! (server.rs:651)
 → IntoResponse (server.rs:774) → Problem{type, title, status, detail|errors, request_id, line}
```

Verdicts. Context kept: the line number survives from `jsonl.rs:73` to the body (`server.rs:697`, `server.rs:706`). Context dropped on purpose: the file path (`_import.jsonl`, a temp file) is dropped at `failure.rs:200` for the wire and kept for the command line; good. State: staging and promote share one transaction (`mod.rs:359-365`); a rejected batch returns before `tx.commit()` and the transaction rolls back on drop, so nothing is half-written. The run stays `running` so the client can resend, as `http-api.md` "Status codes" says. No panic site is on this path except the `spawn_blocking` join, which is mapped (`mod.rs:1751`). A 4xx is not logged beyond the `TraceLayer` response line at `INFO` (`server.rs:1338`); that is the right level.

The desktop uploader maps the `line` back to a file through `HttpError::problem().line` (`crates/libs/import/src/pipeline.rs:206-211`), so the number is used, not only shown.

### 3.2 An sqlx error inside `db/conversation_messages.rs`

```
db/conversation_messages.rs   (&mut *conn).fetch_all(bind_all(&sql, &params)).await?
   18 bare `.await?`, 0 `.context(` in 1,070 lines; 3 `ApiError::validation` (F2)
   sqlx::Error ──From<sqlx::Error> for ApiError (server.rs:889)──► Internal(anyhow::Error::from(e))
   handler `?`  →  Result<_, ApiError>
IntoResponse (server.rs:774) → to_problem (server.rs:644)
   tracing::error!(error = %error_chain(err), "A request failed with 500 Internal Server Error")  server.rs:651
      inside span "request"{method, uri, request_id}  server.rs:1325-1336
      → stderr and logs/ file (logging.rs:46-57, file_layer)
   Problem{type: about:blank, title: Internal server error, status: 500,
           detail: "internal server error", request_id}
web apiClient → problemFromBody (api.ts:143)
   → ApiError(500, "internal server error", requestId = problem.request_id)   api.ts:104
   → requestId is read by nothing (grep `request_id|requestId` in web/src: api.ts only)
```

What is logged: the sqlx message alone (for example `error returned from database: no such column: x`) under the request span. The span gives method and URI, so the handler is known; the statement is not, because `From<sqlx::Error>` adds nothing and the `db/` functions add no context. What is shown: a fixed sentence and a request id the web app never renders. Finding 5.

### 3.3 A panic inside an exporter thread started by `spawn_job`

```
extract  src-tauri/src/commands/extract.rs:161 → start_job → spawn_job(app, job, closure)   jobs.rs:118
  thread::spawn → run_job: panic::catch_unwind(AssertUnwindSafe(run))                        jobs.rs:145
     Ok(Ok(summary)) ───────────────► emit(FINISHED, summary)                                 jobs.rs:127
     Ok(Err(event)) ─────────────────► emit(ERROR, {detail: "{err:#}", user_message: None})   events.rs:230-236
            (the closure's two `inspect_err(|e| run_log.error(e))` wrote it to the run log)   extract.rs:228, 230
     Err(payload) ───────────────────► emit(ERROR, {detail: "the job panicked: <msg>",
                                              user_message: Some("<Stage> failed because of a bug in Message Crate.")})   jobs.rs:148-152
            ✗ the closure is gone; nothing writes `detail` to the Import Run's log
            ✓ Rust's default panic hook printed "thread … panicked at file:line" to the desktop process's stderr
  events::emit (events.rs:36): app.emit Err ──► eprintln! only; UI keeps waiting              events.rs:38
web  onExtractEvents (tauri.ts:635) → awaitTauriJob
     onError: (err) => reject(new Error(err.user_message ?? err.detail))                      tauri.ts:693
            ✗ `detail` dropped whenever `user_message` is set, which is exactly the panic case
  → runJob (useImportJob.ts:845) → runImport catch (useImportJob.ts:1678)
     msg = e.message; cancelled = msg === CANCELLED_MESSAGE                                   useImportJob.ts:1687
     recordError(stage, msg) → issue {kind: "error", item: "Import", reason: msg}              useImportJob.ts:836
     finishImport(status: "failed")
```

What the person reads: `Staging failed because of a bug in Message Crate.` What the Import Run's log holds: the progress lines up to the panic, and no line about it. What holds the panic text: the desktop process's stderr, which a packaged app on macOS or Windows writes nowhere. `panic = unwind` is kept for this catch (root `Cargo.toml:80-81`), and the catch works; the message is then thrown away twice. Finding 1.

### 3.4 The `imessage-reader` sidecar crashing or writing malformed lines

```
Helper::spawn_at  crates/libs/ios-backup/src/helper.rs:131
   Command.spawn() Err ───────────────► with_context("start {path}")                          helper.rs:142
   stderr thread: read_to_string, `let _ =` on its Err                                        helper.rs:149-153
   write request Err ──────────────────► context("send the request to imessage-reader")      helper.rs:157-160
next_event  helper.rs:180
   stdout EOF ─────────────────────────► exited_early(): wait() status + last 8 stderr lines  helper.rs:300-310, tail() :348
   read Err ───────────────────────────► context("read from imessage-reader")                 helper.rs:185
   line not an Event ──────────────────► with_context("imessage-reader sent something unexpected: {line}")  helper.rs:186-187
   Event::Error{message} ──────────────► anyhow!(message)   (the helper's own `fail()` emitted it and exit(1)-ed, main.rs:49-54)
   Source{protocol_version ≠ PROTOCOL_VERSION} ─► bail!                                        helper.rs:194-201
helper panics: no panic hook in the helper (grep set_hook: none) → default hook to stderr → exit 101
   → parent sees EOF → exited_early → "imessage-reader stopped before finishing (exit status: 101): thread 'main' panicked at …"
   → exporter anyhow → ExtractErrorEvent.detail (user_message None, so shown) and run log via inspect_err
stderr_text  helper.rs:313-318: handle.join().ok().unwrap_or_default()
   ✗ a panic in the stderr reader thread turns the helper's whole stderr into ""
Drop  helper.rs:322-328: kill + wait, both `let _ =` (correct: already-exited is an expected Err)
```

This flow is the best of the seven: a crash is reported with its exit status and the tail of what it said, a malformed line is reported with the line, and the protocol version is refused rather than adapted (CLAUDE.md allows exactly this). Two gaps: the stderr thread's own panic is silenced (finding 11), and `next_event` has no cancel check or timeout, so a helper that hangs with its pipes open holds the job thread. **Unable to verify** whether a Cancel reaches a hung helper: `helper.rs` holds no cancel flag; the exporter's loop around `next_event` in `crates/exporters/imessage-ir-exporter` would prove it.

### 3.5 ffmpeg exiting non-zero or hanging

```
run_ffmpeg_with  crates/libs/media/src/tools.rs:416
   resolve_tools()? ; ffmpeg None ────► anyhow!("ffmpeg not found {where_looked}")            tools.rs:421-423
   spawn Err ──────────────────────────► context("start ffmpeg")                                tools.rs:431
   stderr thread read_tail(limit), bounded, never grows past 2×limit                          tools.rs:433, 476-490
   stop = None  (desktop: process.rs:775, 808, 849, 866, 934, 952)
       child.wait() ──────────────────► no timeout; a hung ffmpeg holds the job thread; Cancel cannot reach it   tools.rs:457
   stop = Some  (server: versions.rs:138, 220)
       try_wait poll; stop set ───────► kill + wait, bail!("stopped while ffmpeg ran")          tools.rs:435-456
   reader.join() Err (panic) ─────────► anyhow!("the thread reading ffmpeg's stderr panicked")  tools.rs:459-461
   non-zero ──────────────────────────► bail!("ffmpeg failed ({status})") or with the stderr tail   tools.rs:465-470
callers: .with_context("convert video {path}") etc. → ExtractErrorEvent.detail (desktop) | media pass warn (server)
process.rs:934  if run_ffmpeg(&hevc_args).is_err() { … AVC fallback }
   ✗ why HEVC failed is never logged; the AVC error, if any, is the only one reported
```

The exit path is complete; the hang path has no end on the desktop side. The timeout design belongs to the sibling audit; the flow consequence belongs here: a hang in a desktop conversion is a job that never sends `extract:finished` or `extract:error`, the case `run_job` was written to prevent for panics.

### 3.6 A tokio task panic (Demo Account build) and the media queue thread

```
DemoBuild::run  crates/server/server/src/server_api.rs:548
   outer = tokio::spawn(async { inner = tokio::spawn(building); select!{inner, stopping} … })   server_api.rs:553-555
   inner Ok(Err(e)) ─────────────────► tracing::error! + Failed(e)                              server_api.rs:572-575
   inner Err(cancelled) ─────────────► warn + remove_part_built_demo_account + Failed            server_api.rs:576-587
   inner Err(panicked) ──────────────► tracing::error!("… stopped unexpectedly: {panicked}") + remove_failed_demo_build + Failed   server_api.rs:589-599
   outer panics (only its own code) ─► JoinHandle stored; stop() does `let _ = task.await`      server_api.rs:640
        ✗ state stays Building for the process lifetime: is_building() true, login and rebuild refused
MediaQueue::start  media_queue.rs:71
   thread "media-queue" with its own current_thread runtime                                     media_queue.rs:74-80
   work_through Err ──────────────────► tracing::warn! then wait for wake (recovers)            media_queue.rs:82-87
   work_through panics ───────────────► unwinds the thread; loop ends; thread exits
        ✗ no catch_unwind, no restart, no log line at the time (panic hook → stderr only)
        the only line is at shutdown: "did not end cleanly"                                      media_queue.rs:122-125
Axum request handlers
   no tower_http CatchPanicLayer (grep CatchPanic: none); no std::panic::set_hook anywhere in the server
   a handler panic → hyper's connection task → connection closed, no response, no request_id, no problem body
   the panic text → default hook → stderr only; not through tracing, so not in logs/ nor GET /v1/server/log-lines
```

The inner task is handled well. The media pass is finding 2; the handler gap is finding 4; the outer task is finding 11.

### 3.7 The web side

```
module-level work chain  web/src/screens/import/useImportJob.ts:489-497
   asWork(action): tracked = action().finally(clear); work = tracked              rejections stay on `tracked`
   takeRunFor: await running.catch(() => {})                                     :518  (intentional: another account's run)
   callers: start (:1799, body try/finally), continueAfterIdentities (:1851), approve (:1883, try/finally),
            resumeAtReview (:1937), cancelRun (:2074 → :1741, try/finally)
   UI: onApprove={() => void approve()}  onCancelRun={() => void cancelRun()}     web/src/screens/ImportScreen.tsx:869-870
       ✗ a throw the inner functions do not catch is an unhandled rejection; no listener exists
recordWrites queue  useImportJob.ts:633-660
   recordWrites = recordWrites.then(async () => { try { await save } catch { /* documented */ } })
   ✓ every link catches, so the chain never breaks; a failed write is lost by design (comment :640-645)
queryFn errors  web/src/lib/routeQuery.ts
   QueryCache.onError / MutationCache.onError → endsSession(e) → onUnauthorized                 :66-83
   retry once unless endsSession                                                                :95
   everything else → query.error, rendered per screen with 36 ad-hoc `e instanceof Error ? e.message : String(e)` (F15)
Import Run's own calls bypass TanStack: endSessionIfRefused → SessionRefusedError                useImportJob.ts:904-913
error boundary: none (grep ErrorBoundary|componentDidCatch|onUncaughtError|unhandledrejection in web/src: no hits)
   root: ReactDOM.createRoot(rootEl).render(<React.StrictMode>…)                                web/src/main.tsx:13
   a render throw unmounts the tree; the person sees a blank window
cancellation
   Rust: Cancelled displays "cancelled" (crates/core/message-crate-core/src/process.rs:99); bail!("cancelled") (crates/libs/staging/src/write_queue.rs:778);
         Err("cancelled".into()) (crates/core/message-crate-core/src/attachment_jobs.rs:124, 187)
   → ExtractErrorEvent.detail = format!("{err:#}")   events.rs:233
   → tauri.ts:693 new Error(detail)
   → CANCELLED_MESSAGE = "cancelled" (web/src/lib/runCancel.ts:8); msg === CANCELLED_MESSAGE at useImportJob.ts:1265, 1354, 1687
   ✗ any .context() added between the cancel and the job boundary makes "<context>: cancelled", recorded as an Import Error
```

## 4. Inventories

### 4.1 `.unwrap()` / `.expect()` outside tests, every production site

Method: every `.unwrap()`/`.expect(` before the first `#[cfg(test)]` in a file, excluding `tests.rs`, `lib_tests.rs`, `test_support.rs`, `tests/`. The design audit's counts are reproduced exactly (server 68, src-tauri 1, core 26, libs 190 by this method where the audit said 51, exporters 17 where it said 15, helpers 29). The difference from the audit's libs and exporters figures is the exclusion rule, not the code. Test-only modules then removed, with the declaring line as proof:

| Scope | Counted | Test-only (proof) | Production |
|---|---|---|---|
| server | 68 | 49: `openapi/credential_matrix.rs` x35 and `openapi/document_rules.rs` x5 (`#[cfg(test)] mod` at `openapi.rs:302-306`), `db/write_guard.rs` x9 (`#[cfg(test)]` at `db/mod.rs:35`) | 19 |
| src-tauri | 1 | 0 | 1 |
| core | 26 | 23: `testutil.rs` (feature `testutil`, `Cargo.toml:27`; `lib.rs:18`) | 3 |
| libs | 190 | 155: `ir-format/src/lib_tests.rs` x140 (`#[cfg(test)] #[path]` at `lib.rs:83`), `ios-backup/src/manifest_fixture.rs` x5, `testutil.rs` x2, `reader_build.rs` x2 (`#[cfg(any(test, feature = "testutil"))]` at `ios-backup/src/lib.rs:31-36`), `backup_domain.rs` x4 (`#[cfg(all(test, unix))]` at `:94`), `media/src/testutil.rs` x2 (`media/src/lib.rs:16`) | 35 |
| exporters | 17 | 1: `sms-backup-restore-exporter/src/testutil.rs:16`, but see finding 13 | 16 |
| helpers | 29 | 29: all in `chat-db-fixture` (a fixture crate "for the tests", `Cargo.toml:12`, `publish = false`); `imessage-reader` has 0 (`log.rs:49` is under `#[cfg(test)]` at `log.rs:34`) | 0 |

Verdict key: **infallible** = provably cannot fail from the code at the site; **invariant** = relies on a condition established elsewhere and stated in the message or a comment; **risk** = a real panic on reachable input, with where it lands.

Server (19):

| Site | Verdict |
|---|---|
| `request_id.rs:36` `HeaderValue::from_str(uuid)` | infallible |
| `credentials.rs:182` `hash_password(const)` | invariant (Argon2 on a constant) |
| `openapi.rs:278` `to_string_pretty(spec)` | invariant (utoipa spec); CLI `dump-openapi` only |
| `server.rs:782` `StatusCode::from_u16(problem.status)` | invariant: statuses come from `ProblemType::status` |
| `server.rs:792`, `:798` header values from digits | infallible |
| `keyed_locks.rs:41` (also `:56`, `:62`) `.expect("keyed lock map poisoned")` | invariant: the guarded section is `entry().or_default().clone()`; see finding 6 |
| `media_queue.rs:80`, `:96` runtime and thread build | risk, documented `# Panics` (`:67-70`); at startup, main thread |
| `reset_demo.rs:2121` `NonZeroU32::new(5_000)` | infallible (const) |
| `assets_api/media_links.rs:62` OS random; `:74` HMAC key | infallible |
| `search/parse.rs:291`, `:315` `peek().unwrap()` | infallible: `matches!(self.peek() …)` one line above |
| `search/parse.rs:371` `.expect("parse_unary checked for a token")` | invariant |
| `search/value.rs:73` `and_hms_opt(0,0,0)` | infallible |
| `openapi/response_fields.rs:32` `to_value(spec)` | invariant |
| `db/storage.rs:207` `i64::try_from(share)` after i128 maths | infallible with the comment's bound (`:203-205`) |
| `db/sql.rs:30` `args.add(..).expect("error encoding argument")` | invariant, same as sqlx's own `bind` (comment `:21-22`) |

src-tauri (1): `main.rs:116` `.expect("error while running tauri application")`, main thread at startup; risk accepted.

core (3): `process.rs:154`, `:161` lock poison inside `thread::scope` (invariant: `scope` re-panics first); `process.rs:163` `.expect("every job has a result")` (infallible: every index is assigned or `scope` already panicked); `progress.rs:192` lock poison (invariant).

libs (35):

| Site | Verdict |
|---|---|
| `ios-backup/src/helper.rs:144-146` stdin/stdout/stderr `.take().expect` | infallible: all three are `Stdio::piped()` at `:136-138` |
| `media/src/tools.rs:99, 108, 117, 127, 196, 273, 281` `.expect("tools state lock")` | risk: process-global `OnceLock<Mutex>`; one panic while held makes every later call panic; see finding 6 |
| `mms-parts/src/lib.rs:134` regex | infallible (constant) |
| `build-version/src/lib.rs:89-90` `env::var("CARGO_*")` | invariant: build script only, documented `# Panics` (`:84-87`) |
| `staging/src/write_queue.rs:544, 562, 598, 609, 622` lock poison | invariant inside `thread::scope` (`:555`) |
| `phone/src/lib.rs:303` regex from libphonenumber data | invariant |
| `obfuscate/src/lib.rs:45, 50` regexes; `:101` HMAC | infallible |
| `obfuscate/src/lib.rs:180, 181, 239` `d[0..4].try_into().unwrap()` | infallible: `digest()` returns `[u8; 32]` (`:100`) |
| `import/src/prepare.rs:148, 617, 626, 794, 893, 993` lock poison | invariant |
| `import/src/prepare.rs:1040` `job_tx.send(..).expect("prepare workers alive")` | risk: fails only when every worker thread has exited (all panicked); lands on the Tauri job thread, caught by `run_job` |
| `import/src/run.rs:341` `into_inner().expect` | invariant |
| `import/src/pipeline.rs:737` lock poison | invariant |
| `go-sms-mms/src/emoji.rs:7` regex | infallible |

exporters (16):

| Site | Verdict |
|---|---|
| `sms-backup-plus-exporter/src/flat_eml.rs:17, 19`; `assets.rs:13` regexes | infallible |
| `sms-backup-plus-exporter/src/email_numbers.rs:51` `next().expect("one number")` | infallible: `by_key.len() == 1` at `:50` |
| `sms-backup-plus-exporter/src/emit.rs:170` "just inserted or already present" | invariant (entry API) |
| `sms-backup-plus-exporter/src/emit.rs:482`, `go-sms-pro-exporter/src/emit.rs:434` "from_phones guarantees a phone owner handle" | invariant on the form's validation in core; a form bug panics the job thread, caught by `run_job` |
| `sms-backup-plus-exporter/src/write.rs:400` `next().expect("one peer")` | infallible: `len() == 1` at `:399` |
| `imazing-exporter/src/emit.rs:281, 283, 318, 555, 767, 802` map lookups "kept above" | invariant |
| `whatsapp-exporter/src/emit.rs:597` serialize reactions | infallible |
| `whatsapp-exporter/src/emit.rs:686` `from_str(json).expect("reactions written by ingest_chat")` | invariant: reads its own scratch database |

Production panics that are not `unwrap`/`expect`: `server.rs:649`, `:727` `unreachable!` (exhaustive match, infallible); `openapi/shared_parts.rs:191`, `:198` `panic!` (OpenAPI build-time check, CLI only); `reexport/src/lib.rs:472-473` `unreachable!` (matched above); `imessage-reader/src/data_source.rs:163` `panic!("Database connection is closed!")` (infallible: `take()` happens only in `Drop`, `:170`). No `todo!`, `unimplemented!`, or `std::env::var(..).unwrap()` in production outside the build script.

### 4.2 Swallowed results

`let _ =` on a `Result`: about 95 production sites. By kind: file cleanup `remove_file`/`remove_dir_all` (36, acceptable), `writeln!` to stdout/stderr in the CLI (26, acceptable), child `kill`/`wait` (7, acceptable: already-exited is an Err), channel sends at shutdown (5), `set_global_default` (`logging.rs:56`, deliberate for in-process tests), `tokio::signal::ctrl_c` (`server.rs:1490`). Named because they hide something a person might want:

| Site | What is lost |
|---|---|
| `src-tauri/src/tool_downloads.rs:940`, `:943` | `make_executable` and `write_manifest` failures: the tool is kept as "adopted" and silently re-downloaded or refused at the next start |
| `src-tauri/src/local_server.rs:851` `reader.join()` | a panicked output-reader thread loses the server's last lines from `Event::ChildExited{output}` |
| `crates/server/server/src/db/session_tokens.rs:109` | the DELETE of an expired token; harmless, the lookup returns `None` either way |
| `crates/libs/import/src/run.rs:352` `journal.compact()` | documented; the journal only grows |
| `crates/libs/import/src/http.rs:486`, `journal.rs:326` | documented best-effort |
| `crates/libs/ios-backup/src/helper.rs:151` stderr `read_to_string` | a read error leaves the text read so far; acceptable |
| `crates/server/server/src/imports_api/staging.rs:524` | nothing: the `?` on the same statement propagates the Err; `let _` drops only the Ok value |

`.ok()` that drops an `Err`: 151 sites, most on `to_str`/`parse`/`metadata` into an `Option`. Worth naming: `crates/libs/media/src/tools.rs:167`, `:173` (`resolve_tools().ok()`: why ffmpeg is unusable is lost in `ffmpeg_path()`); `crates/server/server/src/assets_api.rs:182` (`hash_file(&path).ok()?`: an I/O fault while verifying reads as "the asset is not here", so the client re-uploads and the disk fault is never logged); `src-tauri/src/tool_downloads.rs:259-260` (an unreadable manifest makes every file "put there by someone else"); `crates/libs/staging/src/transcode.rs:575-578`; `helper.rs:316` (finding 11).

`unwrap_or_default()`: 168 sites, almost all on `Option`. `assets_api/media_links.rs:198` (`Query::try_from_uri(..).unwrap_or_default()`: a malformed query parses as no parameters; the URL is the server's own) and `:323` (`DateTime::from_timestamp(expires, 0).unwrap_or_default()`: an out-of-range expiry prints 1970; unreachable) are the only two on a `Result`.

`process::exit`: `crates/server/server/src/cli.rs:743` (second Ctrl-C, status 130) and `crates/helpers/imessage-reader/src/main.rs:54` (after `Event::Error` is emitted). `main.rs` of the server returns `ExitCode` and prints `Error: {error:?}` (the chain) at `main.rs:14`.

### 4.3 `catch` in `web/src` and `src-tauri`

99 `catch` blocks and 11 `.catch(` in non-test `web/src`; 45 of the blocks are empty `catch {`, 4 `.catch(() => …)` discard. No `ErrorBoundary`. `console.error` x2 (`OpenPathButton.tsx:24`, `ImportRunView.tsx:235`), `console.warn` x2 (`useImportJob.ts:1427`, `:1438`). The empty blocks in `lib/storage.ts`, `lib/timeZone.ts`, `lib/theme.ts`, `lib/auth.tsx:231, 356, 481, 490` are commented and best-effort by design. The ones that convert a failure into a fact are finding 9. `src-tauri` has no `catch` other than `catch_unwind` at `jobs.rs:145`.

### 4.4 Integer casts and indexing

Rust `as` never panics; the risk is truncation or a sign flip. Narrowing casts in production (19 sites, 3 in `robustness_tests.rs` excluded): `crates/libs/go-sms-mms/src/wsp.rs:122, 510, 511` `uintvar()? as usize` (a `u64` of at most 35 bits; on a 32-bit target it truncates, and the value then feeds `Cursor::take` which checks `remaining() < n` at `:89`, so the worst case is a wrong parse, never an out-of-bounds read); `wsp.rs:414 .get(id as usize)` bounds-checked; `crates/libs/import/src/http.rs:211` (message text), `:231` (`min(part_size)` first); `crates/libs/mail/src/lib.rs:238 (idx + 1) as u32`; `imessage-reader/src/emit.rs:579 part as u32`; `exit_with_parent.rs:95` handle to `usize` on Windows; `asset_uploads.rs:127 … as u32` part count; `db/session_tokens.rs:31`, `db/schema.rs:72`, `db/api_tokens.rs:214-215` (bool to i32). `as i64` from `u64`/`usize`: 148 sites, all sizes or counts that cannot reach `i64::MAX`. `f64 as i64` at `sms-backup-plus-exporter/src/identity.rs:51`, `assets.rs:170`, `sms-backup-restore-exporter/src/read.rs:442`: Rust saturates and maps NaN to 0, no panic. `search/value.rs:283-286` guards the range by hand. **Unable to verify** the lower bound of `part_size` at `asset_uploads.rs:127`; `asset_uploads.rs` where the session's part size is set would prove it.

Slice indexing on input-controlled lengths, all checked: `wsp.rs:173 data[start..]` (`start = pos ≤ len`); `wsp.rs:92` (`remaining() < n` first, so `pos + n` cannot overflow a slice length); `go-sms-mms/src/pdu.rs:150 msg.parts[index]` (indices come from `mms_parts::body_of` over a 1:1 map of `msg.parts`, `:145`); `mms-parts/src/lib.rs:172 src[4..]` after `src.get(..4)` matched the ASCII `cid:`; `sbr/src/read.rs:203-246` (`count` counts ASCII hex digits, so `digits[..count]` is on a char boundary); `read.rs:778 &hex[..16]` on 64 hex chars; `mail/src/parse.rs:285 &line[1..]` after `bytes[0] == b'>'`; `ir-format/src/read_csv.rs:51 rows[0]` after the `is_empty` bail at `:47`. **Unable to verify**: `crates/libs/phone/src/lib.rs:79, 173 digits[1..]` and `crates/libs/obfuscate/src/lib.rs:149 fake_digits.as_bytes()[di]` assume ASCII-only strings; `digits_as_written` and the builder of `fake_digits` would prove it.

### 4.5 Mutex poisoning, three policies

| Policy | Sites | Effect after a panic while held |
|---|---|---|
| `unwrap_or_else(PoisonError::into_inner)` | `media_queue.rs:97, 116`; `server_api.rs:512, 607, 636`; `logging/files.rs:109`; `journal/src/lib.rs:65, 171`; `import/prepare.rs:854`; `src-tauri` `jobs.rs:51`, `local_server.rs:701, 939, 952`, `app_directories.rs:141`, `export_directories.rs:208` | continues with possibly half-updated state |
| `.expect("… poisoned")` | `keyed_locks.rs:41, 56, 62`; `media/tools.rs` x7; `write_queue.rs` x5; `import/prepare.rs` x6; `pipeline.rs:737`; `run.rs:341`; core `process.rs:154, 161`, `progress.rs:192` | a second panic in whoever locks next |
| `.map_err(..)?` | `jobs.rs:71, 106` (to `String`), `credentials.rs:100` (to `ApiError::Internal`), `export_directories.rs:181` | reports the poison as an error |

The process-global `tools_state()` in `crates/libs/media/src/tools.rs` is the one where the second policy hurts: a poison there panics every later ffmpeg or ffprobe call, which in the server runs on the media-queue thread (3.6) and so ends the pass for good. Finding 6.

### 4.6 Where a panic lands

| Origin | Catcher | Result | Logged where |
|---|---|---|---|
| Axum handler | hyper connection task | connection closed, no body | stderr only (default hook) |
| `spawn_blocking` inside a handler | `JoinError` mapped at `imports_api/mod.rs:1751`, `assets_api.rs:880, 1411, 1448`, `server_api.rs:36-39`, `accounts_api.rs:959-963` | `500` problem | server log with request id |
| Demo build inner task | `server_api.rs:589-599` | `Failed` state, data removed | server log |
| Demo build outer task | nobody (`server_api.rs:640` `let _ = task.await`) | state stuck `Building` | nowhere |
| media-queue thread | nobody | pass stops until restart | shutdown only |
| `exit_with_parent` watcher thread (Windows) | `let _ = self.gone.await` (`exit_with_parent.rs:54`) | **Unable to verify**: whether a panicked watcher reads as "parent gone" and exits the server | |
| Tauri job thread | `run_job` (`jobs.rs:145`) | `extract:error` with `user_message` | stderr only; see finding 1 |
| scoped workers (`core/process.rs:142`, `write_queue.rs:555`) | `thread::scope` re-panics in the caller | reaches the job thread above | |
| import prepare workers (`import/prepare.rs:993` loop) | nobody; `result_tx.send` never happens | **Unable to verify** whether the consumer waits forever on the missing `PrepareResult`; `prepare.rs` around `inflight` and `result_rx.recv()` would prove it | |
| Tauri plain command | Tauri runtime | **Unable to verify** whether the invoke promise rejects; the `tauri` crate's command runner would prove it | |
| `local_server.rs:794, 830, 933` threads, `main.rs:73` sweep, `tool_downloads.rs:852, 1022` | nobody | watch thread: the started server's status never advances; sweep: nothing | stderr only |
| `imessage-reader` | none in the helper; the parent reads exit 101 and the stderr tail (3.4) | exporter error with the panic text | run log and UI |

## 5. Findings

Importance is 1 to 10, where 10 is "a person loses data or cannot find out why something failed" and 1 is "tidiness".

### 1. An exporter panic's message is lost twice (7/10)

Evidence. `src-tauri/src/commands/jobs.rs:145-152` builds `ExtractErrorEvent{detail: "the job panicked: …", user_message: Some(panic_text(name))}` after `catch_unwind`; nothing writes `detail` to the Import Run's log, because the `run_log.error` calls live inside the closure that panicked (`extract.rs:228, 230`). `web/src/lib/tauri.ts:693` then rejects with `new Error(err.user_message ?? err.detail)`, so the UI's issue row (`useImportJob.ts:1689`, `:836`) holds only `Staging failed because of a bug in Message Crate.` The `docs/architecture/import-run-logs.md` rule that one viewer reads both logs cannot show what it was never given.

Remediation. Give `run_job` a log sink and keep `detail` on the web side.

```rust
// src-tauri/src/commands/jobs.rs
pub(crate) fn spawn_job<F>(app: AppHandle, job: Job, run_log: RunLog, run: F) where … {
    thread::spawn(move || {
        let outcome = run_job(job, run);
        drop(local_server_job);
        match outcome {
            Ok(summary) => events::emit(&app, events::FINISHED, summary),
            Err(error) => {
                run_log.error(&error.detail);          // the Import Run's log gets the panic text
                events::emit(&app, events::ERROR, error)
            }
        }
    });
}
```

```ts
// web/src/lib/tauri.ts:693
onError: (err) => {
  onLog?.(err.detail);                                   // the run's log panel sees the chain
  reject(new Error(err.user_message ?? err.detail, { cause: err.detail }));
},
```

### 2. The media pass dies silently on a panic (6/10)

Evidence. `crates/server/server/src/media_queue.rs:74-96`: the thread's loop has no `catch_unwind`; a panic in `work_through` or in anything it calls (for example the `tools.rs` lock, finding 6) ends the thread. `self.thread` keeps the dead handle; nothing restarts it; `wake()` notifies nobody. The one log line is at `stop()` (`:124`), at shutdown. Thumbnails and Previews stop for every Import Run until the server restarts.

Remediation.

```rust
// media_queue.rs, inside the spawned closure
loop {
    let pass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(work_through(&pool, &cfg, &stop))
    }));
    match pass {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(error = format!("{error:#}"), "The pass that makes Thumbnails and Previews stopped. It starts again after the next Import Run"),
        Err(payload) => tracing::error!(panic = %panic_message(&*payload), "The pass that makes Thumbnails and Previews panicked. It starts again after the next Import Run"),
    }
    if stop.load(Ordering::Relaxed) { break; }
    runtime.block_on(wake.notified());
}
```

Pair it with the panic hook in finding 4 so the panic's own text reaches the log file.

### 3. Cancellation is a string contract across the process boundary (6/10)

Evidence. `crates/core/message-crate-core/src/process.rs:98-99` `#[error("cancelled")] pub struct Cancelled`; `crates/libs/staging/src/write_queue.rs:778` `anyhow::bail!("cancelled")`; `crates/core/message-crate-core/src/attachment_jobs.rs:124, 187` `Err("cancelled".into())`; `src-tauri/src/commands/events.rs:233` `detail: format!("{err:#}")`; `web/src/lib/runCancel.ts:8` `CANCELLED_MESSAGE = "cancelled"`; compared at `useImportJob.ts:1265, 1354, 1687`. One `.with_context(..)` between a `check_cancel` and the job boundary turns a Cancel into `<context>: cancelled`, which `useImportJob.ts:1689` records as an Import Error and `finishImport` completes as `failed`. F19 asked whether anyone matches the text; the answer is the web app, three times, and the match is the only thing that keeps a Cancel from being a failure. **Unable to verify** whether every exporter path is free of such a context; `src-tauri/src/commands/extract.rs` `run_imessage` and friends, and each exporter's `run`, would prove it.

Remediation. Carry the kind on the event, decide it on the Rust side once.

```rust
// src-tauri/src/commands/events.rs
#[derive(Serialize)]
pub struct ExtractErrorEvent {
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    pub cancelled: bool,                                   // new
}
impl From<anyhow::Error> for ExtractErrorEvent {
    fn from(err: anyhow::Error) -> Self {
        let cancelled = err.chain().any(|e| e.downcast_ref::<message_crate_core::Cancelled>().is_some());
        Self { detail: format!("{err:#}"), user_message: None, cancelled }
    }
}
```

Replace `bail!("cancelled")` at `write_queue.rs:778` and the two `Err("cancelled".into())` with `Cancelled`, and in `tauri.ts:693` reject with a `CancelledError` subclass that `useImportJob.ts` checks with `instanceof` instead of `===`.

### 4. A handler panic bypasses the problem document and the server's log file (6/10)

Evidence. No `tower_http::catch_panic` layer in `crates/server/server/src/server.rs` (grep `CatchPanic`: none) and no `std::panic::set_hook` in the server. A panic in a handler is caught by hyper's connection task; the client gets a closed connection, no `x-request-id`, no body; the panic text goes to stderr through Rust's default hook, which `logging.rs` does not route into the `logs/` files, so `GET /v1/server/log-lines` never shows it. `docs/architecture/http-api.md` "Failures" says every failure answers a problem document with `request_id`; a panic is the one failure that does not.

Remediation.

```rust
// crates/server/server/src/logging.rs, in init()
std::panic::set_hook(Box::new(|info| {
    let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
    tracing::error!(location, panic = %info, "A thread panicked");
}));

// server.rs, in the router's layer stack, inside the request_id layer
.layer(tower_http::catch_panic::CatchPanicLayer::custom(|_: Box<dyn Any + Send>| {
    ApiError::Internal(anyhow::anyhow!("the handler panicked")).into_response()
}))
```

The hook also fixes the "stderr only" cells in 4.6 for the media pass, the Tauri job thread is a different process and keeps finding 1's fix.

### 5. Database errors reach the 500 log without a statement, and the request id never reaches a person (5/10)

Evidence. `crates/server/server/src/db/conversation_messages.rs` has 18 bare `.await?` on sqlx calls and no `.context(`; `From<sqlx::Error> for ApiError` (`server.rs:889-893`) wraps with nothing; `error_chain` (`server.rs:808`) therefore prints one link. The span (`server.rs:1325-1336`) supplies method, URI and request id, so the handler is known and the statement is not; a `no such column` from a `format!`-built statement such as `:421-425` cannot be told from another in the same handler. On the client, `ApiError.requestId` (`web/src/lib/api.ts:104`) is read by no component (grep in `web/src`: `api.ts` only), so a person who sees `internal server error` has nothing to quote.

Remediation. Context at the `db/` boundary, request id in the one error renderer F15 proposes.

```rust
// db/conversation_messages.rs, each sqlx call
.fetch_all(bind_all(&sql, &params)).await
.map_err(|e| anyhow::Error::from(e).context("list a conversation's messages"))?
```

```ts
// web/src/lib/errorText.ts (F15)
export function errorText(e: unknown): string {
  if (e instanceof ApiError && e.status >= 500 && e.requestId) return `${e.message} (request ${e.requestId})`;
  return e instanceof Error ? e.message : String(e);
}
```

F2's `DbError` would make the first snippet one `From` impl instead of eighteen `map_err`s.

### 6. Three poisoning policies, and one global lock that turns a panic into a permanent one (5/10)

Evidence. Table 4.5. `crates/libs/media/src/tools.rs:99-281` guards a process-wide `OnceLock<Mutex<ToolsState>>` with `.expect("tools state lock")` at seven sites. The guarded sections call `probe_tool`-style functions that run child processes (`:99-128`), so a panic while held is possible, after which `ffmpeg_path()`, `tools_dir()` and every conversion panic in turn: in the server that is the media-queue thread (finding 2), in the desktop app it is every later job.

Remediation. One policy per crate, written down; for `tools.rs`:

```rust
fn state() -> std::sync::MutexGuard<'static, ToolsState> {
    tools_state().lock().unwrap_or_else(std::sync::PoisonError::into_inner)   // the state is a cache; a half-written cache is re-probed
}
```

and shrink the guarded section so probing happens outside the lock (`:99-128` holds the lock across `probe`).

### 7. Unguarded `void` actions and no error boundary in the web app (5/10)

Evidence. `web/src/screens/ImportScreen.tsx:869-870` `onApprove={() => void approve()}`, `onCancelRun={() => void cancelRun()}`; `approve` (`useImportJob.ts:1883-1903`) and `cancelRun` (`:1741-1749`) wrap their bodies in `try/finally` with no `catch`. The inner calls (`moveStageAtReview :967`, `runMediaStage :1314`, `runUpload :1185`) catch their own failures, so the exposure is any throw outside them, for example `discardRun` (`:1743`) answering a 500. Nothing listens for `unhandledrejection` or `error`, and no `ErrorBoundary` exists (`web/src/main.tsx:13-21`, `web/src/App.tsx`), so a render throw blanks the window and a rejected `void` promise is invisible.

Remediation.

```tsx
// web/src/main.tsx
window.addEventListener("unhandledrejection", (e) => console.error("Unhandled rejection", e.reason));
ReactDOM.createRoot(rootEl).render(
  <React.StrictMode>
    <AppErrorBoundary>            {/* class component: getDerivedStateFromError → a "Something broke; reload" screen */}
      …
    </AppErrorBoundary>
  </React.StrictMode>,
);
```

```ts
// useImportJob.ts, the two actions
} catch (e: unknown) {
  recordError(scratch.activeStage, errorText(e));
} finally {
```

### 8. The final job event has no fallback delivery (4/10)

Evidence. `src-tauri/src/commands/events.rs:36-40`: `app.emit` failure goes to `eprintln!`. `jobs.rs:7-13` explains that the web app waits for `extract:finished` or `extract:error` to start the next stage; when that one emit fails the wait never ends, the exact hang `run_job` exists to prevent for panics. The `extract` command returns `Ok(())` as soon as the thread starts (`extract.rs:239`), so the invoke promise cannot carry the result either.

Remediation. Store the outcome in `AppState` beside `RunningJob`, and let the web app poll `job_outcome` after a timeout, or retry the emit:

```rust
// jobs.rs, after run_job
if let Err(error) = app.emit(events::ERROR, &error) {
    eprintln!("{}", undelivered_line(events::ERROR, &error));
    state.lock().unwrap_or_else(PoisonError::into_inner).last_outcome = Some(Err(error));  // read by a `job_outcome` command
}
```

### 9. Empty catches that turn a failure into a fact (4/10)

Evidence. `web/src/screens/ImportScreen.tsx:91` `probePath`: any rejection becomes `{exists: false, …}`, so a permission error on the backup directory reads as "no such file". `useImportJob.ts:991` `mediaToolsMissingFor`: a failed `tools_status` call becomes "ffmpeg and ffprobe are missing". `useImportJob.ts:1825` `invokeImessageBackupIdentities(..).catch(() => [])`: a backup that cannot be read shows no identities and the form goes on. `web/src/lib/useAccountProfile.ts:79` `.catch(() => null)`. Each replaces "could not find out" with a definite answer.

Remediation. Return a tri-state and show the third state.

```ts
async function probePath(path: string): Promise<PathStat | "unknown" | null> {
  try { return mapPathStat(await invokePathStat(trimmed)); } catch { return "unknown"; }
}
```

### 10. Best-effort failures with no log line at all (3/10)

Evidence. `src-tauri/src/tool_downloads.rs:940, 943` (`make_executable`, `write_manifest`); `crates/server/server/src/assets_api.rs:182` (`hash_file(..).ok()?` treats an I/O fault as a missing asset); `crates/libs/media/src/process.rs:934` (`if run_ffmpeg(&hevc_args).is_err()`: the HEVC reason is dropped before the AVC fallback); `crates/libs/media/src/tools.rs:167, 173` (`resolve_tools().ok()`). None is wrong to continue; each should say why it continued.

Remediation. `tracing::warn!(error = %format!("{e:#}"), …)` on the server sites; on the desktop sites write to the run log where one is in scope, and `eprintln!` otherwise, since `src-tauri` has no logger of its own (3 `eprintln!`, 0 `tracing`, 0 `log` in the crate).

### 11. Two silent dead ends (3/10)

Evidence. `crates/libs/ios-backup/src/helper.rs:313-318` `handle.join().ok().unwrap_or_default()`: a panic in the stderr reader turns the helper's whole stderr into `""`, so `exited_early` (`:300`) reports the status with no tail. `crates/server/server/src/server_api.rs:640` `let _ = task.await` on the outer Demo build task: if that task's own code panicked, `DemoBuildState` stays `Building`, so `is_building()` (`:650`) refuses the Demo Account login and a rebuild until restart.

Remediation. `helper.rs:316`: `.and_then(|h| h.join().map_err(|_| ()).ok()).unwrap_or_else(|| "(stderr reader panicked)".into())`. `server_api.rs:640`: `if let Err(e) = task.await { tracing::error!("Demo Account build task ended unexpectedly: {e}"); self.set(DemoBuildState::Failed(e.to_string())); }`.

### 12. The design audit's unwrap counts overstate production risk (3/10)

Evidence. Table 4.1: 49 of the server's 68 counted sites are in three `#[cfg(test)]` modules, 23 of core's 26 are the `testutil` feature, 155 of libs' 190 are test files, and all 29 "helpers" sites are the `chat-db-fixture` crate. The production figures are 19, 1, 3, 35, 16, 0, and no site is a confirmed panic on input a person controls.

Remediation. When the metric is next computed, exclude modules declared under `#[cfg(test)]` or `#[cfg(any(test, feature = "testutil"))]` and the fixture crate, or quote both numbers.

### 13. A test helper compiled into a production crate (2/10)

Evidence. `crates/exporters/sms-backup-restore-exporter/src/lib.rs:13` `pub mod testutil;` with no `cfg`, so `testutil.rs:16` `fs::write(path, "partial").unwrap()` ships in the exporter. Nothing calls it at runtime.

Remediation. `#[cfg(any(test, feature = "testutil"))] pub mod testutil;` as `crates/libs/ios-backup/src/lib.rs:31-36` does.

## 6. Anti-pattern checklist

| Anti-pattern | Present | Where |
|---|---|---|
| Swallowed exceptions | Yes, bounded | 45 empty `catch {}` in `web/src`, most commented; finding 9 names the four that assert a fact. Rust: finding 10. |
| Generic catch-all hiding specifics | Yes, by design in one place | `ApiError::Internal` hides the chain from the client and logs it (`server.rs:644-664`): correct. `tauri.ts:693` hides `detail` behind `user_message`: finding 1. |
| Errors used for flow control | Yes, typed | `StageNotRecordedError`, `SessionRefusedError` (`useImportJob.ts:884, 893`) and `throw new Error(CANCELLED_MESSAGE)` (`runCancel.ts:41`): documented. `throw new Error("no-op add")` to reach a `catch` (`ImportScreen.tsx:197-199`): not. |
| Missing error boundaries | Yes | No React boundary, no `unhandledrejection` listener (finding 7); no `CatchPanicLayer` or panic hook in the server (finding 4); no `catch_unwind` on the media-queue thread (finding 2). |
| Inconsistent error formats | Yes | Rust to String in `src-tauri`: 43 `{e}`, 9 `{e:#}`, 7 `.to_string()` (F11). Web: 36 `instanceof Error ? e.message : String(e)` (F15). Poisoning: three policies (finding 6). |

## 7. A standard template

Rust library crate (exporters, libs, core): a `thiserror` enum only where a caller branches on the kind (`Cancelled`, `ReadRecordsError`, `HttpError` do this right); otherwise `anyhow::Result` with a `.with_context(|| format!("<verb> {}", path.display()))` on every I/O and child-process call, naming the resource. Never `let _ =` on a `Result` without a one-line comment saying what is lost and why that is fine. A worker thread's result crosses `join()` as `Result<T, String>` or re-panics through `thread::scope`; never `handle.join().ok()`.

Server: `db/` returns `DbError` (F2); `From<DbError> for ApiError` is the one mapping; every `spawn_blocking` join is mapped to `ApiError::Internal` with the task's name, as `assets_api.rs:880` does; a long-lived thread wraps its loop in `catch_unwind` and logs the payload; `logging::init` installs a panic hook that writes through `tracing`; the router carries `CatchPanicLayer` inside the request-id layer so a panic still answers a problem document with `request_id`. A best-effort failure is `tracing::warn!(error = %format!("{e:#}"), "<what was not done>. <what happens instead>")`, the sentence shape `media_queue.rs:84-87` already uses.

Desktop host: a job's failure is `ExtractErrorEvent { detail: "{err:#}", user_message, cancelled }`; `run_job` writes `detail` to the Import Run's log before `emit`; a failed final `emit` is stored in `AppState` for a `job_outcome` command. A plain command's `Result<_, String>` formats with `{e:#}`, never `{e}`, when the source is `anyhow` (F11's shared command error would make this one `From`).

Web: one `errorText(e)` (F15) that appends `request_id` on a `5xx`; `ApiError` is the only type thrown by `apiClient`; desktop errors are `Error` subclasses (`CancelledError`, `SessionRefusedError`) checked with `instanceof`; an `AppErrorBoundary` at the root and an `unhandledrejection` listener that logs; every `void action()` handler either catches inside `action` or is `action().catch(showError)`; an empty `catch` carries a comment and returns "unknown", not a fact.

## 8. Unable to verify

1. Whether a Cancel reaches a hung `imessage-reader`: `crates/libs/ios-backup/src/helper.rs` holds no cancel flag; the loop around `next_event` in `crates/exporters/imessage-ir-exporter` would show it.
2. Whether every exporter path adds no `.context()` above a `Cancelled` (finding 3): `src-tauri/src/commands/extract.rs` `run_*` functions and each exporter's `run`.
3. Whether a panicked prepare worker (`crates/libs/import/src/prepare.rs:993`) leaves the consumer waiting on a `PrepareResult` that never comes: the `inflight` accounting around `result_rx.recv()` in the same file.
4. Whether Tauri rejects an invoke promise when a plain (non-job) command panics: the `tauri` crate's command runner, outside this repository.
5. Whether a panic in the Windows parent-watcher thread (`crates/server/server/src/exit_with_parent.rs:100`) makes `let _ = self.gone.await` (`:54`) read as "parent gone": the lines after `:54`.
6. The ASCII assumption behind `crates/libs/phone/src/lib.rs:79, 173` and `crates/libs/obfuscate/src/lib.rs:149`: `digits_as_written` and the builder of `fake_digits`.
7. The lower bound of `part_size` at `crates/server/server/src/asset_uploads.rs:127`: where the upload session's part size is chosen in the same file.
8. The web consequence of a panicked `watch` thread in `src-tauri/src/local_server.rs:830`: whether `local_server_status` reports "starting" forever; `web/src/lib/localServer.ts` and the status screen would show it.

## 9. Suggested order of work

1. Finding 4's panic hook (one function in `logging.rs`): it makes findings 2, 6 and the stderr-only cells of 4.6 visible in the log file before anything else changes.
2. Finding 1 (two small edits): the panic text reaches the Import Run's log and the UI's log panel.
3. Finding 3 with F19: a `cancelled` flag on the event and `Cancelled` in place of the three string errors.
4. Finding 2 and `CatchPanicLayer` (finding 4, second half).
5. Finding 5 together with F2 and F15: `DbError`, `errorText`, the request id on screen.
6. Finding 7, then 8, 9, 10, 11, 13 as small pull requests.
