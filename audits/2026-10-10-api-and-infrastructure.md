# API and infrastructure security audit

- **Date:** 2026-10-10
- **Commit:** `530dc1791` (branch `docs/initial-software-design-analysis`)
- **Scope:** the whole repository less `web-next/`, `vendor/`, `target/`, `node_modules/` and `docs/dist`: the server (`crates/server/server/src`), the web SPA (`web/src`), the desktop app (`src-tauri/`), Docker (`docker/`, `config/config.docker.toml`) and CI (`.github/workflows/`)
- **Method:** read-only review of source, configuration and documentation. Nothing was built, run or tested. Every line number below was read at this commit.
- **Companion report:** `audits/2026-10-09-initial-software-design-analysis.md` (findings F1 to F19). Where a remediation here lands in a module that report proposes, the cross-reference is given instead of repeating it.

## 1. Threat model used for severity

The repository states its deployment model, and every severity below is rated against it:

- "Message Crate is built for a home network, not for the open internet. If you want to expose Message Crate to the internet, you should stick it behind a reverse proxy with an auth layer of your choosing." (`README.md:130-131`)
- "The server speaks plain HTTP. Passwords, Session tokens, and messages cross the network unencrypted ... That is acceptable on a home network where every device is trusted. Anything beyond that, such as reaching the Message Crate from the internet, needs a reverse proxy with TLS in front of the server." (`docs/src/content/docs/docs/user/features/owner/run-on-another-machine.md:36-42`)
- The server binds `127.0.0.1:8080` unless told otherwise (`crates/server/server/src/config.rs:194-196`). The Docker image binds `0.0.0.0:8080` inside the container (`config/config.docker.toml:17`) and relies on the port publishing to limit reach (`config/config.docker.toml:11-15`).
- The documented first-run Docker command publishes on loopback only: `-p 127.0.0.1:8080:8080` (`docs/src/content/docs/docs/user/features/owner/run-with-docker.md:27,40`). Opening the server to the LAN is a documented, deliberate step (`run-on-another-machine.md:18-30`).
- The desktop app starts the shipped server on `127.0.0.1:8080` (`src-tauri/src/local_server.rs:68`) and offers a setting that rebinds it to `0.0.0.0` (`src-tauri/src/local_server.rs:520-526`), with on-screen copy that names the plain-HTTP risk (`web/src/screens/settings/SystemSection.tsx:347-351`).

So the baseline is: one process, one SQLite file, loopback by default, plain HTTP, trusted LAN as the widest intended reach, and TLS plus an extra auth layer delegated to a reverse proxy for anything wider. A finding that only matters once the server is reachable by untrusted devices says so.

**Assumptions stated:** the reverse proxy, when used, is not in this repository and is not audited. `web-next/` is out of scope. The Playwright and browser tooling was not used.

## 2. Summary

| Severity | Count |
|---|---|
| Critical | 0 |
| High | 0 |
| Medium | 3 |
| Low | 8 |
| Informational (recorded as Pass) | 7 |

**Risk score: 4 / 10** under the stated model (loopback or trusted home network). The same code reachable by untrusted devices without the documented TLS proxy would rate about 7 / 10, driven by findings M1, M2 and L1.

The server's core API hygiene is strong: one problem-document error shape with no stack traces, a request id on every response, body caps on every route with a smaller cap on the credential routes, exact-origin CORS with no cookies, hashed credentials with constant-time media-link verification, OS-random tokens, and SQL confined to `db/`. The gaps are at the edges: the published compose file opens the port wider than the documentation's first command, the password policy allows an account with no password and an owner password of one character, and the browser-served SPA carries no security headers.

### Top five fixes, in order of risk reduced per hour

1. **M1:** change `docker/compose.yml:19` to `"127.0.0.1:8080:8080"` so the published compose file matches the documented first-run command, and add a `HEALTHCHECK`.
2. **M2:** refuse an empty password on self-registration and a password under a minimum length for the owner and for any password a person sets on their own account; keep the owner's power to create a passwordless account as an explicit choice.
3. **M3:** add a `SetResponseHeaderLayer` stack in `http_app` (Content-Security-Policy, `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Referrer-Policy`, `Cache-Control: no-store` on `/v1`).
4. **L1:** add a request-header and body timeout and a concurrency ceiling in front of `axum::serve`.
5. **L3:** take the two `:5173` development origins out of `config/config.docker.toml`.

## 3. Findings

Each finding gives: severity, CWE, evidence, why it matters, exploitability, remediation and defence in depth.

---

### M1. The published compose file publishes port 8080 on every host interface

- **Severity:** Medium
- **CWE:** CWE-1188 (Initialization of a Resource with an Insecure Default), CWE-319 (Cleartext Transmission of Sensitive Information)

**Evidence**

- `docker/compose.yml:1` says "Save this file and run: docker compose up -d", and `docker/compose.yml:19` publishes `"8080:8080"`, which Docker binds on `0.0.0.0`.
- `docker/compose.release.yml:31` publishes `"0.0.0.0:8080:8080"` explicitly.
- The documented first-run command is `-p 127.0.0.1:8080:8080` with the explanation "Makes the server reachable at port 8080 from this computer only" (`run-with-docker.md:27,40`), and the LAN variant is a separate, later step with a warning section (`run-on-another-machine.md:18-45`).
- `config/config.docker.toml:11-15` acknowledges the exposure ("Compose publishes port 8080 on every interface of the host ... Restrict that with the host firewall") but the container has to bind `0.0.0.0` for publishing to work at all (`config/config.docker.toml:17`), so the only place the reach is chosen is the compose file.
- Neither user-guide page mentions the compose file (grep for `compose` over both pages returns nothing), so a person who downloads `compose.yml` follows its own header and gets a different default from the one the guide teaches.

**Why it matters**

The server speaks plain HTTP (`run-on-another-machine.md:36-37`), accounts may have no password (M2), and `GET /v1/server` plus the login card are reachable by anyone who can connect. A laptop on a cafe network or a shared office LAN that runs `docker compose up -d` from the published file is reachable by every device on that network with no step that says so.

**Exploitability**

Any device on the same network: `curl http://<host>:8080/v1/server` answers `state`, `version` and whether the Demo Account exists (`server_api.rs:56-83`), and `POST /v1/session` with `{"username":"demo","password":""}` logs in to the Demo Account (`owner-home.md:71`). Against a real account with no password (M2) the same request logs in.

**Remediation**

```yaml
# docker/compose.yml
    ports:
      # This computer only. To let other devices on a trusted network
      # connect, change the line to "8080:8080" and read
      # https://messagecrate.app/docs/user/features/owner/run-on-another-machine/
      - "127.0.0.1:8080:8080"
```

Apply the same to `docker/compose.release.yml:31`, which is a developer file but is "closer to what you ship" (`compose.release.yml:2`).

**Defence in depth**

- Add a `HEALTHCHECK` to `docker/Dockerfile` (none exists; the file ends at `ENTRYPOINT`, `Dockerfile:86`), so `docker ps` and orchestrators see a server that stopped answering: `HEALTHCHECK --interval=30s --timeout=3s CMD ["message-crate-server", "health", "--url", "http://127.0.0.1:8080/health"]` once such a subcommand exists, or install `curl` and probe `/health` (`server.rs:1523-1525`).
- Log a one-line warning at startup when the bind address is not loopback (`server.rs:1410-1414` has the listener; the `say!` on `:1417` is the natural place), naming the risk the docs name.

---

### M2. Password policy: accounts may have no password, and the owner's may be one character

- **Severity:** Medium (only once the server is reachable by other devices; informational on loopback)
- **CWE:** CWE-521 (Weak Password Requirements), CWE-306 (Missing Authentication for Critical Function) for the passwordless path

**Evidence**

- `crates/server/server/src/credentials.rs:208-216` `hash_user_password`: "There is no minimum length: an empty password is `None`, an account with no password."
- `credentials.rs:198-206` `hash_owner_password`: "one character is enough."
- Self-registration takes the empty password: `crates/server/server/src/accounts_api.rs:364` `hash_user_password(req.password.as_deref().unwrap_or(""))` runs for the stranger's path (`by_owner == false`, `:343-360`) as well as the owner's.
- A person changing their own password may remove it: `accounts_api.rs:1088-1092` chooses `hash_user_password` for every non-owner target.
- Login accepts an empty password for a NULL hash: `credentials.rs:186-196` `verify_login_password`, and `session_api.rs:210-211` calls it.
- A new account gets every permission, delete included: `crates/server/server/src/db/account_profile.rs:806-807` ("The new account gets every permission (`Permissions::all()`)"), backed by `schema/sql/accounts.sql:25-30` (`DEFAULT 1`).
- The guess rate is 20 per 60 seconds per account (`credentials.rs:22-23` `AUTH_RATE_WINDOW`, `AUTH_RATE_MAX`), which is 28,800 guesses a day against one account with no lockout.
- The user guide documents the passwordless account and advises against it: "The form allows an account with no password. An account that holds real messages should have one." (`docs/src/content/docs/docs/user/your-messages/create-the-owner-and-an-account.md:35`). The guide also states that on a LAN "The Owner's password is the only thing between the network and every account." (`run-on-another-machine.md:45`).

**Why it matters**

The design's one protection on a LAN is the password. A policy that allows none, or one character, puts the whole archive behind a choice the form invites with "Password (optional)" (`owner-home.md:110`). Argon2 with a per-hash salt (`credentials.rs:148-156`) protects the stored hash, not an online guess at a one-character password, and the per-account limiter allows over 28,000 guesses a day.

**Exploitability**

Needs network reach to the server (see M1 or the desktop toggle). `POST /v1/session` with `{"username":"<name>","password":""}` for a passwordless account; or a scripted loop within the 20-per-minute limit for a short owner password. The owner's session reaches the accounts collection, settings and the server log (`docs/architecture/http-api.md:479-483,673-676`).

**Remediation**

Keep the owner's ability to make a passwordless account for a trusted household member, but make it an explicit act, and never the default of a stranger's registration or of a self-set password.

```rust
// crates/server/server/src/credentials.rs
/// The fewest characters a password a person sets for themself may have.
pub(crate) const MIN_PASSWORD_CHARS: usize = 8;

fn require_minimum_length(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ApiError::validation(format!(
            "password must be at least {MIN_PASSWORD_CHARS} characters"
        )));
    }
    Ok(())
}

pub(crate) fn hash_owner_password(password: &str) -> Result<String, ApiError> {
    require_minimum_length(password)?;   // replaces the is_empty() check at :201
    require_hashable(password)?;
    Ok(hash_password(password)?)
}

/// A password an account sets for itself: never empty, never short.
pub(crate) fn hash_self_set_password(password: &str) -> Result<String, ApiError> {
    require_minimum_length(password)?;
    require_hashable(password)?;
    Ok(hash_password(password)?)
}
```

```rust
// crates/server/server/src/accounts_api.rs:364 (registration)
let password_hash = if by_owner {
    hash_user_password(req.password.as_deref().unwrap_or(""))?   // owner may still make a passwordless account
} else {
    Some(hash_self_set_password(req.password.as_deref().unwrap_or(""))?)
};
```

And at `accounts_api.rs:1088-1092`, use `hash_self_set_password` when `reach` is `Reach::Own`, leaving `hash_user_password` for `Reach::Owner`.

**Defence in depth**

- Add a per-account lockout after N consecutive failures (say 10 in 10 minutes, with `Retry-After`), in `credentials.rs` beside `check_auth_rate_limit` (`:54-56`), so the day's guess budget falls from 28,800 to a few hundred.
- When an account has no password, show it on the owner's account list (the profile already carries `is_demo`; a `has_password` flag beside it costs one column read in `account_profile.rs:523`).
- Consider refusing `set_open_to_network` and a non-loopback `--bind` when any non-demo account has no password, or at least say so in the startup line.

---

### M3. No HTTP security headers on the browser-served SPA or the API

- **Severity:** Medium
- **CWE:** CWE-693 (Protection Mechanism Failure), CWE-1021 (Improper Restriction of Rendered UI Layers), CWE-16 (Configuration)

**Evidence**

- A grep over `crates/server/server/src` for `Content-Security-Policy`, `X-Frame-Options`, `Strict-Transport-Security`, `Referrer-Policy`, `SetResponseHeader` and `Permissions-Policy` finds nothing. The only security header set anywhere is `x-content-type-options: nosniff` on asset downloads (`crates/server/server/src/assets_api.rs:966-969`).
- `http_app` (`server.rs:1248-1342`) stacks `DefaultBodyLimit` (`:1305`), the body limiter, the 413 rewrite, CORS (`:1317`), `TraceLayer` (`:1323`) and the request id (`:1340`); no header layer. The SPA is served by `ServeDir` (`:1302`) with the same stack.
- `web/index.html:1-58` carries no `<meta http-equiv="Content-Security-Policy">`.
- The session token is kept in plaintext `localStorage` under `message-crate-auth` (`web/src/lib/auth.tsx:66`, `persistState` at `:127-136`), so any script that runs in the page's origin can read it.
- A grep over `web/src` for `Cache-Control` on the server side finds nothing, so a `GET /v1/session` or `/v1/accounts/{id}` answer carries no `no-store`.
- Mitigating facts: no `dangerouslySetInnerHTML`, `innerHTML` or `srcdoc` in `web/src` (grep), React escapes text by default, the app uses `HashRouter` (`web/src/main.tsx:4,16`) so routes never reach `Referer`, and the desktop app's own CSP is strict (`src-tauri/tauri.conf.json:24-36`).
- HSTS is not applicable: the server speaks plain HTTP by design and the TLS proxy is outside the repository (`run-on-another-machine.md:41`).

**Why it matters**

The SPA renders imported message text, contact names and attachment previews from other people's phones. React's escaping is one layer; a CSP is the second, and the token's exposure to any script in the origin is what makes the second one worth having. `X-Frame-Options` closes clickjacking of the owner's settings screen from another site on the LAN. `nosniff` on every response closes content-type confusion on the JSON and HTML the server serves itself.

**Exploitability**

No vector was found in this audit; the finding is the absence of a second layer. An XSS in a dependency or a future screen would read `localStorage["message-crate-auth"]` and hold the 30-day session (L4). The server runs no reflected-HTML paths: every failure is `application/problem+json` (`server.rs:775-790`), and `api_not_found` echoes the path into a JSON `detail` only (`server.rs:973-975`).

**Remediation**

Enable tower-http's `set-header` feature (`crates/server/server/Cargo.toml:41` currently lists `["limit", "cors", "fs", "trace"]`) and add the headers below `build_cors_layer` in `http_app`. The `connect-src` must allow other hosts because the website lets a person enter another server's address (`run-on-another-machine.md:62`), the same reason the Tauri CSP does (`tauri.conf.json:28`).

```rust
// crates/server/server/src/server.rs, in http_app, after .layer(build_cors_layer(&cors_origins))
use tower_http::set_header::SetResponseHeaderLayer;
use axum::http::{header, HeaderValue};

.layer(SetResponseHeaderLayer::if_not_present(
    header::CONTENT_SECURITY_POLICY,
    HeaderValue::from_static(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob: http: https:; media-src 'self' blob: http: https:; \
         connect-src 'self' http: https:; font-src 'self' data:; object-src 'none'; \
         base-uri 'self'; form-action 'none'; frame-ancestors 'none'",
    ),
))
.layer(SetResponseHeaderLayer::if_not_present(
    header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"),
))
.layer(SetResponseHeaderLayer::if_not_present(
    header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"),
))
.layer(SetResponseHeaderLayer::if_not_present(
    header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"),
))
```

For `Cache-Control: no-store` on `/v1` only, add it as a `route_layer` on `api` before `.merge(health_router)` (`server.rs:1278-1286`), so the static files keep their own caching. The design audit's F1 proposes a `middleware` module for exactly this stack; put the layer there if that split lands first.

`web/index.html:7-52` has an inline script for the theme; under `script-src 'self'` it needs a hash or a move to a file. Add `'sha256-<hash>'` to `script-src` or move the block into `web/src/theme-boot.ts`.

**Defence in depth**

- Serve the token to the SPA with a shorter idle timeout (L4) so a stolen token has less life.
- Mirror the header set in the reverse-proxy documentation so the TLS proxy adds `Strict-Transport-Security`.

---

### L1. No request timeouts or concurrency ceiling in front of the listener

- **Severity:** Low (Medium if the server is reachable by untrusted devices)
- **CWE:** CWE-400 (Uncontrolled Resource Consumption)

**Evidence**

- `server.rs:1461-1467` `serve_until_shutdown` calls `axum::serve(listener, app).with_graceful_shutdown(...)` with no `TimeoutLayer`, `ConcurrencyLimitLayer`, `LoadShedLayer` or header-read timeout; a grep of `server.rs` for `timeout`, `TimeoutLayer`, `ConcurrencyLimit`, `LoadShed` and `header_read_timeout` finds only a comment about SQLite's `busy_timeout` (`:394`).
- `limit_request_body` caps size (`server.rs:1033-1064`), not time: a body under `MAX_REQUEST_BODY_BYTES` (512 MiB, `:1021`) may arrive at one byte a minute.
- The import batch route streams up to 512 MiB to a temp file per request (`crates/server/server/src/imports_api/mod.rs:1734-1738`); the attachment upload streams to disk under the settings cap (`assets_api.rs:1068`).
- The import run itself is serialised by a global semaphore (design audit F7), which limits the damage from a slow import but not from many slow connections.

**Why it matters**

A handful of slow or idle connections hold hyper tasks and, for uploads, file handles and disk. On loopback this is a self-inflicted problem; on a LAN with a misbehaving device it is a denial of service against a server with one SQLite file and a single import slot.

**Exploitability**

Trivial once reachable: open N TCP connections and send headers slowly, or `PUT /v1/assets/<sha>` with `Content-Length: 500000000` and send nothing.

**Remediation**

```rust
// Cargo.toml: tower-http features += "timeout"; add tower = { version = "0.5", features = ["limit", "load-shed"] }
// server.rs, in http_app, after the request-id layer:
.layer(tower::limit::ConcurrencyLimitLayer::new(256))
// and on the API router only, so a 1 GiB export download is not cut off:
.route_layer(tower_http::timeout::RequestBodyTimeoutLayer::new(Duration::from_secs(120)))
```

A header-read timeout needs the hyper-util builder rather than `axum::serve`; the smaller first step is the two layers above. Choose the timeout per route class: the JSON routes finish in milliseconds; the asset `PUT` and the import batch may need a per-chunk idle timeout rather than a total.

**Defence in depth**

The documented reverse proxy (nginx, Caddy) already applies header and body timeouts; say so in the TLS-proxy guidance so the person who exposes the server gets them.

---

### L2. Rate limiting covers the credential routes only, with one flat figure

- **Severity:** Low
- **CWE:** CWE-307 (Improper Restriction of Excessive Authentication Attempts), CWE-770 (Allocation of Resources Without Limits)

**Evidence**

- The limiter guards `POST /v1/session` (`session_api.rs:169`), `POST /v1/accounts` for strangers (`accounts_api.rs:359`), `POST /v1/server/claim` (`server_api.rs:164`) and the `current_password` checks (`docs/architecture/http-api.md:692-696`), as the rules document says (`http-api.md:688-706`).
- One figure for all buckets: `AUTH_RATE_MAX = 20` in `AUTH_RATE_WINDOW = 60 s` (`credentials.rs:22-23`). Per-account for passwords, server-wide for register and claim (`credentials.rs:27-50`).
- No limit on any authenticated route, on `POST /v1/assets/{sha256}/media-links` (`crates/server/server/src/assets_api/media_links.rs:296`), or on search (`search/lex.rs:9` caps query length at 2 KiB, not rate).
- The limiter is a process-local map on `AppState` (`credentials.rs:35` `AuthRateLimits`), pruned on every call (`:105-108`).
- The rules reject per-address counting with a stated reason (behind a proxy every visitor shares one address) (`http-api.md:702-706`).

**Why it matters**

Against the skill's checklist, "implemented on all endpoints" fails. Against the stated model the gap is small: every other route needs a credential, and the one unauthenticated read, `GET /v1/server`, costs three SQLite reads (`server_api.rs:118-130`). The real weakness is the per-account password budget (M2).

**Exploitability**

A credential holder can hammer search or asset reads; a stranger can only call `GET /v1/server` and `/health` fast. Neither is a confidentiality issue.

**Remediation**

- Lower the password bucket and add backoff (M2's defence in depth).
- If the server is to be reachable by other devices, a modest server-wide ceiling on `/v1` (for example `tower_governor` keyed by nothing, 500 requests per second) protects SQLite without counting addresses.
- "Distributed rate limiting" is not applicable: one process owns one SQLite file (`docs/adr/0017-sqlite-is-the-only-database-engine.md`, and the operation lock in `server.rs:1372`).

---

### L3. CORS: development origins ship in the production image, and `"*"` is a one-word footgun

- **Severity:** Low
- **CWE:** CWE-942 (Permissive Cross-domain Policy with Untrusted Domains)

**Evidence**

- `build_cors_layer` (`server.rs:933-960`): any origin equal to `"*"` returns `CorsLayer::permissive()` (`:934-936`); otherwise an exact allow list via `AllowOrigin::list` (`:956`) with `AllowMethods::mirror_request()` and `AllowHeaders::mirror_request()` (`:957-958`), always including the three packaged desktop origins (`PACKAGED_DESKTOP_ORIGINS`, `:916-920`).
- The Docker image's config lists `http://localhost:5173` and `http://127.0.0.1:5173` (`config/config.docker.toml:24-30`), the Vite dev server's port (`web/vite.config.ts:56-58`), beside the desktop origins that are built in anyway.
- No cookies are used; the credential is a Bearer header (`web/src/lib/api.ts:174-176`), and `allow_credentials` is never set, so the exact-list mode cannot leak a credential by itself.
- `scripts/run-dev.sh:116-119` uncomments the same list for development, which is where it belongs.

**Why it matters**

A page served by any development server on port 5173 of the viewer's own machine may call a production Message Crate's API cross-origin. It still needs a token, so the gain for an attacker is small, but the two lines have no production purpose. `"*"` with `permissive()` also mirrors any origin; the comment says "local debugging only" (`:929`) but nothing stops it in a shipped config.

**Remediation**

```toml
# config/config.docker.toml: remove the two :5173 lines.
cors_origins = [
  "https://tauri.localhost",
  "http://tauri.localhost",
  "tauri://localhost",
]
```

And in `build_cors_layer`, log at `warn` when `"*"` is used, or refuse it unless `cfg!(debug_assertions)`.

---

### L4. Session lifetime is a fixed 30 days with no idle timeout, and the token lives in `localStorage`

- **Severity:** Low
- **CWE:** CWE-613 (Insufficient Session Expiration), CWE-922 (Insecure Storage of Sensitive Information)

**Evidence**

- `SESSION_TTL_SECS = 30 days` (`crates/server/server/src/db/session_tokens.rs:14`), set at login (`:279`) and at rotation (`:213`), never extended or shortened by use; `lookup_session` (`:94-115`) only deletes an expired row.
- One Session per account: `INSERT ... ON CONFLICT(account_id) DO UPDATE` (`session_tokens.rs:216-221, 285-290`), so a second login replaces the first, which is a sound revocation story.
- Token: `mc-user-` plus 32 characters from a 62-symbol alphabet drawn from `SysRng` (`session_tokens.rs:11, 26-33, 42-50`), about 190 bits; `b % 62` has a small modulo bias (8 of 62 symbols are 1.25 times as likely), which does not matter at this length. Stored as unsalted SHA-256 (`:37-38`), acceptable for a high-entropy random secret.
- Web storage: `persistState` writes `{serverUrl, token, accountId}` to `localStorage["message-crate-auth"]` (`web/src/lib/auth.tsx:66, 127-136`); the desktop app's page does the same inside the webview.
- `DELETE /v1/session` revokes (`session_api.rs:267-289`); a password change rotates the token and deletes every API token (`credentials.rs:302-321`).

**Why it matters**

A token copied from a shared computer's browser storage works for up to 30 days of inactivity. `localStorage` is readable by any script in the origin, which M3's CSP mitigates.

**Remediation**

Add an idle window: on each `lookup_session` hit, when `expires_at - now < SESSION_TTL_SECS - IDLE_SECS`, update `expires_at` to `now + IDLE_SECS` (sliding), with `IDLE_SECS` of 7 days and the hard cap of 30 days kept as `created_at + 30 days`. The write is one `UPDATE` on the row already read (`session_tokens.rs:96-102`); throttle it to once an hour to keep the auth path cheap (design audit F18 notes the auth path already does a schema check per request).

---

### L5. API tokens default to 365 days, may never expire, and have no rotation route

- **Severity:** Low
- **CWE:** CWE-613

**Evidence**

- `DEFAULT_API_TOKEN_TTL_SECS = 365 days` (`crates/server/server/src/db/api_tokens.rs:31`); `expires_in_days: 0` means no expiry (`api_tokens.rs:375-379`); no upper bound on `expires_in_days` (`accounts_api/api_tokens.rs:93`).
- Lookup refuses disabled and expired rows and records `last_accessed_at` (`api_tokens.rs:114-160`).
- Scopes are `import` and `export` only, never `delete`, and are capped by the account's permissions on every request (`server.rs:1668-1670`, `AuthCapability::ApiToken(auth.permissions.intersect(...))`), when made and when listed (`http-api.md:484-491`). The owner sees no `token_hint` (`accounts_api/api_tokens.rs:39-41, 184-194`).
- There is no rotate route; the path is revoke and create (`http-api.md:491-494`), and a password change deletes every token (`credentials.rs:309`).
- Hash: the same SHA-256 of a `SysRng` token (`api_tokens.rs:98, 118`).

**Why it matters**

A never-expiring token that leaks from a script's config stays valid until someone notices. The scope design already limits what it can do (no browse, no delete, capped by the account).

**Remediation**

Cap `expires_in_days` at, say, 730 in `create_api_token` (`accounts_api/api_tokens.rs:249-266`), and make `0` (never) an owner-controlled server setting rather than any account's choice. Show `last_accessed_at` prominently in the token list (the field exists, `api_tokens.rs:283`).

---

### L6. Docker runtime stage is a Node image that runs no Node, on an end-of-life tag

- **Severity:** Low
- **CWE:** CWE-1104 (Use of Unmaintained Third Party Components), CWE-250-adjacent (unnecessary software in the runtime)

**Evidence**

- `docker/Dockerfile:56` `FROM node:20-bookworm-slim AS runtime`; the entrypoint runs only `message-crate-server` (`docker/entrypoint-release.sh:21,25`). Node 20 reached end of life on 30 April 2026, so the tag no longer receives Node updates; the Debian layer still gets `apt-get update` at build time (`Dockerfile:58-63`).
- The frontend builder uses `node:22` (`Dockerfile:16`), so the two differ for no reason.
- Good: non-root `USER node` (`:81`), `tini` as PID 1 (`:86`), `ca-certificates` and `ffmpeg` only (`:59-62`), one `VOLUME` (`:84`), the toolchain pinned by `rust-toolchain.toml` (`:29-39`).
- No `HEALTHCHECK` (the file has none).

**Remediation**

```dockerfile
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates ffmpeg tini \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --uid 1000 --user-group --home /app --shell /usr/sbin/nologin app
# ... COPY lines unchanged ...
RUN chown -R app:app /app
USER app
HEALTHCHECK --interval=30s --timeout=3s --start-period=120s \
  CMD ["message-crate-server", "health"] || exit 1
```

`compose.yml:17` and `compose.release.yml:32` already run as `1000:1000`, so the `useradd --uid 1000` keeps the volume ownership unchanged. The `HEALTHCHECK` needs either a small `health` subcommand in `cli.rs` that fetches `/health` (`server.rs:1523-1525`) or `curl` in the image; the subcommand keeps the image smaller. The 120-second start period covers the first-start Demo Account build (`server.rs:1377-1380`).

---

### L7. CI supply chain: actions pinned by tag, installers unsigned

- **Severity:** Low
- **CWE:** CWE-829 (Inclusion of Functionality from Untrusted Control Sphere), CWE-494 (Download of Code Without Integrity Check)

**Evidence**

- Actions are pinned by major tag (`actions/checkout@v7`, `docker/build-push-action@v7`, `docker/login-action@v4`, `EmbarkStudios/cargo-deny-action@v2`, `taiki-e/install-action@v2`), with one full-version pin, `cargo-bins/cargo-binstall@v1.23.0` (`.github/workflows/ci.yml:519`). No action is pinned to a commit SHA.
- The release job states "Installers ship unsigned: no signing or notarization step exists in this job. Whether to sign them is open in issue #1019." (`ci.yml:472-474`). The GitHub Release step uploads `dist/*` with no checksum file (`ci.yml:596-611`).
- Good: top-level `permissions: contents: read` (`ci.yml:49-50`), `contents: write` only on `github-release` (`:582-583`), Docker Hub secrets only in the `docker` job which runs on a `v*` tag or an explicit dispatch input (`:383, 397-398`), no `pull_request_target` anywhere in `.github/workflows/`, Dependabot on `github-actions` weekly (`.github/dependabot.yml:55-58`), cargo-deny advisories, licences and bans plus `npm audit --audit-level=high` on lockfile changes and weekly (`.github/workflows/audit.yml:19-37, 50-84`), two advisories ignored with written reasons (`deny.toml:18-21`).

**Why it matters**

A tag can be moved; a SHA cannot. The desktop installers are the one artefact a person runs with their own privileges, and nothing lets them verify the download. The tool downloads inside the app are, by contrast, checksum-pinned (see P6).

**Remediation**

- Pin every `uses:` to a commit SHA with the tag in a trailing comment; Dependabot updates SHA pins.
- Until #1019 is decided, add to the `github-release` job: `(cd dist && sha256sum * > SHA256SUMS)` before `gh release create`, and name the file in the release notes.

---

### L8. The desktop app trusts whatever answers at `127.0.0.1:8080`

- **Severity:** Low (local attacker model)
- **CWE:** CWE-300 (Channel Accessible by Non-Endpoint), CWE-346 (Origin Validation Error)

**Evidence**

- `probe` (`src-tauri/src/local_server.rs:116-140`) sends `GET /v1/server` and `is_message_crate_answer` (`:147-160`) accepts any 2xx JSON with a `schema_fingerprint` key, or any 5xx body with `type`, `status` and `request_id`.
- `ensure_started` then uses "A Message Crate already answering at that address ... as it is" (`CLAUDE.md`, `docs/adr/0018-the-desktop-app-starts-the-server-it-ships.md`), and the window logs in to it over HTTP (`web/src/lib/localServer.ts:29-31`, `web/src/lib/auth.tsx:283-303`).
- Mitigating: the rule is a documented decision (ADR 0018) so that Docker and the app coexist; the shipped server is started with `--exit-with-parent` (`local_server.rs:536-537`) and its own data directory (`:532-534`).

**Why it matters**

A process already running as the same user can bind 8080 first and collect the owner's password. That attacker already has the user's files, including the SQLite database, so the gain is small; it is recorded for completeness.

**Remediation**

Record the Message Crate `id` (`server_api.rs:56-62`, 32 random hex digits) the app last used at that address in its app-data directory, and when the answered `id` differs and a database exists in the app's own data directory, show "A different Message Crate is answering at 127.0.0.1:8080" before logging in. This keeps ADR 0018 and adds a one-field check.

---

### Passes recorded (informational)

**P1. Error handling.** `ApiError::Internal` answers a fixed `"internal server error"` with `type: about:blank` and logs the full chain once (`server.rs:646-664`); `Display` shows the chain only off-HTTP (`:733-739`); Axum rejections map to 400 / 415 / 413 / 422 as problem documents (`crates/server/server/src/extract.rs:67-77`); unknown `/v1` paths and wrong methods are problems, never plain text (`server.rs:973-979, 1272-1274, 1301`); every response carries a server-made `x-request-id` and ignores a client's (`crates/server/server/src/request_id.rs:9-11, 34-41`). **Pass.**

**P2. Request size limits.** 32 KiB on the three stranger routes (`server.rs:964-971, 1006`), 32 MiB for JSON (`:1014, 1305`), 512 MiB default cap with a `Content-Length` pre-check and a `Limited` body (`:1021, 1033-1064`), attachment uploads held to the settings value read per request (`:1046-1052`), multipart parts held to the manifest's part size (`assets_api.rs:1256-1275`), address book 8 MiB (`contacts_api/address_book.rs:70`), search 2 KiB (`search/lex.rs:9`), password 1 KiB (`credentials.rs:20`). JSON depth: `serde_json 1.0.150` with its default 128-level recursion limit; no `unbounded_depth` feature anywhere (`Cargo.toml:50`). **Pass.**

**P3. Path handling.** Asset ids are a 64-hex newtype that is the only way to build a store path (`assets_api.rs:100-120`); log file ids are `i64` mapped to `server-{number:06}.log` (`server_api/log_files.rs:77-80`, `logging/files.rs:154-157, 167`); `[paths]` directory names refuse separators, `..`, drive letters and dot-names (`config.rs:77-103`). The asset download sets `nosniff`, a fixed `Content-Disposition: attachment; filename="asset"` and `Accept-Ranges` (`assets_api.rs:966-978`). **Pass.**

**P4. Media Links.** HMAC-SHA256 under a 32-byte per-process `SysRng` key whose `Debug` hides it (`media_links.rs:40-63`), signed over account, asset, expiry and the live session hash (`:67-76`), verified in constant time with `verify_slice` (`:92-94`), one-hour TTL (`:34`), ended by logout, new login, password change or restart (`http-api.md:510-535`), hidden from the log by `logged_uri` (`server.rs:1225-1247`). **Pass.**

**P5. Credential handling.** Argon2id with library-generated salt (`credentials.rs:148-156`); timing equalisation for unknown usernames (`:179-196`, `session_api.rs:188-199`); one refusal sentence for every failed login (`session_api.rs:123-127`); refused logins written to the Audit Trail (`session_api.rs:190-199, 212-219`); disabled accounts refused on every credential (`server.rs:1650-1652`); the owner's session carries no message permissions and an API token can never become the owner (`server.rs:1660-1671`); the Demo Account's limits are fixed by id (`account_profile.rs:458-467`, `docs/adr/0016`). Bearer parsing is case-insensitive on the scheme and refuses an empty token (`server.rs:1532-1558`). The app headers are length-capped and ASCII-checked and never decide anything (`server.rs:1574-1591`). **Pass.**

**P6. Desktop downloads and TLS.** Tool downloads go to GitHub over HTTPS (`src-tauri/src/tool_downloads.rs:62, 165-168`) with a rustls client, connect and read timeouts (`:437-447`), SHA-256 pinned for the asset and for the unpacked program (`:144-160, 494-516`), written to a temp file and renamed atomically (`:481, 525-527`). The shared HTTP client trusts OS roots plus webpki and sets no `danger_accept_invalid_certs` (`crates/libs/http/src/lib.rs:69-88`). **Pass.**

**P7. Desktop CSP and capabilities.** `tauri.conf.json:24-36` sets `default-src 'self'`, `script-src 'self'`, `object-src 'none'`, `frame-src 'none'`, `form-action 'none'`, `base-uri 'self'`; `connect-src`, `img-src` and `media-src` allow `http:` and `https:` because the server address is the person's choice (`run-on-another-machine.md:62`). `dangerousDisableAssetCspModification: ["style-src"]` (`:37`) keeps `'unsafe-inline'` for styles only. Capabilities are the minimum the app uses: dialog open and save, shell open, window destroy (`src-tauri/capabilities/default.json:8-16`). `withGlobalTauri: true` (`:13`) is acceptable under `script-src 'self'`. **Pass, with the note that the website build has none of this (M3).**

**API versioning (checklist item 3).** `/v1` is a path number with an explicit no-compatibility policy: "No compatibility alias, deprecation window or version handshake is ever added" (`http-api.md:914-935`; `CLAUDE.md`, "No backwards compatibility, anywhere"). The apps report `x-message-crate-app` and `x-message-crate-version`, which the server records on the session and never acts on (`server.rs:1579-1591`, `http-api.md:922-934`). "Deprecated version handling" is therefore **Not Applicable** by design, and "breaking change management" is **Pass** as a documented, enforced policy with the client-side Product Version comparison.

**Unauthenticated disclosure.** `GET /v1/server` answers `id`, `state`, `demo_account`, `version`, `schema_fingerprint` and `asset_max_bytes` to anyone (`server_api.rs:56-83, 118-130`); `/health` answers `ok` (`server.rs:1515-1525`). The apps need every field, the version is also in the installer's name, and the id is random. Informational; no change recommended.

## 4. Checklist against the skill's "Check for" list

| # | Item | Result | Where |
|---|---|---|---|
| 1 | CORS: no wildcard in production | **Pass** | `"*"` is opt-in and not in `config.docker.toml`; `server.rs:934-936` |
| 1 | CORS: proper origin validation | **Pass** | exact list, `AllowOrigin::list`, `server.rs:938-956`; L3 for the stray dev origins |
| 1 | CORS: credentials handling | **Pass** | no cookies, bearer header, `allow_credentials` never set; `web/src/lib/api.ts:174-176` |
| 2 | Rate limiting on all endpoints | **Fail** (by design) | credential routes only; L2, `http-api.md:688-706` |
| 2 | Different limits for different operations | **Partial** | per-account vs server-wide buckets, one figure; `credentials.rs:22-50` |
| 2 | Distributed rate limiting | **N/A** | one process, one SQLite file; `credentials.rs:31-35` |
| 3 | Deprecated version handling | **N/A** | policy forbids compatibility windows; `http-api.md:914-935` |
| 3 | Breaking change management | **Pass** | documented policy; recorded app headers; `server.rs:1579-1591` |
| 4 | Body parser limits | **Pass** | P2 |
| 4 | File upload restrictions | **Pass** | settings-driven cap, part size from manifest; `server.rs:1046-1052`, `assets_api.rs:1256-1275` |
| 4 | JSON depth limits | **Pass** | serde_json default recursion limit; `Cargo.toml:50` |
| 5 | Helmet.js configuration | **N/A** | Rust server; the equivalent is `SetResponseHeaderLayer` (M3) |
| 5 | CSP headers | **Fail** (server) / **Pass** (desktop) | M3; `tauri.conf.json:24-36` |
| 5 | X-Frame-Options | **Fail** | M3 |
| 5 | X-Content-Type-Options | **Partial** | asset downloads only, `assets_api.rs:966-969`; M3 |
| 5 | Strict-Transport-Security | **N/A** | plain HTTP by design; the TLS proxy's job, `run-on-another-machine.md:41` |
| 6 | Token secure storage | **Pass** (server) / **Partial** (browser) | SHA-256 of a `SysRng` token, hints masked, owner never sees hints; plaintext `localStorage` in the SPA, L4 |
| 6 | Token rotation policy | **Partial** | revoke and recreate; password change revokes all; 365-day default, `0` = never; L5 |
| 6 | Token scope limitations | **Pass** | import/export only, capped per request, never delete; `server.rs:1668-1670` |
| 7 | No stack traces in production | **Pass** | `server.rs:646-664` |
| 7 | Generic error messages | **Pass** | fixed sentence for 500; one sentence for every failed login, `session_api.rs:123-127` |
| 7 | Proper status codes | **Pass** | registry in `problem.rs`, one shape, `http-api.md:182-283` |

## 5. Unable to verify

| Claim | What would prove it |
|---|---|
| Whether hyper, as `axum::serve` configures it, applies any header-read timeout by default in the versions locked here | `Cargo.lock` entries for `hyper` and `hyper-util`, and `axum::serve`'s builder in the locked axum 0.8.x source |
| Whether `tower_http::services::ServeDir` in `tower-http 0.7.1` refuses dot-files and encoded `..` segments as its documentation says | the locked `tower-http` source under `~/.cargo/registry`; nothing in this repository configures it beyond `ServeDir::new(static_dir)` (`server.rs:1302`) |
| Whether the Tauri shell plugin's default `open` scope (http, https, mailto, tel) is what the app relies on, or whether a narrower scope is set | `src-tauri/src/main.rs:41-45` initialises `tauri_plugin_shell::init()` with no visible scope; a grep for `plugin-shell` in `web/src/lib/tauri.ts` found no caller, so the permission `shell:allow-open` (`capabilities/default.json:14-15`) may be unused |
| The exact UI copy and confirmation step shown before the owner can create an account with no password | `web/src/screens/` owner account creation form; the user guide names the heading "Password (optional)" (`owner-home.md:110`) |
| Whether the nightly and docs workflows' `id-token: write` (`docs.yml:83-85`) is scoped to the Pages deploy step alone | `.github/workflows/docs.yml:80-95` |

## 6. Suggested order of work

1. M1 (one line in `compose.yml`, one in `compose.release.yml`) and the startup warning for a non-loopback bind.
2. M2 (`credentials.rs`, two call sites in `accounts_api.rs`) with a changelog entry under the in-development heading, since a stranger's empty-password registration starts to answer `422`.
3. M3 header layer, placed in the `middleware` module the design audit's F1 proposes if that split lands first, otherwise in `http_app`.
4. L3 (`config.docker.toml`), L6 (`Dockerfile` runtime stage and `HEALTHCHECK`), L7 (SHA pins and `SHA256SUMS`).
5. L1, L4, L5 as one "limits and lifetimes" change set, each a constant and a layer.
6. L8 when ADR 0018 is next revisited.
