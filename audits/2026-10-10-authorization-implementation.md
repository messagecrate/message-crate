# Authorization implementation audit

- Date: 2026-10-10
- Commit: `530dc1791`
- Scope: the whole codebase less `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`. The enforcement point is the server (`crates/server/server/`); `web/` only gates what it shows (`web/src/lib/desktopFeatures.ts`).
- Method: read-only. Every `*_api.rs` handler was mapped to the guard it takes, every `db/` query a handler reaches was checked for its account filter, and the three credentials (Session, API token, Media Link) were traced through `server.rs`. No code was compiled or run.
- Companion: `audits/2026-10-09-initial-software-design-analysis.md` (design). Where a point overlaps, its finding id is cited and not repeated.

## 1. Summary

Authorization on this server is in good shape. Every `/v1` handler takes a typed guard as an Axum extractor (`auth_guard!`, `crates/server/server/src/server.rs:323`), so a route cannot compile without one; the owner is a capability with no permissions rather than a flag; every row read or written by an account is filtered by `account_id` in `db/`; an id another account holds answers `404` from one function (`db/ownership.rs`); bulk routes check ownership per item; and a credential matrix test (`crates/server/server/src/openapi/credential_matrix.rs:1-70`) calls every documented operation with ten kinds of credential and compares the answer with the declared `security`. No Critical, High or Medium finding was found.

**Risk score: 2 / 10.**

| Severity | Count |
| --- | --- |
| Critical | 0 |
| High | 0 |
| Medium | 0 |
| Low | 5 (F1 to F5) |
| Informational | 4 (F6 to F9) |

### Top fixes, in the order they reduce risk fastest

1. **F1.** When the owner sets an account's password, end the account's live Session and its API tokens in the same transaction, as the account's own password change already does (`crates/server/server/src/credentials.rs:302-318`). Today the owner has no route that ends a Session at all.
2. **F2.** Make "no token can ever act as the owner" structural: refuse an API token whose account is the owner in `resolve_auth_on_conn` (`server.rs:1668`), and have `load_account_auth` answer `Permissions::none()` for `OWNER_ACCOUNT_ID` as it answers `DEMO_ACCOUNT_PERMISSIONS` for the Demo Account (`db/account_profile.rs:531-537`).
3. **F4.** Send `Referrer-Policy: no-referrer` and `Cache-Control: private, no-store` on the three asset routes that take a Media Link, and set `referrerPolicy="no-referrer"` on the media elements in `web/`.
4. **F3.** Refuse `POST /v1/accounts/{id}/api-tokens` for the Demo Account by its id, as ADR 0016 does for every other durable act.
5. **F5.** Check `public_registration` before hashing the password in `create_account`, and add a back-off to the per-account login counter for a server that is reachable beyond the trusted network.

## 2. Threat model and assumptions

The documented deployment is a self-hosted server on one computer or a trusted home network, speaking plain HTTP, with Docker publishing the port on `127.0.0.1` by default (`docs/src/content/docs/docs/user/features/owner/run-with-docker.md:27`, `run-on-another-machine.md:35-45`). Reaching it from the internet "needs a reverse proxy with TLS in front of the server", and the guide says of the LAN case: "anyone who can listen on that network can read them" (passwords, Session tokens, messages). The server's own login is not internet security (user memory: `login-is-not-internet-security`).

Actors, from `docs/architecture/http-api.md:473-700` ("Credentials and reach"), ADR 0008, ADR 0016, ADR 0020:

- **The owner** (account id 1): administers accounts and the server, holds no messages, reads metadata and never content.
- **An account**: everything with its own data, limited by its `import`, `export`, `delete` permissions which the owner sets.
- **An API token**: a program's credential for one account, `import` and/or `export`, never `delete`, never a Session, capped by the account's permissions on every request.
- **A Media Link**: reads one asset of one account for an hour, with no header.
- **The Demo Account** (account id 2): no password, open to anyone at the login card; may export and use the trash, may not import or delete for good.
- **A stranger**: `GET /v1/server`, `POST /v1/session`, `POST /v1/server/claim`, and `POST /v1/accounts` while registration is open.

Assumptions made where the code did not settle a question: the operator follows the Docker guide and does not publish the port before claiming; the `static_dir` points at the built website and not at the data directory; the SQLite file is reachable only by the server process.

## 3. How authorization is built

- **Credential resolution.** `resolve_auth` (`server.rs:1563`) reads `Authorization: Bearer`, then `resolve_auth_on_conn` (`server.rs:1612`) looks the token hash up as a Session (`db/session_tokens.rs:94-119`, expiry checked and an expired row deleted) and then as an API token (`db/api_tokens.rs:114-159`, `disabled` and `expires_at` checked). It then loads the account's row (`db/account_profile.rs:517-538`), refuses a disabled account (`server.rs:1654`), and builds the capability: a Session on account 1 is `Owner` (`server.rs:1663`), any other Session carries the row's permissions, and an API token carries `row permissions ∩ token permissions` (`server.rs:1668`).
- **Guards.** Ten `require_*` functions (`server.rs:121-284`) and seven newtypes from `auth_guard!` (`server.rs:323-381`): `FullAccess`, `Owner`, `LoggedIn`, `ImportAccess`, `ExportAccess`, `ImportOrExportAccess`, `FullDeleteAccess`. The owner has `Permissions::none()` (`server.rs:83-92`), so every permission guard refuses it by construction. `refuse_for_demo_account` (`server.rs:174`) is keyed on the id, and `require_import_access` and `require_delete_access` call it first.
- **Account scoping.** The credential names the account; no route takes an account parameter (`resolve_import_account`, `server.rs:1687`). Routes under `/v1/accounts/{id}` decide who the caller is to the row with `require_account_reach` (`accounts_api.rs:213-246`): the account itself, or the owner when `Admits::Owner`, with a stranger answered `403` whether or not the row exists and only the owner told `404`.
- **Row ownership.** `db/ownership.rs:19-57` (`owns_conversation`, `owns_contact`) and `missing_ids` (`:85-115`) for bulk; every list and get query joins or filters on `account_id` (section 6, item 1 and 8).
- **Middleware.** Routing, then the `Accept` check and the declared-query check as `route_layer`s (`server.rs:1248-1290`), then body limits, CORS, tracing and the request id. The guards run as extractors inside the handler, after routing, so no path reaches a handler body without its guard.

## 4. Who reaches what

| Route group | Guard | Account scoping | Checked |
| --- | --- | --- | --- |
| `GET /v1/server`, `POST /v1/server/claim`, `POST /v1/session` | none (public router, 32 KiB body) | n/a | `server_api.rs:118`, `:158`; `session_api.rs:148` |
| `POST /v1/accounts` | `Option<AuthIdentity>`: owner, or no credential while registration is open; any other credential `403` | n/a | `accounts_api.rs:342-367` |
| `GET /v1/session` | `AuthIdentity` (any credential) | caller's own | `session_api.rs:92` |
| `DELETE /v1/session` | raw bearer; API token `403` | own token | `session_api.rs:267-291` |
| `/v1/accounts` list | `Owner` | all | `accounts_api.rs:264` |
| `/v1/accounts/{id}` and everything under it | `LoggedIn` + `require_account_reach(Admits::Owner)` | target row | `accounts_api.rs:450`, `:802`, `:897`, `:1052`, `:1167`, `:1240`, `:1289`, `:1374`, `:1407`, `:1445`, `:1484` |
| `POST/PATCH .../api-tokens` | `FullAccess` + `HOLDER_ONLY` | holder only | `api_tokens.rs:249`, `:363` |
| `GET/DELETE .../api-tokens` | `LoggedIn` + `Admits::Owner` | holder or owner, hint hidden from owner | `api_tokens.rs:146`, `:216`, `:315`, `as_shown_to :184-199` |
| `/v1/audit-trail`, `/deleted-accounts`, `/v1/server/settings`, `/storage`, `/log-*`, `/demo-account` | `Owner` | all | `audit_trail_api.rs:107`, `:137`; `server_api.rs:274`, `:297`, `:387`, `:729`, `:760`; `log_files.rs:42`, `:74`; `log_lines.rs:68` |
| Browse: conversations, messages, contacts, groups, tags, saved searches, search fields, phone countries, address book | `FullAccess` | `auth.account_id` into every `db/` call | `conversations_api.rs:44-309`, `messages_api.rs:137`, `:203`, `contacts_api.rs:209-428`, `named_set_api.rs` macro, `saved_searches_api.rs:57-175` |
| Permanent deletes: `DELETE /v1/conversations/{id}`, `/contacts/{id}`, `/trash` | `FullDeleteAccess` (session + `delete`, demo refused by id) | `is_owned` then `is_trashed` | `conversations_api.rs:307`, `contacts_api.rs:426`, `trash_api.rs:31`; `db/trash.rs:281-313`, `:329-366` |
| `/v1/imports` and under | `ImportAccess` | `get_owned_import` / `require_running_import` | `imports_api/mod.rs:1140-1709`; `db/imports.rs:524-535`, `:590-596`, `:647-653` |
| `/v1/exports` and under | `ExportAccess` | `get_export(account_id, id)` and `WHERE account_id` on every write | `exports_api.rs:330-555`; `db/exports.rs:234-237`, `:260-266`, `:297-304` |
| Asset writes, uploads, `HEAD` | `ImportAccess` / `ImportOrExportAccess` | per-account directory (`assets_dir_for_account`) | `assets_api.rs:665`, `:1031`, `:1177`, `:1241`, `:1312`, `:1399`, `:1437` |
| `GET /v1/assets/{sha256}`, `/preview`, `/thumbnail` | `AssetReadAccess`: header (session, or token with `export`) or Media Link | per-account directory; `file_of(version, account, sha)` | `media_links.rs:156-190`, `assets_api.rs:718`, `:777`, `:826`, `:838-865` |
| `POST /v1/assets/{sha256}/media-links` | `FullAccess` | asset must be in the caller's store | `media_links.rs:296-335` |

Every handler in `openapi.rs:115-253` appears in this table; none is unguarded.

## 5. Findings

### F1. The owner's password set leaves the account's live Session and API tokens valid (Low)

- **CWE:** CWE-613 (Insufficient Session Expiration).
- **Evidence:** `crates/server/server/src/accounts_api.rs:1100-1111`, `replace_account_password`, `Reach::Owner` branch: `account_profile::update_password_hash(&mut tx, target, new_hash)` and an audit entry, nothing else. Contrast the self-service branch at `:1097`, `change_password_on_conn` (`credentials.rs:302-318`), which also runs `api_tokens::delete_all_api_tokens` and `session_tokens::rotate_account_session_token`. The function that would do it exists and is used by the Demo build: `session_tokens::revoke_account_sessions` (`db/session_tokens.rs:404`), called from `server_api.rs:798-810` and `db/audit_trail.rs:782`. Disabling an account (`apply_flags`, `accounts_api.rs:717-800`) refuses the Session on each request (`server.rs:1654`) but does not end it, so re-enabling revives it. No route lets the owner end a Session.
- **Why it matters:** The owner resets a password when the holder reports it lost or taken. A Session token captured on the network (the guide says plain HTTP makes this possible on the LAN) or an API token the holder did not make stays usable for up to 30 days (`SESSION_TTL_SECS`, `db/session_tokens.rs:14`), and every Media Link signed under that Session stays open with it. The behaviour is deliberate and tested: `accounts_api/tests.rs:1040-1081` asserts "bob's session carries on", and the owner guide says "Neither one ends a Session the account has open" (`docs/src/content/docs/docs/user/features/owner/owner-home.md:146`). The design chose convenience for the common case (owner hands over a first password) over the recovery case.
- **Exploitability:** Needs a stolen Session or API token first. On a trusted LAN per the deployment model this is outside the threat model; it matters when the server is reachable from an untrusted network. PoC: log in as `alice`, keep `token A`; as the owner `PUT /v1/accounts/{alice}/password`; `GET /v1/conversations` with `token A` still answers `200`.
- **Remediation:**

```rust
// accounts_api.rs, replace_account_password, Reach::Owner branch
Reach::Owner => {
    let mut tx = begin_write(&mut conn).await?;
    account_profile::update_password_hash(&mut tx, target, new_hash).await?;
    // A password the owner sets is a recovery: end what the old one opened.
    session_tokens::revoke_account_sessions(&mut tx, target, AuditActor::Owner).await?;
    api_tokens::delete_all_api_tokens(&mut tx, target).await?;
    audit_trail::record_about(&mut tx, AuditAction::PasswordSet, AuditActor::Owner, target, Details::default()).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
```

  If the hand-over case must keep the Session, the alternative is an explicit owner action (`DELETE /v1/accounts/{id}/session`) and revoking in `apply_flags` when `disabled` turns on. Update `owner-home.md:146`, `accounts_api/tests.rs:1040` and `http-api.md:505` ("a password change" ends a Media Link) to match whichever is chosen.

### F2. An API token on the owner's row would carry import, export and delete (Low, defense in depth)

- **CWE:** CWE-284 (Improper Access Control), CWE-1188 (Insecure Default Initialization of Resource).
- **Evidence:** `server.rs:1668-1670`: `Credential::ApiToken(tok) => AuthCapability::ApiToken(auth.permissions.intersect(tok))`, with `auth.permissions` from the row. `db/account_profile.rs:531-537` forces permissions only for `is_demo_account`. `schema/sql/accounts.sql:25-30`: `can_import`, `can_export`, `can_delete` default `1`, and `insert_account_at` (`db/account_profile.rs:839-857`), which writes the owner at `claim_server` (`server_api.rs:178-185`), sets none of them. So the owner's row holds `1,1,1`. Nothing on HTTP can mint such a token: `create_api_token` is `FullAccess` (rejects `Owner`) and `HOLDER_ONLY` (`api_tokens.rs:249-258`). ADR 0008 states the invariant: "the owner can never mint a token and no token can ever act as the owner."
- **Why it matters:** The invariant holds today because of two guards on one route, not because the resolver refuses it. A row written by hand, a restore from another installation, or a future route that forgets `HOLDER_ONLY` yields a credential that imports into and exports from account 1, which "holds no messages", with `resolve_import_account` returning `1`.
- **Exploitability:** Requires write access to the SQLite file or a code change; not reachable over HTTP as the code stands.
- **Remediation:**

```rust
// server.rs, resolve_auth_on_conn, before the capability match
if matches!(credential, Credential::ApiToken(_)) && account_profile::is_server_owner(account_id) {
    return Err(ApiError::AuthenticationRequired("invalid API token".into()));
}
```

```rust
// db/account_profile.rs, load_account_auth
permissions: if is_demo_account(account_id) {
    DEMO_ACCOUNT_PERMISSIONS
} else if is_server_owner(account_id) {
    crate::db::permissions::Permissions::none()
} else {
    Permissions::from_ints(import, export, delete)
},
```

  The second change also makes the owner's row in `GET /v1/accounts` report what the guards enforce, as the Demo Account's does.

### F3. The Demo Account can mint API tokens, with no expiry (Low)

- **CWE:** CWE-269 (Improper Privilege Management).
- **Evidence:** `api_tokens.rs:249-313`, `create_api_token`: `FullAccess` admits a Demo Session, `HOLDER_ONLY` admits the holder, no `refuse_for_demo_account`. Permissions are capped by `auth.permissions()` (`:261`), which for the Demo Account is `DEMO_ACCOUNT_PERMISSIONS` (export only). `expires_in_days: 0` means no expiry (`:93-96`). ADR 0016 lists the acts refused for the Demo Account by id; minting a credential is not among them, and `http-api.md:582-588` does not mention it.
- **Why it matters:** Anyone at the login card gets a durable, headless credential that reads Demo Data through Export Runs and `GET /v1/assets/{sha256}` after the visit ends, and `GET /v1/accounts/2/api-tokens` shows every visitor the masked hints of the tokens other visitors made. The data is made up, so the exposure is to Demo Data only; the token dies with `reset-demo` (`account_api_tokens ... ON DELETE CASCADE`, `accounts.sql:73`).
- **Exploitability:** Trivial: log in as `demo` with an empty password, `POST /v1/accounts/2/api-tokens {"label":"x","expires_in_days":0}`.
- **Remediation:** one line at the top of `create_api_token`: `refuse_for_demo_account(auth.account_id, "API tokens cannot be made, because anyone can enter it")?;`, and add the act to ADR 0016's list. If a demo of the token feature is wanted, cap `expires_in_days` for the Demo Account instead.

### F4. The Media Link credential travels in the URL with no referrer or cache policy (Low)

- **CWE:** CWE-598 (Use of GET Request Method With Sensitive Query Strings).
- **Evidence:** `media_links.rs:296-335` puts `media_link=<account>.<expires>.<hmac>` in `url`, `preview_url`, `thumbnail_url`. `stream_file` (`assets_api.rs:888`) sets `Content-Type`, `Accept-Ranges`, `ETag`, `Content-Range` and nothing else; a grep of `crates/server/server/src` for `Referrer-Policy`, `Cache-Control` and `no-store` finds nothing, and a grep of `web/src` for `referrerpolicy` finds nothing. The design accepts the URL as the only place a media element can carry a credential (`http-api.md:499-540`) and bounds it: one asset, one hour, HMAC over the account, the asset, the expiry and the Session hash, ended by logout, new login, password change, expiry or restart (`media_links.rs:216-249`).
- **Why it matters:** A URL is written to browser history, to any proxy log, and to the `Referer` header of a request the page makes next. Within the bound above the exposure is one attachment for an hour, which the design already judged acceptable; the two headers close the remaining paths at no cost. `logged_uri` (`server.rs:1225-1246`) already hides it from the server's own log.
- **Exploitability:** Requires reading the browser's history or an intermediary's log on the path; only matters beyond the device, and mostly when a TLS proxy or corporate proxy sits in front.
- **Remediation:** in `stream_file`, add `(header::REFERRER_POLICY, "no-referrer")` and `(header::CACHE_CONTROL, "private, no-store")` to the response headers; in `web/`, set `referrerPolicy="no-referrer"` on the `<img>`, `<video>` and `<audio>` elements that take a Media Link URL.

### F5. Login guessing and registration cost under an exposed server (Low; matters only when reachable beyond the trusted network)

- **CWE:** CWE-307 (Improper Restriction of Excessive Authentication Attempts), CWE-400.
- **Evidence:** `credentials.rs:22-23`: `AUTH_RATE_WINDOW = 60 s`, `AUTH_RATE_MAX = 20`, a sliding window with no back-off or lockout, counted per account for login (`session_api.rs:163-169`) and once for the whole server for `register` and `claim` (`accounts_api.rs:359`, `server_api.rs:164`). `create_account` (`accounts_api.rs:364-369`) hashes the password with Argon2 before it reads `public_registration`, so a closed server does the hashing work for a stranger it then refuses. `POST /v1/session` answers one sentence for every refusal and verifies a dummy hash for an unknown username (`session_api.rs:181-192`), which is right.
- **Why it matters:** 20 guesses a minute is 28,800 a day per account. On a trusted home network this is irrelevant; behind a TLS proxy on the internet it is a modest brute-force budget against a user account that may have a short password (a user account "has no minimum", ADR 0008), and the `UsernameTaken` `409` of an open registration confirms which usernames exist.
- **Exploitability:** Network reach to the server and a weak password.
- **Remediation:** move the `public_registration` check above `hash_user_password` in `create_account`; have `refuse_when_full` (`credentials.rs:125`) widen `Retry-After` with each full window (double the wait, cap at ten minutes); document in `run-on-another-machine.md` that the TLS proxy should carry its own authentication for an internet-facing server, which the memory `login-is-not-internet-security` already records as the position.

### F6. First-come claim of an unclaimed server (Informational)

- **Evidence:** `server_api.rs:158-205`, `claim_server`: no credential, rate limited server-wide, refused with `409` once claimed, inside one write transaction so two racers cannot both win. ADR 0008 "Considered and rejected: a claim token" accepts the race because an unclaimed server is empty and the operator publishes the port after claiming.
- **Assessment:** Pass for item 15, recorded so the accepted risk is visible: an operator who publishes the port before claiming hands the installation to the first visitor. The Docker guide's `-p 127.0.0.1:8080:8080` default makes the safe order the default.

### F7. `cors_origins = ["*"]` is fully permissive (Informational)

- **CWE:** CWE-942.
- **Evidence:** `server.rs:933-936`: any entry equal to `*` returns `CorsLayer::permissive()`. The default (empty list) allows only `PACKAGED_DESKTOP_ORIGINS` (`:916-920`) with `mirror_request` methods and headers (`:957-961`). No `allow_credentials`, and authentication is a Bearer header, never a cookie, so there is no CSRF path and a wildcard does not expose a token.
- **Assessment:** Pass for item 13. With `*` a page on any website can drive the three credential-free routes through a visitor's browser as a pivot into the LAN (login guesses, registration while open). The config comment says "local debugging only"; a start-up warning line when `*` is set, or refusing it outside a debug build, would make the footgun visible.

### F8. A user account changes its own password on its Session alone (Informational)

- **Evidence:** `accounts_api.rs:1067-1083` requires `current_password` only for `Reach::OwnersOwn`; ADR 0008 states the choice ("a user account changes its own on its session alone") and `http-api.md:177-180` repeats it. The change rotates the Session and deletes every API token (`credentials.rs:302-318`), so the legitimate holder is logged out everywhere.
- **Assessment:** A stolen user Session converts to a permanent takeover, and the real holder learns of it by being logged out. Documented and consistent; the owner's `current_password` requirement is the right place to spend ceremony. Recorded because F1 and F8 together mean neither party can end the other's hold without a login race.

### F9. Fixed 30-day Session, no idle timeout, same for the owner (Informational)

- **Evidence:** `db/session_tokens.rs:14`, `SESSION_TTL_SECS = 30 days`, used by `open_session` (`:279`) and rotation (`:213`). One Session per account (`accounts.sql:46-48`, `account_id` is the primary key), replaced on each login.
- **Assessment:** Reasonable for a device-bound desktop app. For the owner, whose Session reaches every account, a shorter TTL or an idle timeout is the usual hardening when the server is exposed. Not a defect under the stated model.

## 6. Checklist

| # | Item | Result | Where it was checked |
| --- | --- | --- | --- |
| 1 | BOLA / IDOR on object routes | **Pass** | `db/ownership.rs:19-57`; `get_conversation_summary` `conversations.rs:188`; `get_conversation_messages` `conversation_messages.rs:923`, `:839`; `list_conversation_source_stats` `conversations.rs:424`; `get_message` `messages_api.rs:208-212`; contacts `contacts/read.rs:347`, `contacts.rs:350-356`; `saved_searches.rs:156`, `:226`, `:248`; `named_membership.rs:453`, `:522`, `:557`; imports `db/imports.rs:530-535`, `:590-596`, `:647-653`; exports `db/exports.rs:234-237`, `:260-266`, `:297-304`; API tokens `api_tokens.rs` by `(account_id, id)`; assets by per-account directory `assets_api.rs:632-648`, `:871-884`, with `Sha256::parse` (`:114-121`) and `require_upload_id` (`asset_uploads.rs:98-104`) refusing anything that is not hex, so no path escapes the account's directory |
| 2 | Function-level authorization | **Pass** | every handler in `openapi.rs:115-253` takes a guard (section 4); `web/` gating is display only (`desktopFeatures.ts`) |
| 3 | Missing checks on sensitive endpoints | **Pass** | bulk export `exports_api.rs:330` (`ExportAccess`); server log `log_files.rs:42`, `:74`, `log_lines.rs:68` (`Owner`); Swagger UI off by default (`server.rs:1249`, `:1298`); `/health` reports "ok" only |
| 4 | RBAC mapping and deny by default | **Pass** | `AuthCapability` `server.rs:56-68`; `Owner` has `Permissions::none()` (`:83-92`); every permission guard refuses it; roles are the fixed ids `OWNER_ACCOUNT_ID`, `DEMO_ACCOUNT_ID` (`account_profile.rs:458`, `:487`), never a client field; F2 notes the one place the owner's row, not its capability, is read |
| 5 | Privilege escalation | **Pass** | `update_account` refuses `disabled`/`can_*` from a non-owner (`accounts_api.rs:826-836`, `touches_flags` `:554-559`); `create_account` takes no flags (`:290-303`); token scopes `∩` holder's (`api_tokens.rs:261`) and `can_delete` in the body is refused by `deny_unknown_fields` (`:81-96`); owner cannot be deleted (`:914`), disabled or given permissions (`:830`); demo flags fixed (`:811`) |
| 6 | JWT validation | **N/A** | no JWT. Sessions are opaque `mc-user-` tokens stored hashed with an expiry (`accounts.sql:46-54`, `session_tokens.rs:94-119`), revocable (`:404`), rotated on self password change (`credentials.rs:313`); F9 notes the TTL |
| 7 | API token scope checking | **Pass** | scopes `import`/`export` only, never `delete` (`Permissions::token`); intersected with the account on every request (`server.rs:1668`); a token is refused every browse route (`FullAccess`), `DELETE /v1/session` (`session_api.rs:280`), every `/v1/accounts/{id}` route (`LoggedIn`) and permanent deletion (`FullDeleteAccess`); F2, F3 |
| 8 | Multi-tenant isolation | **Pass** | the credential names the account (`resolve_import_account` `server.rs:1687`); no `account=` parameter exists (`declared_query.rs` refuses undeclared ones); every list filters `WHERE account_id` (section 4) |
| 9 | Bulk endpoint protections | **Pass** | export selection `missing_ids` per id (`exports_api.rs:194-230`); group/tag members `member_exists` per id and account-joined delete (`named_membership.rs:636`, `:765-773`); contact summaries `WHERE ct.account_id` (`contacts/read.rs:453`); address book load maps a `contact_id` not in the account's snapshot to a new contact, never to another account's row (`address_book.rs:627-635`); `DELETE /v1/trash` reads only the account's rows (`db/trash.rs:336-352`); import batches bound to a run the account owns (`imports_api/mod.rs:1724-1728`) |
| 10 | Field-level authorization | **Pass** | owner's views are their own types `OwnerImportRun` (`imports_api/mod.rs:1088-1118`, top-level integer counts only) and `OwnerExportRun` (`exports_api.rs:65-82`, `scope_kind` only); `token_hint` hidden from the owner (`api_tokens.rs:184-199`) and in the Audit Trail (`audit_trail_api.rs:68-73`); `top_attachments` lose the conversation for the owner (`accounts_api.rs:1255-1260`); `Account` carries `has_password`, never the hash (`:106-146`); `Details` and `AuditEntry` hold counts, labels, permissions, source and scope kind, no content (`db/audit_trail.rs:322-360`, `:367-418`) |
| 11 | Error handling and enumeration | **Pass** | another account's row is `404` everywhere outside `/v1/accounts/{id}` (`ownership.rs:1-9`); under `/v1/accounts/{id}` a stranger is `403` whether or not the row exists and only the owner learns `404` (`accounts_api.rs:206-246`); login answers one sentence and verifies a dummy hash for an unknown username (`session_api.rs:125-127`, `:189`); the credential matrix test asserts the `404` rule (`credential_matrix.rs:16-18`). Open registration's `409 UsernameTaken` is inherent to registration (F5) |
| 12 | Middleware ordering | **Pass** | `http_app` `server.rs:1248-1340`: guards are extractors and run inside the matched handler; `route_layer` checks (`Accept`, declared query) run before auth and reveal only what `/openapi.json` already publishes; `fallback_service(ServeDir)` serves static files outside `/v1` only; `/v1/{*rest}` answers a JSON `404` (`:1277-1279`) |
| 13 | CORS and CSRF | **Pass** | `build_cors_layer` `server.rs:933-962`: exact origin list plus the Tauri origins, no `allow_credentials`, Bearer auth, no cookies; F7 on the `*` escape hatch |
| 14 | Open redirect | **N/A** | no `redirect`/`next`/`returnTo` parameter in `web/src` (grep); `AuthGuard.tsx` routes to fixed paths `/login` or `/onboarding`; the server issues no redirects |
| 15 | Fallback and debug routes | **Pass** | `PUT/GET /v1/server/demo-account` are `Owner` (`server_api.rs:729`, `:760`); `reset-demo`, `create-database`, `reset-owner-password`, `create-owner` are CLI only (`cli.rs`, ADR 0008); `POST /v1/server/claim` is public by design and refused once claimed (F6); `/docs` is off unless `openapi_ui = true` |

## 7. Unable to verify

- **The web app's handling of Media Link URLs beyond the grep.** The grep of `web/src` found no `referrerPolicy`, and `serverApi.ts:505` makes the link; which component sets the URL on which element was not read. `web/src/components/` attachment viewers would prove whether any element already carries a policy (F4).
- **Whether `saved_searches::delete` answers `404` for an id the account does not hold.** `saved_searches.rs:247-249` runs an account-scoped `DELETE`; the handler (`saved_searches_api.rs:173-181`) answers `204` on `Ok`. The body of `delete` after `:249` (whether it checks `rows_affected`) was not read. Either way nothing of another account is touched.
- **The `Identity` rows the owner reads at `GET /v1/accounts/{id}/identities`.** `handles::identities(IdentitiesOf::Account)` (`db/handles.rs:447-520`) returns the account's own handles with message counts; ADR 0008 permits handles. The exact columns of `IdentityRow` were not read, so whether a first or last message date is included is unverified; a date is metadata under the ADR either way.
- **`ServeDir` behaviour when `static_dir` is misconfigured to the data directory.** Not tested; the default is `static` (`server.rs:1256-1260`).

## 8. Cross-references to the design audit

- **F1 (design) "server.rs is a God module"**: the whole authorization core, `AuthCapability` through `resolve_auth_on_conn`, lives in that module (`server.rs:56-384`, `:1563-1690`). This audit found that core correct; the design finding stands as the reason it is hard to see at a glance.
- **F18 (design) "schema check on every authenticated request"**: `resolve_auth_on_conn` calls `schema::ensure_accounts_schema` first (`server.rs:1617`). It is a performance point, not an authorization one, and is not repeated here.
- **F2 (design) "db returns the HTTP error type"**: `ownership.rs` and `trash.rs` return `sqlx::Error`/`Result`, which is the right shape for the one function that decides `404`; no authorization consequence.
