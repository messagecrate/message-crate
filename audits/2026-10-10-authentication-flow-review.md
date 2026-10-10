# Authentication flow review

Date: 2026-10-10. Commit audited: `530dc1791` (branch `docs/initial-software-design-analysis`).
Scope: the whole repository except `web-next/` (legacy, out of the product path), `vendor/`, `target/`, `node_modules/` and `docs/dist`.
Method: read-only. The server's credential code (`credentials.rs`, `session_api.rs`, `server.rs`, `server_api.rs`, `accounts_api.rs` and `accounts_api/api_tokens.rs`, `db/session_tokens.rs`, `db/api_tokens.rs`, `db/account_profile.rs`, `assets_api/media_links.rs`, `db/audit_trail.rs`), the schema (`schema/sql/accounts.sql`), the web app's login state (`web/src/lib/auth.tsx`, `api.ts`, `storage.ts`, `authGuards.ts`, the login and settings screens), the desktop commands that carry a token (`src-tauri/src/commands/upload.rs`, `export.rs`) and the shared HTTP client (`crates/libs/http`). Every claim cites a `path:line` that was read; a claim that could not be confirmed is marked **Unable to verify** with the file that would prove it. Line numbers are at the audited commit. Paths are relative to the repository root.

The skill's checklist is written for Node/Express with JWTs and cookies. Message Crate has neither: it uses an opaque bearer token stored hashed, Argon2id, no cookies and no email. Each item below is judged on the principle behind it, and marked Not Applicable where the concept has no counterpart here.

Cross-reference: `audits/2026-10-09-initial-software-design-analysis.md` is the design audit. Where a finding below overlaps one of its F1..F19, the id is named and the point is not repeated.

## 1. Threat model this review rates against

The product states its deployment model in three places, and this review holds every finding to it:

- `README.md:130-131`: "Message Crate is built for a home network, not for the open internet. If you want to expose Message Crate to the internet, you should stick it behind a reverse proxy with an auth layer of your choosing."
- `docs/src/content/docs/docs/user/features/owner/run-on-another-machine.md:35-41`: the server speaks plain HTTP; passwords, Session tokens and messages cross the network unencrypted; anything beyond a trusted home network needs a reverse proxy with TLS. `:43-45`: the owner's password "is the only thing between the network and every account", and public registration should stay off.
- `docs/architecture/http-api.md`, "Credentials and reach" (`:473` onward), is the rule set for the Session, the API token, the Media Link, the owner and the rate limits.

Defaults that follow from it: the server binds `127.0.0.1:8080` (`crates/server/server/src/config.rs:194-196`), the desktop app starts it there (`src-tauri/src/local_server.rs:68`), and the Docker image binds `0.0.0.0:8080` (`config/config.docker.toml:17`) and publishes it on every interface (`docker/compose.release.yml:31`).

So: a finding that needs an attacker on the same network is in scope and rated at its LAN severity. A finding that only matters when the server is reachable from the internet is said to be so, in its own words, and rated for the LAN with the exposed rating in brackets. The account login is not a substitute for the proxy the README asks for, and nothing below argues that it should be.

## 2. Summary

The authentication design is sound and unusually well documented for a project this size: Argon2id with the library defaults, random 190-bit tokens stored as SHA-256, one guard macro so a route cannot compile without its capability check, permissions read from the database on every request, the owner carrying no message permissions at all, a Media Link signed with a per-process HMAC key and compared in constant time, no credential in any log line, and an Audit Trail that records refused logins without recording what was typed as a password. No injection, no mass assignment, no open redirect and no token in a URL were found.

The findings are about policy rather than mechanism. Two are rated Medium on the home-network model:

1. Any account except the owner may have no password at all, and the login accepts an empty password for it. The UI offers this as "Password (optional)".
2. An account changes its own password with the Session alone; the current password is not asked for, though it is asked for when the same account deletes itself.

The rest are Low or informational: a fixed-window rate limit with no backoff and no minimum password length; an owner-set password that leaves the holder's Session and API tokens running; a login that tells a correct-password guesser the account is disabled; a 30-day token in `localStorage` on a site served without a Content-Security-Policy; API tokens that may never expire; a modulo bias in the token alphabet; Unicode usernames that look alike; and a Docker image that listens on every interface before an owner exists.

**Risk score: 4 / 10** on the stated home-network model (7 / 10 if the port is reachable from the internet without the proxy and TLS the README requires, because every Low that depends on network reach becomes real, and F1 and F3 together make a password guessable).

Counts: Critical 0, High 0, Medium 2, Low 7, Informational 2.

## 3. Findings

Severity is the LAN rating; the bracketed rating is for a server exposed to the internet against the README's advice.

### A1. Any account may have no password, and the login accepts an empty one

- **Severity:** Medium (High if exposed). **CWE-521** Weak Password Requirements; **CWE-258** Empty Password in Configuration File.
- **Evidence:**
  - `crates/server/server/src/credentials.rs:208-216` `hash_user_password`: "There is no minimum length: an empty password is `None`, an account with no password."
  - `credentials.rs:186-196` `verify_login_password`: a `None` or empty hash accepts exactly the empty password.
  - `crates/server/server/src/session_api.rs:210-211` calls it on every login.
  - `crates/server/server/src/accounts_api.rs:364` `create_account` stores `hash_user_password(req.password.as_deref().unwrap_or(""))` for an account made by the owner and for one made by a stranger under public registration (`:349-362`, `:369-373`).
  - `accounts_api.rs:1087-1092` `replace_account_password` lets a user account clear its password to `None`; `web/src/screens/settings/ChangePasswordSection.tsx:15,154` offers this as "Reset password... This account now has no password".
  - `web/src/screens/settings/NewAccountPanel.tsx:69` heads the owner's form "Password (optional)"; `docs/src/content/docs/docs/user/features/owner/owner-home.md:110` and `docs/.../your-messages/create-the-owner-and-an-account.md:35` say "The form allows an account with no password. An account that holds real messages should have one."
  - `docs/src/content/docs/docs/developer/reference/config-and-accounts.md:106-108` documents the rule.
  - The Demo Account needs a NULL hash by design: `docs/adr/0016-the-demo-account-is-fixed-not-configured.md`, `crates/server/server/src/db/account_profile.rs:458,476`, `server.rs:163-181`.
- **Why it matters:** The docs say the owner's password is "the only thing between the network and every account" (`run-on-another-machine.md:43`). For a passwordless account there is nothing between the network and that account's whole archive: anyone who can reach the port and knows or guesses the username logs in with `{"username":"alice","password":""}`. The guidance "should have one" is advice, and the mechanism that exists for the Demo Account alone is open to every account. Usernames are not secret: the Audit Trail and the owner's list hold them, and people reuse first names. Under public registration a stranger can also register with no password, which the docs do not say.
- **Exploitability:** On the LAN, `curl -X POST http://server:8080/v1/session -H 'content-type: application/json' -d '{"username":"alice","password":""}'` answers `201` with a Session token for any account made without a password. No rate limit applies in a meaningful way: one request suffices.
- **Remediation:** Keep the NULL hash for id 2 only.

  ```rust
  // credentials.rs
  /// Hash a user account's password. Every account but the Demo Account has one.
  pub(crate) fn hash_user_password(password: &str) -> Result<String, ApiError> {
      if password.is_empty() {
          return Err(ApiError::validation("the account must have a password"));
      }
      require_hashable(password)?;
      Ok(hash_password(password)?)
  }
  ```

  Then `accounts_api.rs:364` and `:1091` take `Some(hash)`, `verify_login_password` keeps its `None` arm for the Demo Account only (or asserts `is_demo_account` in that arm), the "Reset password" action in `ChangePasswordSection.tsx` goes, and the schema comment at `schema/sql/accounts.sql:12-13` changes to "NULL for the Demo Account only". The project's "no backwards compatibility" rule (`CLAUDE.md`) makes this a clean change: existing passwordless rows need a one-time owner action, which `GET /v1/accounts` can surface through its existing `has_password` field (`accounts_api.rs:79-81`, computed at `:152-154`).
- **Defense in depth:** a minimum length (see A3); an owner-visible warning in the accounts list for any account whose `has_password` is false until the rows are fixed.

### A2. An account changes its own password without its current password

- **Severity:** Medium. **CWE-620** Unverified Password Change; **CWE-640** Weak Password Recovery Mechanism.
- **Evidence:**
  - `crates/server/server/src/accounts_api.rs:1028-1029`: "For a user account the session is the credential, and the current password is not asked for." `:1067-1085`: `current_password` is checked only when `reach == Reach::OwnersOwn`; the `else` branch checks only that the pair matches.
  - `credentials.rs:293-297` `change_password_on_conn`: "The logged-in session is the credential: the current password is not asked for."
  - By contrast, the same account deleting itself must send `current_password` when it has one (`accounts_api.rs:940-948` `has_password` then `require_current_password`), and the owner changing its own must too (`:1067-1074`). `docs/architecture/http-api.md` ("Credentials and reach", the rate-limit paragraph) confirms the deletion rule.
  - The Session lasts 30 days (`crates/server/server/src/db/session_tokens.rs:14` `SESSION_TTL_SECS`) and is kept in `localStorage` (`web/src/lib/auth.tsx:127-136`, `web/src/lib/storage.ts:21-27`). One Session per account (`schema/sql/accounts.sql:45-66`, `session_tokens.rs:206-232` upsert on `account_id`).
- **Why it matters:** Whoever holds the token holds the account for 30 days anyway; the password check at this one route is what turns a temporary hold into a permanent one. With it missing, a token read from a shared or unlocked browser, a backup of the browser profile, or any XSS (A6) lets the attacker set a new password, which revokes the holder's API tokens (`credentials.rs:309`) and rotates the Session (`:312`) so that the holder is logged out and the attacker holds the only live Session. The holder cannot log back in and must ask the owner. The project already decided deletion needs the password; a password change is at least as consequential.
- **Exploitability:** With a stolen `mc-user-...` token: `PUT /v1/accounts/{id}/password` with `{"password":"x","password_confirmation":"x"}` answers `200` and a new token. Nothing else is needed.
- **Remediation:** Apply the deletion rule to the change, through the same rate-limited check.

  ```rust
  // accounts_api.rs, replace_account_password, after require_account_reach
  if reach.is_own() {
      let password_hash = account_profile::load_password_hash(&mut conn, target).await?;
      if has_password(password_hash.as_deref()) {
          let Some(current) = req.current_password.as_deref() else {
              return Err(ApiError::validation("Current password is required to change the password."));
          };
          require_current_password(&state, target, password_hash.as_deref(), current)?;
          if req.password == current {
              return Err(ApiError::validation("New password must be different from the current password."));
          }
      }
  }
  if req.password != req.password_confirmation { /* as now */ }
  ```

  `require_current_password` (`accounts_api.rs:985-1000`) already counts a wrong guess in the account's login bucket, so this adds no new guessing path. The web form already has a `requireCurrent` branch (`ChangePasswordSection.tsx:58`); make it unconditional for an account with a password. Update the doc comment at `:1028-1035` and `credentials.rs:293-297`.
- **Defense in depth:** the Audit Trail already records `password_set` by the holder (`credentials.rs:313-318`); have the web app show the holder a notice when the entry's session differs from the current one.

### A3. Brute force: fixed window only, no password policy, Argon2 on the async executor

- **Severity:** Low (Medium if exposed). **CWE-307** Improper Restriction of Excessive Authentication Attempts; **CWE-521**; **CWE-400** Uncontrolled Resource Consumption.
- **Evidence:**
  - `crates/server/server/src/credentials.rs:22-23` `AUTH_RATE_WINDOW` 60 s, `AUTH_RATE_MAX` 20; `:125-137` `refuse_when_full` refuses only while 20 hits sit inside the window, with no growth in the wait. `:35` the limiter is a process-local `HashMap`, so a restart forgets it. `:93-122` `with_bucket`.
  - Per-account bucket (`:41-43` `password_bucket`) counts every login attempt as the account; a username nobody holds gets its own bucket (`:48-50`), so a spray across usernames is 20 per minute per name with no server-wide ceiling.
  - Password policy: owner needs one character (`:198-206` `hash_owner_password`), a user account none (`:208-216`); maximum 1024 bytes (`:20`, `:219-224`). The username rule is the only shape check (`:235-243`). `credentials/tests.rs` pins this in `the_owner_needs_one_character_and_a_user_needs_none`.
  - Argon2 cost: `Argon2::default()` (`credentials.rs:152`, `:163`) is Argon2id with the library defaults m = 19 MiB, t = 2, p = 1 (`argon2-0.6.0/src/params.rs:48,58,67`; `lib.rs:222-224` in the cargo registry), the OWASP minimum. `verify_password` is synchronous and is called from the async handler without `spawn_blocking` (`session_api.rs:189`, `:211`; `accounts_api.rs:993` through `passwords_match`), so each verify holds a Tokio worker for the hash's duration and 19 MiB.
  - Body cap for the credential routes is 32 KiB (`server.rs:1004-1006`, `:964-970`), which bounds the per-request input.
- **Why it matters:** 20 per minute is 28,800 guesses a day at one account, and with no minimum length and a human-chosen password that is enough from the LAN over a weekend. The limit resets on restart, so a crash loop (or an attacker who can cause one) resets it. A spray over made-up usernames costs the server one Argon2 per request with no cap but the network, which is a cheap CPU and memory denial from one LAN host. None of this reaches a server the README's proxy protects, but a self-hoster who skips the proxy relies on exactly this limiter.
- **Exploitability:** `for p in $(cat wordlist); do curl -s -o /dev/null -w '%{http_code}\n' -X POST http://server:8080/v1/session -H 'content-type: application/json' -d "{\"username\":\"alice\",\"password\":\"$p\"}"; sleep 3; done` runs under the limit indefinitely. For the CPU case, 200 parallel logins with distinct usernames each run Argon2 on a worker thread.
- **Remediation:**
  1. Progressive delay per account: when `hits.len() >= AUTH_RATE_MAX`, double the window for that bucket on each further refusal (store `(hits, penalty)` in the map), with a cap of, say, one hour. Keep the per-account key.
  2. One server-wide bucket for unknown usernames beside the per-name one, so a spray is capped at, for example, 60 per minute across all names.
  3. `tokio::task::spawn_blocking(move || verify_password(&hash, &password)).await` in `create_session`, `require_current_password` and `claim_server`, so a verify never holds a worker.
  4. A minimum length for every password that is set: 8 characters in `require_hashable`, which also fixes the owner's one-character floor.
- **Defense in depth:** write the owner a line in the server log when a bucket trips (the Audit Trail already has `login_refused` rows, `db/audit_trail.rs:711-733`); document in `run-on-another-machine.md` that the limiter is per process.

### A4. An owner-set password leaves the holder's Session and API tokens running

- **Severity:** Low. **CWE-613** Insufficient Session Expiration.
- **Evidence:**
  - `crates/server/server/src/accounts_api.rs:1032-1035`: "The owner setting another account's answers `204`. That is the whole of it: the account's sessions carry on". `:1100-1110` the `Reach::Owner` branch writes the hash and an Audit Trail entry and nothing else.
  - The holder's own change revokes every API token and rotates the Session (`credentials.rs:302-321`). The CLI `reset-owner-password` ends the owner's sessions (`crates/server/server/src/owner_cli.rs:67-69`, `:99`). The test `an_owner_password_reset_leaves_the_session_browsing` (`session_api/tests.rs`) pins the owner-set behaviour.
- **Why it matters:** The owner sets a holder's password in two cases: the holder forgot it, or the holder's device or Session is compromised. In the second case the new password changes nothing for 30 days: the stolen Session browses on, and every API token keeps importing and exporting. The owner's only remedy is to disable the account (`server.rs:1654-1656` refuses a disabled account on every request) or revoke each token by hand, and the UI does not say so.
- **Exploitability:** Attacker holds a Session token of account 100; owner sets a new password for account 100; attacker's `GET /v1/conversations` still answers `200` until the token's `expires_at`.
- **Remediation:** In the `Reach::Owner` branch, inside the same transaction: `api_tokens::delete_all_api_tokens(&mut tx, target)`, `DELETE FROM account_session_tokens WHERE account_id = $1`, and `audit_trail::record_session_end(.., AuditReason::Revoked, AuditActor::Owner)` when `live_login_entry` finds one. Change the test to assert the opposite. If "sessions carry on" is a deliberate choice (the holder may be logged in on the desktop while the owner helps), make it a request field, `end_sessions: bool`, default true.

### A5. Login tells a correct-password guesser that the account is disabled, and skips Argon2 while the Demo Account builds

- **Severity:** Low. **CWE-204** Observable Response Discrepancy; **CWE-208** Observable Timing Discrepancy.
- **Evidence:**
  - `crates/server/server/src/session_api.rs:210-221` verifies the password first; `:223-236` then answers `403 account-disabled` (`problem.rs:152`) only when the password was right. A wrong password for a disabled account answers the generic `401 invalid-credentials` (`:125-127`). `disabled_account_cannot_log_in` (`session_api/tests.rs`) pins the 403.
  - `:205-207`: while the Demo Account is being built, a login as `demo` returns `invalid_login()` before any Argon2 work, so its timing differs from every other refused login. `GET /v1/server` reports `demo_account` openly (`server_api.rs:123-126`), so this leaks nothing new.
  - `POST /v1/accounts` answers `409 username already taken` (`credentials.rs:265-287`) to a stranger while public registration is on, which enumerates usernames; inherent to self-registration and the docs say to keep it off (`run-on-another-machine.md:44-45`).
  - Otherwise the two refusal paths do the same work: a lookup, `trim_refused_logins`, one Argon2 (`:189` dummy hash or `:211` real hash) and one Audit Trail insert (`:190-197`, `:212-219`). `invalid_login` is one sentence for both (`:123-127`).
- **Why it matters:** A disabled account is often one the owner disabled because of a problem; the login confirms the password to whoever guessed it, and the password stays guessable at the full rate (A3) while disabled. The oracle is small (the attacker needs the right password to see it) and the owner re-enables accounts, so a confirmed password is worth something later.
- **Exploitability:** Guess passwords for a disabled account; a `403` instead of `401` means the guess was right.
- **Remediation:** In `create_session`, when `auth.disabled`, record `AuditReason::AccountDisabled` as now but answer `invalid_login()`. The holder learns they are disabled from the owner, which is what `account-disabled` on an existing Session (`server.rs:1654-1656`) already does. Keep the demo-building shortcut; it reveals a public fact.

### A6. A 30-day bearer token in `localStorage`, on a site served without a Content-Security-Policy

- **Severity:** Low. **CWE-922** Insecure Storage of Sensitive Information; **CWE-1021**/CSP absence as hardening.
- **Evidence:**
  - `web/src/lib/auth.tsx:66` `STORAGE_KEY`, `:127-136` `persistState` writes `{serverUrl, token, accountId}`; `:101-111` `loadPersisted`; `web/src/lib/storage.ts:21-27` writes `window.localStorage`. `web/src/lib/authGuards.ts:24-43` validates the shape on read. The token in memory is `web/src/lib/api.ts:4,13-15` and sent as `Authorization: Bearer` (`:174-176`).
  - `web/index.html:7` onward is an inline theme script, and the page has no CSP `<meta>`; the server sets no `Content-Security-Policy` header for the static site (the only security header found is `x-content-type-options: nosniff` on asset bytes, `crates/server/server/src/assets_api.rs:967-968`). The desktop WebView has a CSP (`src-tauri/tauri.conf.json:24-37`, `script-src 'self'`), the browser route does not.
  - The architecture rejects a cookie with a stated reason (`docs/architecture/http-api.md`, "Credentials and reach", "Rejected: a cookie": the desktop origin is `tauri://localhost` and plain HTTP cannot carry `SameSite=None; Secure`). This review agrees the cookie is not available.
  - No `dangerouslySetInnerHTML`, `innerHTML` or token-in-URL was found in `web/src` (grep over `*.ts`, `*.tsx`, tests excluded). React escapes text by default.
  - `auth.tsx:246`: on restore, the saved login is cleared only on `401`; a `403 account-disabled` leaves the token in `localStorage`.
- **Why it matters:** The token is readable by any script on the origin for 30 days with no idle expiry (A7). Today nothing injects script, so this is hardening: one XSS anywhere in `web/` or in a reverse proxy's error page served on the same origin would hand over the archive for a month, and A2 would then make it permanent. A CSP costs little once the inline script is moved to a file.
- **Exploitability:** None today; conditional on a future XSS.
- **Remediation:** Serve a CSP for the static site from the server: `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'` via `tower_http::set_header::SetResponseHeaderLayer` on the `ServeDir` fallback (`server.rs:1302`), with the theme script moved from `web/index.html:7` into a file under `web/public/`. Clear the saved login on `403 account-disabled` too (`auth.tsx:246`). Consider `sessionStorage` for the website, keeping `localStorage` for the desktop app where the window is the only reader.

### A7. Session lifetime is a fixed 30 days with no idle expiry; API tokens may never expire

- **Severity:** Low. **CWE-613** Insufficient Session Expiration.
- **Evidence:**
  - `crates/server/server/src/db/session_tokens.rs:14` `SESSION_TTL_SECS = 30 days`; `:206-232` and `:266-307` set `expires_at = now + TTL` at login or rotation; nothing extends or shortens it per request; `:106-114` deletes an expired row only when its token is presented.
  - `crates/server/server/src/accounts_api/api_tokens.rs:91-93`: `expires_in_days` "Omit for the default (365 days). Pass `0` for no expiry." `db/api_tokens.rs:31` `DEFAULT_API_TOKEN_TTL_SECS` 365 days; `:376-389` `Some(0) => None`. No upper bound on `days`.
  - The password change rotates the Session and keeps it alive (`credentials.rs:312`, `session_tokens.rs:194-197`).
- **Why it matters:** A browser left logged in on a shared computer stays logged in for a month regardless of use. An API token with `expires_in_days: 0` is a permanent credential that only a person can end; the owner can revoke it (`http-api.md`, "Credentials and reach") but has to notice it first. The skill's 5 to 15 minute access token does not fit a self-hosted app with no refresh flow; a bounded idle timeout does.
- **Exploitability:** Open a logged-in browser on day 29.
- **Remediation:** Sliding idle expiry of 7 days capped at 30 absolute: on each `lookup_session` hit where `expires_at - now < 23 days`, `UPDATE account_session_tokens SET expires_at = min(created_at + 30d, now + 7d)`. Make "never" an explicit confirmation in `ApiTokensSection.tsx` and cap `expires_in_days` at, for example, 730 in `create_api_token`. Both are policy, so the http-api.md section should state the chosen figures.

### A8. Token alphabet has a modulo bias

- **Severity:** Informational. **CWE-331** Insufficient Entropy (not exploitable at this size).
- **Evidence:** `crates/server/server/src/db/session_tokens.rs:11` `TOKEN_ALPHANUM` has 62 symbols; `:26-34` `generate_prefixed_token` maps each random byte with `% TOKEN_ALPHANUM.len()`. 256 mod 62 = 8, so `A` to `H` each appear with probability 5/256 and the rest 4/256. `:42-50` `fill_random` uses `rand::rngs::SysRng` (`rand 0.10.3`, `Cargo.lock`), which is correct.
- **Why it matters:** Min-entropy per character is log2(256/5) = 5.68 bits instead of 5.95, so a 32-character token has about 182 bits of min-entropy instead of 190. Both are far beyond any online or offline guess against a SHA-256 hash. This is noted because it is in the credential generator and the fix is two lines.
- **Remediation:** Draw 24 random bytes and encode with `base64::engine::general_purpose::URL_SAFE_NO_PAD` (32 characters, uniform), or reject bytes >= 248 before the modulo. `mask_api_token` (`db/api_tokens.rs:39`) and the `mc-api-Sd..mE` hint still work.

### A9. `401` answers carry no `WWW-Authenticate` header

- **Severity:** Informational. RFC 9110 section 15.5.2 requires the header on a `401`.
- **Evidence:** `crates/server/server/src/problem.rs:141-143` maps `InvalidCredentials`, `AuthenticationRequired` and `MediaLinkInvalid` to `401`; no `WWW-Authenticate` is set anywhere in `crates/server/server/src` (grep).
- **Why it matters:** The product's own clients branch on the problem `type`, so nothing breaks. Generic HTTP tooling expects the header, and the OpenAPI document declares a `bearer` scheme.
- **Remediation:** In `ApiError::into_response` for the three `401` variants, add `WWW-Authenticate: Bearer realm="message-crate"` (and `error="invalid_token"` for an expired or unknown token, per RFC 6750).

### A10. Unicode usernames with ASCII-only case folding

- **Severity:** Low. **CWE-1007** Insufficient Visual Distinction of Homoglyphs.
- **Evidence:** `crates/server/server/src/credentials.rs:235-243` `is_valid_username` accepts any `char::is_alphanumeric`, which includes every script. The lookup folds with `COLLATE NOCASE` (`db/account_profile.rs:257`), ASCII only, and the rate-limit bucket folds with `to_ascii_lowercase` (`credentials.rs:48-50`) to match. `require_username_free` (`:265-287`) is therefore consistent with the lookup.
- **Why it matters:** `admin` and `Admin` are one account (good), but `аdmin` with a Cyrillic `а` is a second account that is indistinguishable from the first in the owner's accounts list, the Audit Trail and a login card. The risk is confusion for the owner (permissions granted to the wrong row, an audit entry misread), not escalation: every permission check is by id.
- **Remediation:** Restrict usernames to ASCII (`c.is_ascii_alphanumeric()`), or apply NFKC normalization and refuse mixed scripts. The former matches the `COLLATE NOCASE` folding exactly.

### A11. The Docker image listens on every interface before an owner exists

- **Severity:** Low (Medium on a shared or untrusted network). **CWE-1188** Insecure Default Initialization of Resource.
- **Evidence:**
  - `crates/server/server/src/server_api.rs:133-140` `claim_server` is unauthenticated by design: "whoever installs the software claims it and then publishes the port, in that order and at times of their choosing." `:158-216` is correct: rate limited server-wide (`:164`), hashed with the owner rule (`:165`), checked and inserted in one write transaction (`:171-186`).
  - `config/config.docker.toml:17` `bind = "0.0.0.0:8080"`; `docker/compose.release.yml:31` `"0.0.0.0:8080:8080"`; `docker/entrypoint-release.sh:25` starts `serve` with that config. `docs/src/content/docs/docs/developer/docker.md:74`: on a fresh volume the server "leaves Message Crate unclaimed" and listens.
  - The server's own default is loopback (`config.rs:194-196`), and the desktop app keeps it there (`src-tauri/src/local_server.rs:68`).
- **Why it matters:** The Docker route publishes the port first and lets the person claim second, the reverse of the order the code comment relies on. On a home network with a guest, an IoT device or a compromised laptop, whoever posts to `/v1/server/claim` first owns the installation and the Demo Account's data. The rate limit (20 per minute) does not stop one request. The memory rule for this repository, Docker and app server parity, says not to design a Docker-only variant of the server; this finding is about the compose file and the docs, not the server.
- **Exploitability:** `curl -X POST http://host:8080/v1/server/claim -H 'content-type: application/json' -d '{"username":"me","password":"p"}'` from any LAN host before the installer opens the browser.
- **Remediation:** `docker/compose.release.yml:31` publishes `"127.0.0.1:8080:8080"` by default, with a comment on how to widen it after claiming; `docker.md` says to claim before widening. Alternatively, the entrypoint takes `MC_OWNER_USERNAME` and `MC_OWNER_PASSWORD_FILE` and runs the existing `create-owner` (`owner_cli.rs:25`) before `serve`, so the port never listens unclaimed. The first is smaller and keeps the two routes alike.

## 4. Prioritized fixes

1. **A2**: require `current_password` for an account's own password change, through `require_current_password`. One handler, one form branch, one test. Removes the permanent-takeover step from every token theft.
2. **A1**: make a password required for every account but the Demo Account. One function (`hash_user_password`), two call sites, one UI heading, two doc lines. Closes the one-request login.
3. **A3**: progressive delay per account, a server-wide ceiling for unknown usernames, `spawn_blocking` around Argon2, and an 8-character minimum. Turns a weekend attack into a year.
4. **A4**: the owner's password set ends the holder's Session and API tokens. Gives the owner a remedy that works.
5. **A11** and **A6**: compose binds loopback by default; the server sends a CSP for the static site. Both are configuration-sized.

## 5. Checklist

| # | Item | Result | Where |
|---|---|---|---|
| 1 | Password hashing: slow hash with a random salt, verify with the library, no double hashing | Pass | Argon2id, library defaults (m 19 MiB, t 2, p 1), salt by the library: `credentials.rs:148-156`, `:159-166`; one hash per set, `accounts_api.rs:1088-1092`. Note: synchronous on the async executor (A3). |
| 2 | Secret and key strength and storage | Pass | No JWT. Session and API tokens are random (`session_tokens.rs:26-50`) and stored as SHA-256 (`:37-39`; `schema/sql/accounts.sql:49-50`, `:76-77`). Media Link key is 32 random bytes made at start and never written (`media_links.rs:58-64`). Modulo bias noted (A8). |
| 3 | Token settings: lifetimes bounded, claims enforced | Fail (Low) | Session 30 days fixed, no idle expiry (`session_tokens.rs:14`); API token default 365 days, `0` = never (`api_tokens.rs:91-93`). A7. Expiry is enforced on every lookup (`session_tokens.rs:106-114`, `db/api_tokens.rs:131-138`). |
| 4 | Refresh token rotation, reuse detection, cookie storage | N/A | No refresh token. One Session per account; a new login replaces it (`session_tokens.rs:266-307`). Stored in `localStorage`, not a cookie, by documented decision (`http-api.md`, "Rejected: a cookie"); see A6. |
| 5 | Session invalidation on password change | Partial | Own change rotates the Session and deletes every API token (`credentials.rs:302-321`); CLI owner reset ends sessions (`owner_cli.rs:99`); owner-set password for another account ends nothing (A4). Disable is enforced per request (`server.rs:1654-1656`). |
| 6 | Brute force protection | Partial | Fixed window 20 / 60 s per account and for register and claim server-wide (`credentials.rs:22-23`, `session_api.rs:169`, `accounts_api.rs:359`, `server_api.rs:164`); wrong `current_password` counts in the same bucket (`accounts_api.rs:985-1000`). No backoff, no server-wide cap for unknown usernames, resets on restart (A3). |
| 7 | Account enumeration defenses | Partial | One sentence for unknown user and wrong password (`session_api.rs:125-127`), dummy Argon2 for unknown users (`:189`), same Audit Trail write on both paths. `403 account-disabled` after a correct password (A5); `409` on registration (inherent, registration off by default). |
| 8 | Password reset flow | N/A | No self-service reset and no email. The owner sets a password (`accounts_api.rs:1100-1110`) or the shell does (`owner_cli.rs:75`). |
| 9 | Email verification | N/A | No email. |
| 10 | SQL injection in auth paths | Pass | Every auth query binds parameters: `session_tokens.rs:96-101`, `account_profile.rs:257-259`, `db/api_tokens.rs:119-124`, `audit_trail.rs:746-751`. |
| 11 | AuthZ integrity: server-side, deny by default | Pass | Permissions read from the row on every request and intersected with the token's (`server.rs:1651-1671`); the Demo Account's are fixed by id (`account_profile.rs:533-535`); guards are extractors so a route cannot compile without one (`server.rs:323-381`); the owner carries none (`:63-68`, `:86-93`); the credential names the account, no `account=` parameter (`:1687-1689`). |
| 12 | Cookie and CSRF configuration | N/A | No cookies. Bearer header only (`api.ts:174-176`). CORS: exact origin list plus the desktop origins; `["*"]` is permissive and documented as debugging only (`server.rs:916-959`, `config.rs:151-158`); no `allow_credentials`. |
| 13 | Input validation and normalization | Partial | Username trimmed and shape-checked (`credentials.rs:231-255`); password capped at 1024 bytes (`:20`); body capped at 32 KiB on credential routes (`server.rs:1004-1006`). No minimum length (A3); Unicode usernames (A10). |
| 14 | Mass assignment | Pass | `disabled`, `can_import`, `can_export`, `can_delete` refused unless the owner acts on another account (`accounts_api.rs:810-812`, `:826-837`); `CreateApiTokenRequest` is `deny_unknown_fields` and scopes are intersected (`api_tokens.rs:81`, `:260-261`). |
| 15 | JWT misuse | N/A | No JWT. |
| 16 | Logging and telemetry | Pass | Trace span logs method, hidden-value URI and request id only (`server.rs:1322-1337`, `:1194-1245`); `media_link` and `q` hidden; `docs/architecture/server-log.md:119,129-130`. Audit Trail never holds a credential (`schema/sql/accounts.sql:404-406`); a refused login records the typed username cut to 128 characters (`audit_trail.rs:718-721`), which may capture a password typed into the wrong field; minor and common. No `Debug` print of a request struct found. |
| 17 | Dependency and crypto hygiene | Pass | `argon2 0.6.0`, `password-hash 0.6.1`, `rand 0.10.3`, `hmac 0.13.0`, `sha2 0.11.0`, `subtle 2.6.1` (`Cargo.lock`); HMAC verified with `verify_slice` in constant time (`media_links.rs:92-94`); no custom token parser; `audit.yml` runs `cargo deny`. |
| 18 | Transport and CORS | Fail by design (documented) | Plain HTTP; TLS is the proxy's job (`run-on-another-machine.md:35-41`). The shared client detects an http to https redirect that drops `Authorization` (`crates/libs/http/src/session.rs:98,152-154`, `auth_error.rs:71`). CORS as in item 12. Only matters off the home network. |
| 19 | Open redirect / `next` parameter | Pass | No `next`, `redirect` or `returnTo` parameter and no `window.location` assignment from input in `web/src` (grep); the login screen navigates by state (`LoginScreen.tsx`). |
| 20 | Operational controls | Partial | Media Link key rotates on every restart; tokens are random so there is no shared secret to rotate; the Audit Trail records logins, refusals with reason, session ends and token events for the owner (`audit_trail.rs:711-733`). No alerting (reasonable for self-hosted); no documented procedure for a suspected token leak beyond "revoke it". |

## 6. Hardening suggestions and quick tests

Hardening beyond the findings:

- Show `has_password` in the owner's accounts list with a warning until A1 lands (**Unable to verify** whether it is shown today; see section 7).
- Add `Referrer-Policy: no-referrer` and `X-Frame-Options: DENY` beside the CSP in A6.
- Record the client address of a refused login in the server log (not the Audit Trail) so an owner behind no proxy can see where a spray comes from; the http-api.md note about forwarded addresses means the raw peer address, labelled as such.

Quick tests that would pin the fixes (server crate, beside the existing `session_api/tests.rs` and `accounts_api/tests.rs`):

1. A2: `PUT /v1/accounts/{id}/password` by the account with a password and no `current_password` answers `422`; with a wrong one answers `401` and the twenty-first wrong one answers `429`; with the right one answers `200` and the old token is refused.
2. A1: `POST /v1/accounts` by the owner with `"password": ""` answers `422`; `POST /v1/session` with `"password": ""` for an account answers `401`; the Demo Account still logs in with an empty password.
3. A4: after the owner sets account 100's password, account 100's previous token answers `401` on `GET /v1/session` and `GET /v1/accounts/100/api-tokens` lists nothing.
4. A3: after 20 refused logins, the `Retry-After` of the 41st is longer than that of the 21st; 100 logins as 100 different unknown usernames inside a minute answer `429` past the server-wide ceiling.
5. A5: a disabled account with the right password answers `401 invalid-credentials`, and the Audit Trail holds `login_refused` with reason `account_disabled`.
6. A8: 10,000 generated tokens have a per-character distribution whose maximum frequency is within 10 percent of the minimum.

## 7. Unable to verify

| Claim | Proving file(s) |
|---|---|
| Whether the owner's accounts list shows `has_password`, so the owner can see passwordless accounts today | `web/src/screens/owner/OwnerAccountsPanel.tsx`, `web/src/screens/owner/useOwnerAccounts.ts` (grep for `has_password` found nothing in either; the field exists on the wire at `accounts_api.rs:79-81`) |
| Whether reqwest's default redirect policy strips `Authorization` on a same-host http to https redirect, or only on a host change, which decides whether `session.rs:98` describes reqwest or the server | `crates/libs/http/src/lib.rs:83-89` `build_client` (no explicit `redirect` policy is set) and reqwest's own `redirect::Policy` documentation |
| Whether any reverse-proxy guidance in the docs tells the operator to pass or strip `Authorization`, which affects item 18 | `docs/src/content/docs/docs/user/features/owner/run-on-another-machine.md` (the audited lines defer the proxy setup) |
| Whether the Tauri WebView persists `localStorage` across app restarts in a location another local user can read, which decides the weight of A6 on the desktop | `src-tauri/tauri.conf.json` and the platform WebView's data directory rules |
| Whether `trim_refused_logins` and the two Audit Trail inserts on a refused login hold SQLite's write lock long enough to slow a concurrent import, which would make a login spray a write-contention problem too | `crates/server/server/src/db/write_tx.rs`, `session_api.rs:175,190-197` |

## 8. Cross-references to the design audit

- The authentication core lives in `server.rs` (`AuthIdentity`, the guards, `resolve_auth_on_conn`, CORS, the body limits, the log redaction). The design audit's **F1** (God module) applies; moving `bearer_token`, `resolve_auth*` and the `auth_guard!` block into `credentials.rs` or an `auth.rs` would also put every rule this review cites in one file.
- `ensure_accounts_schema` runs on every authenticated request (`server.rs:1617`, `session_api.rs:273`, `api_tokens.rs:264`). That is **F18**; not repeated here.
- The owner's `reset_demo.rs` and the Demo Account's NULL hash are the reason A1 must keep a NULL arm for id 2 (`docs/adr/0016`); the design audit's **F13** covers that file's shape.
