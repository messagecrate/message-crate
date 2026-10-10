# Business logic vulnerabilities audit

- Date: 2026-10-10
- Commit: `530dc1791`
- Scope: the whole repository except `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`. The server (`crates/server/server/`) carries nearly all business rules, so most evidence is there; the desktop pipeline (`crates/libs/import/`, `crates/libs/export/`) was read for what the client asserts and the server believes.
- Method: static reading only. Nothing was compiled or run. Every line reference below was read in this commit.
- Cross-reference: `audits/2026-10-09-initial-software-design-analysis.md` (the design audit). Where a finding overlaps it, the design audit's finding id is named and the overlap is not repeated.

## Assumptions

1. There are no payments, prices, quotas or currencies. The "Price manipulation" items of the checklist are Not Applicable; the "inventory" item is read as resource caps.
2. The attacker classes are: a stranger at the login card; a visitor in the Demo Account; an account holder with a Session; an account holder's program with an import or export API token; a second client on the same account at the same time; the owner; and the contents of a conversation file or backup the account holder imports.
3. SECURITY.md says the product stores private message archives. The reach rules in `docs/architecture/http-api.md` ("Credentials and reach", `:473-706`) and the glossary in `CONTEXT.md` are taken as the intended behaviour; a finding is where the code lets a party do what those documents say it may not, or where the code reaches a state the documents say cannot exist.

## Business logic threat model

### Assets

| Asset | Where it lives | Who may touch it |
|---|---|---|
| An account's messages, attachments, contacts | `messages`, `attachments`, `contacts`, `handles`, per-account asset directory | The account's Session; its import token (writes), its export token (reads through an Export Run); the owner may delete, never read |
| The Import Run and Export Run records, the Audit Trail | `imports`, `exports`, `audit_entries` | Written by the server as part of the act; never edited or deleted by a route (`db/audit_trail.rs:743-757` is the one documented trim) |
| The Trash and the door to permanent deletion | `trashed_conversations`, `trashed_contacts` | A Session with `delete`; the Demo Account never |
| The account list, permissions, status, server settings | `accounts`, `server_settings` | The owner alone (`accounts_api.rs:802-860`) |
| Availability of one SQLite database and one process | pool of 4 connections (`db/engine.rs:56`), `busy_timeout` 15 s (`:20`) | Every writer waits for every other |

### Trust boundaries

- The credential names the account; no route takes `account=` (`server.rs:1687-1690` `resolve_import_account`). Every `db/` query read for this audit binds `account_id`.
- The server trusts the client for: an Import Run's `status`, `summary`, timings and `bytes_uploaded` (`imports_api/mod.rs:1342-1388`); the run's `stage` (`db/imports.rs:579-602`); the `form`, `run_dir`, `device_id`, `source_fingerprint`, `source_identities` and `tool` of a run; the `X-Message-Crate-App` headers (`server.rs:1579-1591`). None of these decides access. Two of them decide what the owner and the holder later read as the record (F-04).
- The server verifies for itself: an asset's SHA-256 on every store (`assets_api.rs` `store_verified`, `copy_to_verified_temp`; `asset_uploads.rs:384-422`); that selection ids belong to the account (`exports_api.rs:194-202`, `db/ownership.rs:85-114`); that only trashed rows are deleted for good (`db/trash.rs:281-321`); that a finished run is never rewritten (`db/imports.rs:758-781`, `db/exports.rs:259-271`).

### State machines

- Import Run: `running` to one of `completed`, `completed_with_issues`, `failed` (client's choice, `imports_api/mod.rs:792-799`) or `cancelled` (discard, `db/imports.rs:634-671`; account deletion, `:1221-1235`). Every transition is one `UPDATE ... WHERE status = 'running'` inside `begin_write`, so two closers cannot both win. One running run per account is a partial unique index (`schema/sql/accounts.sql:248-250`). The `stage` column is a free label (F-05).
- Export Run: `running` to `completed` or `cancelled`; same single-statement guard (`db/exports.rs:252-278`). No cap on how many are open (F-02).
- Trash: mark, unmark, delete-for-good; the delete checks the mark inside its own write transaction (`db/trash.rs:289-295`).
- Account: created (owner or open registration), disabled, deleted. Deletion cascades and detaches runs and entries (`db/audit_trail.rs:774-798`). A deleted id is never reused by `AUTOINCREMENT` (`schema/sql/accounts.sql:3-9`), except through F-01.

### Concurrency primitives

- `KeyedLocks` per account for imports and per (account, sha256) for multipart completion (`keyed_locks.rs`, `server.rs:385-401`).
- A process-global `tokio::sync::Semaphore` of 2 for imports (`imports_api/mod.rs:1767-1772`); the design audit's F7 covers its placement.
- SQLite `BEGIN IMMEDIATE` through `db::begin_write` everywhere a check and a write must land together; the test-only authorizer in `db/write_guard.rs` fails a test that writes `messages` outside one.

## Findings

Severity is on the skill's scale: Critical, High, Medium, Low. No Critical or High finding was made.

### F-01. A deleted account is recreated with no password when one of its requests was already past the credential check

- Severity: **Medium** (High on a server reachable from outside its network, because the recreated account logs in with an empty password)
- CWE-367 (Time-of-check Time-of-use Race Condition), CWE-287 (Improper Authentication)

**Evidence.**

- `crates/server/server/src/db/account_profile.rs:196-207` `ensure_account_row`: `INSERT INTO accounts (id, username) VALUES ($1, $2) ON CONFLICT DO NOTHING`, with the username set to the id and every other column at its default: `password_hash` NULL, `can_import`, `can_export`, `can_delete` all 1 (`schema/sql/accounts.sql:13-31`).
- It is called on three HTTP paths with the credential's account id:
  - `crates/server/server/src/imports_api/mod.rs:1301`, in `create_import`, before the run's write transaction;
  - `crates/server/server/src/imports_api/mod.rs:399-425` `prepare_import` (`:410`), called from `import_jsonl_files_on_conn` (`:314-346`) before `begin_write` and before `require_running_import`; this runs for every batch, from `run_import_path` (`:1776-1827`);
  - `crates/server/server/src/exports_api.rs:104`, in `start_export_run`.
- The credential is resolved once, at the start of the request (`crates/server/server/src/server.rs:1563-1577`, `:1612-1684`). A batch then streams its body to disk (`imports_api/mod.rs:1735-1745`), waits for the process-wide semaphore (`:1786`) and the account lock (`:1798`), and only then runs `prepare_import`. That wait is unbounded and can be minutes behind two long imports.
- `crates/server/server/src/db/account_profile.rs:376-391` `delete_account` deletes the row and cascades; `crates/server/server/src/db/audit_trail.rs:774-798` `prepare_account_deletion` revokes sessions and marks the running run `cancelled` with `account_id` NULL (`db/imports.rs:1221-1235`).
- `crates/server/server/src/credentials.rs:186-194` `verify_login_password`: a NULL or empty hash accepts the empty password. `session_api.rs:210-211` uses it for login.

**Why it matters.** The owner deletes an account whose client has a batch or an export creation in flight. The row comes back under the deleted id, named by that id, with no password and every permission, on a server that may be closed to registration. Anyone who can reach the login card then enters it (`POST /v1/session {"username":"<id>","password":""}`) and holds an account with import and delete permission. The invariant `schema/sql/accounts.sql:3-9` states, that a deleted account's id never passes to a new account, is broken by an explicit-id insert. The recreated account holds no messages (the cascade removed them), so this is a foothold and a policy bypass rather than a disclosure.

**Exploitability.** Needs the owner to delete an account while one of its requests is between the credential check and `ensure_account_row`. For a batch that window is the whole wait on the semaphore and account lock, which the holder's own slow imports can lengthen; an owner deleting "the account that is importing right now" is an ordinary admin act. A unit test proves the mechanism without timing: call `delete_account`, then `ensure_account_row(conn, id)`, then `lookup_account_by_username(conn, "<id>")` and `load_password_hash`; the account exists with a NULL hash.

**Remediation.** The credential already proved the row exists; the HTTP paths must not create one. Delete the three calls and, where a guard is wanted, refuse instead of inserting:

```rust
// imports_api/mod.rs create_import (:1301), prepare_import (:410), exports_api.rs start_export_run (:104)
// remove: crate::db::account_profile::ensure_account_row(&mut conn, account).await?;
// if a check is wanted, the account's absence is the credential's failure:
if crate::db::account_profile::username_for_account(&mut *conn, account).await?.is_none() {
    return Err(ApiError::account_gone());
}
```

Keep `ensure_account_row` for the seed and command-line paths that make accounts on purpose (`reset_demo.rs`, `import_cli.rs`), and rename it so the intent is visible (`insert_account_row_for_seed`). Defense in depth: in `run_import_path`, re-resolve the account's existence after the locks are taken (`:1802`), since everything before that point ran on a credential checked at request start; and add a regression test that a batch arriving after `DELETE /v1/accounts/{id}` answers `404` and leaves no row in `accounts`.

### F-02. No cap on open Export Runs; each copies every matched message id

- Severity: **Medium**
- CWE-770 (Allocation of Resources Without Limits or Throttling)

**Evidence.**

- `crates/server/server/src/exports_api.rs:94-124` `start_export_run`: inside one `begin_write`, `list_run_messages` (`db/exports.rs:371-388`) inserts one `export_messages` row for every message the scope matches, then counts them.
- `schema/sql/accounts.sql:255-321`: the `exports` table has only `ix_exports_account_started`. There is no partial unique index like `ux_imports_active_account` (`:248-250`) and no count check in `start_export`.
- Rows are deleted only by `finish_export` (`db/exports.rs:252-278`) or by account deletion (`:559-592`). Nothing closes an abandoned run.
- `POST /v1/exports` needs only the `export` scope on a Session or an API token (`exports_api.rs:319`).

**Why it matters.** A program holding an export token, which by design cannot browse, can open an unbounded number of runs with `{"scope": {"kind": "everything"}}`. Each one inserts N rows (N = the account's message count) while holding the database write lock, so every other account's import, trash and login write waits for it (`busy_timeout` 15 s, `db/engine.rs:20`, then a `500`). The `export_messages` table grows by N per call until the account is deleted. The design audit's F17 names pipeline bottlenecks; this is an authenticated amplifier of them.

**Exploitability.** Authenticated, export scope. A loop of `POST /v1/exports` with no `complete` or `cancel`. No timing.

**Remediation.** Either hold an account to one open Export Run, as Import Runs are held, or cap the open count under the write lock. The schema route is the one that holds against a racing client:

```sql
-- schema/sql/accounts.sql, after ix_exports_account_started
-- At most one running Export Run per account, as for imports.
CREATE UNIQUE INDEX IF NOT EXISTS ux_exports_active_account
    ON exports(account_id) WHERE status = 'running';
```

and in `db/exports.rs::start_export`, map the unique violation to the `state-conflict` problem (`409`), as `db/imports.rs:401-447` does with `StartImportError::AlreadyActive`. If several concurrent exports are wanted, count inside the transaction instead:

```rust
// exports_api.rs start_export_run, after begin_write
let open: i64 = sqlx::query_scalar(
    "SELECT COUNT(*) FROM exports WHERE account_id = $1 AND status = 'running'")
    .bind(account_id).fetch_one(&mut *tx).await?;
if open >= MAX_OPEN_EXPORTS { return Err(ApiError::StateConflict(
    format!("{open} Export Runs are open; complete or cancel one first"))); }
```

Defense in depth: close a run as `cancelled` when no page has been read for a day (the server already has a periodic pass in `media_queue`), and record the closure in the Audit Trail so the owner sees abandoned runs.

### F-03. Unknown usernames at the login card buy unlimited Argon2 work and Audit Trail rows

- Severity: **Medium**
- CWE-770 (Allocation of Resources Without Limits or Throttling), CWE-307 (Improper Restriction of Excessive Authentication Attempts)

**Evidence.**

- `crates/server/server/src/session_api.rs:160-199` `create_session`: a username that names no account is counted in `unknown_username_bucket(&username)` (`credentials.rs:46-48`), one bucket per lowercased spelling, at `AUTH_RATE_MAX = 20` per 60 s (`credentials.rs:23`).
- For every such request the server runs a real Argon2 verification against a dummy hash (`session_api.rs:189`, `credentials.rs:179-184`, `:186-194`) and inserts an Audit Trail row (`session_api.rs:190-197`, `db/audit_trail.rs:711-741`). The rows are trimmed only when older than 90 days (`db/audit_trail.rs:36`, `:743-757`).
- `docs/architecture/http-api.md:693-706` gives registration and claim one server-wide bucket because "a count per name lets a script that tries a new name each time straight through", and rejects counting by address. Login has no server-wide bucket.
- The body is capped at 32 KiB (`server.rs:1007-1008` `MAX_AUTH_BODY_BYTES`) and the password at 1024 bytes (`credentials.rs:20`), so the cost per request is one Argon2 verify at the crate's default parameters plus one insert.

**Why it matters.** A stranger varying the username (`a1`, `a2`, ...) is never limited. Each request costs the server a memory-hard hash and a write, and the Audit Trail the owner reads fills with `login_refused` rows for 90 days. On a small self-hosted machine a few parallel loops hold the CPU and the write lock. The timing-equalising dummy hash is the right idea; the missing piece is a ceiling on how often it runs for names nobody holds.

**Exploitability.** Unauthenticated. No timing. Bounded only by the attacker's bandwidth.

**Remediation.** Add a server-wide bucket for unknown usernames, checked before the dummy verify and the audit insert, with its own cap:

```rust
// credentials.rs
pub(crate) const UNKNOWN_USERNAME_RATE_MAX: usize = 120; // per AUTH_RATE_WINDOW, whole server

// session_api.rs create_session, in the `None` arm of the bucket match (:163-166)
None => {
    check_auth_rate_limit_with_max(&state.auth_rate_limits, "password:unknown",
        UNKNOWN_USERNAME_RATE_MAX)?;
    unknown_username_bucket(&username)
}
```

(`check_auth_rate_limit_at` at `credentials.rs:86-96` takes its cap from the constant; a variant that takes the cap as an argument is a six-line change.) When the server-wide bucket refuses, answer `429` before the dummy verify runs. Defense in depth: write one `login_refused` row per unknown username per window rather than one per attempt, and document in the developer reference that a reverse proxy in front of an exposed server should rate-limit `POST /v1/session` by address as well (the project's memory note says the login is not internet security; this finding is about keeping the server up, not about guessing).

### F-04. The Import Run's outcome is the client's word

- Severity: **Low**
- CWE-602 (Client-Side Enforcement of Server-Side Security), CWE-345 (Insufficient Verification of Data Authenticity)

**Evidence.**

- `crates/server/server/src/imports_api/mod.rs:1342-1388` `complete_import` accepts `status` (`completed`, `completed_with_issues`, `failed`; `validate_import_status` `:792-799`), `summary`, `bytes_uploaded`, `duration_ms`, `parse_ms`, `attachments_ms`, `prepare_ms`, `upload_ms`, `issues` and `notes` from the body.
- `crates/server/server/src/db/imports.rs:712-800` `complete_import` derives `message_count` and `attachment_count` from the stamped rows (`:726-754`) but writes `status`, `summary_json` and the timings as sent. `set_import_stage` (`:579-602`) writes any `summary_json` the client sends at a stage change.
- The verdict is computed in the desktop client only: `crates/libs/import/src/report.rs:257-267` `outcome_status`, sent by `crates/libs/import/src/run.rs:748-762`.
- The owner's view of a run (`OwnerImportRun`, `docs/architecture/http-api.md:666-680`) carries the summary's counts and the outcome, so the client's claim is what the owner reads.

**Why it matters.** A run that stored 50,000 messages can be recorded `failed`; one that stored none can be recorded `completed`. Import History and the Audit Trail then misstate what happened, and the owner's monitoring (the one thing the owner may see, per ADR 0008) rests on a number the account's client chose. This is an integrity gap in the record, not a disclosure or a permission gap.

**Exploitability.** Authenticated, import scope; one `POST /v1/imports/{id}/complete` with a chosen `status`. An older or faulty build does it by accident.

**Remediation.** Decide the contradiction server-side where the facts are known:

```rust
// db/imports.rs complete_import, after message_count is derived (:736)
if args.status == ImportStatus::Failed && message_count > 0 {
    return Err(ImportLookupError::InvalidRun {
        message: format!("import {import_id} stored {message_count} messages and cannot be recorded as failed"),
    });
}
```

Record the client's `summary` beside a server-computed summary rather than instead of one; the counts the owner reads should be the server's. Defense in depth: show "as reported by the client" next to the client-supplied timings in Import History.

### F-05. The Import Run's stage is a label, so the Review stops are enforced only by the desktop app

- Severity: **Low**
- CWE-841 (Improper Enforcement of Behavioral Workflow)

**Evidence.**

- `crates/server/server/src/db/imports.rs:579-602` `set_import_stage`: `UPDATE imports SET stage = $1 ... WHERE ... status = 'running'`. Any stage, in any order, any number of times.
- `crates/server/server/src/imports_api/mod.rs:1707-1765` `create_import_batch` and `db/imports.rs:550-565` `require_running_import` check `status` only; `stage` is never read on the batch path. `BatchContext::from_row` (`mod.rs:551`) carries no stage.
- `CONTEXT.md`, "Review": the run "waits for the person to approve or cancel it before spending more time or touching Message Crate". `docs/architecture/http-api.md:707-725` requires only that a finished run refuses changes; it does not order the stages.

**Why it matters.** A script, or an older build, can post batches while the run says `staging_review`, or move a run from `parse` straight to `upload`. The server cannot tell an approved run from one that skipped its Review, so the record of "what was approved" (`summary_json` at the stage change) is not a record of approval.

**Exploitability.** Authenticated, import scope, the holder's own data. Self-inflicted; no cross-account effect.

**Remediation.** Either state in `docs/architecture/http-api.md` that `stage` is informational and the Review is the client's, or enforce the two rules the glossary states:

```rust
// db/imports.rs: forward-only stage order
const STAGE_ORDER: [ImportStage; 6] = [Parse, Write, StagingReview, Media, MediaReview, Upload];
// in set_import_stage, inside begin_write: refuse when the new stage is earlier than the stored one.

// imports_api/mod.rs create_import_batch, after require_running_import (:1729)
if row.stage != Some(ImportStage::Upload) {
    return Err(ApiError::StateConflict(format!("import {import_id} is at {:?}, not upload", row.stage)));
}
```

### F-06. The sweep after a run removes a just-uploaded original with no grace period

- Severity: **Low**
- CWE-362 (Concurrent Execution using Shared Resource with Improper Synchronization)

**Evidence.**

- `crates/server/server/src/asset_store.rs:548-586` `take_out_unnamed`: originals are swept with `grace_secs = 0` (`:571`), Previews with `PREVIEW_GRACE_SECS` (`:600`, one hour, `:575`). The grace check is `:689-691`.
- `asset_store.rs:371-373` `may_take_out` skips the sweep only when the account has a running run; `asset_store.rs:588-596` `sweep_after_run` is called by `complete_import` and `discard_import` (`imports_api/mod.rs:1384`, `:1697`).
- `crates/server/server/src/assets_api.rs:1031-1036` `replace_asset` needs the import scope, not a running run, so an asset can be stored before any run names it.
- The desktop creates the run before it uploads (`crates/libs/import/src/run.rs:317` `start_import_run`), so one desktop client is safe. A second client on the same account is not.

**Why it matters.** Account holder's script uploads assets by `PUT /v1/assets/{sha}`; the holder's desktop completes an unrelated run; the sweep moves the script's files out; the script's batch records them `missing` (`imports_api/staging.rs:134-145`, `:201-212`) and the attachments are lost from the import.

**Exploitability.** Two clients on one account, timing within seconds. Data loss for the holder, no cross-account effect.

**Remediation.** Give originals the grace Previews have:

```rust
// asset_store.rs take_out_unnamed (:567-573)
pub(crate) const ORIGINAL_GRACE_SECS: u64 = 60 * 60;
sweep_store_dir(account_id, &paths.assets_dir_for_account(account_id), &named, ORIGINAL_GRACE_SECS, &mut removal)
```

Defense in depth: refuse `PUT /v1/assets/{sha}` and the multipart start with `409` when the account has no running run, so every upload belongs to a run the sweep can see.

### F-07. A batch body is spooled to disk before any concurrency limit is taken

- Severity: **Low**
- CWE-770 (Allocation of Resources Without Limits or Throttling). Overlaps the design audit's F7 (`audits/2026-10-09-initial-software-design-analysis.md:477-495`, the process-global semaphore and its placement); not repeated here.

**Evidence.**

- `crates/server/server/src/imports_api/mod.rs:1735-1745`: `stream_body_to_file` writes up to `MAX_REQUEST_BODY_BYTES` (512 MiB, `server.rs:1021`) into a fresh `tempfile::tempdir()` per request, before `run_import_path` takes the semaphore (`:1786`) and the account lock (`:1798`).
- `MAX_CONCURRENT_IMPORTS = 2` (`:1767`) is process-wide; the wait has no timeout.

**Why it matters.** N parallel `POST /v1/imports/{id}/batches` from one account spool N × 512 MiB before any of them is held back; two long batches from two accounts make every other account's batch wait indefinitely. A conversation file is text; 512 MiB is the attachment cap, not a sensible JSONL cap.

**Exploitability.** Authenticated, import scope.

**Remediation.**

```rust
// imports_api/mod.rs create_import_batch: take the account lock before reading the body
let _guard = state.account_import_locks.lock(account.to_string()).await;
// and a JSONL-specific cap
pub(crate) const MAX_BATCH_BODY_BYTES: usize = 64 * 1024 * 1024;
// run_import_path: bound the wait
let _import_permit = tokio::time::timeout(Duration::from_secs(300), import_semaphore().acquire())
    .await.map_err(|_| ApiError::ServiceUnavailable { retry_after_secs: 60 })??;
```

### F-08. Registration reads `public_registration` outside the transaction that creates the account

- Severity: **Low**
- CWE-367 (Time-of-check Time-of-use Race Condition)

**Evidence.** `crates/server/server/src/accounts_api.rs:369-373` reads the setting on a bare connection; `begin_write` opens at `:380`. `crates/server/server/src/server_api.rs:298-323` `update_server_settings` writes the setting on a bare connection too.

**Why it matters.** One registration can land after the owner closed the server. Window: the time between the read and the insert, milliseconds.

**Remediation.** Move `server_settings::load(&mut tx)` under `begin_write` in `create_account`, and write the setting change inside a transaction in `update_server_settings`.

### F-09. Client-asserted fields stored verbatim; the form's credential strip is a deny-list

- Severity: **Low** (informational)
- CWE-602 (Client-Side Enforcement of Server-Side Security)

**Evidence.** `crates/server/server/src/imports_api/mod.rs:803-821` `FORM_CREDENTIAL_KEYS` names two keys (`backupPassword`, `whatsappKey`) and removes only those from `form_json`, which is stored for the life of the run. `run_dir`, `device_id`, `source_fingerprint`, `source_identities`, `tool` (`:1308-1319`) and the `X-Message-Crate-App` headers (`server.rs:1579-1591`, 64 ASCII characters) are recorded as sent. None decides access, and the owner never reads `form_json`, `source_fingerprint` or `source_identities` (`docs/architecture/http-api.md:666-680`).

**Why it matters.** A new secret field in the desktop form reaches the database unless someone remembers the list. The app headers appear in the Audit Trail as fact.

**Remediation.** Allow-list the form keys the Import screen needs to restore itself instead of deny-listing secrets; label the recorded app in the Audit Trail as "reported by the client".

## Checked and holding

These were read for the failure modes the checklist names and hold as the rules say.

- Export scope ids are checked against the account before the snapshot (`exports_api.rs:194-222`, `db/ownership.rs:85-114`); pages read the snapshot, never the scope (`db/exports.rs:482-519`).
- Finishing a run is one statement with the status in its `WHERE`, for imports and exports (`db/imports.rs:758-781`, `:645-657`; `db/exports.rs:259-271`, `:296-307`). Two closers cannot both win; a finished run answers `409`.
- Permanent deletion checks ownership and the Trash marker inside the delete's write transaction (`db/trash.rs:289-295`, `:329-394`); the `DELETE` re-checks the marker (`:405-412`).
- Claiming checks `is_claimed` inside `begin_write` (`server_api.rs:171-176`); registration checks the username inside it (`accounts_api.rs:380-381`).
- Contact Group and Message Tag membership SQL binds the account on both the set and the member (`db/named_membership.rs:309-323`, `:752-775`).
- API token scopes are intersected with the account's at creation and on every request (`accounts_api/api_tokens.rs:258-261`, `server.rs:1668-1670`); expiry arithmetic saturates (`db/api_tokens.rs:376-389`); `expires_at` of 0 or in the past is refused (`:131-138`).
- Media links: HMAC over account, asset, expiry and the live Session hash, compared in constant time (`assets_api/media_links.rs:75-94`), expiry checked first (`:221`), a restart ends them all (`server.rs:418-421`).
- Every asset store hashes the bytes it writes and refuses a mismatch (`assets_api.rs` `copy_to_verified_temp`; `asset_uploads.rs:384-422`); the fingerprint in the path is 64 hex digits (`assets_api.rs:115`).
- Paging refuses `limit` 0 or over 500 and an `offset` that does not fit `i64` (`paging.rs:176-200`); export places saturate (`db/exports.rs:458-469`).
- Replace-mode wipes are bound to the account (`db/schema.rs:534-583`); a message whose guid the source holds is skipped (`db/staging.rs:1204-1218`); the one path that changes stored text is the documented later-edit promotion (`db/staging.rs:1795-1830`, "Earlier version" in `CONTEXT.md`).
- `delete_account_messages` takes the account's import lock so it runs between batches (`accounts_api.rs:1194-1198`).
- Permissions and status are the owner's alone; the Demo Account is refused by id on every closed route (`accounts_api.rs:809-860`, `server.rs:174-293`); self-deletion needs `delete` and the current password, rate-limited in the login's bucket (`accounts_api.rs:923-947`, `:985-999`).
- Time zone: stored only when chrono-tz parses it, read with a UTC fallback (`db/account_profile.rs:622-655`); it is the holder's own and affects only their lists and exports.
- Dedupe clears and sets flags within one account in one write transaction, ranked deterministically (`dedupe.rs:185-299`, `:884-899`).
- The Audit Trail has one documented delete (unknown-username refusals older than 90 days) and one documented update (marking a deleted account's entries); no route reaches either (`db/audit_trail.rs:743-757`, `:788-793`).

## Risk score and prioritized fixes

**Summary risk score: 4 / 10.** The core invariants (account isolation, trash-only deletion, immutable run records, verified assets) are enforced in SQL under write transactions and hold. The three Medium findings are a deleted account coming back without a password (F-01), and two resource exhaustion paths, one unauthenticated (F-03) and one for an export token (F-02). No finding lets one account read another's messages.

Fixes in the order that reduces risk fastest:

1. **F-01**: delete `ensure_account_row` from the three HTTP paths; add the regression test. One hour.
2. **F-03**: a server-wide bucket for unknown usernames ahead of the dummy Argon2 verify. One hour.
3. **F-02**: `ux_exports_active_account` partial unique index, or a counted cap under `begin_write`, plus an abandoned-run sweep. Half a day.
4. **F-04 and F-05 together**: refuse `failed` with stored messages, and either document `stage` as informational or enforce `upload` for batches. Half a day.
5. **F-06, F-07, F-08**: grace for originals, lock before spooling with a JSONL cap and a bounded wait, setting read inside the transaction. Half a day in all.

## Checklist

| Check | Result | Where |
|---|---|---|
| 1. Race conditions: concurrent request handling | **Fail** | F-01 (credential checked, account deleted, row recreated), F-06 (sweep vs. upload), F-08 (setting read outside transaction). Run closers, trash deletion, claim and registration pass. |
| 1. Race conditions: double-spending prevention | N/A | No currency. The nearest analogue, completing or cancelling a run twice, passes (`db/imports.rs:758-781`, `db/exports.rs:259-271`). |
| 1. Race conditions: inventory management | **Fail** (read as resource caps) | F-02 (open Export Runs), F-07 (spooled batch bodies). Import Runs pass (`ux_imports_active_account`). |
| 2. Price manipulation: client-side price validation only | N/A | No prices. |
| 2. Price manipulation: discount or coupon abuse | N/A | None exist. |
| 2. Price manipulation: currency manipulation | N/A | None exists. |
| 3. Workflow bypass: skipping validation steps | **Fail** | F-05: batches accepted at any stage. |
| 3. Workflow bypass: status manipulation | **Fail** | F-04: the run's outcome is the client's. A finished run cannot be rewritten (pass). |
| 3. Workflow bypass: approval process bypass | **Fail** | F-05: the Review stop is client-side only. |
| 4. Time-based: TOCTOU | **Fail** | F-01, F-08. Trash deletion, claim, registration username, run closers pass. |
| 4. Time-based: expiration bypass | Pass | Sessions (`db/session_tokens.rs:106-114`), API tokens (`db/api_tokens.rs:131-138`), media links (`media_links.rs:221`) all check expiry on use; a value that does not parse is expired. |
| 4. Time-based: timezone manipulation | Pass | The zone is the account's own, validated by chrono-tz, with a UTC fallback; it affects only the holder's own lists and exports. |
| 5. Integer overflow or underflow: calculation errors | Pass | Saturating arithmetic on expiry (`db/api_tokens.rs:382`, `media_links.rs:310-311`, `session_tokens.rs:279`), places (`db/exports.rs:458-469`), counts (`db/trash.rs:379`). |
| 5. Integer overflow or underflow: negative value handling | Pass | `offset` and `limit` are `usize` and checked against `i64` (`paging.rs:191-197`); member ids at or below 0 are refused as not held (`named_set_api.rs:54-57`, `db/named_membership.rs:309-323`). |

## Unable to verify

- **How long `complete_import` waits behind a batch's open transaction, and what the client sees past `busy_timeout`.** `db/engine.rs:20` sets 15 s; a batch's staging and promote run inside one `BEGIN IMMEDIATE` (`imports_api/mod.rs:336-346`). Whether a 15 s overrun answers `500` or is retried needs a running server; `imports_api/tests.rs` would hold such a test if one exists.
- **The Argon2 parameters in the compiled binary.** `credentials.rs:152`, `:163` use `Argon2::default()`; the figures quoted for F-03 are the `argon2` crate's documented defaults. `Cargo.lock` and the crate's `Params::DEFAULT` would confirm them.
- **Whether `has_messages` can be false on a second replace-mode batch after the first batch's messages were deleted between batches** (`imports_api/mod.rs:1806-1813`, `db/imports.rs:1061-1068`). Reading says a second wipe would then remove nothing new, because the first wipe already removed every older message of the source; a test in `imports_api/tests.rs` that deletes between batches would settle it.
- **The web app's own handling of the Review.** `web/src/` was not read; the desktop pipeline (`crates/libs/import/src/run.rs`, `pipeline.rs`) was. F-05 is a server-side statement and does not depend on it.
