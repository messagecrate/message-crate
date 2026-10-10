# Logging and monitoring audit

- **Audit:** logging-monitoring (code-audit skill 0.1.0)
- **Date:** 2026-10-10
- **Commit:** `530dc1791`
- **Scope:** the whole repository less `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`. Read-only; nothing was built or run.
- **Judged against:** the skill's checklist, and the repository's own written rules: `docs/architecture/server-log.md`, `docs/architecture/import-run-logs.md`, `docs/adr/0020-the-audit-trail-outlives-the-account.md`, `docs/architecture/http-api.md` ("Failures", "Credentials and reach").
- **Cross-reference:** `audits/2026-10-09-initial-software-design-analysis.md` (design audit), cited as D-F1, D-F10, D-F13.

## 1. Summary

**Risk score: 3.5 / 10 (low to moderate).**

Logging is a designed subsystem, and the design holds up in the code. One line format (`crates/libs/log-lines`) serves the server's log and the desktop app's Import Run logs; every request carries a server-made `request_id` into its span and its problem body; query values are hidden by default and shown only from an allow-list of ten names; a 500's error chain is logged once, server-side, and the client gets a constant sentence; log files rotate by size with a full-disk path that keeps every line whole; the three log routes take the owner's Session and nothing else; and an integration test (`crates/server/server/tests/server_log.rs`) runs the server at `RUST_LOG=trace` and fails if a password, a token or its hash, message text, attachment bytes, or a contact's name, phone or email reaches any log file. The Audit Trail (`db/audit_trail.rs`) records logins, refusals, sessions, tokens, permissions, deletions, address book loads, registration changes and account lifecycle, and outlives the account.

The gaps are around the edges, not the core:

1. An unauthenticated client can push the owner's log history out of the 250 MB window because every response, including `/health` and static files, writes one line that repeats the request's raw path and query names, and nothing caps that line's length (F1).
2. No `WARN` fires on a refused login or a rate-limit refusal, so the server log's record of a brute-force attempt is a run of `INFO` lines with `status=401`; detection means a person reading the Audit Trail (F2).
3. Log files and directories take the process umask; nothing narrows them (F3).
4. Only LF and CR are escaped on the way into a line; other control bytes pass through (F4).
5. Background tasks lose the request span, and audit entries carry no request id, so a warning from the media queue cannot be tied to the request that caused it (F5).
6. The owner's attachment-size-limit change and a Demo Account rebuild leave no Audit Trail entry (F6).

Severity counts: **Critical 0, High 0, Medium 2, Low 7, Informational 2.**

## 2. What the subsystem is (as read)

| Piece | Code | Rule |
|---|---|---|
| Line format: writer, parser, filter, backward walk | `crates/libs/log-lines/src/lib.rs` (`format_lines` :80, `parse_line` :111, `LineFilter` :127, `lines_backward` :160) | `server-log.md` "One line per event" |
| Server subscriber: stderr plus rotating files | `crates/server/server/src/logging.rs` (`init` :48, `SERVER_LOG` :38, `write_files_in` :70, `EventWriter` :127) | `server-log.md` |
| Rotation, full disk, newline escaping | `crates/server/server/src/logging/files.rs` (`SERVER_LOG_LIMITS` :27, `write_event` :101, `trim` :149, `one_line` :221) | `server-log.md` "Trimmed by size only" |
| Reading back: ids, pages, files | `crates/server/server/src/logging/read.rs` (`read_lines` :78, `list_files` :131) | `server-log.md` "Reading it" |
| Request id | `crates/server/server/src/request_id.rs` (`layer` :34, `current` :28) | `http-api.md` "Failures" |
| Per-request line and redaction | `crates/server/server/src/server.rs` (`LOGGED_QUERY_VALUES` :1194, `logged_uri` :1225, `TraceLayer` :1323-1336, `to_problem` :646-665) | `server-log.md` "What a line never holds" |
| Log routes, owner only | `crates/server/server/src/server_api/log_lines.rs` (`list_log_lines` :71), `server_api/log_files.rs` (`list_log_files` :44, `get_log_file` :74) | `server-log.md` "The owner reads it" |
| Audit Trail | `crates/server/server/src/db/audit_trail.rs` (`AuditAction` :44-85, `insert` :573, `trim_refused_logins` :743, `prepare_account_deletion` :774), `audit_trail_api.rs`, `schema/sql/accounts.sql:407-468` | ADR 0020 |
| Credentials, rate limiter | `crates/server/server/src/credentials.rs` (`AUTH_RATE_WINDOW`/`AUTH_RATE_MAX` :22-23, `check_auth_rate_limit` :54), `session_api.rs` (`create_session` :151) | `http-api.md` :690-704 |
| Desktop Import Run logs | `crates/core/message-crate-core/src/run_log.rs` (`RunLogAccount` :117, `format_run_log` :81), `src-tauri/src/app_directories.rs` (`RunLog` :84, `write` :138), `src-tauri/src/run_logs.rs` (`Reader::reads` :57, `readable_log` :249), `src-tauri/src/commands/run_logs.rs` | `import-run-logs.md` |
| Desktop events and child server output | `src-tauri/src/commands/events.rs` (`ExtractErrorEvent` :226, `run_log_sink` :255), `src-tauri/src/local_server.rs` (`spawn_server` :876, `keep_output` :926, `OUTPUT_LINES_KEPT` :94) | ADR 0018 |
| Web log viewer, API client | `web/src/components/logs/logSource.ts`, `web/src/lib/api.ts` | ADR 0002 |

Assumption: the server is reached either on `127.0.0.1` from the desktop app or through a TLS reverse proxy of the operator's own (per the memory rule "Login is not internet security"). Findings that need network reach are rated with that in mind.

## 3. Findings

### F1. Unauthenticated requests can push the owner's log history out of the 250 MB window

- **Severity:** Medium
- **CWE:** CWE-400 (Uncontrolled Resource Consumption); CWE-778 (Insufficient Logging), because the effect is lost evidence
- **Evidence:**
  - `crates/server/server/src/server.rs:1323-1336`: `TraceLayer::new_for_http()` with `DefaultOnResponse::new().level(Level::INFO)` writes one `INFO` line per response. The span (`:1325-1333`) carries `method`, `uri = %logged_uri(request.uri())` and `request_id`.
  - `server.rs:1302`: `.fallback_service(ServeDir::new(static_dir))` is inside that layer, as is `/health` (`:1291`), so every static 404 and every probe is a line.
  - `server.rs:1225-1248` `logged_uri`: keeps the whole path and, for the query, every parameter *name* as written (`format!("{name}=[hidden]")`); only values are replaced. There is no length cap.
  - `server.rs` has body limits only (`RequestBodyLimitLayer` :967, `DefaultBodyLimit` :1305); no request-line or header-size setting is passed to `axum::serve` (`:1461`), so hyper's default applies (see Unable to verify).
  - `logging/files.rs:27-30` `SERVER_LOG_LIMITS`: 5 files of 50,000,000 bytes; `files.rs:149-160` `trim` deletes the oldest when a sixth starts.
  - No credential is needed: `/health` (`server.rs:1523`), `GET /v1/server` (`server_api.rs:118`), `POST /v1/session` (`session_api.rs:151`) and the static fallback all answer anonymously.
- **Why it matters:** The log is the only record of 500s, media-queue failures, schema rebuilds and the server's own warnings (the Audit Trail does not hold them). A client that sends a few hundred to a few thousand requests with a long path or many long query-parameter names rolls the four older files off disk, and with them the evidence of whatever came before. The rotation bounds the disk, which `server-log.md` sets out to do; it does not bound who decides what the 250 MB holds.
- **Exploitability:** Reachable by anyone who can reach the port. Reproduction, against a dev server with no secrets: `for i in $(seq 1 2000); do curl -s -o /dev/null "http://127.0.0.1:8080/$(head -c 200000 /dev/zero | tr '\0' a)"; done`, then `ls -l data/logs/`: `server-000001.log` is gone and the newest files hold only the flood. (The exact per-request size depends on hyper's request-line limit; see section 6.)
- **Remediation:**
  1. Cap the logged URI in `logged_uri`:
     ```rust
     // server.rs, end of logged_uri
     const LOGGED_URI_MAX: usize = 2048;
     fn capped(mut s: String) -> String {
         if s.len() > LOGGED_URI_MAX {
             let cut = s.floor_char_boundary(LOGGED_URI_MAX);
             s.truncate(cut);
             s.push_str("…[cut]");
         }
         s
     }
     ```
     and return `capped(...)` from both arms.
  2. Keep `/health` and successful static answers out of the `INFO` stream: replace `DefaultOnResponse` with an `on_response` closure that logs at `DEBUG` when the span's `uri` is `/health` or the status is `2xx` on the fallback, and at `INFO` otherwise. The web app probes `/health` every 30 s per open tab (`web/src/lib/useServerHealth.ts:54`, `HEALTH_SUCCESS_RECHECK_MS`), so this also removes 2,880 lines a day per tab from the owner's view.
  3. Defense in depth: add a test beside `logging/tests.rs` that writes one event of 1 MB and asserts the stored line is at most the cap; and note in `server-log.md` that a line is cut at 2 KB of URI.

### F2. Refused logins and rate-limit refusals produce no `WARN`; brute force is visible only by reading the Audit Trail

- **Severity:** Medium
- **CWE:** CWE-778 (Insufficient Logging); CWE-223 (Omission of Security-relevant Information)
- **Evidence:**
  - `crates/server/server/src/session_api.rs` and `credentials.rs` contain no `tracing::` call at all (grep count 0). The three `record_refused_login` calls (`session_api.rs:190`, `:212`, `:227`) write Audit Trail rows only.
  - `session_api.rs:167-169`: a rate-limited attempt is refused before anything is checked, "and the Audit Trail records nothing of it". It leaves one `INFO` line with `status=429`.
  - The per-response line (`server.rs:1323-1336`) carries `method`, `uri`, `request_id`, `status`, `latency`: no account id, no reason, and by design no address (`http-api.md:702-706` rejects believing a forwarded address).
  - `server-log.md` "Reading it": the Owner Home Logs panel "shows the log as it was when it was read"; nothing polls, counts or alerts. No metrics or alerting dependency exists (grep for `prometheus|metrics|opentelemetry|statsd` in `crates/server/server` returns nothing).
  - `get_health` (`server.rs:1523-1525`) answers the constant `ok\n`: liveness only, no database, disk or log-directory check.
- **Why it matters:** The rate limiter (`credentials.rs:22-23`: 20 attempts per 60 s per account bucket) slows a guesser to 28,800 guesses a day per account, which is enough for a short password list. The Audit Trail records each refusal with username, reason and connecting app (`db/audit_trail.rs:711-732`), so the data exists, but the owner's log, the thing `server-log.md` says is read "when it fails", shows nothing above `INFO`, and the Logs panel opens at warnings and up. A sustained attack looks like traffic.
- **Exploitability:** Not an exploit in itself; it lengthens the window in which one goes unnoticed.
- **Remediation:**
  1. One `WARN` per refusal, with structured fields and no secrets (the username is validated to `[A-Za-z0-9_.-]{1,128}` at `credentials.rs:245-252` for live accounts; for an unknown username log the bucket, not the text):
     ```rust
     // session_api.rs, beside each record_refused_login
     tracing::warn!(
         account_id = account.map(|(id, _)| id),
         reason = reason.as_str(),
         app = app.map(|a| a.kind.as_str()),
         "A login was refused"
     );
     ```
  2. One `WARN` when a bucket first trips, inside `refuse_when_full` (`credentials.rs:125-137`), with the bucket name and `retry_after_secs`; it fires at most once per window per bucket because the bucket stays full until the oldest hit leaves.
  3. Defense in depth: a `GET /v1/server/health-detail` for the owner (database opens, log directory writable, free bytes), or extend `GET /v1/server/storage` (`server_api.rs:387`) with the log directory's bytes. Keep `/health` constant for probes.

### F3. Log files and the Logs Directory are created with the process umask

- **Severity:** Low
- **CWE:** CWE-732 (Incorrect Permission Assignment for Critical Resource)
- **Evidence:**
  - `crates/server/server/src/logging/files.rs:215-217` `open_append`: `OpenOptions::new().create(true).append(true)`; `files.rs:65` `fs::create_dir_all(dir)`. No mode is set.
  - `src-tauri/src/app_directories.rs:95-96`: `create_dir_all(logs_dir)` then `File::options().create(true).append(true)`. No mode.
  - grep for `set_permissions|mode(0o|PermissionsExt|umask` in `crates/server/server/src`, `src-tauri/src`, `crates/core`, `crates/libs` finds only test files.
  - `docker/Dockerfile:81` runs as `USER node`; `docker/compose.yml:17` sets `user: "${UID:-1000}:${GID:-1000}"`. The volume's files therefore take the host umask of that uid.
  - `import-run-logs.md` "What a line holds": a run log "may carry a conversation's identities in the file names it gives", such as `+15555550101.jsonl`.
- **Why it matters:** With the common umask `022`, files are `0644` and directories `0755`. On a Linux desktop shared by more than one operating-system user, or a Docker host where the volume path is browsable, another user reads the server's log (every account's routes and ids) and the run logs (phone numbers and email addresses in file names). `import-run-logs.md` "A filter, not a guard" accepts that the *same* user can read them with or without Message Crate; it does not speak to other users.
- **Exploitability:** Needs a second local account on the same machine. Low likelihood on a single-user desktop.
- **Remediation:**
  ```rust
  // logging/files.rs
  fn open_append(path: &Path) -> io::Result<File> {
      let mut options = OpenOptions::new();
      options.create(true).append(true);
      #[cfg(unix)]
      {
          use std::os::unix::fs::OpenOptionsExt as _;
          options.mode(0o600);
      }
      options.open(path)
  }
  // in LogFiles::open, replace fs::create_dir_all(dir)? with:
  let mut builder = fs::DirBuilder::new();
  builder.recursive(true);
  #[cfg(unix)]
  {
      use std::os::unix::fs::DirBuilderExt as _;
      builder.mode(0o700);
  }
  builder.create(dir)?;
  ```
  The same two changes in `app_directories.rs:95-96`. The server already opens the database file the same way, so this matches the rest of the Data Directory only if `open_db.rs` does too (not checked here; outside this audit's scope).

### F4. Control bytes other than LF and CR pass into a line unescaped

- **Severity:** Low
- **CWE:** CWE-117 (Improper Output Neutralization for Logs)
- **Evidence:**
  - `crates/server/server/src/logging/files.rs:221-236` `one_line`: escapes `\n` and `\r` as the two characters `\n`; every other byte is copied.
  - `crates/libs/log-lines/src/lib.rs:80-89` `format_lines`: splits on `str::lines()` (LF and CRLF), so a lone `\r` stays inside a line; `lines_backward` (`:167`) strips only a *trailing* `\r`.
  - Attacker-influenced text that reaches lines: the whole output of a failed `wtsexporter` run at `WARN` (`src-tauri/src/commands/events.rs:271-273`, documented in `import-run-logs.md`); error chains that quote file names from a backup (`asset_store.rs:176-181` `path = %path.display()`, `%error`); `error = %error_chain(err)` on every 500 (`server.rs:651`). The request path and query names are safe: percent-encoded on the wire, and hyper rejects raw control bytes in the request line.
- **Why it matters:** A `\x1b[` sequence or a `\x08` run in a downloaded `server-000001.log` or `import-*.log` rewrites what the owner sees in a terminal (`cat`, `less -r`, `tail`). The web viewer renders lines as React text nodes (`web/src/components/logs/`), so it is not affected. The escape that exists is for the line-per-event invariant, not for display safety.
- **Exploitability:** Needs a crafted file name in a backup, or a crafted `wtsexporter` failure. Low.
- **Remediation:** escape every byte below 0x20 (except the LF/CR handling already there) and 0x7f in `one_line`, and run the same pass in `format_lines`:
  ```rust
  // files.rs one_line, inner match
  b'\r' if bytes.peek() == Some(&&b'\n') => {}
  b'\n' | b'\r' => line.extend_from_slice(b"\\n"),
  b if b < 0x20 || b == 0x7f => line.extend_from_slice(format!("\\x{b:02x}").as_bytes()),
  _ => line.push(b),
  ```
  ```rust
  // log-lines lib.rs, before the for loop in format_lines
  let text: String = text.chars().map(|c| if c.is_control() && c != '\n' { '\u{FFFD}' } else { c }).collect();
  ```
  Add a case to `logging/tests.rs:105` (`a_line_break_inside_an_event_is_written_as_backslash_n`) for `\x1b`.

### F5. Background tasks lose the request span, and audit entries carry no request id

- **Severity:** Low
- **CWE:** CWE-778 (Insufficient Logging), as a correlation gap
- **Evidence:**
  - `request_id.rs:21-24`: the id is a `tokio::task_local!`, scoped by `REQUEST_ID.scope(id, next.run(request))` (`:38`). It does not cross `tokio::spawn`.
  - `tokio::spawn` without `.instrument(...)` or `in_current_span()` at `asset_store.rs:418`, `:525`, `server_api.rs:553`, `keyed_locks.rs:93`; grep for `.instrument(` or `Span::current` in `crates/server/server/src` finds nothing.
  - The warnings written from those tasks carry ids of their own (`media_queue.rs:179` `account_id, import_id`; `:218`, `:296` `account_id, sha256`; `asset_store.rs:445`, `:590` `account_id`) but no `request_id`.
  - `schema/sql/accounts.sql:407-456` `audit_entries`: no `request_id` column; `db/audit_trail.rs:573-603` `insert` binds none.
  - The per-response line (`server.rs:1325-1333`) has no `account_id`; `resolve_auth_on_conn` (`server.rs:1620` per D-F18) does not record the resolved account on the span.
- **Why it matters:** `http-api.md` "Failures" promises "every log line under a request carries it". The lines a request's spawned work writes do not, so the owner reading "An Asset's Thumbnail and Preview could not be made" cannot find the upload that queued it, and an Audit Trail entry cannot be matched to its request line except by time.
- **Exploitability:** None; investigative cost only.
- **Remediation:**
  ```rust
  use tracing::Instrument as _;
  tokio::spawn(async move { /* ... */ }.in_current_span());
  ```
  at each `tokio::spawn` listed; `spawn_blocking` closures can capture `tracing::Span::current()` and `let _g = span.enter();`. Declare `account_id = tracing::field::Empty` on the request span and `tracing::Span::current().record("account_id", auth.account_id)` once auth resolves. Optionally add `request_id TEXT` to `audit_entries` and bind `request_id::current()` in `insert`; the schema fingerprint changes, which the project treats as free (memory: "Schema rebuild is not a cost").

### F6. Two owner actions leave no Audit Trail entry: the attachment size limit, and a Demo Account rebuild

- **Severity:** Low
- **CWE:** CWE-778 (Insufficient Logging)
- **Evidence:**
  - `crates/server/server/src/server_api.rs:297-326` `update_server_settings`: records `RegistrationOpened`/`RegistrationClosed` when `public_registration` changes (`:311-319`); `set_asset_max_bytes` at `:322-324` writes no entry.
  - `server_api.rs:760-794` `replace_demo_account`: `end_demo_sessions` (`:798-810`) revokes the Demo Account's sessions, which `session_tokens.rs:410` records as `session_ended`/`revoked`; nothing records that the account's data was replaced. `AuditAction` (`db/audit_trail.rs:44-85`) has no variant for either.
  - Also absent, by design and acceptable: reads of the log and the trail (`log_lines.rs`, `log_files.rs`, `audit_trail_api.rs` write nothing); Media Link creation; 401/403 on non-login routes (present as `INFO` status lines only).
- **Why it matters:** ADR 0020 scopes the trail to "the owner's changes to accounts", and these are not accounts, so the code follows the ADR. But raising the attachment limit changes what every account can store, and a Demo rebuild deletes and recreates an account's messages; both are owner acts a hosted person might ask about later, which is the ADR's stated purpose.
- **Remediation:**
  ```rust
  // db/audit_trail.rs AuditAction
  /// The owner changed the attachment size limit.
  AssetLimitChanged,
  /// The owner replaced the Demo Account's data.
  DemoAccountRebuilt,
  // server_api.rs update_server_settings, after set_asset_max_bytes
  crate::db::audit_trail::record(&mut conn, &NewEntry::about_no_account(AuditAction::AssetLimitChanged, AuditActor::Owner)
      .with_details(Details { attachments: Some(i64::try_from(bytes).unwrap_or(i64::MAX)), ..Default::default() })).await?;
  ```
  and `DemoAccountRebuilt` about `DEMO_ACCOUNT_ID` in `replace_demo_account` after `start` succeeds. Add both words to the `action` comment in `schema/sql/accounts.sql:412-417`.

### F7. Refused logins for a live username are kept forever and can be grown from outside

- **Severity:** Low
- **CWE:** CWE-770 (Allocation of Resources Without Limits or Throttling)
- **Evidence:**
  - `db/audit_trail.rs:743-755` `trim_refused_logins`: deletes only `reason = 'unknown_username'` older than `UNKNOWN_USERNAME_RETENTION_DAYS` (90, `:36`). ADR 0020: "Refused logins for a username that matches no account belong to no one and are deleted after 90 days".
  - `session_api.rs:169` counts the attempt, then `:209-218` records `WrongPassword` for a known account before answering; `credentials.rs:22-23` allows 20 per 60 s per account bucket.
  - The Demo Account has no password and is a fixed id (`ADR 0016`), and "reset-demo leaves the old Demo Account's entries in place" (ADR 0020), so its refusals also accumulate across rebuilds.
- **Why it matters:** An anonymous client who knows one username (the Demo Account's is fixed) adds 28,800 rows a day to `audit_entries`, each about 200 bytes with its `details` JSON: roughly 2 GB a year per username, in the database rather than the size-bounded log. The holder's `GET /v1/accounts/{id}/audit-trail` pages through all of it. The ADR's reason for keeping entries (a record the owner cannot clear) is sound; it does not require one row per identical refusal.
- **Exploitability:** Sustained, low-rate, unauthenticated traffic. Low.
- **Remediation:** Collapse repeated refusals: when the newest entry for the same `account_id`, `reason` and `app_kind` is within the window, increment a `count` in `details` instead of inserting (one `UPDATE` on the newest row, which the trail's "never edited" rule should name as the one sanctioned amendment), or record refusals per window with `first_at`/`last_at`/`count`. Either keeps the fact and bounds the growth to one row per minute per account.

### F8. Audit Trail immutability is enforced by code only

- **Severity:** Low
- **CWE:** CWE-693 (Protection Mechanism Failure), as missing defense in depth
- **Evidence:** `schema/sql/accounts.sql:407-468` defines `audit_entries` with four indexes and no trigger (grep `TRIGGER` in `accounts.sql` returns nothing). The sanctioned writes are `insert` (`db/audit_trail.rs:573-603`), the `DELETE` in `trim_refused_logins` (`:746-751`) and the `UPDATE ... SET deletion_entry_id` in `prepare_account_deletion` (`:781-786`); `account_id` is `ON DELETE SET NULL` (`:426`).
- **Why it matters:** ADR 0020 says "nobody edits it". SQLite has no roles, so the owner with the file can always edit; a trigger does not change that. It does make a future route or a hand-run `UPDATE` fail loudly rather than silently rewrite history.
- **Remediation:**
  ```sql
  CREATE TRIGGER IF NOT EXISTS audit_entries_no_rewrite
  BEFORE UPDATE ON audit_entries
  WHEN NEW.at <> OLD.at OR NEW.action <> OLD.action OR NEW.actor <> OLD.actor
     OR NEW.username IS NOT OLD.username OR NEW.reason IS NOT OLD.reason
     OR NEW.details IS NOT OLD.details
  BEGIN SELECT RAISE(ABORT, 'audit entries are never edited'); END;
  CREATE TRIGGER IF NOT EXISTS audit_entries_no_delete
  BEFORE DELETE ON audit_entries
  WHEN OLD.action <> 'login_refused' OR OLD.reason <> 'unknown_username'
  BEGIN SELECT RAISE(ABORT, 'audit entries are never deleted'); END;
  ```
  This permits the two sanctioned paths (`deletion_entry_id`, `account_id` set NULL) and the 90-day trim. If F7's aggregation is adopted, add `details` to the allowed changes for the newest refusal only, or prefer the per-window row.

### F9. The `say!` lines and the first-start Demo seed never reach the log files

- **Severity:** Low
- **CWE:** CWE-778 (Insufficient Logging)
- **Evidence:**
  - `server.rs:47-52` `say!` writes to stderr directly, outside `tracing`. Used at `:1368` ("The server's log is in"), `:1383` ("The database at {} is open, in journal mode {mode}"), `:1401` (attachment limits), `:1417` (the listening line), `:1422`, `:1501`.
  - `server.rs:1374-1377`: on a new database `serve` runs `reset_demo::seed_new_database`, whose output is `println!` (`reset_demo.rs:133-142` `print_reset_header`) and `eprintln!` (`:292`, `:301-307`), also outside `tracing`.
  - `logging.rs:12-14` says the subcommands print to stdout because "they are not the server"; these calls run inside `serve`.
  - The desktop app reads `LISTENING_LINE` from the child's stderr (`local_server.rs:935`), so stderr must keep it.
- **Why it matters:** A downloaded `server-*.log` starts mid-story: no bind address, no database path, no journal mode, no record that the Demo Account was seeded at first start. Those are the first things asked when a server misbehaves.
- **Remediation:** Make `say!` also emit `tracing::info!`, or replace each `say!` in `serve` with `tracing::info!` and keep one `say!` for the listening line (which the app parses). Route `seed_new_database`'s progress through `crate::progress::Progress::Log` (`progress.rs:39`), which already exists for exactly this case. Cross-reference D-F13 (`reset_demo.rs` holds its own printing) and D-F1 (the macro and `logged_uri` live in `server.rs`; move them with the split).

### F10. The desktop app process has no log of its own

- **Severity:** Low
- **CWE:** CWE-778 (Insufficient Logging)
- **Evidence:** `src-tauri/Cargo.toml` has no logging crate (only `tauri-plugin-dialog` :20 and `message-crate-log-lines` :29 are relevant). The app's own faults go to `eprintln!`: `commands/events.rs:37-39` (an event no window received), `tool_downloads.rs:998-1001` (manifest write failure after an ffmpeg download), `crates/core/message-crate-core/src/process.rs:75`, `:85` (log lines with no sink). `local_server.rs:94` keeps the child server's last 40 lines in memory for a failure report (`:855`, `:866`) and writes them nowhere.
- **Why it matters:** A GUI app's stderr is discarded by the desktop. The server writes its own files, and each Import Run writes its own, so the gap is narrow: tool downloads, event delivery, and the app's start-up decisions (ADR 0018: which server it found or started).
- **Remediation:** Open one `app.log` in the Logs Directory with `RunLog`-style appends through `format_lines`, route the three `eprintln!` sites to it, and let `run_logs::list` offer it to the owner beside `import-*.log`. Keep lines free of paths outside the app's own directories.

### F11. Four `console.*` calls remain in the production web bundle

- **Severity:** Informational
- **CWE:** CWE-532 (not met; recorded for the checklist)
- **Evidence:** `web/src/components/OpenPathButton.tsx:24` `console.error("Failed to open path", caught)`; `web/src/screens/import/ImportRunView.tsx:235` `console.error("Could not name the Import Run's log", caught)`; `web/src/screens/import/useImportJob.ts:1427`, `:1438` `console.warn(...)`. `web/src/lib/api.ts` logs nothing; the token is a module variable (`:13-14`) and goes only into the `Authorization` header (`:175`, `:207`, `:236`).
- **Why it matters:** None of the four prints a token, a message or a contact; `caught` may carry a Tauri error string with a path. Browser console only.
- **Remediation:** Optional: turn on Biome's `noConsole` and route the two `caught` values through `apiErrorMessage` so a path is not printed. No security action needed.

### F12. No telemetry or crash reporting exists, and no document says so

- **Severity:** Informational
- **Evidence:** grep for `telemetry|analytics|crash report|sentry|phone home|error report` across `docs/src`, `docs/adr`, `docs/architecture`, `CONTEXT.md` returns nothing. `src-tauri` reaches one external host, `https://github.com` (`tool_downloads.rs:62`), to download ffmpeg and wtsexporter; no updater plugin is declared in `src-tauri/Cargo.toml` or `tauri.conf.json`. `web/src` fetches only the configured server and `/health`.
- **Why it matters:** Absence is the right state for a private message archive; it is also a property worth writing down so a future dependency does not add one unnoticed.
- **Remediation:** One sentence in `docs/architecture/server-log.md` or the user docs: "Message Crate sends no telemetry, crash reports or usage data; the only outbound connections are the tool downloads from GitHub the desktop app asks for." Consider a `cargo deny` ban on well-known telemetry crates in `audit.yml`.

## 4. What passes (recorded so the checklist has its evidence)

- **Secrets and content never logged, and tested.** `tests/server_log.rs:84-310` runs `serve` at `RUST_LOG=trace`, exercises owner, account, API token, import with a named contact and attachment, search, media link and password change, then asserts no password, `$argon2` hash, token, message word, attachment bytes, contact name, phone or email is in any log file. `logged_uri` (`server.rs:1225-1248`) hides every query value not in `LOGGED_QUERY_VALUES` (`:1194-1205`), including `q`, `media_link` and the log's own `text`. The Audit Trail hides an API token's hint from every reader but the account itself (`audit_trail_api.rs:58-83`), and `mask_api_token` (`db/api_tokens.rs:39`) keeps a head and tail only.
- **500s logged once, server-side, with a constant client sentence.** `server.rs:646-665` logs `error = %error_chain(err)` at `ERROR` and answers `detail: "internal server error"` with the `request_id`.
- **Request id on every response and in every problem body.** `request_id.rs:34-41`; `to_problem` binds `request_id::current()`; a client-sent id is dropped (`request_id.rs:9-11`).
- **Rotation bounded by size, full disk handled.** `files.rs:101-147`: next file before 50 MB, oldest deleted past 5, a failed write truncated back to the last whole line (`:124-134`), a file ending mid-line never appended (`:69`, `:117`). Tests at `logging/tests.rs:37`, `:61`, `:86`, `:192`.
- **Log routes owner-only.** `Owner(_auth)` extractor on `list_log_lines` (`log_lines.rs:71-74`), `list_log_files` (`log_files.rs:44-47`), `get_log_file` (`:74-77`); `security(("session" = ["owner"]))` in each OpenAPI annotation.
- **Run-log reader guards the file name and symlinks.** `run_logs.rs:249-273` `readable_log`: `is_run_log_name` rejects separators, `symlink_metadata(...).is_file()` refuses a link, and the account line gates the reader. `import_run_log` (`app_directories.rs:71-77`) uses `file_name()` of the run directory, so a window-supplied `run_dir` cannot leave the Logs Directory.
- **Audit Trail coverage** (ADR 0020), each confirmed at a write site:

  | Event | Recorded | Where |
  |---|---|---|
  | Login success | Yes | `db/session_tokens.rs:281` `record_login` |
  | Login refused (unknown, wrong password, disabled) | Yes | `session_api.rs:190`, `:212`, `:227` |
  | Rate-limit refusal | No, by design | `session_api.rs:167-169` (see F2) |
  | Session ended (logout, replaced, revoked, expired) | Yes | `session_tokens.rs:274`, `:375`, `:410`; expiry derived at read (`audit_trail.rs:5-8`) |
  | Account created (owner, registration, claim, CLI) | Yes | `accounts_api.rs:407`, `server_api.rs:187`, `owner_cli.rs:54` |
  | Disabled / enabled | Yes | `accounts_api.rs:734-738` |
  | Password set (owner, holder, CLI) | Yes | `accounts_api.rs:1103`, `credentials.rs:313`, `owner_cli.rs:91` |
  | Permissions changed | Yes | `accounts_api.rs:768` |
  | API token created / deleted | Yes | `accounts_api/api_tokens.rs:270`, `:332` |
  | Messages deleted for good, conversation deleted, trash emptied | Yes | `db/account_profile.rs:610`, `db/trash.rs:302`, `:362` |
  | Account deleted (entries kept, unlinked) | Yes | `db/audit_trail.rs:774-797`, `account_profile.rs:383` |
  | Address book loaded / exported | Yes | `contacts_api/address_book.rs:172`, `:259` |
  | Registration opened / closed | Yes | `server_api.rs:311-319` |
  | Import Run / Export Run | Yes, from their tables | `audit_trail.rs:829-837` `Source::ImportRun`, `ExportRun` |
  | Attachment size limit changed | No | `server_api.rs:322-324` (F6) |
  | Demo Account rebuilt | No | `server_api.rs:760-794` (F6) |
  | 401/403 on authenticated routes | No (status line only) | F2, F6 |
  | Log, trail, storage reads by the owner | No | acceptable |

- **Who reads the trail.** `GET /v1/audit-trail` is `Owner` only (`audit_trail_api.rs:110-116`); `GET /v1/accounts/{id}/audit-trail` goes through `require_reach(..., Admits::Owner)` (`accounts_api.rs:1484-1510`), so an account reads its own and the owner reads all, as ADR 0020 says.
- **Run logs hold what the doc says.** `RunLog::issue` writes `item: reason` (`app_directories.rs:129-135`); exporter `LogSink` lines are progress sentences (`crates/core/message-crate-core/src/process.rs:22-47`); the Media pass labels an asset as `account_id/assets_path` (`process_assets.rs:613-615`), a fingerprint path, not a file name from the backup.

## 5. Prioritized fixes

1. **Cap `logged_uri` at 2 KB and demote `/health` and static 2xx lines to `DEBUG`** (F1). Two small edits in `server.rs`; removes the eviction lever and most of the log's noise.
2. **`WARN` on each refused login and on a bucket tripping** (F2). Four `tracing::warn!` lines; makes the Owner Logs panel, which opens at warnings and up, show an attack.
3. **`0o600` files and `0o700` directories for both logs** (F3). Two `open_append`-style helpers.
4. **Escape control bytes in `one_line` and `format_lines`** (F4). One match arm and one `chars().map`.
5. **`in_current_span()` on the four `tokio::spawn` sites, `account_id` on the request span, and `AssetLimitChanged` / `DemoAccountRebuilt` actions** (F5, F6).

Then, as design choices for the maintainer: aggregate repeated refusals (F7), triggers on `audit_entries` (F8), `say!` and the first-start seed through `tracing` (F9), an `app.log` for the desktop app (F10), and one sentence on telemetry (F12).

## 6. Unable to verify

- **hyper's default request-line and header buffer size under `axum::serve`** (`server.rs:1461`), which bounds how many bytes one flooding request can put in one line (F1). Proving: the `hyper`/`hyper-util` versions in `Cargo.lock` and `hyper::server::conn::http1::Builder::max_buf_size` default.
- **Whether the vendored `sqlx-sqlite`** (`Cargo.toml:98`) emits statement logs through `tracing` rather than `log`. The server enables no sqlx `tracing` feature (`crates/server/server/Cargo.toml:36`) and installs no `LogTracer` (`logging.rs:48-58` uses `set_global_default`, which does not bridge `log`), so with `log` nothing reaches the files even at `RUST_LOG=trace`; with `tracing` the SQL text would, though sqlx does not include bound values. Proving: `vendor/sqlx-sqlite/Cargo.toml` features and `vendor/sqlx-sqlite/src/logger.rs`.
- **Whether `hyper`/`h2` at `RUST_LOG=trace` log header values** (an `Authorization` header) on an HTTP/2 connection. `tests/server_log.rs:90` proves the HTTP/1 path only. Proving: the same test driven by an h2c client.
- **The mode Tauri creates the app-data directory with** on each OS, which F3's severity depends on. Proving: `src-tauri/src/commands/paths.rs` `logs_dir` and Tauri's `app_data_dir`.
- **The exact hint width of `mask_api_token`** (`db/api_tokens.rs:39-45` reads `HINT_HEAD + HINT_TAIL`; the constants were not read). Proving: the constants beside it.

## 7. Compliance checklist

| # | Check | Result | Evidence |
|---|---|---|---|
| 1a | Passwords not logged | Pass | `tests/server_log.rs:295-298`; no `tracing` call in `credentials.rs`/`session_api.rs` |
| 1b | Session and API tokens not logged (hashed or not) | Pass | `tests/server_log.rs:308-310`; `media_link=[hidden]` via `LOGGED_QUERY_VALUES` |
| 1c | PII (names, phones, emails, message text) not in the server log | Pass | `tests/server_log.rs:301-304`; `q=[hidden]` |
| 1c' | PII in Import Run logs | Partial, by documented decision | `import-run-logs.md` "What a line holds": file names carry identities; F3 on who else can read them |
| 1d | Credit card numbers | N/A | none handled |
| 1e | API keys | Pass | as 1b |
| 2a | Failed login attempts recorded | Pass (Audit Trail) / Fail (server log has no `WARN`) | `session_api.rs:190-231`; F2 |
| 2b | Authorization failures recorded | Fail | `INFO` status line only; no trail entry (F2, F6) |
| 2c | Input validation failures recorded | Pass | 4xx status in the request line; `detail` kept out of the log by design |
| 2d | System errors recorded with context | Pass | `server.rs:651` chain plus `request_id`; `DefaultOnFailure` 5xx line |
| 3a | Input sanitization in logs | Partial | LF/CR escaped (`files.rs:221-236`); other control bytes not (F4) |
| 3b | Structured logging | Pass | `tracing` fields throughout; text output by design, parseable by `log-lines` |
| 4a | Secure storage (permissions) | Partial | umask only (F3); owner-only routes Pass |
| 4b | Rotation policy | Pass | 5 x 50 MB, size-only (`files.rs:27-30`); eviction exposure F1 |
| 4c | Backup strategy | Pass | the Data Directory copy is the backup (`server-log.md`); run logs stay on the computer, never deleted (`import-run-logs.md`), and are not bounded in count |
| 5a | Unusual activity detection | Fail (not present) | F2; self-hosted, no alert channel |
| 5b | Error rate monitoring | Fail (not present) | no metrics; `/health` constant (`server.rs:1523`) |
| 5c | Performance anomalies | Fail (not present) | `latency` is in each line; nothing aggregates it |
| 6 | Audit trail immutable and outliving the account | Pass (code) / Partial (schema) | `db/audit_trail.rs:774-797`; no triggers (F8) |
| 7 | Telemetry / crash reporting | Pass (none) | F12; undocumented |
