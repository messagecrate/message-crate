# Input validation audit

Date: 2026-10-10. Commit: `530dc1791`. Scope: the whole repository less `web-next/`, `vendor/`, `target/`, `node_modules/` and `docs/dist`.

This audit follows the `input-validation` skill: it reviews every boundary where untrusted data enters Message Crate (HTTP bodies, query strings, path segments, headers, the JSON Lines import body, the search language, the address book CSV, phone numbers, time zones, the config file, CLI arguments, account credentials, the desktop app's IPC commands, and the parsers that read phone backups) and every place that data leaves again as HTML, mail, CSV or XML.

Two sibling audits run beside this one. **database-security** owns SQL construction; this report notes only that every user value it saw reaches SQLite as a bound parameter and leaves the proof to that audit. **file-handling-business-logic** owns file paths and uploads in depth; where a boundary turns into a path (asset storage, attachment paths in an import, the iOS backup `Manifest.db`, desktop run directories) this report names the check and stops.

Assumptions, stated because nothing could be asked:

- No code was compiled or run. Every claim rests on reading the source at this commit; behaviour that depends on a library's run-time choice is marked **Unable to verify** and the proving file is named.
- Line numbers are those of the working tree at `530dc1791`.
- The design audit `audits/2026-10-09-initial-software-design-analysis.md` is cross-referenced by its finding ids (F1 to F19) where it overlaps.

## Threat model

Message Crate is self-hosted. The published deployment guidance is that the server speaks plain HTTP and is for a trusted home network, and that reaching it from the internet needs a reverse proxy with TLS in front (`docs/src/content/docs/docs/user/features/owner/run-on-another-machine.md:35-41`). The desktop app starts its own server at `127.0.0.1:8080` (`CLAUDE.md`, `src-tauri/src/local_server.rs`), and Settings can open it to the network (`src-tauri/src/commands/local_server.rs:83`). So the callers are:

1. **A stranger on the network** who reaches the unauthenticated routes (`POST /v1/session`, `POST /v1/accounts` when registration is open, `POST /v1/server/claim`, `GET /v1/server`, `/health`). These matter most when the server is exposed beyond the device.
2. **An authenticated account or API token** that sends malformed or oversized bodies, by mistake or on purpose.
3. **A hostile backup file**: a phone backup, exported chat or CSV that a person imports. The exporters and the server's import pipeline parse it. This caller is always present, because a message's sender writes the text, names and attachment names a backup holds.
4. **A hostile message sender** whose text and attachment names flow through to the web app, to exports (EML, MBOX, CSV, SBR XML), and to anyone who opens them.

Each finding says which of these it needs.

## Summary

**Risk score: 3 / 10.**

The HTTP interface is unusually disciplined for a project of this size. Every body is typed with serde and rejected as `malformed-body`, `validation-failed` or `unsupported-media-type` by one extractor (`crates/server/server/src/extract.rs:66-78`); every list route caps `limit` at 500 and `offset` at 50,000 (`crates/server/server/src/paging.rs:12-22,176-206`); a query parameter a route does not declare is refused before the handler runs (`crates/server/server/src/declared_query.rs:142-162`); request ids are made by the server and never read from the client (`crates/server/server/src/request_id.rs:9-11,33-40`); three body caps are chosen on purpose (32 KiB on the public routes, 32 MiB on JSON, 512 MiB on everything else: `crates/server/server/src/server.rs:1006-1021`). The search language is a hand-written lexer and parser with a 2,048-byte, 32-term, 64-node and depth-32 limit (`crates/server/server/src/search/lex.rs:9`, `parse.rs:43-47`), compiles to bound parameters with LIKE metacharacters escaped (`emit.rs:199-226`) and FTS5 terms quoted (`fts.rs:11-13`), and uses no regular expression at all (no `regex` dependency in `crates/server/server`). The web app has no `dangerouslySetInnerHTML`, `innerHTML`, `window.open`, `javascript:` or `href` built from user data anywhere in `web/src` (grep over `*.ts`, `*.tsx`, non-test), so message text, names and file names are rendered through React's own escaping.

The backup parsers are bounds-checked (`crates/libs/go-sms-mms/src/wsp.rs`), stream XML without entity expansion (`crates/libs/sbr/src/read.rs:824-826`, quick-xml 0.39.4), and the export writers escape XML attributes (`crates/libs/sbr/src/lib.rs:213-230`), encode mail header values (`crates/libs/mail/src/lib.rs:562-580,667-680`) and guard the address book CSV against spreadsheet formulas (`crates/server/server/src/contacts_api/address_book.rs:206-210`).

What remains is one Medium and eight Low findings. The Medium is a memory amplification in the import batch route: a 512 MiB JSON Lines body is written to a temporary file and then loaded whole, twice, into memory, with two imports allowed at once. The Lows are missing length caps on a handful of free-text fields, an unvalidated client-supplied MIME type stored and replayed for assets, no security response headers on the browser-served app, a fail-open edge in the undeclared-query check, the owner password on the command line, CSV formula injection in the message export CSV (the address book export already guards it), and two documented trust decisions in the desktop app worth recording.

| Severity | Count |
|---|---|
| Critical | 0 |
| High | 0 |
| Medium | 1 (F1) |
| Low | 8 (F2 to F9) |

## Findings

### F1. The import batch loads a 512 MiB body whole into memory, twice, with two runs at once

- **Severity:** Medium
- **CWE:** CWE-770 (Allocation of Resources Without Limits or Throttling), CWE-400
- **Evidence:**
  - `crates/server/server/src/imports_api/mod.rs:1735-1741`: `create_import_batch` streams the body to a temporary file with `stream_body_to_file(..., crate::server::MAX_REQUEST_BODY_BYTES)`, so a batch may be 512 MiB (`server.rs:1021`).
  - `crates/server/server/src/jsonl.rs:52-86`: `read_records` reads every line into `lines: Vec<String>` before parsing ("records still accumulate in memory", line 44), then calls `models::parse_ir_lines(lines)`.
  - `crates/server/server/src/models.rs:232-292`: `parse_ir_lines` parses each line into a `serde_json::Value`, then into `ConversationHeader` or `IrMessage`, and pushes every record onto `out: Vec<ExportRecord>` before returning.
  - `crates/server/server/src/imports_api/mod.rs:1767`: `MAX_CONCURRENT_IMPORTS: usize = 2`.
- **Why it matters:** At the moment `parse_ir_lines` returns, the process holds the raw lines (up to 512 MiB), the `serde_json::Value` of the line being parsed, and the typed records of every line so far. Two concurrent imports double that. A home server with 2 to 4 GiB of memory can be pushed into swap or the OOM killer by one import-scoped credential sending two large bodies, and the whole Message Crate goes down, not only the import.
- **Exploitability:** Needs the `import` scope on a session or API token (`ImportAccess`, `imports_api/mod.rs:1712`), so a stranger cannot reach it. An API token with the `import` scope is exactly the credential a person hands to a script, and a buggy script that packs every file of a large backup into one batch triggers it by accident. Matters on any deployment; worst on a small home server.
- **Remediation:** Stream the parse. `read_records` already reads line by line; make `parse_ir_lines` take `impl Iterator<Item = io::Result<String>>` and hand each record to the staging step as it is parsed, so at most one line and the records of one conversation are in memory. Pair that with a batch cap below the request cap, sized for the largest batch the desktop app sends:

  ```rust
  // imports_api/mod.rs
  /// The cap on one JSON Lines batch. The desktop app sends one conversation
  /// file per batch; the largest real file seen is well under this.
  pub(crate) const MAX_IMPORT_BATCH_BYTES: usize = 64 * 1024 * 1024;
  // ...
  let n = stream_body_to_file(request.into_body(), &jsonl_path, MAX_IMPORT_BATCH_BYTES).await?;
  ```

  Defense in depth: keep `MAX_CONCURRENT_IMPORTS` at 2 but make the semaphore also bound the bytes in flight. The design audit's F7 and F17 cover the same shape on the desktop side (the write queue loaded whole).

### F2. Free-text fields with no length cap

- **Severity:** Low
- **CWE:** CWE-1284 (Improper Validation of Specified Quantity in Input)
- **Evidence:** each of these trims the value and stores it with no upper bound other than the body cap (32 MiB for JSON, `server.rs:1014`):
  - Account `preferred_name`: `crates/server/server/src/accounts_api.rs:612-614` (`name.map(str::trim).filter(|n| !n.is_empty())`, then `set_preferred_name`).
  - Account identity `address` on `PATCH /v1/accounts/{id}`: `accounts_api.rs:627-650` (`entry.address.trim()`, typed by `handles::handle_type_of`, stored).
  - Contact name on `PATCH /v1/contacts/{id}` (`UpdateContactRequest.name`, `contacts_api.rs:70-74`): `crates/server/server/src/db/contacts.rs:89-91` trims and refuses only an empty name.
  - Saved search `query`: `crates/server/server/src/saved_searches_api.rs:19-40` (`CreateSavedSearchRequest`, `UpdateSavedSearchRequest`); `db/saved_searches.rs:98-110` caps the `name` at 80 characters (`MAX_NAME_LEN`, `db/named_membership.rs:21`) and never looks at `query`.
  - Import issue `item` and `reason`: `crates/server/server/src/imports_api/mod.rs:748-760`; `validate_import_issues` (`:767-781`) checks only the `kind`.
  - Import run `form`, `source_fingerprint` and `source_identities` JSON, stored as text: `imports_api/mod.rs:1288-1294` (`optional_json_string`).
  For contrast, the fields that are capped: username 1 to 128 characters of `[alphanumeric _ - .]` (`credentials.rs:236-243`), password at most 1,024 bytes (`credentials.rs:20,219-224`), API token label at most 120 bytes (`db/api_tokens.rs:392-399`), Contact Group, Message Tag and Saved Search names at most 80 characters (`db/named_membership.rs:272-275`), source id at most 64 (`config.rs:247-268`), app build header at most 64 (`server.rs:1574-1587`).
- **Why it matters:** A multi-megabyte contact name or account name is stored, indexed, returned on every list page that shows it, and written into the Audit Trail and log lines. A Saved Search whose `query` is over `MAX_QUERY_BYTES` (2,048, `search/lex.rs:9`) or has more than 32 terms can be saved but never run; the person learns this only when they click it. None of this crosses a trust boundary, but it is the one place the interface accepts a value it will later refuse or struggle to show.
- **Exploitability:** Needs a session. Self-inflicted or an account annoying the owner. Low on every deployment.
- **Remediation:** One helper beside `paging.rs`, used by every route that stores free text:

  ```rust
  // crates/server/server/src/text_limits.rs
  pub(crate) const MAX_NAME_CHARS: usize = 200;      // names people type
  pub(crate) const MAX_IDENTITY_CHARS: usize = 320;  // an email address or number
  pub(crate) const MAX_REASON_CHARS: usize = 2_000;  // one sentence, with room

  pub(crate) fn bounded<'a>(field: &str, value: &'a str, max: usize) -> Result<&'a str, ApiError> {
      let value = value.trim();
      if value.chars().count() > max {
          return Err(ApiError::validation(format!("{field} must be at most {max} characters")));
      }
      Ok(value)
  }
  ```

  For a Saved Search, run `search::compile` (or at least `lex::lex` and `parse::parse`) at save time against the list the search names, and answer `validation-failed` with the parser's own message, so what is saved can be run. ADR 0004 accepts that stored text can later fail as an unknown word; refusing text that fails today is consistent with that.

### F3. An asset's MIME type is taken from the uploader's header unvalidated, stored, truncated on read, and replayed

- **Severity:** Low
- **CWE:** CWE-20 (Improper Input Validation), CWE-116 (Improper Encoding or Escaping of Output)
- **Evidence:**
  - `crates/server/server/src/server.rs:1698-1705`: `upload_content_type` returns whatever is before the first `;` of the `Content-Type` header, trimmed, unless it is empty or `application/octet-stream`. No `type/subtype` token check.
  - `crates/server/server/src/assets_api.rs:1038-1041` (`replace_asset`) and `assets_api.rs:1181-1190` (`create_asset_upload`, `body.mime`): both pass that string to storage.
  - `crates/server/server/src/asset_store.rs:514-528`: `store_mime_metadata` writes the trimmed string to a `.mime` sidecar; `:501-509` `read_mime_metadata` reads back at most 1,024 bytes (`file.take(1024)`), so a longer value is silently cut to a different value.
  - `crates/server/server/src/assets_api.rs:963-965`: on download, `HeaderValue::from_str(&mime)` sets `Content-Type`; when the stored string is not a valid header value the header is left out (the `if let Ok`), and the browser sees no type at all.
  - Mitigations in place: `assets_api.rs:966-973` sets `X-Content-Type-Options: nosniff` and a fixed `Content-Disposition: attachment; filename="asset"`, so a `text/html` original downloads rather than renders when opened by URL, and the file name never echoes client data. Previews and thumbnails are typed by the server's own media pass.
  - The mail crate already has the right check for a MIME part: `crates/libs/mail/src/lib.rs:582-590` `part_content_type` accepts only a `type/subtype` pair of RFC 2045 tokens and falls back to `application/octet-stream`.
- **Why it matters:** The value is attacker-chosen (an import script, or a conversation file's `mime_type` carried through the desktop app's upload) and reaches a response header verbatim. Today the `attachment` disposition and `nosniff` stop the obvious abuse (serving HTML on the app's origin, where the session token sits in `localStorage`: `web/src/lib/auth.tsx:66,127-134`). But the protection is two headers on one route rather than a rule on the value, and the silent truncation at 1,024 bytes means the type served is not always the type stored.
- **Exploitability:** Needs the `import` scope to upload. The payoff today is low: a media element given a wrong type fails to play. Would become High the day a route serves an original inline. Matters on any deployment.
- **Remediation:** Validate at the boundary and refuse or normalise:

  ```rust
  // server.rs
  pub(crate) fn upload_content_type(headers: &HeaderMap) -> Option<String> {
      let base = content_type_base(headers)?;
      if base.is_empty() || base.eq_ignore_ascii_case("application/octet-stream") {
          return None;
      }
      // RFC 2045 token/token, at most 255 bytes: anything else is octet-stream.
      let is_token = |s: &str| !s.is_empty()
          && s.bytes().all(|b| b.is_ascii_graphic() && !b"()<>@,;:\\\"/[]?=".contains(&b));
      match base.split_once('/') {
          Some((t, s)) if base.len() <= 255 && is_token(t) && is_token(s) => Some(base.to_ascii_lowercase()),
          _ => None,
      }
  }
  ```

  Apply the same check to `CreateAssetUploadRequest.mime` and in `store_mime_metadata`, and make `read_mime_metadata` refuse (return `None`) rather than truncate when the sidecar is over its cap. Keep `nosniff` and `attachment` as they are.

### F4. The browser-served web app has no security response headers

- **Severity:** Low
- **CWE:** CWE-693 (Protection Mechanism Failure), CWE-1021 (Improper Restriction of Rendered UI Layers)
- **Evidence:**
  - `crates/server/server/src/server.rs:1302`: the SPA is served by `ServeDir::new(static_dir)` as the router's fallback. No `SetResponseHeaderLayer`, `Content-Security-Policy`, `X-Frame-Options`, `Referrer-Policy` or `X-Content-Type-Options` appears anywhere under `crates/server/server/src` (grep, non-test), and `web/index.html` carries no `http-equiv` meta tag.
  - The desktop shell does carry a CSP: `src-tauri/tauri.conf.json:24-36` (`default-src 'self'`, `script-src 'self'`, `object-src 'none'`, `frame-src 'none'`, `base-uri 'self'`, `form-action 'none'`). The same app served from `static/` in a browser gets none of it.
  - The session token is persisted in `localStorage` (`web/src/lib/auth.tsx:66`, `persistState` at `:127-134`), so any script that ever ran on the origin would read it.
  - Today there is no script sink: no `dangerouslySetInnerHTML`, `innerHTML`, `document.write`, `eval`, `new Function`, `srcdoc`, `window.open` or `javascript:` in `web/src` (grep over `*.ts`, `*.tsx`, non-test). The only dynamic `href` values are constants: `web/src/screens/settings/SystemSection.tsx:132,279,283` and `web/src/screens/import/MissingProgramNotice.tsx:92` (`section.url` from `troubleshootingSentence`, `readerSourceUrl`, `readerLicenseUrl`).
- **Why it matters:** This is defense in depth, not a hole. A CSP on the browser route would make a future HTML sink (a link-detection feature, a rich-text renderer, a third-party widget) fail closed rather than open, and `frame-ancestors 'none'` stops another page on the same network from framing the app. The repository already wrote the policy once, for Tauri.
- **Exploitability:** No exploit today. Only matters when the server is reached from a browser (every Docker deployment) and more when exposed beyond the device.
- **Remediation:** Add one layer on the fallback service, with the Tauri CSP's values:

  ```rust
  use tower_http::set_header::SetResponseHeaderLayer;
  use axum::http::{header, HeaderValue};

  let csp = HeaderValue::from_static(
      "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
       connect-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; \
       font-src 'self' data:; object-src 'none'; base-uri 'self'; form-action 'none'; \
       frame-ancestors 'none'");
  let static_app = tower::ServiceBuilder::new()
      .layer(SetResponseHeaderLayer::if_not_present(header::CONTENT_SECURITY_POLICY, csp))
      .layer(SetResponseHeaderLayer::if_not_present(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
      .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
      .service(ServeDir::new(static_dir));
  api.method_not_allowed_fallback(api_method_not_allowed).fallback_service(static_app)
  ```

  Check `connect-src` against the web app's own fetches (`web/src/lib/api.ts:178,209,238` use `${baseUrl}${path}`, same origin in the browser) before shipping. Verify in the browser with the Playwright MCP, as AGENTS.md asks after a `web/` change.

### F5. The undeclared-query check fails open on a query string it cannot re-parse

- **Severity:** Low
- **CWE:** CWE-754 (Improper Check for Unusual or Exceptional Conditions)
- **Evidence:** `crates/server/server/src/declared_query.rs:112-136`, `undeclared`:
  - `:126`: `let uri: axum::http::Uri = format!("/?{query}").parse().unwrap_or_default();`
  - `:127-129`: `axum::extract::Query::<Vec<(String, String)>>::try_from_uri(&uri).map(|q| q.0).unwrap_or_default()`
  Both `unwrap_or_default()` turn a parse failure into an empty parameter list, and an empty list means "nothing undeclared", so `refuse_undeclared_query` (`:142-162`) lets the request through.
- **Why it matters:** The rule in `docs/architecture/http-api.md` ("A query parameter a route does not declare is `validation-failed`") exists so a typo is never silently obeyed. For a query that `serde_urlencoded` cannot read (a bad percent escape such as `limt=%zz`), the check says nothing and the route's own typed `Query<T>` decides. Every list route has a typed `Query`, which answers `validation-failed` for the same input (`extract.rs:20-39`), so the outcome is the same today. A route that reads the query by hand, or one added later with `Option<Query<T>>`, would obey the typo. The one route that reads a query parameter outside a typed struct is the Media Link (`media_link`, declared by its security scheme, `declared_query.rs:77`).
- **Exploitability:** No wrong answer found at this commit. Low on every deployment.
- **Remediation:** Make a parse failure a refusal, and keep the one-sentence shape the file promises:

  ```rust
  fn undeclared(query: &str, declared: &BTreeSet<String>) -> Vec<String> {
      // ...
      let Ok(uri) = format!("/?{query}").parse::<axum::http::Uri>() else {
          return vec!["the query string could not be read".to_string()];
      };
      let Ok(axum::extract::Query(pairs)) =
          axum::extract::Query::<Vec<(String, String)>>::try_from_uri(&uri) else {
          return vec!["the query string could not be read".to_string()];
      };
      // ...
  }
  ```

  Parameter pollution (`limit=10&limit=20`) is already a `validation-failed`: serde's derived visitor refuses a duplicate field, which `extract::Query` maps at `extract.rs:36`. **Unable to verify** by running; the proving test would sit in `crates/server/server/src/extract.rs` tests beside `a_query_parameter_of_the_wrong_type_is_a_validation_422` (`:154`).

### F6. The owner's password is a command-line argument

- **Severity:** Low
- **CWE:** CWE-214 (Invocation of Process Using Visible Sensitive Information)
- **Evidence:** `crates/server/server/src/cli.rs:86-103`: `CreateOwnerArgs.password: String` and `ResetOwnerPasswordArgs.password: String`, both `#[arg(long)]`. `crates/server/server/src/owner_cli.rs:25,75` take the string and hash it (`hash_owner_password`, `credentials.rs:200-207`, which refuses only an empty password).
- **Why it matters:** The password appears in the shell's history, in `ps`/`/proc/<pid>/cmdline` while the command runs, in `docker inspect` and in any CI or compose log that echoes the command. The WhatsApp exporter already solved this shape for the decryption key: `crates/exporters/whatsapp-exporter/src/wtsexporter.rs:320-326` writes the key to a `0600` file "so the secret never appears in the process command line".
- **Exploitability:** Operator-only; a shared host or a logged container start. Low.
- **Remediation:** Accept the password from a file or standard input instead, and refuse the bare argument:

  ```rust
  /// Password for the owner, read from this file (one line). Use `-` to read it from stdin.
  #[arg(long, value_name = "FILE")]
  pub password_file: PathBuf,
  ```

  Document the change in the CLI reference (`dump-cli-docs`). No compatibility alias, per the repository's rule.

### F7. Spreadsheet formula injection in the message CSV exports

- **Severity:** Low
- **CWE:** CWE-1236 (Improper Neutralization of Formula Elements in a CSV File)
- **Evidence:**
  - `crates/libs/ir-format/src/write.rs:232`: `csv::Writer::from_writer(out)` writes message rows (body, names, file names) with the `csv` crate's default quoting, which handles commas, quotes and line breaks and nothing else.
  - `crates/exporters/go-sms-pro-exporter/src/emit.rs:629,674,698`: three more `csv::Writer::from_path` writers for the same kind of text.
  - The address book export already applies the right rule and says why: `crates/server/server/src/contacts_api/address_book.rs:206-210` ("A cell that starts with `=`, `+`, `-`, `@`, a tab or a carriage return is written with a `'` in front, in every column, because a spreadsheet runs such a cell as a formula").
- **Why it matters:** A message body is written by its sender. `=HYPERLINK(...)` or `=cmd|' /C ...'!A1` as the first characters of a message land in a cell as a formula when the export is opened in Excel or LibreOffice. Both programs warn before running external content, which is why this is Low.
- **Exploitability:** Needs a hostile message sender and a person who exports to CSV and opens it in a spreadsheet, then accepts the warning. Any deployment.
- **Remediation:** Share one `csv_safe_cell` between `ir-format`, the go-sms-pro writer and the address book (the address book's belongs in `crates/libs/csv`, `message_csv`, which both sides already depend on), and apply it to every free-text column. The address book load already strips the leading `'` (`address_book.rs:210`), so a round trip stays clean. Where a leading `'` is unwelcome (a message body that really begins with `+`), the reference page `docs/src/content/docs/docs/developer/reference/csv-columns.md` should say the rule.

### F8. `save_download` fetches any host the window names

- **Severity:** Low
- **CWE:** CWE-918 (Server-Side Request Forgery), here from the desktop app
- **Evidence:** `src-tauri/src/commands/download.rs:40-75`: `save_download(url, file_name)` accepts any `http` or `https` URL whose path ends in `/v1/assets/{sha256}` and carries a `media_link` query (`original_by_media_link`, `:56-75`), then fetches it and writes the bytes to the file the person picks in a Save dialog. The doc comment at `:23-27` records the decision: "Its host is not checked, because the app does not know which server the window logged in to".
- **Why it matters:** A script running in the WebView could make the app issue a GET to any address reachable from the computer (an internal service, a LAN device) and write the answer to a file the person chose. The person chooses the path (`choose_save_path`, `paths.rs:263-275`), the app never writes where the window says, and the CSP's `script-src 'self'` (`tauri.conf.json:26`) means a script in the window must already be the app's own. This is a recorded trust decision, not a defect, and it is listed so the decision is visible in an audit.
- **Exploitability:** Needs script execution in the WebView, which needs a hole elsewhere. Low.
- **Remediation:** The app can learn the server it is logged in to: the window already tells it the account and server on each run (`src-tauri/src/commands/run_logs.rs:31-34`, `StartRunLogArgs.account: RunLogAccount`). Keep the current base URL in a `tauri::State` set at login and compare `url.host_str()` and `port` with it in `original_by_media_link`; refuse the rest with the existing message.

### F9. Desktop IPC: the `shell:allow-open` capability and the path-reading commands are broader than the window uses

- **Severity:** Low (informational)
- **CWE:** CWE-250 (Execution with Unnecessary Privileges)
- **Evidence:**
  - `src-tauri/capabilities/default.json`: the main window holds `shell:default` and `shell:allow-open`. No file under `web/src` imports `@tauri-apps/plugin-shell` (grep); the package is a dependency (`web/package.json:25`) and the plugin is initialised (`src-tauri/src/main.rs:41`). The permission is used by the plugin's own interception of `target="_blank"` anchors (`SystemSection.tsx:132,279,283`, `MissingProgramNotice.tsx:92`), all with constant URLs.
  - `src-tauri/src/commands/paths.rs:115-117` `path_stat(path)` and `:122-129` `ios_backup_encrypted(path)` read metadata or `Manifest.plist` at any path the window names, and `:137-158` `imessage_backup_identities(path, ios, backup_password)` opens any directory as an iOS backup. `open_path` (`:183-199`) is the contrast: it resolves the path only inside the export, log and run directories the app made.
  - The form that drives an extract is validated on the Rust side of the IPC (`src-tauri/src/commands/extract.rs:503` calls `Form::to_config`, `crates/core/message-crate-core/src/exporters.rs:260-286`), not trusted from the window.
- **Why it matters:** The WebView runs first-party code under a CSP that allows no foreign script, so these commands see only the app's own calls. They are listed because an IPC command is a boundary, and the two path-reading commands would let a compromised window probe the file system and read `Info.plist` from any directory. `path_stat` is what the Import screen uses to check a typed path, so it is needed; the question is only whether it should answer for paths outside the person's home directory.
- **Exploitability:** Needs script execution in the WebView. Low.
- **Remediation:** Keep as is, but write the decision down beside `open_path`'s (the module's doc already explains that one). If the `shell:allow-open` scope is ever widened, pin it to the three constant hosts the links use.

## Validation matrix: HTTP routes

Legend: **Pass** means every input the route reads is typed, bounded and refused with the documented problem type. **Partial** names what is missing. **N/A** means the route reads nothing from the client beyond the credential and path id. Every path id is `Path<i64>` or `Path<Sha256>` through `extract.rs:42-57` (a non-number answers `validation-failed`); `Sha256::parse` refuses anything but 64 hex digits (`assets_api.rs:115`). Every query string passes `refuse_undeclared_query` first (F5 notes the edge). Every JSON body goes through `extract::Json` (`extract.rs:85-96`) and the 32 MiB cap (`server.rs:1014`).

| Route | Body / query / header inputs | Status | Evidence |
|---|---|---|---|
| `GET /health` | none | N/A | `server.rs:1526` |
| `POST /v1/session` | `username`, `password` | Pass | trimmed, non-empty, password at most 1,024 bytes, rate limit 20/min per account bucket (`session_api.rs:153-171`, `credentials.rs:40-56`); 32 KiB body (`server.rs:964-971,1006`) |
| `GET /v1/session`, `DELETE /v1/session` | `Authorization`, app headers | Pass | app headers only recorded when both present, kind parsed, build at most 64 printable ASCII (`server.rs:1574-1587`) |
| `POST /v1/server/claim` | `username`, `password` | Pass | `require_valid_username`, `hash_owner_password` (`server_api.rs:163-165`); 32 KiB body |
| `GET /v1/server` | none | N/A | |
| `GET /v1/server/settings`, `PATCH /v1/server/settings` | `asset_max_bytes`, `public_registration` | Pass | 1 to `i64::MAX` (`server_api.rs:254-259,303-304`) |
| `GET /v1/server/storage` | none | N/A | |
| `GET`, `PUT /v1/server/demo-account` | `size` | Pass | `DemoDataSize` enum (`server_api.rs:471-472`) |
| `GET /v1/server/log-files` | none | N/A | |
| `GET /v1/server/log-files/{id}` | `id` | Pass | `i64` to `u64 > 0` to a fixed file name (`logging/files.rs:154-167`); path depth is the file-handling audit's |
| `GET /v1/server/log-lines` | `after`, `level`, `text`, `limit` | Pass | `after >= 0`, `limit` 1 to 500, `level` enum, `text` a contains match (`server_api/log_lines.rs:22-24,73-77`). Note: a `text` that matches nothing walks the whole log per request; owner-only |
| `GET /v1/accounts`, `POST /v1/accounts` | registration body | Pass | username rule, password hashing, demo username refused (`credentials.rs:236-290`); 32 KiB body on `POST` |
| `GET`, `PATCH`, `DELETE /v1/accounts/{id}` | `preferred_name`, `time_zone`, `identities[]`, `remove_identities[]`, `confirm` | Partial | time zone parsed with `chrono_tz` or `validation-failed` (`accounts_api.rs:606-610`); identity service checked (`handles.rs:105-118`); `preferred_name` and `address` uncapped (F2); confirm flag required (`:933-944`) |
| `PUT /v1/accounts/{id}/password` | current, new, confirm | Pass | at most 1,024 bytes, must match (`accounts_api.rs:1069-1085`) |
| `GET /v1/accounts/{id}/identities`, `/storage`, `/audit-trail`, `/imports`, `/imports/{import_id}`, `/exports` | paging, `status` filter | Pass | `sorted_page` or `page_params` (`paging.rs:176-206`); status enum (`imports_api/mod.rs:1186-1195`) |
| `DELETE /v1/accounts/{id}/messages` | `confirm` | Pass | `accounts_api.rs:1181` |
| `GET`, `POST /v1/accounts/{id}/api-tokens` | `label`, `permissions`, `expires_in_days` | Pass | label 1 to 120 bytes (`db/api_tokens.rs:392-399`); expiry uses `saturating_add`/`saturating_mul` (`:376-390`); permissions capped by the account's |
| `GET`, `PATCH`, `DELETE /v1/accounts/{id}/api-tokens/{token_id}` | `label` | Pass | same label rule (`accounts_api/api_tokens.rs:371-374`) |
| `GET /v1/audit-trail`, `/deleted-accounts` | paging, `deleted_account_id` | Pass | `i64`, `page_params` (`audit_trail_api.rs:50`) |
| `GET /v1/contacts`, `/conversations`, `/messages` | `q`, `limit`, `offset`, `sort` | Pass | search compiled by one module with its limits (`search/lex.rs:80-85`, `parse.rs:455-466`), sort keys from a declared set (`paging.rs:117-160`), `relevance` only with a positive term |
| `GET /v1/messages/{id}` | id | N/A | |
| `GET /v1/conversations/{id}`, `/sources` | id | N/A | |
| `GET /v1/conversations/{id}/messages` | `q`, `limit`, one of `offset`/`around`/`before`/`after` | Pass | at most one position parameter (`conversations_api.rs:158-160`); no offset cap by design |
| `POST .../trash`, `POST .../restore`, `DELETE /v1/conversations/{id}`, `DELETE /v1/contacts/{id}`, `DELETE /v1/trash` | id | N/A | guards decide who; ids chunked in SQL (`db/trash.rs:423,441,556`) |
| `GET`, `PATCH /v1/contacts/{id}` | `name`, `add_identity`, `update_identity`, `remove_identity`, `set_identity_country` | Partial | country from `phone::country` (`imports_api/mod.rs:1279-1288` pattern, `identity_country.rs`), service enum; `name` uncapped (F2) |
| `POST /v1/contacts` (address book CSV) | `text/csv` body, `mode` query | Pass | `Content-Type` must be `text/csv`, 8 MiB cap, UTF-8, non-empty (`address_book.rs:70,149-160`); `csv` reader `flexible` (`db/address_book.rs:384-385`); demo account refused |
| `POST /v1/contacts/address-book` | `q`, `ids[]` | Pass | `q` compiled; `ids` applied as a Rust-side `HashSet` filter (`db/address_book.rs:1189`), not an `IN` list; output cells formula-guarded, `Content-Disposition` fixed (`address_book.rs:206-210,269-273`) |
| `POST /v1/contacts/summaries` | `ids[]` | Pass | 1 to 500 (`contacts_api.rs:285-290`) |
| `POST /v1/contacts/unmatched-identities` | `identifiers[]` | Pass | at most 500 (`contacts_api.rs:189,214-216`) |
| `GET`, `POST /v1/contact-groups`, `/message-tags` and members | `name`, `add[]`, `remove[]` | Pass | name 1 to 80 characters (`db/named_membership.rs:272-275`); ids chunked (`:725`) |
| `GET`, `POST /v1/saved-searches`, `GET`, `PATCH`, `DELETE /{id}` | `name`, `query`, `list` | Partial | name 1 to 80 (`db/saved_searches.rs:98-110`); `query` not parsed or capped at save (F2) |
| `GET /v1/search-fields/{list}` | none | N/A | |
| `GET /v1/phone-countries` | none | N/A | |
| `GET`, `POST /v1/exports` | `ExportScope`: `q` or `conversation_ids[]`/`message_ids[]`, `format`, paging and `status` on `GET` | Pass | `q` non-blank and compiled; each id list at most 500 and owned by the account (`exports_api.rs:31,149-191,207-216`) |
| `GET /v1/exports/{id}`, `/cancel`, `/complete`, `/messages` | paging | Pass | `page_params` with `MAX_LIST_OFFSET` (`exports_api.rs:395`) |
| `GET`, `POST /v1/imports` | `source`, `phone_country`, `stage`, `form`, `source_fingerprint`, `source_identities`, `mode` | Partial | `source` 1 to 64 of `[a-z0-9-_]` (`config.rs:247-268`); country looked up; stage enum; credentials stripped from `form` (`imports_api/mod.rs:809-830`); JSON blobs uncapped (F2) |
| `GET`, `PATCH /v1/imports/{id}`, `/discard`, `/contacts` | `stage`, paging | Pass | enum; `page_params` |
| `POST /v1/imports/{id}/batches` | JSON Lines body | Partial | `Content-Type` must be `application/jsonl` or `application/x-ndjson` (`imports_api/mod.rs:1714-1719,1752-1757`); run must be running and the account's (`:1724-1729`); empty body refused; schema version checked before the header parses (`models.rs:256-263`, `crates/libs/ir/src/schema_version.rs`); a message before a header, a message without a guid, and invalid UTF-8 are named by line (`jsonl.rs:70-77`, `models.rs:277-283`); attachment paths through `safe_attachment_path` (`imports_api/staging.rs:70-73`, the file-handling audit's). **512 MiB loaded whole (F1)** |
| `POST /v1/imports/{id}/complete` | `status`, `issues[]`, counts | Partial | status one of three (`imports_api/mod.rs:792-799`); issue kind `error` or `skip` (`:767-781`); `item`/`reason` uncapped (F2) |
| `GET`, `HEAD`, `PUT /v1/assets/{sha256}` | `Range`, `If-Range`, `Content-Type`, body | Partial | `Range` parsed to one `bytes=` range, overflow to `u64::MAX`, unsatisfiable answers 416 (`assets_api/ranges.rs:58-102`); body held to the owner's attachment limit before the handler (`server.rs:1033-1066`); bytes hashed and compared with the path (`store_verified`); **MIME unvalidated (F3)** |
| `POST /v1/assets/{sha256}/media-links` | none | N/A | HMAC link, one hour (`assets_api/media_links.rs`) |
| `GET /v1/assets/{sha256}/preview`, `/thumbnail` | `Range` | Pass | same range parser; MIME from the server's own media pass |
| `POST /v1/assets/{sha256}/uploads` | `bytes`, `mime` | Partial | `bytes` at most the attachment limit (`asset_uploads.rs:208-214`); `part_size` chosen by the server, never by the client (`:222-229`); `mime` as F3 |
| `GET`, `DELETE .../uploads/{upload_id}`, `/complete`, `PUT .../parts/{part}` | `part`, body | Pass | `part` 1 to the expected count, body at most the session's `part_size`, length must match (`asset_uploads.rs:340-366`) |
| any `/v1/*` | `Accept` | Pass | `406` only when present and no member matches JSON, `application/*` or `*/*` (`server.rs:1081-1110,1178-1186`) |
| any `/v1/*` | `x-request-id` | Pass | ignored; server makes a UUID v4 (`request_id.rs:33-40`) |
| any `/v1/*` | `Origin` | Pass | explicit allow list plus packaged desktop origins; `*` in config turns on `CorsLayer::permissive()` (`server.rs:933-960`), which is an operator's choice and carries no cookies |

## Validation matrix: desktop IPC commands

The WebView is first-party code under `script-src 'self'` (`src-tauri/tauri.conf.json:26`). "Checked" means the Rust side validates the argument; "trusted" means it does not and the module says why.

| Command | Arguments from the window | Status | Evidence |
|---|---|---|---|
| `extract`, `export`, `upload`, `format` | `Form` or `*Args` | Checked | `Form::to_config` on the Rust side (`commands/extract.rs:503`; `exporters.rs:260-286`); see "What `Form::to_config` checks" below |
| `path_stat`, `ios_backup_encrypted`, `imessage_backup_identities` | any path, optional password | Trusted | `commands/paths.rs:115-158`; password trimmed and handed to the GPL helper over JSON lines (F9) |
| `open_path` | path | Checked | only inside the export, log or run directories the app made; `missing_path_error`; `open::that_detached` (`paths.rs:183-199`) |
| `save_file` | raw body, `file-name` header | Checked | path from the Save dialog, never the window (`paths.rs:228-275`) |
| `save_download` | `url`, `file_name` | Partial | shape of a Media Link enforced; host trusted by decision (F8) |
| `create_run_dir`, `summarize_staging`, `finish_export_dir`, `discard_export_dir` | `label`, `run_dir`, `dir` | Checked | `directory_slug(label)` (`run_directories.rs:228-229`); `directory()` canonicalises and requires the path to be one the app made with its sentinel (`:280-300`) |
| `import_run_log`, `start_import_run_log` | `run_dir` | Checked | only `file_name()` is used, under the Logs Directory (`app_directories.rs:71-78,93-100`) |
| `set_staging_root`, `set_open_to_network`, `start_local_server`, `tools_status`, `retry_tool_downloads`, `home_dir`, `cancel` | path or flag | Trusted / N/A | the person's own choice of directory; tool downloads verify a pinned SHA-256 (`tool_downloads.rs:342,496-514`) |

### What `Form::to_config` checks, and what it leaves to the exporter

`crates/core/message-crate-core/src/exporters.rs:260-286` collects every problem into a `Vec<String>`. It checks: an input path is given, is one line, and exists (`require_single_existing_path`, `:647-666`); the output is non-blank (`required_text`, `:669-673`); the phone country is one `phone::country` knows (`:294-305`); the obfuscation seed is valid hex (`validate_obfuscate_seed`, `:676-688`); ffmpeg is present when a media mode needs it (`:594-597`); the SMS Backup and Restore input is a file, not a directory (`:514-516`); at least one owner phone or email is given where the source needs one (`:549-550,583-584`); an iOS WhatsApp import names a backup (`:379-381`). It does not check: that an owner phone number is a number (`values()` splits and trims, `:583`, and the exporter's `OwnerHandleSet` decides later); that the output directory is writable or distinct from the input; that the WhatsApp key is 64 hex digits (the exporter's `looks_like_path` decides between a path and key material, `wtsexporter.rs:315-326`); or that a backup password is non-empty for an encrypted backup. Each of those fails later with the exporter's own error, which the window shows, so nothing is silently accepted; they are listed so the division is visible.

## Validation matrix: parsers of untrusted backups

| Parser | Input | Findings | Evidence |
|---|---|---|---|
| SMS Backup and Restore XML (`crates/libs/sbr/src/read.rs`) | `.xml`, possibly gigabytes, attributes carry base64 | Streaming `quick_xml::Reader` with `trim_text`, no DTD or external entity support in quick-xml 0.39.4, so XXE and entity expansion do not apply; numeric references decoded by hand with lone surrogates, `&#0;` and values past U+10FFFF dropped and counted (`:200-250`); named entities by `html_escape`; each part's base64 decoded once per MMS (`:380-398,620`); dates bounded to `MAX_DATE_MS` (`:558-577`). Memory is bounded by the largest single `<mms>` element, not the file (file-handling audit) | `read.rs:181-250,380-398,558-577,824-826` |
| Go SMS Pro PDU and WSP (`crates/libs/go-sms-mms`) | binary MMS PDU files | `Cursor::take` checks `remaining() < n` (`wsp.rs:88-95`); `uintvar` at most 5 bytes (`:100-115`); `value_length` 0 to 30 or 31 plus uintvar (`:118-128`); `text_string` requires a NUL (`:168-181`); `encoded_string` checks `end > cur.data.len()` and that the text ends where the length said (`:197-215`); `multipart` refuses more than 256 parts and `take`s each length (`:499-517`); `mms::decode` requires the message-type header first (`mms.rs:183-215`). Emoji regex `\+g([0-9a-fA-F]+)` is linear (`emoji.rs:7`). Pass | as cited |
| iMazing and OpenExtract CSV (`crates/libs/csv`) | `.csv` | `open_csv_lowercase` reads the whole file into memory and parses with `flexible(true)` (`csv/src/lib.rs:83-98`); typed column lookups by header name. Memory equals file size; desktop-local. Pass | `csv/src/lib.rs:83-98`, `imazing-exporter/src/parse.rs:138`, `openextract-exporter/src/parse.rs:79` |
| Conversation CSV read-back (`crates/libs/ir-format/src/read_csv.rs`) | Message Crate's own CSV | `flexible(true)`, rows collected (`:27-53`) | `read_csv.rs:27-53` |
| WhatsApp (`crates/exporters/whatsapp-exporter`) | wtsexporter JSON, SQLite | JSON typed with serde (`parse.rs:199`); SQLite through rusqlite; the key never reaches `argv` (`wtsexporter.rs:315-326`); arguments passed as an array, no shell (`:295-333`). Pass | as cited |
| iOS backup (`crates/libs/ios-backup`) | `Manifest.db`, `Info.plist`, `Manifest.plist` | rusqlite over `Manifest.db`; `fileID` sliced with `get(..2).unwrap_or_default()` (`backup.rs:85-90`); `plist` 1.9.0 `Value::from_reader` for `Info.plist` (`identity.rs:27-31`). Where `relativePath` becomes a path is the file-handling audit's | as cited |
| Apple Messages (`crates/helpers/imessage-reader`) | `chat.db`, typedstream blobs | parsed by `imessage-database` 4.2.0 inside a separate GPL process; the app reads its JSON lines through `imessage-reader-protocol` with a `PROTOCOL_VERSION` check (`crates/libs/ios-backup/src/helper.rs`). A panic in the helper ends the helper, not the app. **Unable to verify** the vendor crate's own bounds; see below | `fields.rs:108-134` slices `text[start..end]` from ranges the vendor crate computed |
| Media probe (`crates/libs/media/src/probe.rs`) | ffprobe JSON | arguments as an array, the path last and absolute (`:35-46`); JSON typed into `MediaProbe` (`:14`). ffmpeg protocol prefixes (`concat:`, `http:`) cannot be reached because the staging directory makes every path absolute. No `-protocol_whitelist file` is set; a cheap hardening | `probe.rs:14,35-46`, `tools.rs:237-238,424-426,530-538` |
| Mail read-back (`crates/libs/mail/src/parse.rs`) | EML/MBOX exports | mboxrd `>From ` unescaping (`:277-279`); `mail-parser` for MIME. Only Message Crate's own exports are read | `parse.rs:248-279` |
| Config file (`crates/server/server/src/config.rs`) | `config.toml` | `toml::from_str` into typed structs; unknown keys collected and refused with their names (`:32,43,47,376-377`); `bind` handed to `TcpListener::bind` (`server.rs:1410`). Operator-trusted | as cited |
| CLI (`crates/server/server/src/cli.rs`) | arguments | clap-typed; `--account` as username or id (`:177-179`, `db/account_profile.rs:272`); owner password on the command line (F6) | as cited |

## Output encoding

| Sink | Data | Status | Evidence |
|---|---|---|---|
| Web app (React 19) | message text, names, attachment names, search terms | Pass | no raw-HTML sink in `web/src`; anchors only to constants (F4 for the missing CSP) |
| RFC 7807 `detail` and `errors` | echoes of input (`sort: unknown key '{name}'`, `unknown query parameter '{name}'`, `username already taken: {username}`) | Pass | always `application/problem+json`, never HTML; no redirect reads client data (`Created<T>` builds `Location` from the new id) |
| Asset download headers | stored MIME | Partial | fixed `Content-Disposition`, `nosniff` (`assets_api.rs:966-973`); MIME unvalidated (F3) |
| Address book CSV | names, groups, identities | Pass | formula guard in every column (`address_book.rs:206-210`), `attachment; filename="address-book.csv"` fixed (`:269-273`) |
| Message CSV exports | bodies, names, file names | Fail (Low) | no formula guard (F7) |
| EML / MBOX (`crates/libs/mail/src/lib.rs`) | identities, names, subjects, attachment types, guids | Pass | identities become `dot_atom`-encoded synthetic addresses, so no line break survives (`:440-470,562-580`); `X-ME-*` values written verbatim only when printable ASCII without `=?`, else as RFC 2047 `Q` words (`:667-700`); attachment `Content-Type` must be `type/subtype` tokens (`:582-590`); guid file names strip control and reserved characters (`:385-410`); body `From ` lines mboxrd-escaped (`:301-327`). Display names and `Subject` pass through `mail-builder` 1.0.0, whose encoder marks `\r`, `\n`, `=` and `?` as bytes needing encoding (`mail-builder-1.0.0/src/encoders/encode.rs:31,66-88`) and whose quoted-name table escapes `\`, `"`, `\r`, `\n` (`headers/rfc2047.rs:60,70-83`). **Unable to verify** by a test; see below |
| SBR XML (`crates/libs/sbr/src/lib.rs`) | every attribute | Pass | `escape_attr` escapes `& < > " '`, writes `\n \r \t` as references and drops other C0 controls with a count (`:213-230`) |
| Server log | request paths, query | Pass | `media_link=[hidden]` (`http-api.md`, "Credentials and reach"); log lines are plain text read back by the server's own parser |

## Checklist

| Check | Result | Notes |
|---|---|---|
| 1. SQL injection: raw queries, dynamic building, stored procedures | **Pass** (as far as this audit reads) | every user value seen is bound (`search/bridge.rs` `Sql::bind_text`, `fts.rs:22,36`; LIKE escaped `emit.rs:218-226`; id lists chunked `db/trash.rs:423`). The database-security audit owns the proof. SQLite has no stored procedures |
| 2. NoSQL injection | **N/A** | SQLite only (`docs/adr/0017`) |
| 3. Command injection: child processes, system commands | **Pass** | every spawn passes an argument array, none uses a shell: `media/tools.rs:237-238,424-426,530-538`, `media/probe.rs:35-46`, `ios-backup/helper.rs:138`, `whatsapp-exporter/wtsexporter.rs:295-333`, `src-tauri/src/local_server.rs:883-885`; `ir/projection.rs:886` is `#[cfg(test)]` (`:510`) |
| 4. XSS: input sanitisation, output encoding, Content-Type | **Pass** with F3 and F4 | React escaping, no HTML sinks; `nosniff` and `attachment` on assets; no CSP on the browser route (F4); MIME unvalidated (F3) |
| 5. XXE: XML parser configuration, file upload handling | **Pass** / **N/A** | quick-xml 0.39.4 resolves no DTD or external entity; the only XML read is a local backup file; the only XML written is attribute-escaped |
| 6. Path traversal: file system operations, directory listing | **Deferred** | `safe_attachment_path` (`imports_api/staging.rs:70-73`), `Sha256`-named asset files (`assets_api.rs:115`), fixed log file names (`logging/files.rs:154-167`), `RunDirectories::directory` canonicalisation (`run_directories.rs:280-300`) and `file_name()`-only log names (`app_directories.rs:71-78`) were read and look right; the file-handling-business-logic audit owns the depth. `ServeDir` serves a built directory with no listing |
| 7a. Request validation: body size limits | **Pass** with F1 | 32 KiB public, 32 MiB JSON, 512 MiB other, the attachment limit on uploads, part size per session (`server.rs:1006-1066`, `asset_uploads.rs:353-357`); chunked bodies cut by `Limited` (`server.rs:1064`); F1 is the one route where the cap is honoured but the memory is not bounded by it |
| 7b. Parameter pollution | **Pass** (**Unable to verify** by running) | serde's derived visitor refuses a duplicate field, mapped to `validation-failed` at `extract.rs:36`; undeclared names refused (`declared_query.rs:112-162`, F5 edge) |
| 7c. Type checking | **Pass** | every body and query is a serde type; every path id an integer or `Sha256`; enums for `status`, `stage`, `kind`, `size`, `service`, `format` |
| 7d. Required field validation | **Pass** | a missing field is `JsonDataError` to `validation-failed` (`extract.rs:66-70`, test `:174-192`); blank-as-missing handled where it matters (`imports_api/mod.rs:1274-1277`, `session_api.rs:153-156`) |

## Prioritised fixes

1. **F1**: stream `parse_ir_lines` and cap the batch body below 512 MiB. Removes the one way an authenticated caller can take the server down with a single request.
2. **F3**: validate the asset MIME type as `type/subtype` tokens at upload and refuse an over-long sidecar instead of truncating it. Small change, closes the only header that echoes client data.
3. **F4**: put the Tauri CSP on the browser route with `frame-ancestors 'none'` and `nosniff`. One layer; turns any future HTML sink into a hard failure.
4. **F2**: one `bounded(field, value, max)` helper on the six uncapped text fields, and parse a Saved Search when it is saved.
5. **F5** and **F6** together in one small pull request: refuse an unreadable query string, and read the owner password from a file or stdin.

## Unable to verify

| Claim | Proving file(s) |
|---|---|
| A duplicated query parameter (`limit=10&limit=20`) answers `validation-failed` rather than taking the last value | a test beside `crates/server/server/src/extract.rs:154` sending `/v1/conversations?limit=10&limit=20` |
| `mail-builder` 1.0.0 encodes a display name or `Subject` holding `\r\n` so the header cannot be split | a test in `crates/libs/mail/src/tests.rs` writing a `MailMessage` whose peer display name is `"a\r\nBcc: x@y"` and asserting the header count; the vendor logic is `~/.cargo/registry/src/index.crates.io-*/mail-builder-1.0.0/src/headers/address.rs:132-148` and `encoders/encode.rs:31,66-88` |
| `imessage-database` 4.2.0 bounds every slice it takes from `attributedBody` typedstream blobs, so a malformed `chat.db` ends the helper cleanly rather than reading out of bounds | the vendor crate's `src/util/typedstream/parser.rs`; the app's side is `crates/helpers/imessage-reader/src/fields.rs:108-134` |
| `plist` 1.9.0 bounds allocations when reading a binary plist with a forged object count | `~/.cargo/registry/src/index.crates.io-*/plist-1.9.0/src/stream/binary_reader.rs` |
| The `shell:allow-open` scope in the Tauri shell plugin version in use allows only `http(s)`, `mailto:` and `tel:` URLs | `src-tauri/Cargo.lock` entry for `tauri-plugin-shell` and its `permissions/default.toml` |
| `axum::extract::Query<Vec<(String, String)>>::try_from_uri` fails for any query string that `Query<PageQuery>` would also fail for, so F5 never changes an answer today | a test in `crates/server/server/src/declared_query.rs` tests (`:166`) with `limt=%zz` |

## Cross-references to the design audit

- F1 here and **F7**, **F17** there describe the same shape at two ends of the pipeline: the server loads a batch whole; the desktop loads the write queue whole.
- F2's suggestion of one text-bounds helper is the kind of shared rule the design audit asks for in **F1** (the `server.rs` God module) and **F2** (`db/` returning `ApiError`): the helper belongs beside `paging.rs`, not in `server.rs`.
- F4 touches the router assembly the design audit lists under **F1** (`server.rs:1251-1352`); adding a layer there is one more reason to split the file.
- The desktop IPC matrix confirms what **F3** there says about `message-crate-core`: `Form::to_config` is the one validation point for every exporter, which is good for this audit and the coupling the design audit objects to.
