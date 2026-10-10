# Initial security analysis

| | |
|---|---|
| Audit | initial-security-analysis (broad first pass: threat model, attack surface, survey of every check) |
| Date | 2026-10-10 |
| Commit | `530dc1791` (branch `docs/initial-software-design-analysis`) |
| Scope | Whole repository except `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist` |
| Method | Read-only. Source read with `grep`, `sed`, `cargo tree`, `cargo metadata`; `cargo deny check advisories licenses bans` run on both graphs (cargo-deny was installed; cargo-audit was not). Nothing was built, tested or executed. |
| Companion | `audits/2026-10-09-initial-software-design-analysis.md` (design audit, findings F1 to F19), cross-referenced by id below |

Ten sibling audits ran at the same time and go deep on authentication, authorization, database, input validation, sessions, secrets, API and infrastructure, file handling, business logic, and logging. Where one of them owns a topic, this report states the survey result in one line and moves on. Dependency and supply chain, the build and release pipeline, Docker image hardening, the desktop app's surface, the GPL sidecar boundary, the search language, and time-of-check issues have no sibling and are covered to full depth here.

## 1. Summary

**Risk score: 4.5 / 10 (moderate).** Message Crate is a self-hosted archive of private messages whose default configuration listens on loopback only, whose authentication primitives are sound (argon2id, OS-random 32-character tokens stored hashed, HMAC media links compared in constant time), whose file handling is careful (hash-verified assets, `O_NOFOLLOW`, one path-safety function used by every importer), and whose GPL sidecar boundary holds on the dependency graph. The risk that remains is concentrated in what happens the moment the server is reachable from another machine: it speaks plain HTTP, two of the three shipped Docker entry points publish it on every interface, an account may have no password, and the login rate limiter can be turned against the account it protects. The build and release pipeline pins actions and base images by mutable tag, ships unsigned installers with no checksums, and runs the server in a Node.js 20 runtime image that the server never uses.

| Severity | Count | Ids |
|---|---|---|
| Critical | 0 | |
| High | 0 | |
| Medium | 5 | S1, S2, S4, S8, S9 |
| Low | 8 | S5, S6, S7, S10, S11, S12, S13, S14 |

**Top five fixes, in the order that reduces risk fastest** (section 7 has the snippets):

1. Publish the shipped Docker entry points on loopback only (`docker/compose.yml`, `docker/compose.release.yml`), and have the server say so in the Owner Home when its bind is not loopback and its scheme is HTTP. (S1, S2)
2. Require a password on every account but the Demo Account, and a minimum length for the owner's. (S4)
3. Build the runtime stage from `debian:bookworm-slim` with a uid 1000 user; drop Node.js 20 from the image. (S8)
4. Pin GitHub Actions and Docker base images by digest, publish `SHA256SUMS` and a build-provenance attestation beside the installers, and run `cargo update -p chacha20`. (S9, S12)
5. Add `Content-Security-Policy`, `X-Content-Type-Options`, `Referrer-Policy` and `frame-ancestors` to every response the server makes, not only to asset downloads. (S6)

## 2. Deployment model and threat model

Every finding below is rated against this model, and says when it matters only once the server is reachable beyond the computer it runs on.

### 2.1 Where the server listens

| Route to a running server | Bind | Published to | Source |
|---|---|---|---|
| Desktop app starts its own server | `127.0.0.1:8080` | this computer | `src-tauri/src/local_server.rs:70` `OWN_ADDRESS`; `:519-525` `Launch::bind` |
| Desktop app, "Let other devices on this network connect" on | `0.0.0.0:8080` | every interface, plain HTTP, while the app is open | `local_server.rs:521-522`; the switch and its warning `web/src/screens/settings/SystemSection.tsx:347-351`; off by default `web/src/lib/localServer.ts:40-47` |
| Docker, the user guide's first-time command | `0.0.0.0:8080` in the container | `-p 127.0.0.1:8080:8080`, this computer | `config/config.docker.toml` `[server] bind`; `docs/src/content/docs/docs/user/features/owner/run-with-docker.md:25-29,40` |
| Docker, `docker/compose.yml` (the published sample) | same | `"8080:8080"`, every interface | `docker/compose.yml:19` |
| Docker, `docker/compose.release.yml` | same | `"0.0.0.0:8080:8080"`, every interface | `docker/compose.release.yml:33` |
| Docker, "Run on another machine" | same | `-p 8080:8080`, every interface, with the warning that passwords, tokens and messages cross the network in clear | `docs/.../owner/run-on-another-machine.md:20-45` |
| Internet | not supported directly | "stick it behind a reverse proxy with an auth layer of your choosing" | `README.md:130-131`; TLS reverse proxy in `run-on-another-machine.md:37-41` |

The server has no TLS of its own. `SECURITY.md` states the product is self-hosted and that fixes apply to the latest release only.

### 2.2 Who is trusted, and who is not

- **The owner** (`OWNER_ACCOUNT_ID`) administers the installation and holds no messages (`docs/adr/0008`). **Accounts** hold messages, each with `import`, `export`, `delete` permissions the owner may limit. **Programs** hold API tokens capped by the account's permissions and never `delete` (`docs/architecture/http-api.md`, "Credentials and reach"). **The Demo Account** has no password and is fixed by id (`docs/adr/0016`).
- **Adversaries considered.** (A) A device on the same network when the server is published there. (B) A crafted backup, export or JSONL file fed to the desktop app or to the server's `import` command. (C) A hostile server the desktop app is pointed at. (D) A compromised dependency, GitHub Action, or download host. (E) The internet, when an owner publishes the port against the documentation.
- **Out of scope.** A local process running as the same operating-system user as the app or the server: there is no privilege boundary between them, so swapping a file in the Tools Directory or setting `MESSAGE_CRATE_IMESSAGE_READER` is not a vulnerability of Message Crate.

### 2.3 Assumptions

- Node.js 20 left maintenance on 30 April 2026 per the Node.js release schedule; the Docker runtime image's base tag is read from the Dockerfile, not from a pulled image.
- The repository's `vendor/sqlx-sqlite` is excluded by instruction; `VENDORING.md` describes it as a byte-identical crates.io copy with one dependency bump, which this audit did not check.

## 3. Attack surface map

### 3.1 Entry points

| Entry point | Where | Notes |
|---|---|---|
| HTTP server | `crates/server/server/src/main.rs` → `cli.rs` `serve` → `server.rs:1317` `run` → `:1251` `http_app` | Axum over SQLite. One binary, `message-crate-server` (`docs/adr/0001`). |
| Server CLI | `cli.rs:24-200` (`import`, `imports discard`, `dedupe`, `create-owner`, `reset-owner-password`, `reset-demo`, `create-database`, `serve`), `import_cli.rs`, `owner_cli.rs` | Local operator only. Docker entrypoint passes arguments through: `docker/entrypoint-release.sh:22-24`. |
| Static web app | `server.rs:1297` `.fallback_service(ServeDir::new(static_dir))` | The SPA in `web/`, same origin as the API. |
| Swagger UI | `server.rs:1291-1293`, only when `[server] openapi_ui = true`; default false `config.rs:204` `default_openapi_ui`, not set in `config/config.docker.toml` | Off in every shipped configuration. |
| Desktop host | `src-tauri/src/main.rs:39-76` (dialog and shell plugins, `generate_handler!` with 37 `#[tauri::command]`s under `src-tauri/src/commands/`) | Webview → Rust command surface. |
| Sidecar | `crates/libs/ios-backup/src/helper.rs:117-170` spawns `imessage-reader`, JSON lines over stdin/stdout | GPL process boundary (`docs/adr/0014`). |
| External programs | `crates/libs/media/src/tools.rs:416-424` ffmpeg, `:526-548` ffprobe; `crates/exporters/whatsapp-exporter/src/wtsexporter.rs:288-340` wtsexporter | Found on `PATH` then in the Tools Directory. |
| Tool downloads | `src-tauri/src/tool_downloads.rs:62` `GITHUB`, `:449-525` `download`; pins in `tool_downloads/pins.rs` | Pinned release + SHA-256 of the archive and of the unpacked program. |
| Untrusted files | `crates/exporters/*`, `crates/libs/sbr`, `crates/libs/go-sms-mms` (PDU/WSP), `crates/libs/ios-backup`, server `jsonl.rs` and `imports_api/` | Adversary (B). |
| CI and release | `.github/workflows/ci.yml` (`docker`, `release`, `github-release` jobs), `audit.yml`, `docs.yml`, `nightly.yml`, `coverage.yml`, `mutants.yml` | Adversary (D). |

### 3.2 Routes

Route groups are one file each: `accounts_api.rs`, `assets_api.rs`, `audit_trail_api.rs`, `contacts_api.rs`, `conversations_api.rs`, `exports_api.rs`, `imports_api/`, `messages_api.rs`, `named_set_api.rs`, `phone_countries_api.rs`, `saved_searches_api.rs`, `search_fields_api.rs`, `server_api.rs` (with `server_api/log_files.rs`), `session_api.rs`, `trash_api.rs`. The OpenAPI document is assembled in `openapi.rs` and is also the list a request's query parameters are checked against: `declared_query.rs:26-75` `DeclaredQueries::from_spec` and `refuse_undeclared_query`, so a parameter no operation declares is refused rather than ignored. `/health` sits outside `/v1` (`server.rs:1279-1283`).

Routes a stranger may call go through `limited_auth_router` (`server.rs:971-979`) with a 32 KiB body cap: session creation, account registration, and the claim of an unclaimed server (`server_api.rs:133-215` `claim_server`). The exact membership of `openapi::public_openapi()` was not read (section 9).

### 3.3 Middleware chain and order

From `http_app` (`server.rs:1251-1315`), outermost first as a request enters:

1. `request_id::layer` makes an `x-request-id` (UUID v4; a client's is ignored) (`:1311`).
2. `TraceLayer` logs one `info` line per response with `logged_uri`, which hides every query value but ten named ones, `media_link` and `q` included (`:1180-1240`).
3. `build_cors_layer` (`:936-965`): exact origin allow list plus the three fixed desktop origins; `["*"]` is permissive and documented as debug-only; methods and headers mirror the request; no credentials.
4. `map_response(json_body_limit_response)` rewrites a plain-text 413 into a problem document (`:995-1007`).
5. `limit_request_body` (`:1033-1080`): 512 MiB for everything but attachment uploads, which take the owner's attachment size limit; multipart parts are bounded by their route; `Content-Length` over the cap refused before any handler.
6. `DefaultBodyLimit::max(32 MiB)` for JSON (`:1299`).
7. Route layers on `/v1` only: `require_json_acceptable` (406 on an `Accept` that admits no JSON, with five byte-answering routes let through) and `refuse_undeclared_query`.
8. The handler. **Authentication is not a middleware**: each handler names its credential through an extractor (`FullAccess`, `Owner`, or `resolve_auth`, `server.rs:1563-1690`), and `resolve_auth_on_conn` looks the Bearer token up in SQLite on every request, session first then API token, then loads the account and refuses a disabled one. The owner's session resolves to `AuthCapability::Owner` with no message permissions; an API token never does (`:1659-1674`).

Every failure is an RFC 7807 problem document (`problem.rs`), and a 500 hides its cause: `server.rs:648-656` writes `detail: "internal server error"` and logs the chain server-side.

### 3.4 External service integrations

GitHub release downloads of ffmpeg, ffprobe and wtsexporter over HTTPS with rustls and the OS trust store (`src-tauri/Cargo.toml` reqwest features; `tool_downloads.rs:437-446` 30 s connect, 60 s read); the server itself, reached by the desktop app over whatever scheme the person typed (`crates/libs/http/src/lib.rs:70-85` `build_client`, webpki plus native roots); Docker Hub for the published image (`ci.yml` `docker` job); messagecrate.app for documentation links (`web/src/lib/thirdPartySoftware.ts`). Nothing phones home.

### 3.5 Database connection points

SQLite only (`docs/adr/0017`). `open_db.rs` `OpenDb::create_or_open`; a sqlx pool on `AppState` (`state.db.acquire()`); `db/write_tx.rs` `begin_write`; `db/write_guard.rs:79-117` installs a `sqlite3_set_authorizer` through `unsafe` FFI so a write outside a write transaction fails; `db/sqlite_functions.rs:40-90` registers a `unicode_lower` function; the schema is embedded from `schema/sql/*.sql` by `db/schema.rs` and compared by fingerprint; `operation_lock.rs` serialises `serve` against the CLI. The `unsafe` in the repository is these two FFI modules, the Windows process API in `exit_with_parent.rs:92-150`, and environment setters in test and helper code; no parser uses `unsafe`.

### 3.6 Authentication and authorization flow

Login: `session_api.rs:140-215`. Username normalised, account looked up, rate bucket chosen (`password:{account_id}` or `password:unknown:{username}`), limiter checked before any work, password length capped at 1024 bytes, argon2id verified (`credentials.rs:158-166`), a dummy hash verified for unknown usernames so timing matches (`:179-194`), refused logins recorded in the Audit Trail. A success writes one session row per account (`db/session_tokens.rs:266-300` `open_session`, `ON CONFLICT ... DO UPDATE`), 30 days (`:14` `SESSION_TTL_SECS`), token `mc-user-` plus 32 alphanumerics from `SysRng` (`:21-47`), stored as SHA-256. API tokens: `db/api_tokens.rs`, `mc-api-` prefix, 365-day default expiry (`:30`), masked hint. Media Links: HMAC-SHA256 under a 32-byte key made at start (`assets_api/media_links.rs:43-62`), one hour (`:34`), bound to the asset, account, expiry and the hash of the Session that made it, verified with `verify_slice` in constant time (`:92-94`). Claim of an unclaimed server: unauthenticated by design, inside one write transaction (`server_api.rs:158-215`). Demo Account: guarded by id in the import and delete guards and in `load_account_auth` (CLAUDE.md; `docs/adr/0016`).

### 3.7 File upload handling

`PUT /v1/assets/{sha256}` and `PUT /v1/assets/{sha256}/uploads/{upload_id}/parts/{part}` (`server.rs:1133-1158`, `asset_uploads.rs`, `assets_api.rs`). The path segment is `Sha256::parse` (64 hex, `assets_api.rs:114`), bytes stream to disk under a cap (`server.rs:1760-1785` `stream_body_to_file`), are hashed and compared with the claim (`assets_api.rs:367` `verify_source_digest`, `:223-290` `store_verified`), opened without following symlinks (`:582` `open_nofollow_read`), stored at `<assets_dir>/<aa>/<sha256><ext>` with a `.mime` sidecar (`asset_store.rs:8-14`). Downloads set `Content-Type` from the sidecar, `X-Content-Type-Options: nosniff` and `Content-Disposition: attachment; filename="asset"` (`assets_api.rs:962-973`), which is what keeps an HTML attachment from running as a page on the API's origin. JSONL import bodies: `POST /v1/imports` (`server.rs:1690` `is_jsonl_content_type`), attachment paths through `message_ir::safe_attachment_path` (`crates/libs/ir/src/attachment_path.rs:43-62`; server call `imports_api/staging.rs:71`; desktop `crates/libs/import/src/prepare.rs:199,294,325`). Address book CSV: `POST /v1/contacts`; the export escapes cells a spreadsheet would run as a formula (`db/address_book.rs:336-360` `FORMULA_STARTS`).

### 3.8 Rate limiting

One sliding-window limiter, 20 hits per 60 s per bucket, on `AppState` (`credentials.rs:22-23,54-140`), applied to login (`session_api.rs:169`), claim (`server_api.rs:164`, bucket `"claim"`), and registration (bucket `"register"`, named at `credentials.rs:28`). Nothing else is rate limited; search is bounded by `MAX_QUERY_BYTES` 2048 (`search/lex.rs:9`), 32 text terms, 64 nodes, depth 32 (`search/parse.rs:43-47`).

## 4. Findings

Each finding: severity, CWE, evidence, why it matters, exploitability with a safe reproduction, remediation and defence in depth, and the exposure at which it applies.

### S1. Credentials and messages cross the network as plain HTTP once the server is published (Medium)

- **CWE-319** Cleartext Transmission of Sensitive Information.
- **Exposure:** none on loopback; applies the moment the server is published to a LAN; High if published to the internet against the documentation.
- **Evidence.** The server binds a `TcpListener` with no TLS (`crates/server/server/src/server.rs:1395-1400`). The desktop app's network switch binds `0.0.0.0` (`src-tauri/src/local_server.rs:519-525`) and the copy says the connection is plain HTTP (`web/src/screens/settings/SystemSection.tsx:350`, `docs/.../settings/system.md:126`). The Docker config binds `0.0.0.0:8080` inside the container (`config/config.docker.toml`). The session token is a Bearer header on every request (`web/src/lib/api.ts:175,207,236`) and is kept for 30 days (`db/session_tokens.rs:14`). A Media Link travels in the URL (`assets_api/media_links.rs:321-324`).
- **Why it matters.** Anyone on the same network segment, or on a shared Wi-Fi, reads the password at login, the 30-day session token on every request, every message, and every attachment. The product's own docs say so (`run-on-another-machine.md:37-41`), and the mitigation they offer is a TLS reverse proxy "outside this guide".
- **Exploitability.** On a LAN where the server is published: `tcpdump -A -i <iface> tcp port 8080 | grep -E 'Authorization|"password"'` shows the token and the password in clear. No code is needed.
- **Remediation.** Keep the design (hosting is the reverse proxy's job) but close the gap between the warning and the behaviour: (1) make the shipped Docker samples loopback-only (S2); (2) have `GET /v1/server` report `bind_is_loopback` and have the Owner Home show a persistent notice when it is false and the page was loaded over `http:`; (3) publish a one-file Caddy recipe in `run-on-another-machine.md`:

  ```caddyfile
  messages.home.arpa {
      tls internal
      reverse_proxy 127.0.0.1:8080
  }
  ```

  Defence in depth: shorten `SESSION_TTL_SECS` when the request arrived over a non-loopback address, or add an idle timeout (S7).

### S2. Both shipped compose files publish the port on every interface, and an unclaimed server belongs to whoever reaches it first (Medium)

- **CWE-1327** Binding to an Unrestricted IP Address; **CWE-306** window on `POST /v1/server/claim`.
- **Exposure:** LAN. Applies to anyone who starts from `docker/compose.yml` or `docker/compose.release.yml` rather than the `docker run` command in the user guide.
- **Evidence.** `docker/compose.yml:19` `- "8080:8080"`; `docker/compose.release.yml:33` `- "0.0.0.0:8080:8080"`; the user guide's first-time command publishes `127.0.0.1:8080:8080` (`run-with-docker.md:27,40`). The claim route is unauthenticated by design: `server_api.rs:133-139` "Whoever reaches an unclaimed one first may claim it: Message Crate is self-hosted, so whoever installs the software claims it and then publishes the port, in that order". With the compose files, publishing happens first. `compose.yml:1-2` tells a person to "Save this file and run: docker compose up -d", so it is the path a non-developer takes.
- **Why it matters.** A freshly started Message Crate on a shared network can be claimed by a neighbour before its installer opens the page, and after that the installer is locked out of their own server until they delete the volume. Once claimed by the right person, S1 applies.
- **Exploitability.** Host A: `docker compose -f docker/compose.yml up -d`. Host B on the same LAN: `curl http://A:8080/v1/server` answers `"state":"unclaimed"`; `curl -X POST http://A:8080/v1/server/claim -H 'content-type: application/json' -d '{"username":"owner","password":"x"}'` answers 201 with the owner's Session. The limiter allows 20 such calls a minute (`credentials.rs:23`), which is 20 more than needed.
- **Remediation.**

  ```yaml
  # docker/compose.yml and docker/compose.release.yml
  ports:
    - "127.0.0.1:8080:8080"   # this computer only; see run-on-another-machine for the network
  ```

  Defence in depth: refuse `POST /v1/server/claim` from a non-loopback peer while the bind is not loopback unless `serve --claim-from-network` was given, and say so in the response. A pure refusal with no negotiation stays inside the "no backwards compatibility" rule. Cross-reference: `docs/adr/0018` already says hosting is Docker's job; this makes the two Docker samples match the documented first-time command.

### S3. (merged into S2)

### S4. An account may have no password, and the owner's password has no minimum length (Medium)

- **CWE-521** Weak Password Requirements; **CWE-1391** Use of Weak Credentials.
- **Exposure:** Low on loopback; Medium on a LAN; High on the internet.
- **Evidence.** `credentials.rs:198-207` `hash_user_password`: "There is no minimum length: an empty password is `None`, an account with no password." `credentials.rs:171-194` `passwords_match` and `verify_login_password`: a NULL hash accepts the empty password. `credentials.rs:195-203` `hash_owner_password`: "one character is enough". Account creation by the owner and by public registration both go through `hash_user_password` (`accounts_api.rs:380-430`).
- **Why it matters.** The product's docs say "The Owner's password is the only thing between the network and every account" (`run-on-another-machine.md:43`). An account with no password is one `POST /v1/session` with `"password":""` away from its archive for anyone who knows or guesses the username, and usernames are 1 to 128 characters of a small alphabet (`credentials.rs:238-245`). The Demo Account needs this behaviour, but it is fixed by id and does not need the rule to hold for every account.
- **Exploitability.** Owner creates `alice` with an empty password through the UI or `POST /v1/accounts`. From any host that reaches the server: `curl -X POST /v1/session -d '{"username":"alice","password":""}'` answers 201 with a 30-day token.
- **Remediation.**

  ```rust
  // crates/server/server/src/credentials.rs
  pub(crate) const MIN_PASSWORD_CHARS: usize = 12;

  pub(crate) fn hash_user_password(password: &str) -> Result<Option<String>, ApiError> {
      if password.chars().count() < MIN_PASSWORD_CHARS {
          return Err(ApiError::validation(format!(
              "a password needs at least {MIN_PASSWORD_CHARS} characters"
          )));
      }
      require_hashable(password)?;
      Ok(Some(hash_password(password)?))
  }
  ```

  Apply the same floor in `hash_owner_password`. Keep the Demo Account's NULL hash as the one exception, written by `reset_demo.rs`, never by a route. Defence in depth: the authentication sibling audit owns the policy; this report records the rule and its exposure.

### S5. The login limiter is keyed by account, so anyone can lock an account out, and nothing is keyed by client (Low)

- **CWE-307** Improper Restriction of Excessive Authentication Attempts; **CWE-645** Overly Restrictive Account Lockout.
- **Exposure:** LAN and beyond.
- **Evidence.** `credentials.rs:40-50` buckets `password:{account_id}` and `password:unknown:{username}`; `:54-57` `check_auth_rate_limit` counts every attempt, successes included (`:80-90`), and `session_api.rs:169` calls it before the password is checked. No bucket names the client address.
- **Why it matters.** Twenty wrong passwords a minute for `alice`, from anywhere, keep `alice` at `429 rate-limited` for as long as the attacker likes; her own correct login counts against the same bucket. Conversely the limiter does not slow an attacker who sprays many usernames, since each gets its own bucket.
- **Exploitability.** `for i in $(seq 20); do curl -s -X POST /v1/session -d '{"username":"alice","password":"wrong"}'; done` then `alice` logs in and receives 429 with `retry_after_secs`.
- **Remediation.** Count failures only for the account bucket: the helpers already exist (`refuse_when_rate_limited` `:62-70`, `count_auth_failure` `:72-77`); change `session_api.rs:169` to call the first before and the second after a failed verify. Add a second bucket per client address from `ConnectInfo<SocketAddr>` (and, only when a `--trusted-proxy` lists the peer, from `X-Forwarded-For`) so a spray from one host is throttled. The authentication sibling audit owns the detail.

### S6. The web app the server serves carries no Content-Security-Policy or other security headers (Low)

- **CWE-693** Protection Mechanism Failure; **CWE-1021** Improper Restriction of Rendered UI Layers.
- **Exposure:** every route, but the impact rises with S1.
- **Evidence.** `http_app` (`server.rs:1251-1315`) adds no header layer; the only security header in the server is `x-content-type-options: nosniff` on asset downloads (`assets_api.rs:966-969`). `web/index.html:4-5` has no `<meta http-equiv="Content-Security-Policy">`. The CSP in `src-tauri/tauri.conf.json:23-36` protects the desktop webview only. The session token is persisted in `localStorage` under `message-crate-auth` (`web/src/lib/auth.tsx:66,127-137`).
- **Why it matters.** No script sink was found (`grep` for `dangerouslySetInnerHTML`, `innerHTML`, `eval` across `web/src` matches nothing; React escapes text), so there is no known exploit today. A CSP is what turns a future XSS from "the 30-day token to someone's whole message archive leaves" into a blocked request, and `frame-ancestors 'none'` stops the login card being framed.
- **Remediation.** In `http_app`, after `fallback_service`:

  ```rust
  use tower_http::set_header::SetResponseHeaderLayer;
  .layer(SetResponseHeaderLayer::if_not_present(
      header::CONTENT_SECURITY_POLICY,
      HeaderValue::from_static(
          "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
           img-src 'self' data: blob:; media-src 'self' blob:; font-src 'self' data:; \
           connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'none'; \
           frame-ancestors 'none'",
      ),
  ))
  .layer(SetResponseHeaderLayer::if_not_present(
      header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
  .layer(SetResponseHeaderLayer::if_not_present(
      header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
  ```

  The SPA is same-origin with the API when the server serves it, so `connect-src 'self'` holds; the desktop webview keeps its own CSP. Verify in the browser with the Playwright MCP that the Swagger UI, when enabled, still loads (it inlines scripts and may need its own relaxed policy on `/docs` only).

### S7. Session tokens last 30 days in `localStorage` with no idle timeout (Low)

- **CWE-613** Insufficient Session Expiration.
- **Evidence.** `db/session_tokens.rs:14` `SESSION_TTL_SECS = 30 days`; `web/src/lib/auth.tsx:127-137` persists the token; one session row per account (`:266-300`), so a new login ends the old one, and a password change rotates it (`credentials.rs:310-312`).
- **Why it matters.** A token read off the wire (S1) or out of a browser profile is good for a month of silent reads.
- **Remediation.** The session-cookie-security sibling owns this. Survey note only: add `last_used_at` and refuse a session idle for more than, say, seven days; offer "sign out everywhere" as `DELETE /v1/accounts/{id}/sessions` for the owner.

### S8. The Docker runtime image is `node:20-bookworm-slim`, an end-of-life Node.js the server never runs (Medium)

- **CWE-1104** Use of Unmaintained Third Party Components; **CWE-1395** Dependency on Vulnerable Third-Party Component.
- **Exposure:** every Docker installation, whatever the bind.
- **Evidence.** `docker/Dockerfile:57` `FROM node:20-bookworm-slim AS runtime`. The entrypoint runs only the Rust binary (`docker/entrypoint-release.sh:22-27`); nothing in the image calls `node` or `npm`. Node.js 20 left maintenance on 30 April 2026. The image also sets no `HEALTHCHECK`, and neither compose file sets `read_only`, `cap_drop`, `security_opt: [no-new-privileges:true]` or `tmpfs`. What is right: `USER node` (`Dockerfile:76`), `tini` as PID 1 (`:81`), a named volume for `/app/data`, `apt-get` lists removed, the toolchain pinned by `rust-toolchain.toml` and installed with `rustup toolchain install` (`:37-40`).
- **Why it matters.** Every CVE in Node.js 20, npm and their shared libraries is in the image and will never be patched, and every scanner a self-hoster runs will flag it. It is also attack surface for anyone who gets a shell in the container.
- **Exploitability.** `docker run --rm --entrypoint node bitrealm/message-crate:latest --version` prints a Node.js version. Not run here.
- **Remediation.** The compose files rely on uid 1000 (`user: "${UID:-1000}:${GID:-1000}"`), so the replacement must create that user:

  ```dockerfile
  FROM debian:bookworm-slim AS runtime
  RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 app \
    && useradd --uid 1000 --gid 1000 --create-home --shell /usr/sbin/nologin app
  WORKDIR /app
  # ... COPY lines as today, then:
  RUN mkdir -p /app/data && chown -R app:app /app
  USER app
  HEALTHCHECK --interval=30s --timeout=5s CMD ["/usr/local/bin/message-crate-server", "health"] 
  ```

  (`health` would be a new tiny subcommand that GETs `/health`; or use `tini -- sh -c` with a static `curl`.) In both compose files add `read_only: true`, `tmpfs: ["/tmp"]`, `cap_drop: [ALL]`, `security_opt: ["no-new-privileges:true"]`; the server writes only under `/app/data` and `/app/config` (`entrypoint-release.sh:15-18` writes `config/config.toml`, so that path needs a writable mount or the entrypoint writes it to `/tmp`). Keep `scripts/check-docker-context.sh` in step.

### S9. Actions and base images pinned by mutable tag; installers unsigned with no checksums or provenance (Medium)

- **CWE-829** Inclusion of Functionality from Untrusted Control Sphere; **CWE-494** Download of Code Without Integrity Check (for the person installing); **CWE-1357** Reliance on Insufficiently Trustworthy Component.
- **Exposure:** every installer and image a person downloads.
- **Evidence.** `.github/workflows/ci.yml`: `actions/checkout@v7`, `actions/cache@v6`, `actions/setup-node@v7`, `docker/setup-buildx-action@v4`, `docker/build-push-action@v7`, `docker/login-action@v4`, `docker/metadata-action@v6`, `cargo-bins/cargo-binstall@v1.23.0`, `actions/upload-artifact@v7`, `actions/download-artifact@v8`; `audit.yml` `EmbarkStudios/cargo-deny-action@v2`; `coverage.yml` and `mutants.yml` `taiki-e/install-action@v2`. All are tags a maintainer or an attacker with that repository's push access can move. `docker/Dockerfile:14,37,57` base images by tag with no `@sha256:` digest. `ci.yml` `release` job comment: "Installers ship unsigned: no signing or notarization step exists in this job. Whether to sign them is open in issue #1019." `github-release` uploads `dist/*` with no `SHA256SUMS`, no `actions/attest-build-provenance`, no `cosign` on the Docker image. What is right: `permissions: contents: read` at workflow level (`ci.yml:46-47`), `contents: write` only on `github-release`, `pull_request` (never `pull_request_target`), the Docker Hub secret used only on a tag or a manual run, `cargo binstall --strategies crate-meta-data` so a missing archive fails rather than compiling, Dependabot on `github-actions` weekly (`.github/dependabot.yml`), `docs.yml` deploy with `pages: write` and `id-token: write` scoped to its job.
- **Why it matters.** A moved tag on any of these actions runs attacker code on the runner that builds the installer people download, with `DOCKERHUB_TOKEN` in reach on a tag run. Without checksums or a signature, a person has no way to tell the installer they downloaded from the one CI built.
- **Exploitability.** Requires compromise of a third-party action repository or Docker Hub library image; not reproducible here.
- **Remediation.** Pin every `uses:` to a full commit SHA with the version as a trailing comment (Dependabot updates these):

  ```yaml
  - uses: actions/checkout@<40-hex-sha> # v7
  ```

  Pin base images: `FROM rust:1.98-bookworm@sha256:<digest>`. In `github-release`, before `gh release create`:

  ```bash
  (cd dist && sha256sum * > SHA256SUMS)
  ```

  and add `actions/attest-build-provenance` with `id-token: write` and `attestations: write`; `cosign sign --yes bitrealm/message-crate@${digest}` after the push with keyless OIDC. Close #1019 by signing the macOS and Windows installers; until then say "unsigned" in the release notes.

### S10. `npx --yes openapi-typescript@7.13.0` executes a remote package with floating transitive dependencies in CI (Low)

- **CWE-829**.
- **Evidence.** `scripts/check-generated-api-types.sh:35` and `web/package.json` `gen:api`. The version is pinned; its dependency tree is resolved fresh on every run with no lockfile. It runs in the `web` job, which holds `contents: read` only and no secrets (`ci.yml`), and only its text output is diffed.
- **Why it matters.** A malicious transitive release runs on the CI runner. Blast radius is small (no secrets, read token), which is why this is Low.
- **Remediation.** Give the generator its own tree with a lockfile: `tools/openapi-typescript/package.json` depending on `openapi-typescript@7.13.0`, commit its `package-lock.json`, and run `npm ci --prefix tools/openapi-typescript && npm exec --prefix tools/openapi-typescript -- openapi-typescript ...`. Dependabot then covers it.

### S11. The owner's password is taken on the command line (Low)

- **CWE-214** Invocation of Process Using Visible Sensitive Information.
- **Evidence.** `crates/server/server/src/cli.rs:89-91` `CreateOwnerArgs { password: String }` with `#[arg(long)]`; `:101-103` `ResetOwnerPasswordArgs`. `docker/entrypoint-release.sh:22-24` `exec message-crate-server "$@"`, so `docker compose run --rm server create-owner --username o --password S3cret` puts the secret in the shell history on the host and in `/proc/<pid>/cmdline` in the container while it runs.
- **Remediation.** Accept `--password-stdin` (read one line from stdin) and the environment variable `MESSAGE_CRATE_OWNER_PASSWORD`; refuse `--password` with a one-line error naming the two. This stays inside `docs/adr/0001` because the server is the one command line.

### S12. A yanked crate in the lockfile; two advisories accepted with reasons that check out (Low)

- **CWE-1104**.
- **Evidence.** `cargo deny check advisories licenses bans` on the workspace: `warning[yanked]: chacha20 0.10.1` via `rand 0.10.3` (used by `demo-seed`, `obfuscate`, `message-crate-server`); `advisories ok, bans ok, licenses ok`. `deny.toml:21-24` ignores RUSTSEC-2026-0194 and RUSTSEC-2026-0195 (quick-xml 0.39.4) with the reason that the chain is `imessage-database → plist → quick-xml 0.39.4` and parses local Apple data only. Verified: `cargo tree -i quick-xml@0.39.4` reaches only `plist`; `sbr` and `go-sms-pro-exporter` use `quick-xml 0.42.0` (`Cargo.toml:63`). On `src-tauri`: `bans ok, licenses ok`, and no GPL exception was encountered, which is the desired outcome. Dependency counts: 455 packages in `Cargo.lock`, 564 in `src-tauri/Cargo.lock`, 229 in `web/package-lock.json`. `rust-toolchain.toml` pins `1.98.1`. `audit.yml` runs cargo-deny and `npm audit --audit-level=high` on lockfile changes and weekly, and is deliberately not a merge gate (`docs/adr/0007`).
- **Remediation.** `cargo update -p chacha20` in a Dependabot-style PR. Keep the two quick-xml ignores; drop them with the Dependabot ignore in `.github/dependabot.yml` when `imessage-database` moves `plist`.

### S13. Desktop webview: a wide `connect-src`, `withGlobalTauri`, and a shell permission the app does not use (Low)

- **CWE-1021**; **CWE-250** least privilege.
- **Evidence.** `src-tauri/tauri.conf.json:15` `"withGlobalTauri": true`; `:24-33` CSP with `connect-src 'self' ipc: http://ipc.localhost http: https:`, `img-src ... http: https:`, `media-src ... http: https:`, `style-src 'self' 'unsafe-inline'` with `dangerousDisableAssetCspModification: ["style-src"]`. `src-tauri/capabilities/default.json` grants `shell:default` and `shell:allow-open`, yet `web/src` imports only `@tauri-apps/plugin-dialog` (`web/src/components/PathPicker.tsx:1`), never `@tauri-apps/plugin-shell`, and the Rust side opens paths through the `open` crate confined to run, export and log directories (`src-tauri/src/commands/paths.rs:171-198` `open_path`, `:104` in `local_server.rs`). What is right: `script-src 'self'`, `object-src 'none'`, `frame-src 'none'`, `form-action 'none'`, `base-uri 'self'`; the download destination comes from the dialog, never from the window (`commands/download.rs:24`); the SPA has no raw-HTML sink.
- **Why it matters.** The wide `connect-src` is needed because the person types the server address, but it means an XSS in the webview could post the token anywhere; `withGlobalTauri` hands `window.__TAURI__.invoke` to any script that does run, and the 37 commands include starting exporters over arbitrary paths. `shell:allow-open` is unused capability.
- **Remediation.** Set `"withGlobalTauri": false` (the SPA already imports `invoke` from `@tauri-apps/api`, `web/src/lib/localServer.ts:37`); remove `shell:default` and `shell:allow-open` from `capabilities/default.json` and the `tauri-plugin-shell` dependency if nothing else needs it; try removing `'unsafe-inline'` from `style-src` under Tailwind v4 and keep it only if the build needs it.

### S14. Refused logins record the typed username, which is sometimes a password (Low)

- **CWE-532** Insertion of Sensitive Information into Log File.
- **Evidence.** `session_api.rs:189-196` `audit_trail::record_refused_login(&mut conn, &username, None, AuditReason::UnknownUsername, app)` stores the username as typed when no account matches; `trim_refused_logins` (`:174`) bounds retention.
- **Why it matters.** A person who types their password into the username field writes it into the owner-readable Audit Trail.
- **Remediation.** The logging-monitoring sibling owns this. Survey note: store a truncated or hashed form for unknown usernames, or none at all beyond the count.

## 5. What held up

Checked and found sound, with the line that proves each.

- **Search language.** FTS5 terms are quoted and `"` doubled (`search/fts.rs:11-12`); LIKE patterns are bound parameters with `ESCAPE '\'` (`search/bridge.rs:48`) after `like_escape` (`search/emit.rs:218-227`); column names are `&'static str` from the field table (`search/fields.rs:32-39`), never from input; the query is bounded (section 3.8). Dynamic SQL elsewhere interpolates enum-derived identifiers only (`db/trash.rs:100-103` `marker()`, `db/attachment_versions.rs:194` `columns()`) and counted placeholders (`dedupe.rs:450`). The database sibling deepens this.
- **GPL sidecar boundary.** `cargo tree --manifest-path src-tauri/Cargo.toml -i imessage-database` matches nothing; `cargo deny` bans and licences pass on both graphs; `deny.toml:58-75` carries the four exceptions and three wrapper bans; `scripts/check-license.sh` checks the other direction. The protocol is JSON lines with `PROTOCOL_VERSION 13` refused on mismatch and never negotiated (`crates/helpers/imessage-reader-protocol/src/lib.rs:80`; `ios-backup/src/helper.rs:196-208`); the request, backup password included, goes over stdin rather than the command line (`helper.rs:155-159`); stderr is drained on its own thread (`:147-152`); the child is killed on drop (`:322-326`); the program is found only at `MESSAGE_CRATE_IMESSAGE_READER` or beside the executable, never on `PATH` (`:47-97`). One hardening: `next_event` reads lines with no length cap (`:180-186`); a corrupt reader could make the app allocate without bound. Same-user only.
- **External program arguments.** ffmpeg inputs always follow `-i` (`media/src/process.rs:764-976`, `versions.rs:120,170`); ffprobe takes the path as the last positional (`media/src/tools.rs:538-548`), and every path handed to either is an absolute join under the asset store or a run directory, so none can begin with `-`. Hardening: write the input as `file:<path>` or put `--` before it. wtsexporter gets every path behind a flag, hex key material in a `0600` `create_new` file in the scratch directory rather than on the command line (`wtsexporter.rs:313-326, 511-530`), a scratch `cwd`, and never `--move-media` (`:335-338`).
- **Tool downloads.** Pinned release and two SHA-256s per file (`tool_downloads/pins.rs`), hash computed on the stream, gunzip only after the archive matched, the program re-hashed, `0o755`, `tempfile::persist` rename, old file replaced only after success (`tool_downloads.rs:449-525`). The download has no byte cap before the hash check, which matters only after a TLS break; the decompression is of a file whose hash already matched the pin.
- **Time-of-check issues.** Asset reads open with `O_NOFOLLOW` and stat the opened descriptor (`assets_api.rs:896-925`); the key file is `create_new` (`wtsexporter.rs:519-521`); the claim runs inside one write transaction (`server_api.rs:168-172`); `exit_with_parent.rs:72` compares `parent_id()` rather than probing a pid, so pid reuse cannot fool it; `media/tools.rs` caches a size-and-mtime stamp of the ffmpeg it found and re-checks, same-user only. No cross-privilege window found.
- **Untrusted parsers.** No `unsafe` in any exporter or parser; `crates/libs/go-sms-mms/src/robustness_tests.rs` exists; quick-xml performs no DTD or external-entity expansion; `safe_attachment_path` refuses `..`, root and prefix components (`ir/src/attachment_path.rs:43-62`) and is the one check on both import paths; worker panics are caught at `join` (`Cargo.toml:80-81`). A crafted file can at worst fail the desktop job. The input-validation sibling deepens this.
- **Error handling and logging.** 500 bodies say "internal server error" and log the chain (`server.rs:648-656`); query values are hidden from the request log except ten named ones (`server.rs:1180-1240`); `media_link=[hidden]`; the server log route is the owner's and names files by number (`server_api/log_files.rs:9,38`; `logging/files.rs:166-167`).
- **Credentials at rest and in the repository.** argon2id with library defaults and a system-RNG salt (`credentials.rs:143-156`); `git ls-files` shows no tracked `data/`, `config/config.toml` or `.env` (`.gitignore:32,38,41`); a sweep for token, key and cloud-credential patterns matched only a masked test fixture (`db/api_tokens.rs:436`). The untracked `edited-not-in-backup-2030.png` at the repository root should not be committed if it is a real screenshot of message data. The secrets-management sibling deepens this.
- **CORS and body limits.** Section 3.3. `Swagger UI` is off by default and, when on, still needs a Bearer token (`config/config.toml.example`).
- **Public registration** defaults off (`db/server_settings.rs:32`) and the docs tell the owner to keep it off on a network (`run-on-another-machine.md:44-45`).

## 6. Handed to sibling audits (one line each)

- **authentication-flow-review:** S4 password policy, S5 limiter keying, the dummy-hash timing path, `MAX_PASSWORD_BYTES`, claim race.
- **authorization-implementation:** `AuthCapability` and the Demo Account guards by id; owner-versus-account reach of `/v1/accounts/{id}/*`; API token scope capping on every request.
- **database-security:** the dynamic-identifier SQL in `db/trash.rs`, `db/attachment_versions.rs`, `dedupe.rs`; the `sqlite3_set_authorizer` write guard; file permissions of `messagecrate.db`.
- **input-validation:** exporters and PDU/WSP parsers, JSONL line validation, `connecting_app` header bounds (`server.rs:1577-1590`).
- **session-cookie-security:** S7; single-session-per-account semantics; Media Link lifetime.
- **secrets-management-audit:** the untracked PNG, `.env.example`, `DOCKERHUB_TOKEN` scope, S11.
- **api-and-infrastructure:** S2, S8, S9; `declared_query.rs`; the 406 rule.
- **file-handling-business-logic:** asset store layout, `.incoming` sweeps, multipart part sizing, `safe_attachment_path`.
- **business-logic-vulnerabilities:** Import Run states, `imports_api` semaphore (design audit F7), dedupe winner selection.
- **logging-monitoring:** S14, `logged_uri` allow list, Audit Trail retention, the server log reader.

## 7. Prioritised fixes

| # | Fix | Findings | Effort | Why first |
|---|---|---|---|---|
| 1 | `127.0.0.1:8080:8080` in both compose files; Owner Home notice when bind is not loopback over `http:`; Caddy recipe in the docs | S1, S2 | hours | Removes the default LAN exposure and the pre-claim window for everyone who starts from compose |
| 2 | Minimum password length for accounts and the owner; Demo Account stays the one NULL hash | S4 | hours | Closes the empty-password login on any published server |
| 3 | `debian:bookworm-slim` runtime with uid 1000; `cap_drop`, `read_only`, `no-new-privileges`, `HEALTHCHECK` | S8 | half a day | Removes an EOL runtime from every image and shrinks the container's reach |
| 4 | SHA-pinned actions, digest-pinned base images, `SHA256SUMS`, provenance attestation, `cosign`; `cargo update -p chacha20` | S9, S12 | half a day | Lets a person verify what they download and stops a moved tag from reaching the release build |
| 5 | Security headers on every response from `http_app` | S6 | an hour | Turns a future XSS from token theft into a blocked request |

Then: S5 (count failures only; client bucket), S13 (`withGlobalTauri: false`, drop `shell:*`), S11 (`--password-stdin`), S10 (locked generator tree), S7 and S14 with their sibling audits.

## 8. Checklist diff

The skill's "Check for" items, then the areas this report was asked to cover to full depth.

| Item | Result | Where |
|---|---|---|
| 1. All entry points identified | Pass | Section 3.1 |
| 2. All routes and endpoints | Pass | Section 3.2; the route table is the OpenAPI document, and undeclared query parameters are refused |
| 3. Middleware chain and order | Pass | Section 3.3; authentication is a per-handler extractor, not a layer, which is a documented choice and consistent |
| 4. External service integrations | Pass | Section 3.4; downloads hash-pinned; the one unencrypted integration is the app to a published server (S1) |
| 5. Database connection points | Pass | Section 3.5; SQLite only, parameterised, static identifiers |
| 6. Authentication and authorization flow | Fail (partial) | Primitives sound (section 3.6); S4 passwordless accounts and one-character owner password; S5 limiter keying; S1 cleartext when published |
| 7. File upload handling | Pass | Section 3.7; hash-verified, nosniff, `attachment` disposition, `O_NOFOLLOW`, one path-safety function |
| 8. API rate limiting | Fail (partial) | Section 3.8; auth routes only, keyed by account (S5); other routes rely on body and query limits, which is acceptable on a LAN and thin on the internet |
| Dependency and supply chain | Fail (partial) | S9 tag-pinned actions and images, S10 floating `npx` tree, S12 yanked `chacha20`; cargo-deny advisories pass; Dependabot covers all five ecosystems |
| Build and release pipeline | Fail | S9: unsigned installers (#1019), no checksums, no provenance, no image signature; token permissions are least-privilege |
| Docker image hardening | Fail | S8: EOL Node.js base, no `HEALTHCHECK`, no `cap_drop`/`read_only`; non-root user and `tini` present; S2 publishes on all interfaces |
| Desktop app surface (Tauri capabilities, CSP, commands) | Pass (with S13) | Strict `script-src`, `object-src`, `frame-src`; dialog-chosen destinations; confined `open_path`; unused `shell:allow-open` and `withGlobalTauri` to tighten |
| GPL sidecar process boundary | Pass | Section 5; graph verified, version refused not negotiated, secrets over stdin, child killed on drop |
| Search language | Pass | Section 5 |
| Time-of-check issues | Pass | Section 5; no cross-privilege window |
| Secrets in repository | Pass | Section 5; sibling deepens |
| Error handling and information exposure | Pass | 500 detail hidden; problem documents carry no internals |
| CORS | Pass | Allow list plus fixed desktop origins; `*` debug-only |
| Transport security | Fail when published | S1 by design; documented; no server-side TLS and no in-product notice |

## 9. Unable to verify

| Claim | What would prove it |
|---|---|
| The exact set of routes behind the 32 KiB `limited_auth_router` cap | `crates/server/server/src/openapi.rs` `public_openapi()`; this report read `server.rs:971-979` and the three handlers it names |
| That the `"register"` bucket is checked on anonymous `POST /v1/accounts` | `crates/server/server/src/accounts_api.rs` before line 380 (`create_account`); the bucket is named at `credentials.rs:28` |
| That the published image contains a `node` binary | `docker run --rm --entrypoint node bitrealm/message-crate:0.10.1 --version`; inferred from `docker/Dockerfile:57` |
| That `vendor/sqlx-sqlite` is byte-identical to its crates.io release plus one dependency bump | Out of scope by instruction; `VENDORING.md` and a diff against `cargo download sqlx-sqlite@<version>` |
| What `edited-not-in-backup-2030.png` at the repository root shows | The file itself (untracked; not opened) |
| Whether the Swagger UI loads under the CSP proposed in S6 | Enable `openapi_ui`, apply the layer, open `/docs` in a browser |
| Whether Tailwind v4's build needs `'unsafe-inline'` in `style-src` (S13) | Remove it in `tauri.conf.json` and run the app |

## 10. Cross-references to the design audit

- **F1** (`server.rs` God module): the middleware stack, auth resolution and error mapping all live in the file this report cites most; splitting it would make the security-relevant layers reviewable on their own.
- **F7** (import semaphore is a process-global static while the auth limiter is on `AppState`): the limiter placement is the right one for S5's client bucket.
- **F18** (`ensure_accounts_schema` on every authenticated request, `server.rs:1620`): also a small cost multiplier on the authentication path; removing it as F18 proposes helps S5's DoS resistance.
- **F11** (`src-tauri` structure): the 37 commands in `src-tauri/src/commands/` are the desktop attack surface of S13.
