# Code Duplication Detection Audit

- **Date:** 2026-10-10
- **Commit:** `530dc1791`
- **Scope:** the whole codebase: `crates/` (35 crates), `src-tauri/src`, `web/src`. Excluded: `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`, and the generated `web/src/lib/serverApi.types.ts`.
- **Relation to the design audit:** `audits/2026-10-09-initial-software-design-analysis.md` already records F6 (three `ProgressEvent` types), F8 (SMS exporter copy-paste), F9 (`sbr::group_chat_key`), F10 (test fixture God module), F13 (demo seed constants), and F15 (web copy-paste). This report cites those by ID and does not restate them; section 4 lists only what extends them. Everything else here is new.

## 1. Summary

The code base is not a copy-paste code base. Measured as normalised lines that sit inside an exact clone of at least 8 normalised lines appearing twice or more, production code is 1.6% (Rust), 1.7% (desktop host), and 2.5% (web); test code is 9.1% (Rust), 8.5% (desktop host), and 6.0% (web). The production figures are upper bounds, because an inline `#[cfg(test)] mod tests` in a production file counts as production here.

What the exact-clone scan and reading found, in 48 clone groups (31 production, D1 to D31; 17 test, T1 to T19 with T3 and T11 folded into T2 and D3):

| Priority | Count | Groups |
| --- | --- | --- |
| High (7 to 8 / 10) | 4 | D1 reserved-name lists copied from Rust into TypeScript; D2 `ExporterConfig` literal built six times; D3 Android SMS date parser and its tests in two crates; D8 `formatBytes` twice in `web/` |
| Medium (5 to 6 / 10) | 13 | D4, D6, D7, D9, D10, D11, D18, D19, T1, T2, T7, T13, T15 |
| Low (1 to 4 / 10) | 31 | the rest of D and T |

The pattern behind most of them is the same: a shared version exists and is used in most places, and a few call sites were written before it existed or without finding it (`message_ir::file_sha256`, `message_ir::testutil::sample_document`, `test_support::seed_conversation`, `paging::page_of`, `web/src/test/providers.tsx`, `web/src/lib/attachmentProgressCopy.ts::formatBytes`). The fixes are mostly deletions.

Drift was found in six groups: D3 (one test table has an extra case), D18 (`is_executable` checks `is_file` in one copy), D19 (`file_sha256` handles `Interrupted` in one copy), D11 (the three reset slices differ in which fields they clear), T9 (`with_directory_mode` probes a write in one copy and a listing in two), and T13 (the form fixture carries `backupPassword`/`phoneCountry` in some copies and not others).

## 2. Method

1. **Exact clones.** Every `.rs`, `.ts`, `.tsx` file in scope was normalised (whitespace collapsed; blank lines, `//` comments, `#[...]` attribute lines, and bare brace or paren lines dropped), then every window of 8 normalised lines was hashed and windows with the same hash at two or more positions were merged into maximal runs. 875 pairs were found; every production pair and the largest test pairs were read.
2. **Near clones.** Identical function names across crates were listed (`grep` for `fn <name>` across `crates/` and `src-tauri/src`) and each group of three or more read. Server handlers and `db/` modules, `src-tauri/src/commands/*`, `crates/libs/import` against `crates/libs/export`, `web/src/screens/settings/*`, `web/src/components/*`, and `web/src/**/*.test.tsx` were read for repeated shapes the hash cannot see.
3. **Data duplication.** Constants and lists that appear on both sides of a process or language boundary were searched for by value.
4. **jscpd was not run**: `web/node_modules/.bin/jscpd` is not installed and installing was out of bounds.

Dismissed as false positives after reading: hook-return destructuring (`web/src/screens/settings/ApiTokensSection.tsx:16-40` against `useApiTokens.ts:168-192`; `NewAccountPanel.tsx:25-35` against `useCreateAccountForm.ts:67-77`), `#[utoipa::path]` attribute blocks, import blocks, `useMemo` dependency lists, the five thin `emit_log` wrappers, the `ProjectionHooks` trait implementations (`attachment_to_ir`, `attachment_digests`), and the `import`/`export` `journal.rs` pair, which already share `crates/libs/journal` (`crates/libs/import/Cargo.toml:11`, `crates/libs/export/Cargo.toml:12`; `import/src/journal.rs:158-160` and `export/src/journal.rs:109-111` both delegate). The `http.rs` and `project.rs` pairs in those two crates have different shapes and are not clones.

Scale used: **importance /10** weighs how likely the copies are to drift into a user-visible bug times how often the code changes. **Effort:** S under an hour, M half a day, L one to two days.

## 3. Findings

### 3.1 Data duplicated across a boundary

#### D1. Reserved Contact Group and Message Tag names live in Rust and again in TypeScript (8/10, effort M)

**Evidence.** `crates/server/server/src/db/named_membership.rs:103-122` lists the 17 reserved Message Tag names and `:145-171` the 26 reserved Contact Group names inside `MembershipSpec`. `web/src/lib/messageTags.ts:18-39` and `web/src/lib/contactGroups.ts:18-46` repeat both lists verbatim, down to the same comment lines ("The two computed groups, as `group:` names them"; "`tag:none` means 'no tag'"). The scan matched 26 and 17 consecutive normalised lines across the two languages. Today they agree. Nothing checks that they do: a name added on one side is accepted by the client and refused by the server, or the reverse.

**Remediation.** One source, the Rust spec, and a gate. The repo already has the precedent of a fixture written by one side's test and read by the other's (`tests/fixtures/search/web-queries.txt`, per `CLAUDE.md`). Add a server test that writes the lists and fails when the committed file differs, and have the web import the file:

```rust
// crates/server/server/src/db/named_membership/tests.rs
#[test]
fn the_reserved_names_the_web_reads_are_these() {
    let json = serde_json::json!({
        "message_tags": tag_spec().reserved,
        "contact_groups": group_spec().reserved,
    });
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../web/src/lib/reservedNames.json");
    let committed: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(committed, json, "run this test with UPDATE_FIXTURES=1 to rewrite {path}");
}
```

```ts
// web/src/lib/messageTags.ts (same change in contactGroups.ts)
import reserved from "./reservedNames.json";
export const RESERVED_TAG_NAMES = new Set(reserved.message_tags.map((s) => s.toLowerCase()));
```

Then delete the two literal arrays. **Unable to verify** whether `reservedGroupError` (`contactGroups.ts:48`) also mirrors the sentences in `special_reserved` (`named_membership.rs:172-`); its body was not read.

#### D23. `IrImessage` and the protocol's `Imessage` carry the same 13 fields (3/10, effort S)

**Evidence.** `crates/libs/ir/src/lib.rs:668-694` and `crates/helpers/imessage-reader-protocol/src/lib.rs:417-443`: the same 13 `Option` fields in the same order. The protocol crate is MIT OR Apache-2.0 and cannot depend on the Fair Core `message-ir`, so the duplication is forced by ADR 0014, not an accident.

**Remediation.** Keep both; add the one test that fails when they drift, beside the conversion that must already exist:

```rust
// crates/libs/ir/src/lib.rs, in a test module
#[test]
fn the_protocol_imessage_round_trips_through_ir_imessage() {
    let json = serde_json::to_value(imessage_reader_protocol::Imessage::default()).unwrap();
    let ir: IrImessage = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(ir).unwrap(), json, "a field exists on one side only");
}
```

**Unable to verify** whether an `impl From<Imessage> for IrImessage` exists; the proving search is `grep -rn "for IrImessage" crates/`.

#### D24. `LoadCounts` and `CreateContactsResponse` repeat eight fields (2/10)

`crates/server/server/src/db/address_book.rs:58-74` and `crates/server/server/src/contacts_api/address_book.rs:23-45`, joined by a `From` impl at `:47`. This is the db-type-to-API-type layering `docs/architecture/http-api.md` asks for, so it stays; it is listed so that the next reader does not file it as a bug.

#### D27. Two identical `FfmpegFiles` pins (2/10)

`src-tauri/src/tool_downloads/pins.rs:118-131` (windows x86_64) and `:132-145` (windows aarch64) carry the same four file names and hashes, because the aarch64 build downloads the x64 binary. Say so in one place:

```rust
const WINDOWS_X64: FfmpegFiles = FfmpegFiles { os: "windows", arch: "x86_64", ffmpeg: (..), ffprobe: (..) };
// in the table:
WINDOWS_X64,
FfmpegFiles { arch: "aarch64", ..WINDOWS_X64 }, // Windows on ARM runs the x64 build
```

### 3.2 Rust production code

#### D2. `ExporterConfig { .. }` is built six times with eleven identical fields (7/10, effort S)

**Evidence.** `crates/core/message-crate-core/src/exporters.rs:336-348`, `:392-404`, `:464-474`, `:487-499`, `:520-532`, `:552-564`. Each copies `output: PathBuf::from(self.output.trim())`, `scratch_dir: scratch_dir.to_path_buf()`, `timezone: None`, `obfuscate`, `media`, `cancel: None`, `log: None`, `progress: None`, `issues: None`, `output_format: self.output_format`, `resume: false`. This file alone holds 100 of the 1,164 cloned production lines in Rust. A new `ExporterConfig` field touches six sites.

**Remediation.** One constructor on the form side:

```rust
// crates/core/message-crate-core/src/exporters.rs
impl ExportForm {
    fn config(&self, inputs: Vec<PathBuf>, scratch_dir: &Path, obfuscate: bool, media: MediaConfig, source: SourceConfig) -> ExporterConfig {
        ExporterConfig {
            inputs,
            output: PathBuf::from(self.output.trim()),
            scratch_dir: scratch_dir.to_path_buf(),
            timezone: None,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            issues: None,
            output_format: self.output_format,
            resume: false,
            source,
        }
    }
}
// each of the six sites becomes one line:
self.config(inputs, scratch_dir, obfuscate, media, SourceConfig::GoSmsPro(GoSmsProConfig { owner_phones }))
```

(The receiver type name is the enclosing `impl` at each site; the six sites are in one `impl` block or sibling blocks of the same type, which the file shows.)

#### D3. The Android SMS millisecond-date parser exists twice, with the same tests (7/10, effort S)

**Evidence.** `crates/libs/sbr/src/read.rs:558` defines `MAX_DATE_MS = 253_402_300_799_999` and `:564-580` (`timestamp_from_date`) parses the `date` attribute as `i64`, filters `0..=MAX_DATE_MS`, and divides by 1000. `crates/exporters/go-sms-pro-exporter/src/xml.rs:60` defines the same constant and `:65-72` (`timestamp_secs_from_date_ms`) the same parse, including the same comment ("Exact: every value up to MAX_DATE_MS fits in an f64 mantissa"). The tests are copies too: `sbr/src/read.rs:1275-1310` and `go-sms-pro-exporter/src/xml.rs:439-472` (`unreadable_dates_are_skipped`, `millisecond_dates_parse_exactly`). **Drift:** the sbr table has `" 1400773261000"` (leading space, `read.rs:1286`) and the go-sms-pro table does not, so a leading-space date is proven rejected on one path only.

Both crates already depend on `message-ir` and `phone` (`crates/libs/sbr/Cargo.toml:9,15`; `crates/exporters/go-sms-pro-exporter/Cargo.toml:14,18`).

**Remediation.** One function in `message-ir`, which already owns `TimePrecision`:

```rust
// crates/libs/ir/src/android_date.rs (new), re-exported from lib.rs
/// The last millisecond of the year 9999, the latest `date` read as real.
pub const MAX_DATE_MS: i64 = 253_402_300_799_999;

/// Unix seconds from an Android backup's millisecond `date`, or `None` for
/// anything but a whole number of milliseconds from 0 to [`MAX_DATE_MS`].
pub fn unix_secs_from_date_ms(date_ms: &str) -> Option<f64> {
    let millis = date_ms.parse::<i64>().ok().filter(|ms| (0..=MAX_DATE_MS).contains(ms))?;
    Some(millis as f64 / 1000.0) // exact: every value up to MAX_DATE_MS fits an f64 mantissa
}
```

Move the two test tables beside it (union of both lists), and leave in each crate only the one test that its `skipped_invalid_date` counter increments.

#### D18. `is_executable` in the WhatsApp exporter and the desktop host disagree (5/10, effort S)

**Evidence.** `crates/exporters/whatsapp-exporter/src/wtsexporter.rs:135-145` and `src-tauri/src/tool_downloads.rs:328-339`: the same `cfg(unix)` / `cfg(not(unix))` pair. **Drift:** the desktop copy also requires `metadata.is_file()`, and on Windows answers `path.is_file()` where the exporter answers `true`. A directory with an executable bit at the wtsexporter path is accepted by the exporter's finder and refused by the app's tool check.

**Remediation.** One copy in the `media` crate, beside `tool_in_dir` (`crates/libs/media/src/tools.rs:258`), which both sides already depend on:

```rust
// crates/libs/media/src/tools.rs
/// Whether `path` is a file the operating system would run.
#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
pub fn is_executable(path: &Path) -> bool { path.is_file() }
```

#### D19. `file_sha256` re-implemented in the desktop host; two in-memory hex digests (5/10, effort S)

**Evidence.** `src-tauri/src/tool_downloads.rs:342-355` streams a file through SHA-256 in 64 KiB blocks; `crates/libs/ir/src/lib.rs:938-954` is the shared version, and `src-tauri/Cargo.toml:33` already depends on `message-ir`. `crates/libs/media/src/lib.rs:279-281` shows the intended shape: a one-line wrapper. **Drift:** the desktop copy retries on `ErrorKind::Interrupted`; the shared one does not, and names the file in its error. Separately, `crates/core/message-crate-core/src/attachment_jobs.rs:430-435` (`hex_sha256`, a manual `format!("{b:02x}")` loop) and `crates/server/server/src/assets_api.rs:162-164` (`sha256_hex`, `hex_encode(digest)`) are the same function.

**Remediation.**

```rust
// src-tauri/src/tool_downloads.rs: delete file_sha256 and use
let actual = message_ir::file_sha256(path)?;
```

Move the `Interrupted` retry into `message_ir::file_sha256` if it is wanted, so both callers get it. For the digest pair, `hex_sha256` in core becomes `hex::encode(Sha256::digest(bytes))`; the server keeps its `Sha256` newtype.

#### D21. Two ffprobe wrappers in the `media` crate (4/10, effort S)

`crates/libs/media/src/probe.rs:34-46` (`probe_media`, `pub`) and `crates/libs/media/src/tools.rs:536-548` (`probe_video`, `pub(crate)`) both build `ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,width,height,avg_frame_rate,bit_rate -of csv=p=0 <path>` from `ffprobe_command()` (`tools.rs:526`). Keep `probe_media`; make `probe_video` call it and map `MediaProbe` to `Probe`, or merge the two result types.

#### D22. `attachment_versions::record` and `share` differ by one predicate (3/10, effort S)

`crates/server/server/src/db/attachment_versions.rs:187-210` and `:254-277`: the same `UPDATE attachments SET {sha} = $1, {path} = $2, {mime} = $3 WHERE sha256 = $4 AND message_id IN (...)` in a write transaction; `share` adds `AND COALESCE({path_column}, '') = ''`. One private `fn point_rows(conn, version, account_id, original_sha, file, only_unset: bool)` and two one-line public wrappers.

#### D26. The `ConvertExportArgs` struct is declared in five exporters (extends F8; 4/10, effort M)

F8 recorded the `convert_export` skeleton. The input struct is also copied: `crates/exporters/imazing-exporter/src/emit.rs:88`, `openextract-exporter/src/emit.rs:32`, `sms-backup-restore-exporter/src/emit.rs:74`, `go-sms-pro-exporter/src/emit.rs:388`, `sms-backup-plus-exporter/src/emit.rs:414`, with `whatsapp-exporter/src/emit.rs:35` as `ConvertRequest`. Every copy carries `transforms`, `output_format`, `cancel`, `resume`, `issues` with the same doc comments. The `run.rs` entry is the same nine lines in `sms-backup-restore-exporter/src/run.rs:15-33` and `go-sms-pro-exporter/src/run.rs:14-31`, and the `run_pipeline` call shape repeats at imazing `run.rs:19`, openextract `run.rs:19`. F8's `run_convert_export` in core is the right home; add to it:

```rust
// crates/core/message-crate-core/src/run.rs
/// What every exporter's convert step takes from the run, besides its own inputs.
pub struct ConvertRun<'a> {
    pub scratch_dir: &'a Path,
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    pub resume: bool,
    pub issues: Option<&'a IssueSink>,
}
impl ExporterConfig { pub fn convert_run(&self, transforms: ExportTransforms) -> ConvertRun<'_> { .. } }
```

Each exporter's args struct then holds only its own fields plus `run: ConvertRun<'a>`.

#### D28 to D30. Small in-file repeats (2/10 each)

- `crates/server/demo-seed/src/conversations.rs:388-400`, `:484-495`, `:543-555`, `:634-645`: the same `open_jsonl` + `write_conversation_header(.., IrConversationType::Individual, None, participants, count, export_meta(source, owner, reference_time))` call. A `fn individual_header(&self, staging, chat_id, participants, count, source, owner)` removes three copies.
- `crates/server/server/src/db/staging.rs:634-657`: the same two `MarkSql` literals (`$1/$2/$3` and `backup_taken_at/deletion/undated_deletion`) are built twice in one `format!`. Two `const`s.
- `crates/server/server/src/db/trash.rs:118-136` and `:145-158`: `move_to_trash` and `restore` share the ownership check and `marker()` lines. Acceptable as is; listed for completeness.

### 3.3 Server handlers and `db/`

#### D4. The Import Run and Export Run list pipelines are the same 45 lines (6/10, effort M)

**Evidence.** `crates/server/server/src/imports_api/mod.rs:1168-1215` (`import_rows_page`) and `crates/server/server/src/exports_api.rs:386-431` (`exports_page`): `page_params(limit, offset, DEFAULT_LIST_LIMIT, Some(MAX_LIST_OFFSET))`, `parse_sort(.., SORT_KEYS, DEFAULT_SORT)`, trim-and-filter `status`, the same validation sentence `"status: unknown value '{status}'; accepted values are {}"` (`imports_api/mod.rs:1193`, `exports_api.rs:411`, the only two sites of that sentence), the db call, then `Ok(Page { items, total, limit: page.limit, offset: page.offset })`. In `db/`, `record_credential` is identical but for the table name (`db/imports.rs:1188-1209`, `db/exports.rs:527-548`: `UPDATE {imports|exports} SET credential = $1, app_kind = $2, app_build = $3, api_token_label = $4, api_token_hint = $5 WHERE id = $6`), and `detach_from_account` has the same three-statement shape (`db/imports.rs:1221-1262`, `db/exports.rs:559-591`).

The `Page { items, total, limit: page.limit, offset: page.offset }` literal is written inline at `audit_trail_api.rs:74,146`, `exports_api.rs:425`, `accounts_api.rs:280,1455`, `imports_api/mod.rs:1155,1209,1454`, `db/contacts/read.rs:273`, `db/conversations.rs:167`, `db/exports.rs:513`, `db/conversation_messages.rs:941,1064`, although `paging.rs:33-45` (`page_of`) and `:53-61` (`whole_page`) exist and `imports_api/mod.rs:1152-1160` (`runs_page`) is a hand-written `Page::map`.

**Remediation.**

```rust
// crates/server/server/src/paging.rs
impl<T> Page<T> {
    /// A page the database counted and cut: `rows` are the page, `total` the whole.
    pub fn from_rows(items: Vec<T>, total: u64, params: &PageParams) -> Self {
        Page { items, total, limit: params.limit, offset: params.offset }
    }
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U> {
        Page { items: self.items.into_iter().map(f).collect(), total: self.total, limit: self.limit, offset: self.offset }
    }
}

/// `status`, trimmed, when it names one of `all`; a validation problem otherwise.
pub fn parse_status<'a>(raw: Option<&'a str>, all: &[&'static str]) -> Result<Option<&'a str>, ApiError> {
    let Some(status) = raw.map(str::trim).filter(|s| !s.is_empty()) else { return Ok(None) };
    if !all.contains(&status) {
        return Err(ApiError::validation(format!("status: unknown value '{status}'; accepted values are {}", all.join(", "))));
    }
    Ok(Some(status))
}
```

```rust
// crates/server/server/src/db/runs.rs (new): the two Run tables' shared statements
#[derive(Clone, Copy)] pub enum RunTable { Imports, Exports }
impl RunTable { fn name(self) -> &'static str { match self { Self::Imports => "imports", Self::Exports => "exports" } } }
pub async fn record_credential(conn: &mut SqliteConnection, table: RunTable, run_id: i64, credential: &CredentialUsed) -> Result<()> { /* body of db/imports.rs:1193-1208 with {table} */ }
```

Delete `runs_page` in favour of `Page::map`, and replace the twelve inline literals with `Page::from_rows`.

#### D6. The "not in the trash" predicate is written in six places (6/10, effort S)

**Evidence.** `crates/server/server/src/search/emit.rs:19` (`NOT_TRASHED_CONTACT`), `:21` (`NOT_TRASHED_CONVERSATION`), `:26-30` (`not_trashed_conversation`), `:34-36` (`not_trashed_contact`), `:41-45` (`not_trashed_contact_id`); `crates/server/server/src/db/participant_names.rs:41-46` (`not_trashed`, the same `NOT EXISTS (SELECT 1 FROM trashed_contacts ...)` with alias `t`); and inline at `db/contacts.rs:691`, `db/conversation_messages.rs:91`, `:710`, `db/address_book.rs:1164`. The comment at `search/emit.rs:24-25` says "The one clause, written once, so the lists and the contact counts cannot drift apart"; it is written six times.

**Remediation.** The trash tables belong to `db/trash.rs`; the predicates move there and `search/emit.rs` and `participant_names.rs` import them:

```rust
// crates/server/server/src/db/trash.rs
/// Conversation `conv` is not in the trash, as a WHERE/ON fragment.
pub(crate) fn not_trashed_conversation(conv: &str) -> String {
    format!("NOT EXISTS (SELECT 1 FROM trashed_conversations tc WHERE tc.account_id = {conv}.account_id AND tc.conversation_id = {conv}.id)")
}
/// Contact `ct` is not in the trash.
pub(crate) fn not_trashed_contact(ct: &str) -> String { not_trashed_contact_id(&format!("{ct}.account_id"), &format!("{ct}.id")) }
pub(crate) fn not_trashed_contact_id(account_expr: &str, id_expr: &str) -> String {
    format!("NOT EXISTS (SELECT 1 FROM trashed_contacts tct WHERE tct.account_id = {account_expr} AND tct.contact_id = {id_expr})")
}
```

Then replace the four inline `NOT EXISTS (SELECT 1 FROM trashed_...` fragments with calls.

#### D7. Nine identical `spawn_blocking` error maps; nineteen "x not found" literals (5/10, effort S)

**Evidence.** `crates/server/server/src/assets_api.rs:646, 880, 1093, 1193, 1264, 1282, 1351, 1411, 1448` each read `.map_err(|e| ApiError::Internal(anyhow::anyhow!("<name> task: {e}")))??`. `ApiError::NotFound("conversation not found".into())` appears six times (`conversations_api.rs:96,124,227` and three more), `"contact not found"` five, `"asset not found"` four, `"API token not found"` four (`accounts_api/api_tokens.rs:226,384` and two more); eleven of them are `.ok_or_else(|| ApiError::NotFound(..))` (`saved_searches_api.rs:129`, `messages_api.rs:221`, `exports_api.rs:269`, `conversations_api.rs:96,124,227`, `contacts_api.rs:319`, `accounts_api.rs:163,724`, `accounts_api/api_tokens.rs:226,384`).

**Remediation.**

```rust
// crates/server/server/src/server.rs (or extract.rs), beside AppState
/// Run `f` off the async runtime and name the task in the error.
pub(crate) async fn blocking<T>(task: &'static str, f: impl FnOnce() -> Result<T, ApiError> + Send + 'static) -> Result<T, ApiError>
where T: Send + 'static {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError::Internal(anyhow::anyhow!("{task} task: {e}")))?
}
// crates/server/server/src/problem.rs
impl ApiError {
    /// `404` for a thing of `kind` this account does not have.
    pub(crate) fn not_found(kind: &str) -> Self { Self::NotFound(format!("{kind} not found")) }
}
// call sites: .ok_or_else(|| ApiError::not_found("conversation"))
```

#### D5. `ApiError`'s eighteen message variants are listed twice (4/10, effort S)

`crates/server/server/src/server.rs:709-726` (`into_problem`) and `:752-769` (`Display`) each list `Self::MalformedBody(m) | Self::UnsupportedMediaType(m) | ... | Self::MediaLinkInvalid(m)`. The compiler forces a new variant into both, so this cannot drift silently; it is pure weight. One `fn message(&self) -> Option<&str>` holding the list, called from both.

#### D25. `ImportRunSummary::from` and `OwnerImportRun::from` move the same fifteen fields (3/10, effort S)

`crates/server/server/src/imports_api/mod.rs:970-999` and `:1098-1118`: `id, source, tool, mode, status, started_at, finished_at, message_count, attachment_count, bytes_uploaded, duration_ms, parse_ms, attachments_ms, prepare_ms, upload_ms, issue_count, note_count, contacts_new, contacts_changed` copied field by field from `ListedImport`. A shared `RunTimings` struct embedded (`#[serde(flatten)]`) in both API types, with one `From<&ImportRow> for RunTimings`, removes the second copy.

### 3.4 Desktop host (`src-tauri/`)

D18 and D19 above are the two that matter. Also:

#### D20. `absolute` and `resolve_absolute` (3/10, effort S)

`src-tauri/src/run_directories.rs:440-450` and `src-tauri/src/commands/paths.rs:337-350`: trim, refuse empty, refuse relative. The second takes the two messages as parameters; the first hard-codes them. Make the first call the second: `resolve_absolute(raw, "Path is empty", "Path must be absolute")`.

The `Result<_, String>` convention across `commands/*` (15 sites in `paths.rs`, 11 in `staging.rs`, 7 in `extract.rs`) was looked at for a repeated conversion and found to be varied rather than repeated: 5 `map_err(|e| e.to_string())`, 3 `map_err(|e| format!("{e:#}"))`, 10 `.to_string())?`. That is an inconsistency (`{e:#}` keeps the context chain, `to_string()` drops it) rather than a clone; a `fn user_error(e: anyhow::Error) -> String { format!("{e:#}") }` would make it one choice. Listed, not scored.

### 3.5 Web production code

#### D8. `formatBytes` exists twice, byte for byte (7/10, effort S)

**Evidence.** `web/src/screens/settings/storage/storageUtils.ts:114-124` and `web/src/lib/attachmentProgressCopy.ts:4-14`: the same eleven lines (`["B", "KB", "MB", "GB", "TB"]`, `digits = value >= 10 || unit === 0 ? 0 : 1`). Twelve files import one or the other; the storage screens import the `storageUtils` copy and the import screens the `lib` copy, so a format change (say, `KiB`) lands on one screen and not the other.

**Remediation.** Delete the `storageUtils.ts` copy and re-export:

```ts
// web/src/screens/settings/storage/storageUtils.ts
export { formatBytes } from "../../../lib/attachmentProgressCopy";
```

Then move `formatBytes` into its own `web/src/lib/formatBytes.ts` so that `attachmentProgressCopy.ts` is not the odd home for it. `countOf` (`storageUtils.ts:128-130`) and `plural` (`web/src/screens/TrashScreen.tsx:96-98`) are the same idea twice as well (D31, 3/10); one `lib/plural.ts`.

#### D9. `checkOptionalPath` in the WhatsApp and iMessage import validators (5/10, effort S)

**Evidence.** `web/src/lib/whatsappImport.ts:122-148` and `web/src/lib/imessageImport.ts:102-128`: the same 27 lines; only the error-key type and the "missing" constant differ.

**Remediation.**

```ts
// web/src/lib/pathChecks.ts
export function checkOptionalPath<K extends string>(
  path: string, stat: PathStat | null, errors: Partial<Record<K, string>>, key: K,
  messages: { missing: string; wrongKind: string }, expectDirectory: boolean,
): void { /* body of whatsappImport.ts:130-147 with the two constants replaced */ }
```

#### D10. The three auth forms repeat the credential fields (5/10, effort M)

**Evidence.** `web/src/screens/auth/ClaimForm.tsx:62-99`, `web/src/screens/auth/CreateAccountForm.tsx:69-114`, `web/src/screens/auth/LoginForm.tsx:51-72`: the same `<TextField label="Username" leadingIcon={<PersonIcon size={16} />} ... autoComplete="username">` and `<PasswordField label="Password" className="mt-3.5" leadingIcon={<LockIcon size={16} />} ... showPassword onToggle>` blocks, with the confirm field repeated in two. A `showPassword` state pair is declared in each form. **Drift:** `CreateAccountForm.tsx:78-79` carries a layout comment the other two lack.

**Remediation.**

```tsx
// web/src/screens/auth/CredentialFields.tsx
export function CredentialFields({ form, confirm, newPassword, disabled }: {
  form: { username: string; setUsername: (v: string) => void; password: string; setPassword: (v: string) => void;
          confirmPassword?: string; setConfirmPassword?: (v: string) => void };
  confirm: boolean; newPassword: boolean; disabled: boolean;
}) { /* the three fields from CreateAccountForm.tsx:69-105, with autoComplete picked from newPassword */ }
```

#### D11. The Import Run store's "back to the form" slice is written three times, differently (6/10, effort S)

**Evidence.** `web/src/screens/import/importRunStore.ts:92-110` (the initial state), `web/src/screens/import/useImportJob.ts:561-573` (`returnToForm`), `useImportJob.ts:1501-1516` (the start of a run). All three set `summaryView, runDir, importRunId, stagingSummary, mediaSummary, mediaFailedCount, mediaToolsMissing, mediaPartiallyRan, computingSummary, reviewError` to the same values. **Drift:** `returnToForm` does not reset `resumeError`, `sourceIdentities`, or `runDirDeleteFailure`, which the initial state has; the run start resets `resumeError` but not the other two. A field added to the store needs three edits, and a missed one leaves a stale value on the form.

**Remediation.**

```ts
// web/src/screens/import/importRunStore.ts
/** Everything a run leaves behind, cleared when the form comes back or a new run starts. */
export const CLEARED_RUN = {
  summaryView: null, runDir: null, importRunId: null, stagingSummary: null, mediaSummary: null,
  mediaFailedCount: null, mediaToolsMissing: [] as string[], mediaPartiallyRan: false,
  resumeError: null, reviewError: null, computingSummary: false, sourceIdentities: null, runDirDeleteFailure: null,
} as const;
// initial state: { phase: "form", running: false, steps, form: null, ...CLEARED_RUN }
// returnToForm: store.set({ phase: "form", ...CLEARED_RUN })
// start of run: store.set({ running: true, phase: "running", form, ...CLEARED_RUN })
```

Whether `returnToForm` leaving `sourceIdentities` alone is intended must be decided when the constant is introduced; the drift is the finding.

#### D12. `<AttachmentFields ...>` with ten props, three times (4/10, effort S)

`web/src/screens/import/ImportFormFields.tsx:586-596`, `:718-728`, `:743-753`: the same ten-prop invocation in three branches of one conditional. Build the element once above the branches (`const attachmentFields = <AttachmentFields ... />`) and place `{attachmentFields}` in each.

#### D13. The identity dialogs are wired twice (4/10, effort M)

`web/src/components/contactDrawer/ContactDrawerHandles.tsx:110-122` and `web/src/screens/settings/IdentitiesSection.tsx:213-225`: the same `<IdentityCountryDialog open={countryPick.target !== null} address=... busy error onClose onPick>` followed by `<AddIdentityDialog open={adding} busy ...>`. Both are identity editors over the same `countryPick` hook. An `IdentityDialogs` component taking `countryPick`, `adding`, `busy`, `error`, `existing`, and the two close handlers removes the second copy.

#### D14. `InstagramBubble` and `WhatsAppBubble` are the same file (4/10, effort S)

`web/src/components/messages/InstagramBubble.tsx:1-27` and `web/src/components/messages/WhatsAppBubble.tsx:1-27` differ in one token: `--instagram-brand` against `--whatsapp-brand`. `SmsBubble.tsx:13-25` and `ImessageBubble.tsx:13-24` share the same six-line preamble (`time`, `mine`, `nameSender`, `body`, `hasAttachments`).

```tsx
// web/src/components/messages/BrandedBubble.tsx
export default function BrandedBubble({ brand, ...props }: MessageBubbleProps & { brand: "instagram" | "whatsapp" }) {
  /* body of InstagramBubble with senderClassName={`text-[var(--${brand}-brand)]`} */
}
// InstagramBubble.tsx: export default (p: MessageBubbleProps) => <BrandedBubble brand="instagram" {...p} />;
```

#### D15. Inline SVG icons outside `components/icons.tsx` (3/10, effort M)

The chevron path `M2.5 3.5 5 6l2.5-2.5` is drawn inline in `web/src/components/Select.tsx:94`, `TimeZoneField.tsx:101`, and `advancedSearch/ChoiceMultiSelect.tsx:99`. Sixteen `function *Icon()` components are defined outside `web/src/components/icons.tsx`, in `ConversationRow.tsx`, `SearchBar.tsx`, `ThemeSettings.tsx`, `DateField.tsx`, `LeftPanel.tsx`, `SortMenu.tsx`, `PasswordField.tsx`, `ApiTokenRevealDialog.tsx`, and `screens/OnboardingScreen.tsx`; each repeats the same eight-attribute `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>` wrapper. Add `ChevronDownIcon` to `icons.tsx` and move the sixteen there over time.

#### D16, D17. Two small structural repeats (3/10 each)

- `web/src/components/InfiniteOffsetList.tsx:173-203` and `:338-357`: the same fifteen-prop type literal on two components, and the same fifteen keys again as the `listProps` object at `:606-620`. One `type VirtualListProps<T>`.
- `web/src/screens/ExportScreen.tsx:292-305` and `web/src/screens/settings/ConvertSection.tsx:156-169`: the same output-format `<Select>` over `EXPORT_FORMATS` inside the same `TauriJobFormShell`. One `ExportFormatSelect`.

### 3.6 Test code

Test duplication is the largest share by volume (9.1% of Rust test lines). Most of it is a fixture that exists and is not used.

#### T1. The "create an Import Run, post one batch, complete it" helper exists four times (6/10, effort S)

`crates/server/server/src/imports_api/tests/time_precision.rs:12-37` (`import_run`), `imports_api/tests/changed_content_dedupe.rs:15-45` (`import`), `imports_api/tests/phone_country.rs:27-61` (`import_run`), and `imports_api/tests.rs:3179-3196` (**Unable to verify** the enclosing helper's name; the clone is confirmed by range). Each: `post_created_json(state, "/v1/imports", token, {source, mode})`, `post_raw(.., "/batches", "application/jsonl", body)`, assert 200, `post_json(.., "/complete", {status: "completed"})`. `test_support.rs` already hosts import helpers (F10 cites `:1091`, `:1127`); add this one there and delete the four.

#### T2. Raw SQL seeding beside the fixture that does it (5/10, effort M)

`INSERT INTO conversations (...)` is written in test code at 40 sites (`conversations_api/tests.rs` 14, `contacts_api/tests.rs` 10, `reset_demo/tests.rs` 4, `dedupe/tests.rs` 2, `db/trash/tests.rs` 2, and seven singletons), while `test_support::seed_conversation` (`test_support.rs:971`, `SeedConversation` at `:783`) has 33 callers. `INSERT INTO accounts (id, username) VALUES ($1, 'alice')` plus `test_pool` plus `ensure_accounts_schema` is written at 23 sites (`db/session_tokens.rs:476-484, 498-506, 527-535, 555-563, 581-589` are five of them, inside one test module), while `TestFixture::account_with_id` (`test_support.rs:166`) exists. Each raw site is a place a schema change breaks tests one at a time. Replace mechanically; where `seed_conversation` cannot express the row (a `group` with a `group_title`, as at `contacts_api/tests.rs:604-612`), extend `SeedConversation`.

#### T4. `test_support.rs` repeats its own "answer 201 Created" block three times (4/10, effort S)

`crates/server/server/src/test_support.rs:262-280` (`register_via_api`), `:345-362` (login), `:417-433` (`post_created_json`): send, read `status`, `Location`, `text`, assert `CREATED`, parse. `openapi/document_rules.rs:603-620` writes the same response reading again. One `async fn created(response: reqwest::Response, what: &str) -> (String, serde_json::Value)`.

#### T5, T6. Two in-file sequences repeated (3/10, 4/10)

- `crates/server/server/tests/server_log.rs:91-124` and `:348-378`: claim the owner, open registration, register alice, log in. One `async fn claimed_with_alice(base) -> Accounts`.
- `crates/server/server/src/assets_api/tests.rs:835-851, 854-879, 908-925, 935-960, 988-1027`: start a chunked upload, `PUT` each part, `POST .../complete`, `GET`. One `async fn upload_in_parts(server, token, bytes) -> UploadOutcome`.

#### T7. The sample conversation document is rebuilt inline where `sample_document` exists (5/10, effort S)

`message_ir::testutil::sample_document` (`crates/libs/ir/src/testutil.rs:13-34`) is used by 14 files. The same document (source `sms-backup-restore`, tool version `10.26.003`, owner `+15555550100`, participant `Sam`) is rebuilt field by field at `crates/libs/import/src/project.rs:134-151`, `crates/libs/import/tests/import_mock.rs:31-48`, `src-tauri/src/commands/upload.rs:383-401`, and `crates/libs/ir-format/src/export_transforms/tests.rs:21-43`. The import crate's dev-dependencies (`crates/libs/import/Cargo.toml:23`) do not enable `message-ir`'s `testutil` feature, which explains two of the four; `src-tauri/Cargo.toml:71` and `crates/libs/ir-format/Cargo.toml:21` already do.

```toml
# crates/libs/import/Cargo.toml, [dev-dependencies]
message-ir = { path = "../../libs/ir", features = ["testutil"] }
```

#### T8. The `IrAttachment` test literal (4/10, effort S)

Eleven lines of `IrAttachment { path, original_name, mime_type, digest_sha256: None, is_sticker: false, transcription: None, sticker_effect: None, size_bytes, missing_reason: None, bytes }` at `crates/libs/staging/src/counted_attachments.rs:160-171`, `crates/libs/staging/src/export_writer.rs:345-355`, `crates/libs/ir-format/src/format_sink.rs:427-437`, `crates/libs/ir-format/src/util.rs:106-117`, `crates/exporters/imessage-ir-exporter/src/convert.rs:1322-1332`, `crates/libs/reexport/src/tests.rs:1795-1804`, `crates/exporters/sms-backup-restore-exporter/src/read/tests.rs:493-501`, `crates/exporters/sms-backup-plus-exporter/src/write/tests.rs:16-24`. Every one of these crates enables `message-ir`'s `testutil`.

```rust
// crates/libs/ir/src/testutil.rs
/// A JPEG attachment record with no file; set `path`, `bytes` or `size_bytes` after.
pub fn attachment() -> IrAttachment {
    IrAttachment { path: None, original_name: Some("photo.jpg".into()), mime_type: Some("image/jpeg".into()),
        digest_sha256: None, is_sticker: false, transcription: None, sticker_effect: None,
        size_bytes: None, missing_reason: None, bytes: None }
}
```

#### T9. `with_directory_mode` three times, one drifted (3/10, effort S)

`crates/exporters/imazing-exporter/src/test_support.rs:13-33` and `crates/helpers/imessage-reader/src/test_support.rs:180-200` are identical (probe: `read_dir` still works). `crates/libs/ir-format/src/lib.rs:58-80` probes a write instead. `ir-format` depends on `message-crate-core` (`crates/libs/ir-format/Cargo.toml:12`) and imazing enables core's `testutil` (`Cargo.toml:25`), so `message_crate_core::testutil::with_directory_mode` serves both; `imessage-reader` keeps its own copy because it may not link Fair Core crates (ADR 0014).

#### T10, T12, T16 to T19. Smaller test repeats (2/10 to 3/10)

- Reading an Import Run log back with `parse_run_log_line` into `(level, text)` pairs: `crates/libs/import/src/progress.rs:527-533`, `src-tauri/src/app_directories.rs:221-227`, `:251-257`. A `run_log_lines(path)` in `message_crate_core::testutil`.
- The `MailMessage` literal: `crates/libs/mail/src/parse.rs:490-`, `:622-`, `:738-`, and `crates/libs/mail/src/tests.rs:5-23`. A `sample_mail_message()` in `mail/src/tests.rs`.
- `stub_attachment` (`crates/helpers/imessage-reader/src/body.rs:134-147`) and `attachment` (`fields.rs:516-529`) are the same `Attachment` literal; `imessage-reader/src/test_support.rs` is the home.
- `crates/exporters/whatsapp-exporter/src/wtsexporter.rs:786-800, 829-838, 867-881`: three expected `command_args` arrays sharing the same seven-element prefix. A `fn base_args(out, json) -> Vec<String>`.
- `crates/libs/import/src/journal.rs:598-620` and `crates/libs/journal/src/lib.rs:298-320`: the same torn-line concurrency test. The `journal` crate's copy proves the property; the import copy can go.
- `crates/libs/export/src/project.rs:804-825` builds inline the `Message` that `seed_message_with_participant` (`:959-985`, same file) builds.

#### T13. The web import form fixture, five files and seven inline copies (6/10, effort S)

The 23-field form (`source: "imessage-ios"`, `backupPath`, `attachmentMedia: "copy"`, ..., `assetMaxBytes: 512 * MIB`) is a helper in `web/src/screens/ImportScreen.test.tsx:222-245` and is written out inline again six times in the same file (`:473-493` and five more per the scan), and as a constant in `web/src/screens/import/useImportJob.test.tsx:282-307`, `web/src/screens/import/logoutDuringUpload.test.tsx:100-123`, `web/src/screens/import/ImportRunView.test.tsx:70-92`, and `web/src/screens/import/ImportFormFields.test.tsx` (`whatsappOwnerPhone` at `:84` and `:365`; **Unable to verify** that this copy is the full form). **Drift:** `useImportJob.test.tsx` and `logoutDuringUpload.test.tsx` include `backupPassword`, `whatsappKey`, and `phoneCountry`; `ImportScreen.test.tsx` has `backupPasswordGiven` and `whatsappKeyGiven` instead; `ImportRunView.test.tsx` has no `assetMaxBytes`. Two of these shapes cannot both be the type the store holds.

```ts
// web/src/test/importForm.ts
import type { ImportForm } from "../screens/import/importForm"; // the real type, so a drifted copy fails to compile
export function importForm(overrides: Partial<ImportForm> = {}): ImportForm {
  return { /* the 23 fields from useImportJob.test.tsx:282-307 */, ...overrides };
}
```

#### T14. Three fixtures copied between two files each (4/10, effort S)

- `message()`: `web/src/components/messages/ImessageBubble.test.tsx:13-28` and `web/src/screens/message/MessageThread.test.tsx:26-41`, identical, and both already import `message as baseMessage` and `participant` from `web/src/test/apiShapes.ts:31,41`. Add `imessageMessage(partial)` there.
- `importRun()`: `web/src/screens/import/ResumeImportPanel.test.tsx:15-29`, `web/src/screens/import/resumeDecision.test.ts:5-19`, `web/src/screens/ImportScreen.test.tsx:202-216`. Add `activeImportRun(overrides)` to `apiShapes.ts`.
- The in-memory `localStorage` stub: `web/src/lib/system-settings.test.ts:11-29` and `web/src/lib/recentSearches.test.ts:6-24`. `web/src/test/setup.ts` is the Vitest `setupFiles` entry (`web/vite.config.ts:80`); a `web/src/test/localStorage.ts` exporting `installMemoryStorage()`.

#### T15. Test providers exist and are bypassed (5/10, effort M)

`web/src/test/providers.tsx` exports `testQueryClient` (`:34`), `renderWithProviders` (`:66`), and `mockedAuth` (`:77`). Yet 9 test files call `new QueryClient(` themselves, 44 local `render*`/`wrap*` helpers are defined across test files (14 files use `renderWithProviders`), and `vi.mock("<path>/lib/auth", ...)` is written in 47 files (22 + 17 + 8 by relative path). The `useAuth` mock body is the natural thing to drift (a field added to the auth context must be added to 47 mocks). Add a `mockAuth(overrides)` helper to `providers.tsx` that calls `vi.mock` with `mockedAuth`, and use `renderWithProviders` where a local `render` only adds the same providers.

## 4. What extends the design audit's findings

- **F8** (SMS exporter copy-paste): D3 adds the date parser and its tests; D26 adds the `ConvertExportArgs` struct in five exporters and the `run.rs` entry in four.
- **F10** (test fixture God module): T1, T2, T4 show the module is also under-used: 40 raw conversation inserts and 23 raw account inserts sit beside the fixture, and the fixture repeats its own response-reading block three times.
- **F15** (web copy-paste): the `instanceof Error ? x.message : String(x)` count is now 42 production sites (F15 said 35) with `apiErrorMessage` (`web/src/lib/apiErrorMessage.ts:10`) used from 26 files; the desktop-read-in-effect pattern is confirmed at `screens/settings/SystemSection.tsx:380-391`, `screens/import/ImportRunView.tsx:228-240`, `screens/ImportScreen.tsx:273`, `:476`, `:532`, `:567`, with `lib/useToolsStatus.ts:27-37` as the `useQuery` shape F15 proposes. Nothing in F15 is repeated here beyond that.
- **F6, F9, F13**: nothing to add.

## 5. Where the shared versions should live

The repo already has the right homes; the recommendation is to fill them rather than add a `util` crate.

| Home | Add | Removes |
| --- | --- | --- |
| `crates/libs/ir/src/android_date.rs` (new, re-exported) | `MAX_DATE_MS`, `unix_secs_from_date_ms` | D3 |
| `crates/libs/ir/src/testutil.rs` | `attachment()`, a `document_with_attachment(text, att)` | T7 call sites, T8 |
| `crates/libs/media/src/tools.rs` | `is_executable`; `probe_video` delegating to `probe_media` | D18, D21 |
| `crates/core/message-crate-core/src/exporters.rs` | `fn config(..)` on the form | D2 |
| `crates/core/message-crate-core/src/run.rs` | `ConvertRun` (with F8's `run_convert_export`) | D26 |
| `crates/core/message-crate-core/src/testutil.rs` | `with_directory_mode`, `run_log_lines` | T9, T10 |
| `crates/server/server/src/paging.rs` | `Page::from_rows`, `Page::map`, `parse_status` | D4 (12 literals, `runs_page`) |
| `crates/server/server/src/db/trash.rs` | the three `not_trashed_*` fragments | D6 |
| `crates/server/server/src/db/runs.rs` (new) | `RunTable`, `record_credential`, `detach_from_account` | D4 |
| `crates/server/server/src/problem.rs` | `ApiError::not_found(kind)` | D7 |
| `crates/server/server/src/server.rs` | `blocking(task, f)` | D7 |
| `crates/server/server/src/test_support.rs` (split per F10) | `import_run`, `created(response)`, `upload_in_parts`, `claimed_with_alice` | T1, T4, T5, T6 |
| `web/src/lib/formatBytes.ts`, `web/src/lib/plural.ts`, `web/src/lib/pathChecks.ts`, `web/src/lib/reservedNames.json` | one each | D8, D31, D9, D1 |
| `web/src/components/icons.tsx` | `ChevronDownIcon`, the sixteen local icons | D15 |
| `web/src/screens/auth/CredentialFields.tsx`, `web/src/components/messages/BrandedBubble.tsx`, `web/src/components/IdentityDialogs.tsx` | one each | D10, D14, D13 |
| `web/src/screens/import/importRunStore.ts` | `CLEARED_RUN` | D11 |
| `web/src/test/apiShapes.ts`, `web/src/test/importForm.ts`, `web/src/test/localStorage.ts`, `web/src/test/providers.tsx` | `imessageMessage`, `activeImportRun`; `importForm`; `installMemoryStorage`; `mockAuth` | T13, T14, T15 |

## 6. Unable to verify

- Whether `web/src/lib/contactGroups.ts`'s `reservedGroupError` (`:48`) mirrors the `special_reserved` sentences at `named_membership.rs:172-`; proving file: `contactGroups.ts` from `:48`.
- Whether `crates/server/server/src/imports_api/tests.rs:3179-3196` is inside a named helper or a test body; proving file: `tests.rs` from `:3160`.
- Whether `web/src/screens/import/ImportFormFields.test.tsx`'s fixture (`:84`, `:365`) is the full 23-field form.
- Whether an `impl From<Imessage> for IrImessage` exists for D23; proving search: `grep -rn "for IrImessage" crates/`.
- jscpd results: not installed, not run.
- The production duplication percentages (1.6%, 1.7%, 2.5%) include inline `#[cfg(test)] mod tests` blocks in production files; a cfg-aware split would lower them.

## 7. Suggested order of work

1. **One sitting, no design needed (S each):** D8, D19, D20, D21, D22, D12, D14, D16, D17, D28, D29, T7 (the Cargo line), T8, T9, T10, T14, T18, T19. Each is a deletion plus an import.
2. **Guard the two boundary copies:** D1 (fixture test plus JSON) and D23 (round-trip test).
3. **The two that change often:** D2 and D11, then D3 with its merged test table.
4. **Server shared pieces:** D6, D7, D4 (in that order; each is one PR), D25.
5. **Web shared pieces:** D9, D10, D13, D15.
6. **Test fixtures, one module at a time, as those tests are next touched:** T1, T2, T4, T5, T6, T13, T15.
