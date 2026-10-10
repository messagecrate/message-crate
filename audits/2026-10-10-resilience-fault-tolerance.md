# Resilience and fault tolerance audit

- **Audit:** resilience-fault-tolerance (system-level view: what happens when a dependency, process, disk, network or peer fails or is slow, and whether the system degrades, recovers, and stays consistent)
- **Date:** 2026-10-10
- **Commit:** `530dc1791`
- **Scope:** the whole repository except `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`
- **Method:** read-only. Every claim cites a `path:line` that was read. Nothing was built or run.
- **Sibling audits:** error types, mapping and code-level retry shape belong to `error-handling-resilience`; propagation and panics belong to `exception-flow-analysis`. This report names a panic or an error mapping only where it decides a system outcome, and defers the rest.
- **Design audit:** `audits/2026-10-09-initial-software-design-analysis.md`. F7 (import concurrency) and F17 (pipeline bottlenecks) are cited by id, not repeated. This audit answers F7's open question about transaction length (finding R1).

## 1. Summary

**Resilience rating: 6/10.**

The system is strong on durability and resumability and weak on time bounds and isolation.

What holds up: every long-running piece of work survives a process death and resumes from durable state. The Upload's journal (`crates/libs/import/src/journal.rs`), the staging write queue's complete-file check (`crates/libs/staging/src/write_queue.rs:459-464`), the `media_queue` table (`crates/server/server/src/media_queue.rs:1-22`), the demo-build record (`crates/server/server/src/reset_demo.rs:435-452`), and the swap-with-rollback of a reset (`reset_demo.rs:1947-1994`) all mean a kill, a crash or a power loss costs work in flight and nothing else. A replayed import batch stores each message once (`crates/server/server/src/imports_api/tests.rs:3078-3083`). Shutdown drains requests, kills ffmpeg and waits for it (`crates/server/server/src/server.rs:1425-1441`). Session refusal mid-run stops the run cleanly and leaves it for the next Upload (`crates/libs/import/src/run.rs:203-212`, `crates/libs/import/src/prepare.rs:947-950`).

What does not: nothing bounds how long a thing may take. An import batch holds SQLite's single write lock for its whole staging, promote and FTS pass in one transaction, while every other writer waits 15 s and then gets a 500 (R1). ffmpeg and ffprobe have no wall-clock limit, and the serial media pass restarts on the same Asset after a reboot (R3). Graceful shutdown has no drain deadline, so `docker stop` kills the process after Docker's default 10 s (R4). The server has no request timeout layer (R19). The HTTP `/health` route reports the process alive with no database check, and no Docker `HEALTHCHECK` or compose `healthcheck` uses even that (R2). A crashed server under the desktop app is reported, not restarted (R5). A network blip during an Upload marks conversations failed one by one instead of pausing (R8).

Findings by importance (1 to 10):

| Id | Importance | Area | Finding |
|----|-----------|------|---------|
| R1 | 8 | server, SQLite | One import batch is one write transaction; `busy_timeout` 15 s is shorter than a batch; every other writer, login included, gets a 500. WAL fallback silently makes it worse. |
| R2 | 7 | deployment | `/health` is liveness only and nothing in Docker uses it; the desktop probe is the only readiness check. |
| R3 | 7 | media | No wall-clock timeout on ffmpeg or ffprobe; the serial pass can wedge on one Asset, and restarts on the same one. |
| R4 | 6 | shutdown | No drain deadline; Docker's default 10 s grace kills an import batch in flight. |
| R5 | 6 | desktop | The app's own server is not restarted after it exits; the screens stop polling. |
| R6 | 6 | server, disk | A batch body is streamed to the OS temp directory, not the Data Directory; the import's work is not cancelled when the client goes away. |
| R7 | 5 | media | Any database error stops the media pass until the next Import Run ends. |
| R8 | 5 | import client | Fixed 3 retries over about 4 s, no breaker; an outage marks every remaining conversation failed. |
| R9 | 5 | server, pool | 4 connections, 2 pinned by imports, sqlx's default acquire timeout, `PoolTimedOut` answered as 500. |
| R10 | 4 | web | No default fetch timeout in `apiClient`. |
| R11 | 3 | server | No per-request timeout or concurrency limit layer. |
| R12 | 3 | tools | One download attempt per start, no retry on a network failure. |
| R13 | 3 | server | A `running` Import Run whose client vanished is never aged out. |
| R14 | 2 | time | `chrono::Local` in output formatting and run directory names. |
| R15 | 2 | server | A poisoned `KeyedLocks` map would refuse every later import until restart. |

## 2. Failure modes traced

Each trace says what the code does today, with the evidence, and names the finding it feeds.

### 2.1 The server child dies or hangs under the desktop app

- **Probe.** `probe` (`src-tauri/src/local_server.rs:116-140`) connects with `CONNECT_TIMEOUT` 2 s (`:86`) and asks `GET /v1/server` with `ANSWER_TIMEOUT` 15 s (`:91`). A 5xx problem document with `type`, `status` and `request_id` counts as a Message Crate (`is_message_crate_answer`, `:147-159`), so a busy server is not mistaken for another program. Tested at `src-tauri/src/local_server/tests.rs:504`.
- **Start.** `spawn_server` (`:876-909`) pipes stdout and stderr and drains both (`keep_output`, `:928-947`), so the child never blocks on a full pipe. `START_TIMEOUT` is 300 s (`:79`); past it, `Event::Deadline` kills the child (`:427-436`).
- **Watch.** `watch` (`:828-871`) polls every `POLL_INTERVAL` 250 ms (`:82`) with `reap_if_exited`, joins the reader threads before reporting, and is generation-guarded so a stale watcher exits.
- **Exit while starting.** `Phase::Starting` + `ChildExited` becomes `Phase::Lost` and re-probes (`:406-417`); with exit code `OPERATION_LOCK_HELD_EXIT_CODE` 75 (`crates/libs/serve-protocol/src/lib.rs:25`) it probes `LOCKED_OUT_PROBES` 1200 times (`:102`), 300 s, waiting for a second window's server. Tested at `local_server/tests.rs:608,627`.
- **Exit while running.** `Phase::Own` + `ChildExited` becomes `Phase::Failed` with no action (`:418-424`). No `Action::Spawn`, no backoff, no count. The web hook polls a ready server every `READY_POLL_MS` 3000 ms (`web/src/lib/useLocalServer.ts:12`) but stops after `failed` (`:55`, `wait` is `null`), and `retry` is a manual call (`:74`). `Event::Check` probes only in `Phase::Found` (`:446-451`), so a dead own server is noticed only by `watch`. See **R5**.
- **Hang.** A server that listens and then stops answering is not detected: `watch` checks exit and the listening line only (`:838-868`); nothing re-probes `Phase::Own`. The TanStack client sees failures with one retry (`web/src/lib/routeQuery.ts:88`). See **R5**.
- **Reverse direction.** `--exit-with-parent` (`crates/server/server/src/exit_with_parent.rs:67-79`) polls `parent_id()` every `PARENT_CHECK_INTERVAL` 2 s (`serve-protocol/src/lib.rs:48`), silences stdio before the drain (`:115-132`) so a `println!` on a closed pipe cannot panic the stop, and ends the server the way SIGTERM does (`server.rs:1472-1484`). Tested at `crates/server/server/tests/exit_with_parent.rs:220,247`. On close the app kills the whole process group or Job Object (`local_server.rs:648-654`, `process_tree`, ADR 0018 note of 2026-10-05). Tested at `local_server/tests.rs:657,685,712`.

### 2.2 SQLite busy or locked

- **Pragmas.** `busy_timeout` 15 s, `synchronous NORMAL`, WAL best-effort (`crates/server/server/src/db/engine.rs:17-38,62-69`). Pool `max_connections(4)` (`:56`); no `acquire_timeout` is set.
- **Write transactions.** Every write is `BEGIN IMMEDIATE` through `begin_write` (`crates/server/server/src/db/write_tx.rs:57-61`), so a second writer waits on the busy handler rather than failing with `SQLITE_BUSY_SNAPSHOT`. That design is sound and well explained (`:1-24`).
- **How long the import holds the lock.** `import_jsonl_files_on_conn` (`crates/server/server/src/imports_api/mod.rs:314-388`) opens one transaction at `:356` and commits at `:372`. Inside it: `require_running_import` (`:360`), `stage_all_files` (`:362`, every JSONL row of the batch into staging, contact linking), `promote_step` (`:368`, which drops the FTS triggers, bulk-inserts, re-indexes FTS and reinstalls the triggers, `imports_api/promote.rs:303-311,492-520`), and `reset_for_account` (`:369`). A batch body can be up to `MAX_IMPORT_BODY_BYTES` 64 MiB from the Upload (`crates/libs/import/src/run.rs:69`) and up to `MAX_REQUEST_BODY_BYTES` 512 MiB at the server (`server.rs:1021`). After the commit, `dedupe_cross_source` runs its own write transaction (`crates/server/server/src/dedupe.rs:197,271`). The Demo Account build imports in batches of `IMPORT_BATCH_BYTES` 64 MiB, each its own transaction (`reset_demo.rs:788,476-480`).
- **Who waits.** Login: `session_api.rs:53-56` takes `begin_write` for `record_login`. Discard and complete (`db/imports.rs:644-649,759-771`), `queue_import_run` (`media_queue.rs:138-147`), `media_queue::remove` (`media_queue.rs:304`), `sweep_unreferenced` (`crates/server/server/src/asset_store.rs:553`), asset completion under `asset_complete_locks` (`server.rs:398`), every account-settings `UPDATE` (`db/account_profile.rs:333-692`). Each waits up to 15 s and then its `sqlx::Error` becomes `ApiError::Internal` (`server.rs:889-893`), a 500. Reads do not wait under WAL. See **R1** and **R9**.
- **Two imports.** `MAX_CONCURRENT_IMPORTS` 2 (`imports_api/mod.rs:1767`) pins 2 of 4 connections for whole runs (`:1785-1802`); the per-account `KeyedLocks` mutex (`:1798`, `crates/server/server/src/keyed_locks.rs:37-51`) serialises one account's batches. Two imports for two accounts take turns on the single SQLite write lock, so the second account's batch waits on `busy_timeout` too, and a batch over 15 s fails the other account's batch with a 500 which the Upload then retries (transient, `crates/libs/http/src/retry.rs:83-118`). See **R1**.
- **WAL fallback.** `try_enable_wal` warns and continues (`engine.rs:62-69`). In rollback-journal mode readers wait on the import's lock as well, so the whole server stalls for each batch. The mode is printed once at start (`server.rs:1379-1386`) and reported nowhere else. See **R1**.

### 2.3 Disk full or read-only data directory

- **Server log.** `write_event` cuts a half-written line back to the last whole one (`crates/server/server/src/logging/files.rs:122-133`) and the tracing writer drops the event; the design is written up in `docs/architecture/server-log.md`. The log opens before the database (`server.rs:1360-1368`), so a database that cannot open is still logged. Good.
- **Journal.** Every event is one buffered row with `sync_data` under a process-wide lock (`crates/libs/journal/src/lib.rs:49,63-88`), and a torn last row is isolated on the next append (`:77-81,90-99`). Good.
- **Reset and first start.** `VACUUM INTO` a sibling path (`reset_demo.rs:995-1014`), checkpoint, then a two-step `Swap` with rollback that reports every problem (`:1842-1994`), and `ReadyWhileRebuilding` restores `server.ready` on drop (`crates/server/server/src/operation_lock.rs:121-174`). A failed first seed is reported and the server starts empty (`reset_demo.rs:281-312`). Good.
- **Assets.** Multipart parts and manifests go to `.incoming/` with temp-and-rename (`crates/server/server/src/asset_uploads.rs:3,159-166`); abandoned sessions are swept after `STALE_UPLOAD_SECS` 24 h at the next upload start (`asset_store.rs:699-743`, `asset_uploads.rs:215-216`); shard temps after `STALE_TEMP_SECS` 1 h (`asset_store.rs:745-756`). Media work directories older than `STALE_WORK_SECS` 24 h are removed at the next pass (`crates/server/server/src/process_assets.rs:405-441`). Desktop scratch is swept at start and request end (`crates/core/message-crate-core/src/scratch.rs:64,138-146`). Good.
- **Staging spool.** `check_headroom` refuses a write the disk plainly cannot hold, and never blocks when free space cannot be read (`crates/libs/staging/src/headroom.rs:84-92`); the write queue checks once up front (`write_queue.rs:518`) and the first error aborts the pool (`:537,559`), leaving a resumable directory. Good.
- **Gap.** A batch body goes to `tempfile::tempdir()` (`imports_api/mod.rs:1731`), the OS temp directory, up to 512 MiB, twice concurrently. See **R6**.

### 2.4 Network failures mid-import or mid-export

- **Retries.** `with_retries` (`crates/libs/http/src/retry.rs:127-148`): exponential from 500 ms, capped at 30 s, jitter from the clock; `classify_retry` treats 5xx, transport and unknown as transient and 4xx, 2xx-unreadable and `NotFound` as permanent (`:83-118`). The Upload passes `max_retries: 3` (`src-tauri/src/commands/upload.rs:154`); exports use `MAX_RETRIES` 3 (`crates/libs/export/src/run.rs:34`). Request timeouts are set per call: 15 s for `head_asset`, 600 s for the batch post and large uploads, 30 to 60 s elsewhere (`crates/libs/import/src/http.rs:146,202,273,315,348,391,440,473,490`).
- **Idempotency.** A replayed batch stores nothing twice because a guid the source already holds is skipped (`imports_api/mod.rs:1683-1688`; test `imports_api/tests.rs:3078-3083`). `complete`, `discard` and `cancel` are deliberately not repeatable; a run that has ended answers 409 and the client reads with `GET` (`docs/architecture/http-api.md:707-726`). Assets are content-addressed by SHA-256 (`asset_uploads.rs:3`), so a re-upload is safe.
- **What a failed batch does.** `apply_outcome` (`crates/libs/import/src/pipeline.rs:525-629`): on error it marks every conversation in the batch failed in the journal (`:601-623`) and the run goes on to the next batch. Only successes are journaled (`journal.rs:246-260,290-311`), so the next Upload re-sends failed conversations. See **R8**.
- **Backpressure.** `PrepareQueue` is a bounded `sync_channel` of `capacity` (`prepare.rs:984`, `DEFAULT_PREPARE_AHEAD` 3, `run.rs:89`) with `DEFAULT_PREPARE_WORKERS` 2 (`run.rs:91`); the result channel is unbounded (`:985`) but holds at most `capacity` results. One batch post is in flight at a time (`pipeline.rs:465-470`). Good.
- **Resume.** The web side records the run and resumes from the stage the server holds (`web/src/screens/import/runRecord.ts:9-36,205`), and a stage change the server did not record is a `StageNotRecordedError` that stops where the server last recorded (`web/src/screens/import/useImportJob.ts:882-886`). Good.

### 2.5 External programs

- **ffmpeg.** `run_ffmpeg_with` (`crates/libs/media/src/tools.rs:416-472`) drains stderr on a thread (fixes a real hang, `:375-379`) and polls `try_wait` only while a `stop` flag is given; with no flag it is `child.wait()` (`:457`). No deadline in either path. `probe_video` is `cmd.output()` with no deadline (`:549`). See **R3**.
- **Media pass.** Serial, oldest first (`media_queue.rs:267-305`), one Asset at a time, holds no pool connection while ffmpeg runs (`:244-245`). A failed Asset leaves the queue (`:292-304`); a hung one never does. On shutdown `ask_to_stop` kills ffmpeg (`:100-105`, `tools.rs:444-452`) and `stop` waits for the thread (`:112-126`). Without ffmpeg the pass leaves the queue in place (`:262-265`). Good degradation, wrong only for a hang.
- **Apple Messages Reader sidecar.** `Helper::spawn_at` drains stderr on a thread (`crates/libs/ios-backup/src/helper.rs:147-153`); an early exit is reported with status and stderr tail (`:299-310`); a broken pipe on send reads the unread error event first (`:254-297`); `Drop` kills and reaps (`:321-329`), so no orphan holds the backup open. No read timeout on `next_event` (`:180-214`), so a sidecar that hangs without exiting hangs the request; a Cancel cannot reach it (**Unable to verify** whether a cancel flag is checked around `next_event` by the caller; `crates/exporters/imessage-ir-exporter/src/run/` would show it).
- **Tool downloads.** Client has 30 s connect and 60 s read timeouts (`src-tauri/src/tool_downloads.rs:434-447`); checksum-gated replace (`:449-455`); one check at a time under a lock file (`lock_check`, `:789-801`); leftovers deleted at the next check (`:82-105`); an import waits with a 250 ms cancel tick (`wait_for`, `:686-732`). A network failure fails every wanted program at once and nothing retries until the next start or **Try again** (`:964-977`, `retry`, `:844-850`). See **R12**.

### 2.6 Server restart mid-run

- **Boot sequence** (`server.rs:1351-1444`): parent watch, tools dir, log, operation lock, seed-if-new, open, `recover_stopped_demo_build` (`server_api.rs:675-683`, removes the part-built account), `media_queue.start` (works through what a stopped server left), then bind and print `LISTENING_LINE`.
- **Import Runs.** An Import Run left `running` by a client or server that died is not touched at boot: `db/imports.rs` has no boot path, only the partial unique index that refuses a second running run (`:379-387`) and `discard_running_import` (`:679-698`). Staging rows are reset at the next batch (`imports_api/mod.rs:417`). The in-flight transaction of a killed batch rolls back in SQLite; the Upload retries it. Resumable by design; the cost is in **R13**.
- **Temp sweeping.** `.incoming/` 24 h, shard temps 1 h, media work directories 24 h, `.msgmedia.tmp.*` beside originals (`crates/libs/media/src/process.rs:242-291`), desktop scratch at start (`scratch.rs:64`). Good.

### 2.7 Backpressure and memory

- The staging write queue is built from every unit in memory (`write_queue.rs:3-5,506-539`), with writers clamped 1 to 8 (`:475-479`); this is F17 and is cited, not repeated. The import client bounds its own memory (2.4). The server streams a batch body to disk rather than memory (`server.rs:1775-1797`) and caps JSON bodies at `MAX_JSON_BODY_BYTES` 32 MiB (`server.rs:1014,1305`) and auth bodies at 32 KiB (`:1006,967`).
- The media queue is a table, so its depth is disk-bound, not memory-bound (`media_queue.rs:1-11`).
- FTS indexing runs once per batch inside the import transaction (`promote.rs:492-506`), which is efficient but is the longest part of the lock hold in **R1**.

### 2.8 Clock and time zone

- `format_local_ts` uses `chrono::Local` and falls back to UTC when a local time is ambiguous (`crates/libs/ir/src/lib.rs:913-918`; same shape in `crates/libs/mail/src/lib.rs:364-367`; `crates/libs/csv/src/zone.rs:53`). Run and export directory names use `chrono::Local::now()` at second or minute precision (`src-tauri/src/run_directories.rs:473`, `src-tauri/src/export_directories.rs:416`). Message ordering is by `timestamp_unix_ms`, not local time. Retry jitter is clock-derived (`retry.rs:150-158`), harmless. See **R14**.

### 2.9 Graceful shutdown

- `serve_until_shutdown` uses axum's `with_graceful_shutdown` (`server.rs:1455-1467`) on Ctrl-C, SIGTERM or parent-gone (`:1472-1514`); `on_signal` stops conversions first so a killed ffmpeg is not read as a bad file (`:1446-1454`); the demo build and media pass are then awaited (`:1434-1440`). Tested at `crates/server/server/src/server/tests.rs:1759-1763`. There is no deadline on the drain. The import semaphore is never closed, so the "server is shutting down" branch at `imports_api/mod.rs:1788` cannot fire, and a new batch accepted during the drain runs to completion.
- Docker: `tini` is PID 1 (`docker/Dockerfile:62,86`) and forwards SIGTERM; `docker/compose.yml:23` has `restart: unless-stopped` and no `stop_grace_period` or `healthcheck`; `docker/compose.release.yml` has neither restart nor healthcheck. No `HEALTHCHECK` in the Dockerfile. See **R2**, **R4**.

### 2.10 Web client offline or token expired mid-run

- `createQueryClient` (`web/src/lib/routeQuery.ts:68-93`): `staleTime` 30 s, one retry except on an ended session, `refetchOnWindowFocus`. `endsSession` is a 401 that is not `invalid-credentials` (`:54-56`). The health probe has an 8 s timeout and a 30 s backoff cap (`web/src/lib/serverHealth.ts:2-6,45-68`). `apiClient` passes a `signal` through but sets no default timeout (`web/src/lib/api.ts:161-183,198-209`). See **R10**.
- Token expiry mid-run: `endSessionIfRefused` reads the token before the call so a late refusal of an old token cannot end a new session (`useImportJob.ts:896-915`); `leaveIfRefused` returns to the form and leaves the run on the server (`:917-930`); logout pauses a running Upload first (`web/src/lib/runningUpload.ts:1-42`); the Rust side sets the cancel flag on refusal and keeps unsent conversations for the next Upload (`run.rs:203-212`, `prepare.rs:947-950`, `pipeline.rs:531-544`). Good.

## 3. Pattern checklist

| Pattern | State | Evidence |
|---------|-------|----------|
| HTTP request timeouts (client) | Present, per call | `import/src/http.rs:146-490`; `http/src/session.rs:93`; `tool_downloads.rs:444-445`; `local_server.rs:86,91` |
| HTTP request timeouts (server) | Absent | no `TimeoutLayer` in `server.rs` `http_app` (`:1305-1341`); body limits only |
| Database query timeouts | `busy_timeout` 15 s only | `engine.rs:20`; no statement timeout; sqlx default acquire timeout, not set in `engine.rs:52-57` |
| Long-running operation limits | Absent for ffmpeg, ffprobe, sidecar reads, import batches, drain | `tools.rs:416-458,549`; `helper.rs:180-214`; `imports_api/mod.rs:356-372`; `server.rs:1461-1466` |
| Retry with backoff and cap | Present | `retry.rs:127-148` |
| Retry limits | Present, fixed 3 | `commands/upload.rs:154`; `export/src/run.rs:34` |
| Idempotency | Present where it matters | guid skip (`imports_api/mod.rs:1683-1688`), content-addressed assets, non-repeatable `complete` by rule (`http-api.md:717-726`) |
| Circuit breaker | Absent | no consecutive-failure trip anywhere; session refusal is the one "stop the run" signal (`run.rs:203-212`) |
| Fallbacks | Present | no ffmpeg: queue waits (`media_queue.rs:262-265`); first seed fails: empty start (`reset_demo.rs:290-312`); WAL fails: warn and continue (`engine.rs:62-69`, see R1) |
| Bulkhead: thread pools | Present | media pass on its own thread and runtime (`media_queue.rs:71-96`); imports on `spawn_blocking` (`imports_api/mod.rs:1747`); prepare workers bounded (`prepare.rs:984`) |
| Bulkhead: connection pool | Weak | 4 connections, 2 reserved for imports by semaphore (`imports_api/mod.rs:1764-1771`); nothing reserved for reads |
| Bulkhead: cross-process | Present | operation lock (`operation_lock.rs:39-86`); tool check lock (`tool_downloads.rs:789-801`); process group kill (`local_server.rs:648-654`) |
| Health checks | Liveness only, unused by Docker | `server.rs:1516-1525`; `docker/Dockerfile`, `docker/compose.yml` |
| Graceful degradation | Present | `shown_as_is` decided without ffmpeg (`docs/architecture/media.md`, rule 2); log keeps writing when the database cannot open |
| Feature flags | Not applicable | none found; not needed for this product |

## 4. Findings

### R1. One import batch is one write transaction, and `busy_timeout` is shorter than a batch (8/10)

**What happens.** `import_jsonl_files_on_conn` holds `BEGIN IMMEDIATE` from `imports_api/mod.rs:356` to `:372` across staging, promote, FTS re-index and staging reset. A batch is up to 64 MiB of JSONL from the Upload (`import/src/run.rs:69`). Every other writer waits on `PRAGMA busy_timeout = 15000` (`engine.rs:20`) and then fails; the failure is a `sqlx::Error` that `server.rs:889-893` turns into a 500. The writers include login (`session_api.rs:53-56`), `media_queue::remove` (`media_queue.rs:304`, which then stops the whole pass, R7), `queue_import_run` (`:138-147`), `sweep_unreferenced` (`asset_store.rs:553`), asset completion, and every settings change. A second account's import batch also waits on the same lock and fails after 15 s, which the Upload retries after about a second (`retry.rs:141-144`), so two concurrent imports take turns in 15 s slices with 500s in between. The design audit's F7 left the transaction length as **Unable to verify**; this is the answer.

**Aggravator.** When WAL cannot be turned on, `try_enable_wal` warns once and continues (`engine.rs:62-69`); in rollback-journal mode readers wait on the import's lock too, so every page load of every account stalls for each batch. Nothing but one start-up line (`server.rs:1383-1386`) says which mode is in force.

**Remediation.**

1. Bound the batch so the hold is bounded. The client already splits by `batch_size` and `MAX_IMPORT_BODY_BYTES`; lower the byte cap:

```rust
// crates/libs/import/src/run.rs:69
pub const MAX_IMPORT_BODY_BYTES: usize = 16 * 1024 * 1024;
```

2. Give short writers a longer wait than the longest batch, and answer a busy database as a retryable 503 rather than a 500:

```rust
// crates/server/server/src/db/engine.rs:20
sqlx::query("PRAGMA busy_timeout = 120000").execute(&mut *conn).await?;

// crates/server/server/src/server.rs:889 (From<sqlx::Error> for ApiError)
if let sqlx::Error::Database(db) = &e
    && db.code().as_deref() == Some("5") /* SQLITE_BUSY */
{
    return Self::ServiceUnavailable { retry_after_secs: 5 };
}
```

   (`ServiceUnavailable` does not exist yet; `RateLimited { retry_after_secs }` at `server.rs:528-530` shows the shape.) The desktop `classify_retry` already treats 5xx as transient.

3. Refuse to serve in rollback-journal mode, or report the mode on `GET /v1/server` and Owner Home so the owner sees it. Minimal: make `try_enable_wal` return the mode and have `run` log at `warn` with the reason and the fix.

### R2. `/health` is liveness only, and nothing in Docker uses even that (7/10)

**What happens.** `get_health` answers `ok` without touching the pool (`server.rs:1516-1525`). A server whose 4 connections are all pinned, or whose write lock is held, answers `ok`. `docker/Dockerfile` has no `HEALTHCHECK`; `docker/compose.yml` and `docker/compose.release.yml` have no `healthcheck:`; `restart: unless-stopped` acts only on process exit. The desktop app's probe does touch the database through `GET /v1/server` (`server_api.rs:118-130`) with a 15 s `ANSWER_TIMEOUT`, and the web app's `checkServerHealth` uses `/health` with 8 s (`serverHealth.ts:45-68`), so the browser's "connected" light can be green while the database is unreachable.

**Remediation.**

```rust
// server.rs: a readiness route beside /health
pub(crate) async fn get_ready(State(state): State<AppState>) -> StatusCode {
    let probe = async {
        let mut conn = state.db.acquire().await.ok()?;
        sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&mut *conn).await.ok()
    };
    match tokio::time::timeout(std::time::Duration::from_secs(5), probe).await {
        Ok(Some(_)) => StatusCode::OK,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}
```

```dockerfile
# docker/Dockerfile, after EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=10s --start-period=300s --retries=3 \
  CMD ["node", "-e", "fetch('http://127.0.0.1:8080/ready').then(r=>process.exit(r.ok?0:1),()=>process.exit(1))"]
```

`--start-period=300s` matches the first start's Demo Account seed (`local_server.rs:77-79`). Point `serverHealth.ts:24,33` at `/ready` as well.

### R3. No wall-clock limit on ffmpeg or ffprobe; the pass restarts on the same Asset (7/10)

**What happens.** `run_ffmpeg_with` waits on `child.wait()` with no stop flag (`tools.rs:457`) or polls `try_wait` against `stop` alone (`:434-455`); `probe_video` uses `cmd.output()` (`:549`). A conversion that never ends (a corrupt container, a network-mounted file, a stuck encoder) holds the serial pass (`media_queue.rs:267-305`) for every account until the server stops. On restart the queue is read oldest first (`media_queue::first`, `:268`), so the same Asset is tried first and the pass wedges again. The design audit's F17 names the missing cap; the system consequence is a permanent, self-repeating stall of all Thumbnails and Previews. The desktop Media Stage uses the same runner.

**Remediation.**

```rust
// crates/libs/media/src/tools.rs: a deadline beside the stop flag
const FFMPEG_DEADLINE: Duration = Duration::from_secs(30 * 60);

fn run_ffmpeg_with(args: &[String], stop: Option<&AtomicBool>) -> Result<()> {
    // ...
    let deadline = std::time::Instant::now() + FFMPEG_DEADLINE;
    let status = loop {
        if let Some(status) = child.try_wait().context("wait for ffmpeg")? { break status; }
        if stopped() || std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            drop(reader);
            bail!(if stopped() { "stopped while ffmpeg ran" } else { "ffmpeg ran past the deadline" });
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(STOP_POLL_MAX);
    };
```

Poll `try_wait` in both branches so the no-stop path gets the deadline too. For `probe_video`, spawn and poll the same way rather than `output()`. A deadline turns a hang into the failure path at `media_queue.rs:292-304`, which removes the Asset from the queue, so the pass moves on and `process-assets` can retry it later.

### R4. Graceful shutdown has no drain deadline; Docker kills after 10 s (6/10)

**What happens.** `with_graceful_shutdown` (`server.rs:1461-1466`) waits for every request in flight with no cap. An import batch runs for minutes (R1). `docker stop` sends SIGTERM through `tini` (`Dockerfile:86`) and, with no `stop_grace_period` in `docker/compose.yml`, sends SIGKILL after Docker's default 10 s. The batch transaction is lost (SQLite rolls it back on next open), the Upload sees a transport error and retries, and `restart: unless-stopped` brings the container back. Data stays consistent; the cost is a lost batch, a retry storm against a starting server, and a confusing log. The import semaphore is never closed, so a batch that arrives during the drain is accepted (`imports_api/mod.rs:1785-1788`). Under the desktop app the server is killed outright by design (ADR 0018), so this concerns Docker and a service manager.

**Remediation.**

```yaml
# docker/compose.yml and docker/compose.release.yml, under services.server
stop_grace_period: 5m
```

```rust
// server.rs: refuse new batches once the stop is requested
// in `on_signal`: import_semaphore().close();
// imports_api/mod.rs:1785-1788 already answers "server is shutting down" on a closed semaphore
```

Pair the longer grace with a server-side drain deadline (for example 4 min) that logs what was still running and exits, so the container never waits on SIGKILL.

### R5. The app's own server is reported dead, not restarted (6/10)

**What happens.** `Phase::Own` + `Event::ChildExited` yields `Phase::Failed` with no action (`local_server.rs:418-424`). The login card shows "Message Crate stopped." and a manual retry (`useLocalServer.ts:29,74`); polling stops after `failed` (`:55`). A hang (listening, never answering) is not detected at all, because `Event::Check` probes only `Phase::Found` (`:446-451`). There is no crash-loop guard because there is no loop; the trade-off chosen is a manual retry. A person mid-import loses the run's connection; the run itself is resumable (2.4).

**Remediation.** Add a bounded automatic restart to `step`, and a periodic probe of `Phase::Own`:

```rust
// local_server.rs: in State
restarts: VecDeque<Instant>,  // starts within the last RESTART_WINDOW

const RESTART_WINDOW: Duration = Duration::from_secs(120);
const RESTARTS_ALLOWED: usize = 3;

// in step, Event::ChildExited while Phase::Own:
state.restarts.retain(|t| t.elapsed() < RESTART_WINDOW);
if state.restarts.len() < RESTARTS_ALLOWED {
    state.restarts.push_back(Instant::now());
    state.phase = Phase::Starting { open };
    actions.push(Action::Spawn { open });
} else {
    state.phase = Phase::Failed { /* as today, message: "Message Crate stopped three times in two minutes." */ };
}
```

`step` is pure (`:330-332`), so this is testable without a process. For the hang: let `Event::Check` probe `Phase::Own` too, and on `Probe::Free` or `Probe::Other` treat it as `ChildExited` with `Action::Kill` first.

### R6. The batch body lives in the OS temp directory, and the import's work outlives its request (6/10)

**What happens.** `create_import_batch` writes the body to `tempfile::tempdir()` (`imports_api/mod.rs:1731-1739`), which is `std::env::temp_dir()`: `/tmp` in the container (the writable layer, not the `/app/data` volume) and the user's temp directory on a desktop. Up to `MAX_REQUEST_BODY_BYTES` 512 MiB (`server.rs:1021`) per batch, two at once. A small or tmpfs `/tmp` fails the write with a 500 (`server.rs:1789-1791`), and `check_headroom` (the staging crate's) is not applied here. Separately, the import runs in `spawn_blocking` with `block_on` (`:1747-1748`); when the client's 600 s timeout fires (`import/src/http.rs:273`) or the connection drops, the handler future is dropped but the blocking task is not cancelled: it keeps its import permit (`:1785`), the account lock (`:1798`) and a pool connection (`:1802`) until it finishes. The dropped handler also drops `temp` (`:1752`), so a task still waiting for its permit finds its JSONL file gone and fails after the wait. The Upload's retry then queues behind the orphan. Consistency holds (guid skip); capacity and disk do not. **Unable to verify** without a test that hyper drops the handler future on a reset connection for this route; `crates/server/server/src/server/tests.rs` is the place to show it.

**Remediation.**

```rust
// imports_api/mod.rs:1731
let batches_dir = state.cfg.paths.data_dir.join(".incoming-batches");
tokio::fs::create_dir_all(&batches_dir).await.map_err(|e| ApiError::Internal(anyhow::anyhow!("mkdir {}: {e}", batches_dir.display())))?;
let temp = tempfile::Builder::new().prefix("batch-").tempdir_in(&batches_dir)
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("temp dir: {e}")))?;
```

Move the `TempDir` into the blocking closure so it lives as long as the work. Sweep `.incoming-batches/` for directories older than an hour at boot, beside the media work directories (`process_assets.rs:417-441`).

### R7. Any database error stops the media pass until the next Import Run ends (5/10)

**What happens.** `work_through` returns `Err` when `first`, `remove` or `count` fails (`media_queue.rs:257,268,304`), including a `SQLITE_BUSY` after 15 s during an import batch (R1). The thread logs a warning and waits on `wake.notified()` (`:83-92`); nothing wakes it until another Import Run completes or is discarded (`:132-147`). On a server that imports once and then only reads, the queue sits unprocessed until restart.

**Remediation.**

```rust
// media_queue.rs:82-93
let mut backoff = Duration::from_secs(5);
while !stop.load(Ordering::Relaxed) {
    match work_through(&pool, &cfg, &stop).await {
        Ok(_) => { backoff = Duration::from_secs(5); wake.notified().await; }
        Err(error) => {
            tracing::warn!(error = format!("{error:#}"), "The pass stopped; it tries again in {}s", backoff.as_secs());
            tokio::select! { () = wake.notified() => {}, () = tokio::time::sleep(backoff) => {} }
            backoff = (backoff * 2).min(Duration::from_secs(300));
        }
    }
}
```

### R8. The Upload's retry policy is fixed, short, and has no breaker (5/10)

**What happens.** Three retries (`commands/upload.rs:154`) with 500 ms, 1 s, 2 s waits plus jitter (`retry.rs:141-144`) give up after about 4 s of outage. `apply_outcome` then marks every conversation in the batch failed in the journal (`pipeline.rs:601-623`) and `flush` sends the next batch, which fails the same way. A Wi-Fi drop of a minute during a 10 000-conversation Upload therefore records thousands of failures, and the person must start another Upload (which does re-send them, since only successes are journaled). Prepare workers keep trying asset `HEAD` and `PUT` meanwhile (`prepare.rs:899,913`). The session-refusal path shows the right shape: it sets the cancel flag and leaves unsent work for the next Upload (`run.rs:203-212`, `prepare.rs:947-950`).

**Remediation.** Treat N consecutive transport failures as a pause, not as failures:

```rust
// pipeline.rs, in Importer
const TRANSPORT_TRIP: u32 = 3;
consecutive_transport_failures: u32,

// in apply_outcome, Err branch, before marking conversations failed:
if error.transient {               // BatchError already knows the class; add the flag from classify_retry
    self.consecutive_transport_failures += 1;
    if self.consecutive_transport_failures >= TRANSPORT_TRIP {
        out.log_at(RunLogLevel::Warn, "The server could not be reached three times in a row, so the Upload pauses here.");
        self.session.stop.cancel();   // same path a refused session takes; conversations stay for the next Upload
        return Ok(false);
    }
} else { self.consecutive_transport_failures = 0; }
```

And raise the backoff ceiling for the last retry (for example 15 s) so a short outage does not trip at all.

### R9. The pool is four connections, two of them reserved for imports, with sqlx's default acquire timeout (5/10)

**What happens.** `max_connections(4)` (`engine.rs:56`) and no `acquire_timeout` (sqlx 0.8.6 per `Cargo.lock:3357-3358`; its default is 30 s, **Unable to verify** in-repo). Two imports pin two connections for their whole runs (`imports_api/mod.rs:1764-1802`); `sweep_unreferenced` and `unless_import_running` take a connection each in their own tasks (`asset_store.rs:419,526`); `GET /v1/server` takes one (`server_api.rs:119`). During two imports and a sweep, every read of every account shares the fourth. `PoolTimedOut` becomes a 500 (`server.rs:889-893`), which the desktop probe correctly reads as "a Message Crate, busy" (`local_server.rs:150-156`) but the browser shows as an error. F7 asked whether a dedicated import connection is worth it; with the lock hold measured in R1, it is.

**Remediation.**

```rust
// engine.rs:52-57
with_pragmas(SqlitePoolOptions::new()
    .max_connections(8)
    .acquire_timeout(std::time::Duration::from_secs(10)))
```

and map `sqlx::Error::PoolTimedOut` to the 503 of R1. Longer term, open the two import connections outside the pool (`SqliteConnection::connect_with`) so imports cannot touch the request pool at all.

### R10. `apiClient` has no default fetch timeout (4/10)

**What happens.** `request` and `requestRaw` (`api.ts:161-215`) call `fetch` with the caller's `signal` or none. A server that accepts the connection and never answers (R1's lock hold, a stalled container) hangs the query until the browser's own limit. TanStack retries once (`routeQuery.ts:88`). Only the health probe has a timeout (`serverHealth.ts:50-51`).

**Remediation.**

```ts
// api.ts: in request() and requestRaw()
const DEFAULT_TIMEOUT_MS = 60_000;
const timeout = AbortSignal.timeout(DEFAULT_TIMEOUT_MS);
const res = await fetch(`${baseUrl}${path}`, {
  method, headers, body,
  signal: signal ? AbortSignal.any([signal, timeout]) : timeout,
});
```

Give the import-stage calls in `useImportJob.ts` a longer explicit signal where they need one.

### R11. No per-request timeout or concurrency limit on the server (3/10)

**What happens.** `http_app` (`server.rs:1305-1341`) applies body limits, CORS, tracing and request ids; no `tower_http::timeout::TimeoutLayer`, `ConcurrencyLimitLayer` or `LoadShedLayer`. A slow or stuck client holds a hyper task indefinitely; many of them hold the pool (R9). For a self-hosted server on a LAN the exposure is low; ADR text in `docs/architecture/http-api.md:473` puts internet exposure behind a proxy.

**Remediation.**

```rust
// server.rs http_app: before the body limit layer, on every route but the batch and asset uploads
.layer(tower_http::timeout::TimeoutLayer::new(std::time::Duration::from_secs(120)))
```

Apply it to the router that excludes `/v1/imports/{id}/batches` and the asset `PUT` routes, which have their own 600 s client timeouts.

### R12. Tool downloads try once per start (3/10)

**What happens.** `check` fails every wanted program on the first `NoNetwork` (`tool_downloads.rs:964-986`) and nothing tries again until the next app start or **Try again** (`retry`, `:844-850`). An app opened on a laptop before Wi-Fi connects has no ffmpeg until it is restarted or the person finds the button. ADR 0019 accepts "says so only where the program is needed"; a single delayed retry would remove the common case.

**Remediation.**

```rust
// tool_downloads.rs check(): wrap the download in a short retry for NoNetwork only
let mut attempt = 0;
let result = loop {
    match download(&client, &pin.url(base), pin, dir, &progress) {
        Err(DownloadError::NoNetwork(why)) if attempt < 2 => {
            attempt += 1;
            std::thread::sleep(Duration::from_secs(30 * attempt));
            let _ = &why;
        }
        other => break other,
    }
};
```

### R13. A `running` Import Run whose client vanished is never aged out (3/10)

**What happens.** No boot or periodic path touches `imports.status = 'running'`; the partial unique index refuses a new run with a message that sends the person to the desktop app or the CLI (`db/imports.rs:379-387`). This is the resumable-runs rule working as written (`http-api.md:707-716`). The cost is that an API-token client that crashed leaves its account unable to import until someone runs `message-crate-server imports discard`. An owner has no HTTP route to do it for another account (**Unable to verify**: `crates/server/server/src/owner_api.rs` or the imports routes would show one).

**Remediation.** Do not auto-discard (it would break resume). Show the age of a running run on Owner Home and on the account's Import screen with a **Discard** action, and log a warning at boot naming runs older than a day:

```sql
SELECT id, account_id, started_at FROM imports
WHERE status = 'running' AND started_at < datetime('now', '-1 day');
```

### R14. `chrono::Local` in output and directory names (2/10)

**What happens.** `format_local_ts` and its twins depend on the machine's zone and fall back to UTC on an ambiguous instant (`ir/src/lib.rs:913-918`, `mail/src/lib.rs:364-367`, `csv/src/zone.rs:53`). Run directory names are `%y%m%d-%H%M%S` local (`run_directories.rs:473`), export names `%Y-%m-%d-%H%M` (`export_directories.rs:416`); at a DST fall-back the same local time recurs an hour later. **Unable to verify** whether a name collision is handled; `run_directories.rs` around `:473` would show it. Message ordering is by `timestamp_unix_ms` and is not affected.

**Remediation.** Name directories in UTC (`chrono::Utc::now()`) or append a counter when the directory exists.

### R15. A poisoned `KeyedLocks` map refuses every later import (2/10)

**What happens.** `lock`, `len` and `Drop` call `.expect("keyed lock map poisoned")` (`keyed_locks.rs:41,56,62`). The std mutex is held only for a map lookup, so poisoning needs a panic inside `entry().or_default().clone()`, which is close to impossible. If it did happen, every import and asset completion would panic with a 500 until restart. Sibling territory (panics); noted here only for the system effect.

**Remediation.** `unwrap_or_else(PoisonError::into_inner)`, the pattern `local_server.rs:699-703` already uses.

## 5. What is done well

- Durable, resumable work everywhere: journal with torn-row isolation (`journal/src/lib.rs:47-99`), complete-file resume in staging (`write_queue.rs:6-10,459-464`), media queue as a table (`media_queue.rs:1-22`), stopped demo build removed at boot (`reset_demo.rs:435-452`), swap with rollback and backups kept when rollback fails (`reset_demo.rs:1947-1994`).
- `BEGIN IMMEDIATE` as the only transaction, with the schema reload that fixed #1600 (`write_tx.rs:1-61`), and a Clippy rule that keeps deferred transactions out.
- Shutdown that stops conversions before draining so a killed ffmpeg is not read as a bad file (`server.rs:1446-1454`), and `--exit-with-parent` that silences stdio before the drain (`exit_with_parent.rs:20-25,115-132`).
- Stderr drained on a thread for every child process (`tools.rs:375-379,433`, `helper.rs:147-153`, `local_server.rs:924-947`), which is the hang that actually happened (#1178).
- The desktop probe treats a 5xx problem document as a Message Crate (`local_server.rs:142-159`), so a busy server is not mistaken for a port clash.
- Pinned checksums and replace-after-verify for downloaded programs (`tool_downloads.rs:13-23`).
- Session refusal mid-run stops the run and keeps unsent work (`run.rs:203-212`, `prepare.rs:947-950`, `useImportJob.ts:896-930`).
- The log keeps writing when the database cannot open, and loses only the lines a full disk cannot hold (`logging/files.rs:122-133`, `server.rs:1360-1368`).
- `check_headroom` refuses an impossible write up front and never blocks on a filesystem that cannot report free space (`headroom.rs:84-92`).

## 6. Fault-injection tests present

| Fault | Test |
|-------|------|
| SIGTERM with a request in flight | `crates/server/server/src/server/tests.rs:1759-1763` |
| Parent process gone, with and without the flag | `crates/server/server/tests/exit_with_parent.rs:220,247` |
| Batch replayed after a 504 | `crates/server/server/src/imports_api/tests.rs:3078-3083` |
| Server child exits while starting; operation lock held by another; stop kills the tree; a server that exits takes its processes | `src-tauri/src/local_server/tests.rs:608,627,657,685,712` |
| Slow or busy Message Crate at the probe | `src-tauri/src/local_server/tests.rs:504` |
| Schema changed by another connection while one idled; a write landing between read and write (`commit_during`) | `crates/server/server/src/db/write_tx.rs:102-109,124` |
| Rename fails mid-swap | `reset_demo.rs:1824-1837` (`install_reset_state_with`), `crates/server/server/src/reset_demo/tests.rs` |
| Journal append under contention | `crates/libs/journal/src/lib.rs:583` |
| Killed request leaves scratch | `crates/core/message-crate-core/src/scratch.rs:228,289` |
| Upload removed under its completion; abort while a part is written | `crates/server/server/src/asset_uploads.rs:760,775,791,805` |
| Disk without room | `crates/libs/staging/src/headroom.rs:196` |
| Cancel during export | `crates/exporters/imessage-ir-exporter/tests/cancel.rs` |

Not found: a test that holds the write lock longer than `busy_timeout` and asserts what a login gets (R1); an ffmpeg that never exits (R3); `ENOSPC` on an asset or batch write (R6); a transport outage mid-Upload asserting what the journal records (R8); the app's own server exiting while `Own` (R5); a client that disconnects during a batch (R6).

## 7. Unable to verify

| Claim | File that would prove it |
|-------|--------------------------|
| sqlx 0.8.6's default `acquire_timeout` is 30 s and `PoolTimedOut` is the error (R9) | `vendor/` or the sqlx-core source; out of scope here |
| hyper drops the handler future, and with it `temp`, when the client resets during a batch (R6) | a test in `crates/server/server/src/server/tests.rs` that resets the connection mid-body |
| whether the server pool is opened through `open_pool_for_path` so `try_enable_wal` applies to `serve` (R1) | `crates/server/server/src/open_db.rs` (`OpenDb::create_or_open`) |
| whether a cancel flag is checked around `Helper::next_event` so a hung sidecar can be stopped (2.5) | `crates/exporters/imessage-ir-exporter/src/run/` |
| whether an owner route can discard another account's running Import Run (R13) | `crates/server/server/src/owner_api.rs` or the imports routes |
| whether a run directory name collision at a DST fall-back is handled (R14) | `src-tauri/src/run_directories.rs` around `:473` |
| Docker's stop grace in the published compose (R4): the file sets none, so Docker's default 10 s is assumed | `docker/compose.yml` (confirmed absent); the default is Docker's, not the repo's |

## 8. Suggested order of work

1. **R3** ffmpeg and ffprobe deadline (one function, turns a permanent stall into a logged failure).
2. **R1** smaller batches plus longer `busy_timeout` plus a 503 for busy; report the journal mode.
3. **R2** readiness route and Docker `HEALTHCHECK`; **R4** `stop_grace_period` and closing the import semaphore on signal.
4. **R7** media pass retries with backoff; **R9** pool size and acquire timeout.
5. **R5** bounded automatic restart in `step`, with a test beside the existing ones.
6. **R8** transport breaker that pauses the Upload; **R6** batch temp directory under the Data Directory.
7. **R10**, **R11**, **R12**, **R13**, **R14**, **R15** as small follow-ups.
