# Session and Cookie Security Audit

- Audit: session-cookie-security
- Date: 2026-10-10
- Commit: `530dc1791`
- Scope: the whole repository except `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`
- Method: read-only review of source and docs. Nothing was built or run.

Sibling audits own neighbouring ground and this report does not repeat them:
**authentication-flow-review** (login, password rules, rate limits) and
**secrets-management-audit** (token entropy, hashing, storage at rest). This
report covers the Session's lifecycle and its transport: how the token
travels, where the browser and the desktop WebView keep it, lifetime and
expiry, rotation, revocation, concurrency, logout, fixation, CSRF and CORS,
and what happens when the server address changes.

## Assumptions

1. The deployment model is the one the docs state: the server listens on
   `127.0.0.1:8080` by default (`crates/server/server/src/config.rs:194-196`,
   `default_server_bind`), on `0.0.0.0:8080` under Docker
   (`config/config.docker.toml:17`), and on every address when the desktop
   app's "open to network" switch is on (`src-tauri/src/local_server.rs:505-525`,
   `Launch::bind`). Reaching it from the internet "needs a reverse proxy with
   TLS in front of the server"
   (`docs/src/content/docs/docs/user/features/owner/run-on-another-machine.md:41`).
   Where a finding only matters once the server is reached from the internet,
   the finding says so.
2. The design uses a bearer token and no cookies. The grep for `Set-Cookie`,
   `SameSite`, `HttpOnly`, `csrf`, `xsrf`, `Sec-Fetch` and `document.cookie`
   over `crates/`, `src-tauri/src`, `web/src`, `web/index.html` and `config/`
   matched nothing in source. Every cookie item in the skill's checklist is
   therefore Not Applicable, and each is judged below against the equivalent
   bearer risk.

## Summary

- Risk score: **4 / 10** for the documented deployment (loopback or local
  network, desktop app or same-origin website). **6 / 10** when the website is
  reached from the internet or used on a shared computer, because the token
  then sits in `localStorage` for 30 days with no idle timeout and no header
  hardening on the page that holds it.
- Findings: 0 Critical, 0 High, 3 Medium, 5 Low.
- The core of the design is sound: one opaque high-entropy token per account,
  stored as a SHA-256 hash with an absolute expiry, issued fresh on every
  login, deleted on logout, never placed in a URL, never written to the
  server log, and bound to the issuing server address on the browser side.
  The gaps are in revocation on owner action and in lifetime policy, not in
  the token itself.

## How the Session works (the model this report judges)

- **Issue.** `POST /v1/session` (`crates/server/server/src/session_api.rs:148-246`,
  `create_session`) verifies the password and calls `open_session`
  (`crates/server/server/src/db/session_tokens.rs:266-307`), which generates
  `mc-user-` plus 32 alphanumeric characters (`generate_session_token`, `:21-34`),
  stores `sha256(token)` (`hash_api_token`, `:37-39`) in
  `account_session_tokens` with `expires_at = now + SESSION_TTL_SECS`
  (`:14`, 30 days), and upserts on `account_id`, which is the primary key
  (`schema/sql/accounts.sql:46-66`). One Session per account: a second login
  replaces the first and the Audit Trail records `Replaced` (`:273-275`).
  Registration opens a Session the same way
  (`crates/server/server/src/accounts_api.rs:412-421`).
- **Present.** The client sends `Authorization: Bearer <token>`
  (`web/src/lib/api.ts:174-176`, `:206-208`, `:235-237`;
  `web/src/lib/serverApi.ts:475-478`). `bearer_token`
  (`crates/server/server/src/server.rs:1532-1555`) reads it; `resolve_auth`
  (`:1563-1569`) and `resolve_auth_on_conn` (`:1612-1690`) look the hash up on
  every request with no process cache, then check `disabled`
  (`:1652-1654`). The only credential accepted in a query string is the
  `media_link` of a Media Link (`crates/server/server/src/declared_query.rs:33-58`,
  `:75-93`; `crates/server/server/src/assets_api/media_links.rs:156-188`,
  `AssetReadAccess`).
- **Expire.** `lookup_session` (`session_tokens.rs:94-119`) rejects and
  deletes a row whose `expires_at` has passed. The expiry is absolute: no
  read extends it. There is no sweeper; a row lives until its token is
  presented again or a new login replaces it, and one row per account bounds
  the table.
- **Rotate.** A login replaces the token (`open_session`). An account changing
  its own password gets a new token and loses its API tokens
  (`crates/server/server/src/credentials.rs:302-319`, `change_password_on_conn`;
  `session_tokens.rs:206-232`, `rotate_account_session_token`), and the browser
  stores the new one (`web/src/screens/settings/ChangePasswordSection.tsx:61`,
  `updateToken`; `web/src/lib/auth.tsx:320-328`).
- **Revoke.** `DELETE /v1/session` (`session_api.rs:267-289`) deletes the row
  the presented token hashes to (`revoke_session_token`, `session_tokens.rs:362-389`)
  and refuses an API token with 403. `revoke_account_sessions`
  (`:404-418`) is called by the `reset-owner-password` shell command
  (`crates/server/server/src/owner_cli.rs:100-105`), by account deletion
  (`crates/server/server/src/db/audit_trail.rs:782`,
  `prepare_account_deletion`), and by a Demo Account rebuild
  (`crates/server/server/src/server_api.rs:796-808`, `end_demo_sessions`).
  It is **not** called when the owner sets another account's password
  (`accounts_api.rs:1100-1112`) nor when the owner disables an account
  (`accounts_api.rs:717-745`, `apply_flags`); see F1.
- **Browser storage.** `persistState` (`web/src/lib/auth.tsx:127-136`) writes
  `{serverUrl, token, accountId}` as JSON under the `localStorage` key
  `message-crate-auth` (`:66`; `web/src/lib/storage.ts:12-27`, `readPref`,
  `writePref`). The server address is a separate key
  (`message-crate-server-address`, `auth.tsx:75`). On start the saved login is
  applied only when its `serverUrl` equals the saved address (`:186`), then
  validated with `GET /v1/session` (`:214-261`); a 401 clears it, a network
  failure keeps it for `retrySavedLogin` (`:246`, `:269-280`).
- **Logout.** `revokeSession` (`auth.tsx:349-361`) sends `DELETE /v1/session`
  with a 2 s timeout, then `clearSession` (`:334-346`) nulls the token, clears
  the TanStack cache and removes the stored login whether or not the server
  answered. The desktop app runs this on window close (`:461-499`); a browser
  tab does nothing on close.
- **Desktop Rust side.** The window hands the token to the `upload` and
  `export` commands per job (`src-tauri/src/commands/upload.rs:64-73`,
  `:146`; `src-tauri/src/commands/export.rs:17-24`, `:62`). It is held in
  `ImportConfig` / `ExportConfig` for the job's duration
  (`crates/libs/import/src/run.rs:95-102`; `crates/libs/export/src/run.rs:38-45`)
  and is not kept in `src-tauri/src/state.rs` (no `token` there). The shared
  `reqwest` client (`crates/libs/http/src/lib.rs:83-89`, `build_client`) uses
  the default redirect policy, `Policy::limited(10)`
  (`~/.cargo/registry/src/*/reqwest-0.12.28/src/redirect.rs:163`), and reqwest
  removes `Authorization` when a redirect changes host or port
  (`redirect.rs:239-251`, `remove_sensitive_headers`); the import client even
  diagnoses the http-to-https case (`crates/libs/http/src/session.rs:98`,
  `:152-165`, `classify_unauthorized`).
- **Media Links.** `create_media_link` (`media_links.rs:296-332`) signs
  `account_id`, `sha256`, `expires` and the hash of the Session token under a
  per-process random key (`MediaLinkKey::random`, `:58-64`); `MEDIA_LINK_TTL`
  is one hour (`:34`). `open_media_link` (`:216-249`) re-signs with the
  account's live Session hash (`live_session_hash`, `session_tokens.rs:129-146`),
  so logout, a new login, a password change, expiry or a restart ends every
  link. The server log hides the value (`server.rs:1225-1245`, `logged_uri`;
  `LOGGED_QUERY_VALUES` `:1195-1205` does not list `media_link`).

## Findings

### F1. The owner's password set and account disable leave the account's Session alive

- **Severity:** Medium
- **CWE:** CWE-613 Insufficient Session Expiration
- **Evidence:**
  - `crates/server/server/src/accounts_api.rs:1032-1035` (doc comment): "The owner setting another account's answers `204`. That is the whole of it: the account's sessions carry on."
  - `crates/server/server/src/accounts_api.rs:1100-1112`, `replace_account_password`, `Reach::Owner` branch: `update_password_hash` and an Audit Trail record, no call to `revoke_account_sessions` or `delete_all_api_tokens`.
  - `crates/server/server/src/db/session_tokens.rs:391-396` (doc on `revoke_account_sessions`): "The owner setting another account's password through `/v1` does not call this."
  - `crates/server/server/src/accounts_api.rs:717-745`, `apply_flags`: `set_account_flags` and an `AccountDisabled` Audit Trail record; the `account_session_tokens` row is untouched.
  - `crates/server/server/src/server.rs:1654-1656`, `resolve_auth_on_conn`: a disabled account's token is refused with `AccountDisabled` (403, `crates/server/server/src/problem.rs:152`) on every request, so the row is inert while disabled, and live again the moment the owner re-enables the account.
- **Why it matters.** The owner's two levers for a compromised account, "set a new password" and "disable it", are the ones that do not end the attacker's Session. After a reset the old token keeps answering for the rest of its 30 days, with the account's `import`, `export` and `delete` permissions. After a disable-then-enable, the attacker's token resumes without a new login. The only route that ends a non-owner Session is deleting the account.
- **Exploitability.** Needs a token already in an attacker's hands (shared computer, copied `localStorage`). Reproduction: (1) log in as a user and keep the token; (2) as owner, `PUT /v1/accounts/{id}/password` with a new password, answer `204`; (3) `GET /v1/session` with the old token still answers `200`. For disable: set `disabled: true` through `update_account` (`accounts_api.rs:802`), observe `403 account-disabled`; set `disabled: false`; the old token answers `200` again.
- **Remediation.** Revoke the Session and the API tokens in both paths, and change the written rule to match. The `Reach::Owner` branch already holds a write transaction:

  ```rust
  // crates/server/server/src/accounts_api.rs, replace_account_password
  Reach::Owner => {
      let mut tx = begin_write(&mut conn).await?;
      account_profile::update_password_hash(&mut tx, target, new_hash).await?;
      // The old password's login ends with it, as reset-owner-password does.
      session_tokens::revoke_account_sessions(&mut tx, target, AuditActor::Owner).await?;
      api_tokens::delete_all_api_tokens(&mut tx, target).await?;
      audit_trail::record_about(&mut tx, AuditAction::PasswordSet, AuditActor::Owner, target, Details::default()).await?;
      tx.commit().await?;
      Ok(StatusCode::NO_CONTENT.into_response())
  }
  ```

  In `apply_flags`, when `req.disabled == Some(true)` and `before.disabled` was false, call `revoke_account_sessions(tx, account_id, AuditActor::Owner)` on the caller's write transaction (the function takes `&mut WriteTx`, `session_tokens.rs:404-408`; `apply_flags` takes `&mut SqliteConnection`, so pass the transaction through or lift the call to `update_account`). Update `docs/architecture/http-api.md`, "Credentials and reach", and the doc comments quoted above. Add a test beside `session_api/tests.rs` that the old token answers 401 after each owner action.
- **Defense in depth.** Show the owner, on the account card, when the account last logged in and from which app (`connecting_app_for_account`, `session_tokens.rs:181-192` already exists), so a reset that did not end a session is visible.

### F2. A user's Session alone changes the user's password, so a stolen token becomes a permanent takeover

- **Severity:** Medium (session-lifecycle angle; the password-policy angle belongs to **authentication-flow-review**)
- **CWE:** CWE-620 Unverified Password Change
- **Evidence:**
  - `crates/server/server/src/accounts_api.rs:1028-1030`: "For a user account the session is the credential, and the current password is not asked for. The owner changing its own must send `current_password`."
  - `crates/server/server/src/accounts_api.rs:1067-1082`: `current_password` is required only when `reach == Reach::OwnersOwn`.
  - `crates/server/server/src/credentials.rs:290-301`, `change_password_on_conn` doc: "The logged-in session is the credential: the current password is not asked for."
- **Why it matters.** Every other control in this report bounds a stolen token to at most 30 days. This route removes the bound: the holder sets a password of their choosing, then logs in forever, and the real holder is locked out until the owner resets it (which, per F1, still does not end the attacker's Session). The rule exists for the owner because "a session left open on a shared machine must not be enough to take it over" (`accounts_api.rs:1029-1030`); the same shared machine holds a user's token in `localStorage` (F3).
- **Exploitability.** Needs a token (as F1). Reproduction: with a user's token, `PUT /v1/accounts/{id}/password` with `{password, password_confirmation}` and no `current_password`; answer `200` with a rotated token.
- **Remediation.** Apply the owner's rule to every account that has a password: require `current_password` when `load_password_hash` returns `Some`, and keep the no-password path for accounts the owner created without one (`accounts_api.rs:1087-1092` already distinguishes these).

  ```rust
  // accounts_api.rs, replace_account_password, before hashing
  let password_hash = account_profile::load_password_hash(&mut conn, target).await?;
  if matches!(reach, Reach::Own | Reach::OwnersOwn) && password_hash.is_some() {
      let Some(current) = req.current_password.as_deref() else {
          return Err(ApiError::validation("Current password is required to change your password."));
      };
      require_current_password(&state, target, password_hash.as_deref(), current)?;
  }
  ```

  The web form (`web/src/screens/settings/ChangePasswordSection.tsx`) then asks every account for the current password.
- **Defense in depth.** Keep the API-token wipe on self change (`credentials.rs:309`); it is right.

### F3. One fixed 30-day absolute lifetime, no idle timeout, no setting, and the website keeps the token across browser restarts

- **Severity:** Medium (Low on a single-user computer; Medium on a shared computer or when the website is reached from the internet)
- **CWE:** CWE-613 Insufficient Session Expiration; CWE-922 Insecure Storage of Sensitive Information
- **Evidence:**
  - `crates/server/server/src/db/session_tokens.rs:13-14`: `SESSION_TTL_SECS = 30 * 24 * 60 * 60`, the only lifetime, used by `open_session` (`:279`) and `rotate_account_session_token` (`:213`). No `[server]` key in `crates/server/server/src/config.rs:139-205` changes it.
  - `session_tokens.rs:94-119`, `lookup_session`: the expiry is read, never extended, and nothing records last use, so there is no idle timeout to apply.
  - `session_tokens.rs:155-173`, `record_connecting_app`: the server knows whether a Session belongs to `desktop` or `website` (`app_kind`, `schema/sql/accounts.sql:55-60`), yet both get the same lifetime.
  - `web/src/lib/storage.ts:14`, `:23`: `window.localStorage`, which survives browser restarts.
  - `web/src/lib/auth.tsx:461-499`: only the Tauri window-close path revokes the Session; a browser tab's close leaves the token in storage for the full 30 days.
  - `web/src/lib/auth.tsx:186`, `:269-280`: a login saved for address A stays in storage after the person types address B on the login card; it is overwritten only by the next successful login (`persistState`, `:308`).
- **Why it matters.** The product stores "people's private message archives" (`SECURITY.md:3`). A website visitor who logs in on a shared or borrowed computer and closes the tab leaves a working credential for a month. The desktop app is better served (window close revokes) but its lifetime is the same.
- **Exploitability.** Local access to the browser profile, or any script running on the page's origin (none found, see "What passes"). Reproduction: log in on the website, close the browser, open it a day later: the app restores the Session (`auth.tsx:180-205`).
- **Remediation.**
  1. Give website Sessions a shorter lifetime than desktop ones, from the app the login names (`connecting_app`, `server.rs:1579-1589`, already passed into `open_session`):

     ```rust
     // crates/server/server/src/db/session_tokens.rs
     pub const DESKTOP_SESSION_TTL_SECS: u64 = 30 * 24 * 60 * 60;
     pub const WEBSITE_SESSION_TTL_SECS: u64 = 24 * 60 * 60;

     fn session_ttl(app: Option<&ConnectingApp>) -> u64 {
         match app.map(|app| app.kind) {
             Some(AppKind::Desktop) => DESKTOP_SESSION_TTL_SECS,
             _ => WEBSITE_SESSION_TTL_SECS,
         }
     }
     // open_session: let expires = now_unix_secs().saturating_add(session_ttl(app));
     ```

     Add `[server] session_ttl_days` (and a website variant) to `ServerConfig` so an owner who exposes the website can shorten it further.
  2. Offer "keep me logged in" on the login card; when it is off, keep the token in `sessionStorage` (per tab, gone on close) instead of `localStorage`. `storage.ts` is the one place that chooses the store, so this is one switch.
  3. Remove the saved login when the person changes the server address on the login card (`LoginScreen.tsx:221-226` calls `setAuthServer`; call `clearPersisted` there when the address differs from the saved login's).
- **Defense in depth.** An idle timeout needs a `last_used_at` column and one write per request past some threshold; the Audit Trail already records login and end, so the cost is one more column. Not required if the lifetimes above are adopted.

### F4. The website the server serves carries no Content-Security-Policy or other hardening headers

- **Severity:** Low (becomes Medium if any XSS sink is ever added; none exists today)
- **CWE:** CWE-693 Protection Mechanism Failure; CWE-1021 Improper Restriction of Rendered UI Layers
- **Evidence:**
  - `crates/server/server/src/server.rs:1301-1302`: `.fallback_service(ServeDir::new(static_dir))` with no header layer. The grep for `SetResponseHeader`, `content-security-policy`, `x-frame-options`, `referrer-policy`, `strict-transport` over `crates/` matched nothing.
  - `web/index.html`: no `http-equiv` meta tag.
  - `src-tauri/tauri.conf.json:23-37`: the desktop WebView does have a CSP (`script-src 'self'`, `object-src 'none'`, `frame-src 'none'`, `base-uri 'self'`, `form-action 'none'`), so the two shells differ.
  - `web/src/lib/storage.ts:14`: the token is readable by any script on the origin.
- **Why it matters.** With a bearer token in `localStorage`, a CSP is the layer that turns a future XSS from "token theft" into "no exfiltration". Today the SPA has no `dangerouslySetInnerHTML`, `innerHTML`, `javascript:` or `window.open` (grep over `web/src`), and every `href={}` points at app constants with `rel="noopener"` (`web/src/screens/settings/SystemSection.tsx:132`, `:279`, `:283`; `web/src/screens/import/MissingProgramNotice.tsx:92`). The missing header is a gap in depth, not an exploit. `frame-ancestors` is absent too; modern browsers partition third-party storage, so a framed copy would appear logged out, but the header costs nothing.
- **Exploitability.** None demonstrated. Requires an injection point that does not exist in the current code.
- **Remediation.** Enable tower-http's `set-header` feature (the compiled feature list is `cors, fs, limit, set-status, trace`; `set-header` is declared but off) and wrap the static service:

  ```rust
  use tower_http::set_header::SetResponseHeaderLayer;
  use tower::ServiceBuilder;

  let static_files = ServiceBuilder::new()
      .layer(SetResponseHeaderLayer::if_not_present(
          header::CONTENT_SECURITY_POLICY,
          HeaderValue::from_static(
              "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
               img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; \
               font-src 'self' data:; object-src 'none'; base-uri 'self'; \
               form-action 'none'; frame-ancestors 'none'",
          ),
      ))
      .layer(SetResponseHeaderLayer::if_not_present(
          header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
      .layer(SetResponseHeaderLayer::if_not_present(
          header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
      .service(ServeDir::new(static_dir));
  api.fallback_service(static_files)
  ```

  If the browser build keeps the server-address field on the login card, widen `connect-src` and `media-src` to `http: https:` as `tauri.conf.json:28-30` does. `style-src 'unsafe-inline'` mirrors the Tauri CSP (`:27`); tightening it is a separate piece of work.
- **Defense in depth.** `Referrer-Policy: no-referrer` also keeps a Media Link URL out of any `Referer` the page might send; today no outbound navigation carries one, so this is belt and braces.

### F5. A `403 account-disabled` does not end the browser's Session, so the refused token stays in storage

- **Severity:** Low
- **CWE:** CWE-613 Insufficient Session Expiration
- **Evidence:**
  - `web/src/lib/routeQuery.ts:54-56`, `endsSession`: true only for `status === 401` and `type !== "invalid-credentials"`.
  - `crates/server/server/src/problem.rs:152`: `AccountDisabled` is `403 FORBIDDEN`.
  - `web/src/lib/auth.tsx:450-459`: `clearSession` runs only through `onUnauthorized` or `onSessionRefused`.
  - No `account-disabled` handling anywhere in `web/src` (grep).
- **Why it matters.** When the owner disables an account, the person's browser keeps the token in `localStorage`, every screen shows its own error, and the app never returns to the login card. Combined with F1, re-enabling the account silently revives that stored token.
- **Exploitability.** Not an attack by itself; it lengthens the life of a refused credential on disk.
- **Remediation.**

  ```ts
  // web/src/lib/routeQuery.ts
  export function endsSession(error: unknown): boolean {
    if (!(error instanceof ApiError)) return false;
    if (error.status === 401) return error.type !== "invalid-credentials";
    return error.status === 403 && error.type === "account-disabled";
  }
  ```

  Show "This account is disabled" on the login card when that is why the Session ended.

### F6. Browser `fetch` calls follow redirects with the bearer attached

- **Severity:** Low
- **CWE:** CWE-201 Insertion of Sensitive Information Into Sent Data
- **Evidence:**
  - `web/src/lib/api.ts:178-183`, `:209`, `:238-243`; `web/src/lib/serverApi.ts:479`: `fetch(...)` with the default `redirect: "follow"` and an explicit `Authorization` header.
  - `web/src/lib/auth.tsx:184-186`: the restore path keeps a token on the server that issued it, but once the base URL is set every call to it follows whatever that server, or a proxy in front of it, answers.
- **Why it matters.** A misconfigured reverse proxy that answers `/v1/...` with a redirect to another host would carry the token with it unless the browser strips it. **Unable to verify** from the repository whether every supported WebView strips `Authorization` on a cross-origin redirect (the Fetch standard added that step; browser adoption dates vary). The Rust client already has the right rule through reqwest (`redirect.rs:239-251`).
- **Exploitability.** Needs control of the server or its proxy, at which point the attacker already sees the token on the first request. Low.
- **Remediation.** One line per fetch: `redirect: "error"` for every `/v1` call in `api.ts` and `serverApi.ts`. The API never redirects (`docs/architecture/http-api.md` defines no 3xx), so nothing legitimate breaks, and a proxy that redirects fails loudly instead of silently.

### F7. No `Cache-Control: no-store` on `/v1` responses, the login response included

- **Severity:** Low
- **CWE:** CWE-525 Use of Web Browser Cache Containing Sensitive Information
- **Evidence:**
  - `crates/server/server/src/server.rs:1265-1330`: no cache header layer on the API router. The grep for `CACHE_CONTROL` and `no-store` over `crates/` matched nothing; `web/src/lib/serverHealth.ts:59` sets `cache: "no-store"` only on the health probe.
  - `crates/server/server/src/session_api.rs:33-42`: the token travels in a JSON body on `201`.
- **Why it matters.** Browsers do not cache `POST` answers by default and shared caches must not store responses to requests carrying `Authorization` (RFC 9111 section 3.5), so this is hygiene rather than a leak. It matters when a proxy or a browser extension treats bearer-authenticated `GET` answers (message bodies, attachments) as cacheable.
- **Exploitability.** Requires a non-compliant cache in the path. Low.
- **Remediation.** `route_layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))` on the `/v1` router, before the static fallback so the SPA's assets keep their own caching.

### F8. The bearer token travels in clear text on the local network when the server is opened to it

- **Severity:** Low (by documented design; the docs reserve TLS for internet reach)
- **CWE:** CWE-319 Cleartext Transmission of Sensitive Information
- **Evidence:**
  - `src-tauri/src/local_server.rs:505-507`: "The server then listens on every address of this computer, same port, over plain HTTP."; `:520-525`, `Launch::bind` returns `0.0.0.0`.
  - `config/config.docker.toml:17`: `bind = "0.0.0.0:8080"`.
  - `docs/src/content/docs/docs/user/features/owner/run-on-another-machine.md:41`: TLS "in front of the server" is named for internet reach only; `:62` tells the person to enter `http://192.168.1.20:8080`.
- **Why it matters.** The cookie `Secure` flag has no bearer equivalent: nothing on the client refuses to send the token over `http:`. On a shared Wi-Fi network the 30-day token (F3) is readable by anyone on the segment. The desktop app will talk `https://` to a private authority (`run-on-another-machine.md:74-77`), so the client side is ready.
- **Exploitability.** Passive sniffing on the same network segment. Only matters when "open to network" or the Docker bind is used on a network the owner does not trust.
- **Remediation.** Say in the "open to network" switch's help text and in `run-on-another-machine.md` that the token is sent in clear over the local network, and that a TLS proxy applies there too when the network is shared. No code change is required for the documented single-household case.

## What passes

- **Fixation.** The server never accepts a client-chosen token: `open_session` generates one (`session_tokens.rs:276`) and nothing pre-login is upgraded. `POST /v1/accounts` opens a fresh Session for a stranger (`accounts_api.rs:412-421`). Pass.
- **Regeneration on login and on self password change.** `open_session` upsert (`:282-294`), `rotate_account_session_token` (`:206-232`). Pass.
- **Logout invalidates server side.** `revoke_session_token` deletes the row in one write transaction and records `LoggedOut` (`:362-389`). `delete_session` works without a guard so a disabled account can still end its own Session (`session_api.rs:253-257`). Pass.
- **No token in a URL.** The only query credential is `media_link` (`declared_query.rs:33-58`), scoped to one asset of one account for one hour and bound to the live Session (`media_links.rs:216-249`). `downloadAttachment` in a browser fetches with the header and saves a blob (`web/src/lib/downloadAttachment.ts:27-28`); the desktop app downloads by Media Link so the Rust side never holds the Session for a download (`:24-25`). Pass.
- **No token in the server log.** The request span logs method, `logged_uri` and request id (`server.rs:1323-1335`); `logged_uri` hides every query value not in `LOGGED_QUERY_VALUES` (`:1225-1245`), and headers are not logged. On the desktop side, `UploadArgs`, `ExportArgs`, `ImportConfig` and `ExportConfig` derive `Debug` with the token as a plain `String` (`upload.rs:64-72`, `export.rs:17-24`, `import/src/run.rs:94-102`, `export/src/run.rs:37-45`), but no `{:?}`, `?cfg` or `?args` formatting of them exists outside tests (grep). Pass, with a note: a `Debug` impl that redacts `token` would make this hold by construction.
- **Token bound to the issuing server in the browser.** The restore path applies a saved login only when `persisted.serverUrl === serverUrl` (`auth.tsx:186`), `retrySavedLogin` does the same (`:271`), and `setBaseUrl` is called before login only (`LoginScreen.tsx:221`, `web/src/screens/auth/*.tsx`, `useServerState.ts:35`). Pass.
- **Desktop WebView.** `tauri.conf.json:24-36` CSP has `script-src 'self'` and no `unsafe-inline` or `unsafe-eval` for scripts; `connect-src` and `media-src` are wide (`http: https:`) because the person types the server address. Capabilities are `core:default`, window destroy, dialog open/save, shell open (`src-tauri/capabilities/default.json`). Pass.
- **CORS.** `build_cors_layer` (`server.rs:933-959`) is an exact allow list plus the three Tauri origins (`PACKAGED_DESKTOP_ORIGINS`, `:916-920`), mirrors requested methods and headers so `Authorization` passes preflight, and never sets `allow_credentials`, so no `Access-Control-Allow-Credentials` is emitted. `["*"]` gives `CorsLayer::permissive()` (`:934-936`), documented as local debugging only; with no cookie there is still nothing a foreign page can send on the person's behalf. Pass.
- **CSRF.** No ambient credential exists: the token is attached by script from the page's own storage, every body is JSON (`crate::extract::Json`), and the API requires an `Accept` that admits JSON (`require_json_acceptable`, `server.rs:1282`). A cross-site form post cannot carry the header. Not Applicable, and nothing is needed.
- **Storage.** Sessions live in SQLite (`account_session_tokens`), not in process memory; "rotate/delete in Settings takes effect without restarting serve" (`server.rs:1565-1566`). Expired rows are removed on presentation (`lookup_session`, `:109-112`) and one row per account bounds the table. Pass.
- **Audit Trail.** Every open, replacement, logout, revocation and own password change writes an entry (`open_session`, `revoke_session_token`, `revoke_account_sessions`, `record_own_password_change`). Pass.
- **Demo Account.** Its rebuild ends every demo Session (`server_api.rs:796-808`) and it cannot change its password (`accounts_api.rs:1062`). Pass.

## Design notes (not findings)

- One Session per account means a login on a second device ends the first device's Session silently; the first device sees a 401 and lands on the login card with no explanation (`auth.tsx:450-459`). For the Demo Account, every visitor logs the previous visitor out. A short "You were logged out because this account logged in elsewhere" notice would help; the server already records `Replaced`.
- The password change renews the Session for another full lifetime (`rotate_account_session_token`, `:213`). That is reasonable; it is noted because the renewed expiry is the Audit Trail's view of the Session, not the login's.
- reqwest keeps `Authorization` on a redirect to the same host and port with a different path or scheme (`redirect.rs:241-242` compares host and `port_or_known_default` only). An `https://host:8080` to `http://host:8080` downgrade would carry the header. Only the server or its proxy can produce that redirect, so this is noted and not scored.

## Checklist

| Item | Result | Evidence or justification |
|---|---|---|
| 1. Session configuration: `Secure` flag | Not Applicable | No cookie. Bearer equivalent: nothing refuses `http:`; the documented local-network deployment sends the token in clear (F8). |
| 1. `HttpOnly` flag | Not Applicable | No cookie. Bearer equivalent: the token is script-readable in `localStorage` by design (`storage.ts:14`); mitigated by no XSS sink and the Tauri CSP, not by a header on the website (F4). |
| 1. `SameSite` attribute | Not Applicable | No cookie, so no ambient credential and no cross-site send to prevent. |
| 1. Session timeout | Fail | Absolute 30 days, no idle timeout, not configurable, same for website and desktop (F3). |
| 1. Session regeneration after login | Pass | Fresh token on every login (`open_session`); no pre-login session to fix. |
| 2. Cookie security: flags | Not Applicable | No `Set-Cookie` anywhere in source. |
| 2. No sensitive data in cookies | Not Applicable | No cookie. The bearer carries no claims; it is an opaque lookup key (`session_tokens.rs:21-34`). |
| 2. Domain and path scoping | Not Applicable | No cookie. Bearer equivalent: the browser sends the token only to the `serverUrl` that issued it (`auth.tsx:186`). Pass. |
| 2. Encryption for sensitive cookies | Not Applicable | No cookie. The stored server-side value is a SHA-256 hash (`hash_api_token`); entropy and hashing choices belong to **secrets-management-audit**. |
| 3. CSRF token implementation | Not Applicable | Header-borne bearer, JSON bodies, no ambient credential. |
| 3. Double submit cookie | Not Applicable | No cookie. |
| 3. Origin header validation | Pass | CORS exact allow list, no credentials flag (`build_cors_layer`). No separate `Origin` check is needed without cookies. |
| 4. Not default in-memory storage | Pass | SQLite table `account_session_tokens` (`schema/sql/accounts.sql:46-66`). |
| 4. Database-backed sessions | Pass | Every request reads the row (`resolve_auth`, `server.rs:1563-1569`). |
| 4. Session cleanup and expiration | Pass with note | Lazy delete on presentation (`lookup_session`), bounded by one row per account; no sweeper. |
| Bearer: XSS theft from storage | Pass with note | No sink found in `web/src`; CSP present in the desktop shell, absent on the website (F4). |
| Bearer: token in URL | Pass | Only `media_link`, scoped and session-bound (`media_links.rs`). |
| Bearer: token in logs | Pass | `logged_uri` redaction; headers not logged; no `Debug` formatting of configs that hold the token. |
| Bearer: cross-origin leakage | Pass with note | Server-keyed restore; reqwest strips on host change; browser `fetch` redirect behaviour Unable to verify (F6). |
| Bearer: revocation on owner action | Fail | Owner password set and disable do not revoke (F1). |
| Bearer: re-authentication for sensitive change | Fail | Own password change needs the Session only (F2). |

## Prioritized fixes

1. **F1**: call `revoke_account_sessions` (and `delete_all_api_tokens`) when the owner sets another account's password, and when the owner disables an account; update the two doc comments and `http-api.md`. One function already exists; the change is two call sites and a test.
2. **F2**: require `current_password` for any account that has one. Mirrors the owner's branch that already exists.
3. **F3**: website Sessions get a shorter lifetime than desktop ones, from `app_kind`; add a `[server]` setting; offer `sessionStorage` when "keep me logged in" is off; clear a saved login when the address changes.
4. **F4 + F7**: one `SetResponseHeaderLayer` stack on the static fallback (CSP, `Referrer-Policy`, `nosniff`) and `Cache-Control: no-store` on `/v1`. Enable tower-http `set-header`.
5. **F5 + F6**: `endsSession` handles `403 account-disabled`; every `/v1` fetch passes `redirect: "error"`.

## Unable to verify

- Whether every WebView the desktop app runs in (WebKitGTK, WKWebView, WebView2) strips `Authorization` on a cross-origin redirect from `fetch`. Proving it needs a redirecting test server and the three platforms; the code that would make it moot is the `redirect: "error"` option in `web/src/lib/api.ts` (F6).
- Whether a reverse proxy in front of a Docker deployment adds `Set-Cookie`, `Strict-Transport-Security` or cache headers. Out of the repository; `config/config.docker.toml` and `Dockerfile` carry none.
- Whether the Tauri WebView's `localStorage` for `tauri://localhost` is readable by other local processes at rest. That is the desktop operating system's profile protection, and belongs to **secrets-management-audit**.
- The Swagger UI at `/docs` can persist a bearer in browser storage when enabled; `openapi_ui` defaults to `false` (`config.rs:203-206`) and no `persistAuthorization` setting appears in `crates/server/server/src` (grep), so it is off by default and not scored.

## Cross-references

- `audits/2026-10-09-initial-software-design-analysis.md` **F18** (schema check on every authenticated request, `resolve_auth_on_conn` calls `ensure_accounts_schema`, `server.rs:1620`): the same hot path this report reads; the fix there does not change any session rule here.
- Same audit **F14** (`web/src/lib/auth.tsx` imports UI components): the file this report cites for persistence and logout; a split would move `persistState`, `loadPersisted` and `clearSession` into a `lib/session.ts`, which is also where the `sessionStorage` switch of F3 would live.
- **authentication-flow-review** owns the password rules behind F2 and the rate limit on `POST /v1/session` (`session_api.rs:163-169`).
- **secrets-management-audit** owns `generate_prefixed_token` (`session_tokens.rs:26-34`, including the `% 62` mapping), the unsalted SHA-256 storage, and `MediaLinkKey::random`.
