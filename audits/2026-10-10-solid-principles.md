# SOLID principles audit

Date: 2026-10-10. Commit audited: `530dc1791` (branch `docs/initial-software-design-analysis`).
Scope: the whole repository except `web-next/`, `vendor/`, `target/`, `node_modules/` and `docs/dist`.
Method: read-only. `grep`, `sed -n` and `find` over the Rust workspace (`crates/`), the Tauri host (`src-tauri/src`) and the web app (`web/src`); no build or test was run. Every claim cites a `path:line` read at this commit and names the identifier. A claim that could not be confirmed is marked **Unable to verify** with the file that would prove it (section 9).

This audit judges the five principles in Rust terms (traits, enums, generics, modules, crates) and React/TypeScript terms (components, hooks, modules). It does not re-derive the God modules, the layering leaks or the module cycles: those are in the design audit `audits/2026-10-09-initial-software-design-analysis.md`, section 4 and findings F1 to F4, F6, F11, F12 and F14, and are cited here by ID. Two sibling audits run beside this one: design-pattern-implementation (patterns) and code-duplication-detection (clones); where a finding here is also a clone or a pattern, this report names the fact and leaves the inventory to them.

Every path is relative to the repository root. Importance is 1 to 10: 10 means it causes defects or blocks change now; 1 is cosmetic.

## 1. Summary

| Principle | Compliance (1 to 10) | One-line verdict |
|---|---|---|
| Single responsibility | 6 | Crate boundaries are clean and written down (ADRs 0001, 0012, 0014). Inside the server and the web app a handful of modules each carry four to seven jobs (F1, F4), and the model crate `message-ir` carries file I/O and naming policy (F12). |
| Open/closed | 5 | A new backup type is an edit in 11 Rust sites and about 8 web sites across a string boundary nothing checks. A new `ApiError` variant is 12 edits. A new `/v1` route is 5 and is well gated. Wildcard `_ =>` arms silently absorb new variants in three places where a wrong default is a defect. |
| Liskov substitution | 7 | Few traits exist. The two production traits both have contract drift: `ProjectionHooks` is honoured by five exporters and bypassed by two, and one implementor empties a method the dedupe step relies on; `MergedArchive::write` returns a file for one implementor and a directory for the other, and the caller ignores it. The `From<X> for ApiError` impls preserve their inputs, except the `anyhow` funnel. |
| Interface segregation | 6 | `AppState` goes whole to 91 handlers that mostly read `db`. `ExporterConfig` carries every vendor's config to every exporter. One component takes 57 props, one desktop module exports 61 names to 24 importers, one test fixture module serves 53 test files. |
| Dependency inversion | 6 | Sinks are closures, so no pipeline crate links Tauri (good). But `db/` returns the HTTP error type (F2), three pipeline crates depend upward on `message-crate-core` for plumbing (F3), and ffmpeg, reqwest and the Tauri `invoke` bridge are concretions with no port, so tests either need the binary or `vi.mock` seven modules. |

**Overall: 6/10.** The architecture is deliberate and enforced where it is written down; the SOLID shortfalls are concentrated in the places the design audit already names, plus the extension path for a new backup type, which this audit counts site by site.

Findings: S1 to S8, O1 to O5, L1 to L5, I1 to I6, D1 to D7 (31 in all). Section 7 lists them with importance.

## 2. Single responsibility

A module has one responsibility when it has one reason to change. The test used here: name the jobs in the module and ask whether a change to one would ever be made without the others.

### S1. `crates/server/server/src/imports_api/mod.rs` holds four jobs (7/10)

**Evidence.** 1,861 lines.

- Wire DTOs: 16 `#[derive(... Serialize | Deserialize, utoipa::ToSchema)]` structs at `:130`, `:570`, `:584`, `:606`, `:658`, `:669`, `:715`, `:727`, `:747`, `:838`, `:853`, `:864`, `:881`, `:899`, `:1009`, `:1591`, with validation `validate_import_issues` `:767`, `validate_import_status` `:792`, `strip_form_credentials` `:812` and the credential key list `FORM_CREDENTIAL_KEYS` `:809`.
- Pipeline orchestration usable without HTTP: `import_jsonl_files_on_conn` `:314`, `prepare_import` `:399`, `stage_all_files` `:448`, `promote_step` `:514`, plus the run lifecycle `OwnedImportRun::start` `:211` and `complete_run` `:254`, which `reset_demo.rs:33` imports for the Demo Account build.
- Twelve handlers: `owner_import_run` `:1059`, `list_imports` `:1140`, `import_rows_page` `:1168`, `get_import` `:1228`, `full_import_run` `:1241`, `create_import` `:1269`, `complete_import` `:1342`, `list_import_contacts` `:1439`, `import_run` `:1559`, `update_import` `:1627`, `discard_import` `:1664`, `create_import_batch` `:1707`, with naming policy `import_contact_group_name` `:1529` and `import_date_ymd` `:1545`.
- Run control: `MAX_CONCURRENT_IMPORTS` `:1767`, `import_semaphore` `:1769`, `run_import_path` `:1776`.

The design audit records the same module under F1 (size) and F7 (the semaphore and `block_on`).

**Why it hurts.** A change to the JSON shape, to the staging order, to a status code and to the concurrency limit all land in one file, and the CLI and demo paths that need only the second job compile the handlers too. The `#[cfg(test)]`-only `import_jsonl_files` at `:280` shows the test-side entry living beside production handlers.

**Fix.** Keep `mod.rs` as the handler group and move the other three jobs to siblings that already exist as a directory:

```rust
// crates/server/server/src/imports_api/mod.rs
pub mod contact_name;
pub mod failure;
pub mod promote;
pub mod staging;
pub mod with_yourself;
mod dto;          // the 16 DTOs, validate_import_issues, validate_import_status, strip_form_credentials
mod pipeline;     // ImportOptions, FixedImportArgs, ImportCounts, OwnedImportRun, import_jsonl_files_on_conn, prepare_import, stage_all_files, promote_step
mod run_control;  // MAX_CONCURRENT_IMPORTS, import_semaphore, run_import_path
pub use dto::*;
pub use pipeline::{FixedImportArgs, ImportCounts, ImportMode, ImportOptions, ImportSchemaMode, import_jsonl_files_on_conn};
```

No call site changes; `reset_demo.rs:33` keeps importing from `crate::imports_api`.

### S2. `crates/server/server/src/reset_demo.rs` is a build, a verifier, a file swap and a console (6/10)

**Evidence.** 2,188 lines; 19 `println!`/`eprintln!` and 15 `sqlx::query` calls in a module outside `db/`.

- Build: `run_reset_demo` `:245`, `seed_new_database` `:290`, `build_demo_account` `:488`, `import_demo_sources` `:820`, `load_demo_address_book` `:937`.
- Database file handling: `prepare_database_snapshot` `:995` (`VACUUM INTO` `:1004`), `checkpoint_and_clean_sidecars` `:1035` (`PRAGMA wal_checkpoint(TRUNCATE)` `:1046`), `sqlite_sidecar` `:1105`, `install_reset_state` `:1820`, `Swap` `:1842`.
- A verifier of about 700 lines: `verify_non_demo_state_preserved` `:1117`, `outliving_rows` `:1273`, `non_demo_state_on_conn` `:1334`, `search_index_digest` `:1490`, `sample_search_terms` `:1519`, `row_owner` `:1736`, `table_columns` `:1797`, `quote_ident` `:1807`.
- Console copy: `print_reset_header` `:132`, the `eprintln!` lines at `:292`, `:301`, `:306`, `:384`, `:567`.
- The seed contract duplicated with `demo-seed` (`IMESSAGE_SOURCE`, `SBR_SOURCE`, `WHATSAPP_SOURCE` at `:41-43` and `crates/server/demo-seed/src/lib.rs:30-32`): F13.

**Why it hurts.** The verifier is a test oracle that ships in the binary; a change to how the Demo Account is announced on the console is a change to the same file as the SQL that digests every table. `docs/architecture/http-api.md:882-893` says the SQL for a table lives in `db/`.

**Fix.** Split into `reset_demo/{build.rs, snapshot.rs, verify.rs, console.rs}` and move the statements into `db/demo.rs` as F13 says. The verifier can then be `#[cfg(any(test, feature = "verify-demo"))]` if the operator path does not need it in release builds.

### S3. `crates/server/server/src/models.rs`: the import's row shapes, a text format for time, and an IR parser (4/10)

**Evidence.** The module doc at `:1-3` names two jobs itself: "Import-side records mapped from message-ir JSONL, and the one text form of a stored message time (`utc_timestamp_text`), which search day bounds use too."

- Seven record types: `ExportRecord` `:19`, `ConversationRecord` `:28`, `ParticipantRecord` `:77`, `MessageRecord` `:119`, `EarlierVersionRecord` `:168`, `AttachmentRecord` `:181`, `TapbackRecord` `:202`, plus `HandleValue` `:88`.
- A parser: `parse_ir_lines` `:232`, `clean_body` `:216`.
- A time format: `utc_timestamp_text` `:537` (`pub(crate)`), used by `search/value.rs:75` and `db/staging.rs:1333`, `:1673`.
- Upward imports: `crate::imports_api::ImportFailure` `:15`, `crate::config::validate_source_id` `:14`.

Who owns the records: `jsonl.rs:8`, `imports_api/staging.rs:20`, `imports_api/contact_name.rs:14`, `import_cli.rs:16` read them; `db/staging.rs:297`, `:577`, `:1776` take `EarlierVersionRecord` by path. So the records are the import's staging input, consumed by `imports_api/` and written by `db/staging.rs`.

**Why it hurts.** Search depends on an import module for a time string; a change to the staging row shape and a change to the search day-bound text share one file.

**Fix.**

```rust
// crates/server/server/src/db/time_text.rs (new): the one text form of a stored time
pub(crate) fn utc_timestamp_text(instant: DateTime<Utc>) -> String { /* body of models.rs:537 */ }
// search/value.rs:75 and db/staging.rs:1333,1673 import crate::db::time_text::utc_timestamp_text.

// Move models.rs to imports_api/records.rs; jsonl.rs, import_cli.rs and db/staging.rs import crate::imports_api::records.
```

### S4. `crates/libs/ir/src/lib.rs`: model plus file I/O, time formatting, naming policy and a projection pipeline (5/10)

**Evidence.** F12 records this. The concrete non-model items at this commit: `file_sha256` `:938` (streams a file), `write_atomic`/`rename_into_place`/`write_atomic_via` re-exported from `durable.rs` at `:33` (the module doc at `durable.rs:1-11` is about fsync and rename), `format_local_ts` `:913` (uses `chrono::Local`, so output depends on the machine), `give_each_document_its_own_file` `:760` and `conversation_stem` `:834` (file naming policy), `parse_android_type` `:993` (one vendor's field), `parse_json_value` `:1002`, `valid_filename` `:970`, `MIB` `:979`, and the projection input types `PendingMessage` `:1050`, `PendingAttachment` `:1087`, `PendingConversation` `:1130` whose pipeline is the 909-line `projection.rs`. The crate also re-exports five types from `imessage-reader-protocol` at `:53-88` and depends on it (`crates/libs/ir/Cargo.toml:14-18`).

**Why it hurts.** Every consumer of the model (server, web-facing export, every exporter, `csv`, `media`) recompiles when a file-naming rule changes, and a "pure" model test cannot avoid `sha2` and `chrono::Local`.

**Fix.** F12's remediation: `fs-util` for `durable.rs` and `file_sha256`; `IdentityType` defined in `phone`; the five shared types in a permissive `message-ir-shared-types`. In addition, move `give_each_document_its_own_file`, `conversation_stem`, `valid_filename` into `message-ir-format` (the crate that writes files) and `parse_android_type` into the two Android exporters that use it.

### S5. `crates/libs/csv`: CSV cells, CSV reading and time-zone parsing (3/10)

**Evidence.** The crate doc at `crates/libs/csv/src/lib.rs:1` says "Shared CSV helpers for writing conversation files." The crate holds: cell shapes `AttachmentCell` `:16`, `ParticipantCell` `:56`; a reader side `open_csv_lowercase` `:83`, `col` `:105`, `parse_bool` `:113`, `field` `:121`; a re-export of `message_ir::format_local_ts` `:66`; and a time-zone model `Zone` (`zone.rs:12`, with `chrono_tz::Tz`) and `parse_utc_offset` (`utc_offset.rs:13`). `Zone` is used only by `imazing-exporter` (`parse_emit.rs:7`, `emit.rs:22`). Every CSV writer links `chrono-tz` (`crates/libs/csv/Cargo.toml:11`) for it.

**Why it hurts.** Small, but it is the pattern that grew `message-ir`: the shared crate becomes the home of whatever two crates needed. F16 lists `csv::Zone` as misplaced.

**Fix.** Move `zone.rs` and `utc_offset.rs` into `imazing-exporter` (its only user), or into a `time-zone` leaf crate if the WhatsApp or OpenExtract exporter will read naive times too.

### S6. `crates/libs/sbr/src/read.rs`: tokeniser, decoder, address model, keying, owner inference (5/10)

**Evidence.** 1,558 lines, imports `base64` `:4`, `phone` `:5`, `quick_xml` `:6`, `sha2` `:8`. Jobs: XML tokenising (`attrs` `:181`, `decode_references` `:200`, `parse_reader_with` `:814`); MMS part decoding and hashing (`decode_part_data` `:380`, `mms_body` `:428`, `attachment_blob` `:460`); address handling (`address_handle` `:659`, `mms_participants` `:668`, `mms_sender` `:686`, `mms_peers` `:704`, `is_owner` `:715`); conversation keying and titling (`MmsConversation` `:721`, `group_title` `:754`, `group_chat_key` `:773`, which F9 says forked from `phone::group_chat_id`); owner inference (`infer_owner_phones` `:923`). The crate has one user, `sms-backup-restore-exporter` (`Cargo.toml` grep).

**Why it hurts.** A fix to base64 tolerance and a fix to group keying are edits to the same 1,558-line file; `group_chat_key` drifted from `phone` partly because it sits far from it.

**Fix.** Split by job: `sbr/src/{xml.rs, parts.rs, addresses.rs, conversations.rs, owner.rs}` with `read.rs` as the streaming driver; replace `group_chat_key` with `phone::group_chat_id` (F9).

### S7. `src-tauri/src/tool_downloads.rs`: manifest, hashing, HTTP, permissions, shared state, file lock, threading (4/10)

**Evidence.** 1,028 lines (F11 names seven jobs). Manifest I/O `read_manifest` `:257`, `write_manifest` `:266`; hashing `file_sha256` `:342` (a second copy of `message_ir::file_sha256`, `crates/libs/ir/src/lib.rs:938`; the clones audit owns the inventory); HTTP `download_client` `:437`, `download` `:458`; permissions `make_executable` `:537`; state `ToolDownloads` `:570` over `Shared { states: Mutex<...>, changed: Condvar }` `:575-577`; the file lock `lock_check` `:789`; threading `retry` `:844`, `retry_with` `:863`; the driver `download_missing` `:812`. Readers: `commands/tools.rs:17` (8 uses), `commands/extract.rs:43`, `commands/staging.rs:41`, `commands/events.rs:13`, `main.rs:28`, `:63`.

**Why it hurts.** The one testable seam is `retry_with`'s `spawn` parameter (`:863-869`); the HTTP, hashing and manifest steps are not injectable, so a test of "a bad hash is refused" needs a real download or a local server.

**Fix.** Keep `ToolDownloads` (state and waiting) and `Program`/`Pinned` (the pin table) in `tool_downloads.rs`; move `download`, `download_client`, `make_executable` and the unpack step into `tool_downloads/fetch.rs` behind one function the driver calls:

```rust
// src-tauri/src/tool_downloads/fetch.rs
pub(super) trait Fetch { fn fetch(&self, pinned: &Pinned, into: &Path) -> Result<(), DownloadError>; }
pub(super) struct Http(reqwest::blocking::Client);
impl Fetch for Http { /* body of download() :458 */ }
// check(dir, base, pinned, run, lock) takes `&dyn Fetch`; tests pass a fetch that writes fixed bytes.
```

Delete the local `file_sha256` in favour of `message_ir::file_sha256` (or the `fs-util` crate from S4).

### S8. Web: `lib/auth.tsx`, `components/AppLayout.tsx`, `screens/ImportScreen.tsx` (6/10)

**Evidence.** F4 and F14 record these. The jobs at this commit:

- `web/src/lib/auth.tsx` (558 lines): owns the `QueryClient` (`:153-154`); pushes globals into `api.ts` (`useEffect` at `:207`); validates the session with `getSession()` (`:223`); persists to storage (`loadPersisted` `:101`, `persistState` `:127`, `clearPersisted` `:139`); runs the logout and Upload-pause flow (`pauseWithinLimit` `:376`, `logout` `:391-437`); listens for the Tauri window close (`win.onCloseRequested` `:472`); deletes Import Run directories (`deleteRunDirectories` `:528` via `invokeDeleteRunDir` `:21`); renders `LogoutDialog` (`:506`) and `UndeletedDirectories` with `PathList` (`:541-548`).
- `web/src/components/AppLayout.tsx` (453 lines): a second route table `modeFromPathname` (`:45-66`) beside `App.tsx:74-80`, which renders the browse routes with `element={null}`; slug resolution (`:138-149`); URL parameter to search state (`:150-163`); per-mode navigation `handleSearch` (`:195-227`) and `handleSearchChange` (`:229`); checked-contact state (`:115-127`); group and tag loading (`:129-130`).
- `web/src/screens/ImportScreen.tsx` (890 lines): 33 `useState` calls (the form fields at `:203-245`), five `useEffect` blocks (resume check `:271`, profile phones `:464`, `:524`, iMessage path probe `:530`, WhatsApp path probe `:564`), `applyRestoredFormState` `:311`, and the hand-assembled `ImportJobFormValues` at `:800-822` (21 fields) passed with 57 props to `ImportFormFields` (`screens/import/ImportFormFields.tsx:66-132`).

**Why it hurts.** A change to the logout wording and a change to how the `QueryClient` is built are the same file; a new browse route is two tables; a new form field is five places (F4).

**Fix.** F4's `authContext.ts` and form reducer; F14's per-route elements. For `auth.tsx` specifically, pass the UI in:

```tsx
// web/src/lib/auth.tsx: AuthProvider takes the dialog as a render prop, so lib/ renders nothing
export function AuthProvider({ children, renderDialog }: { children: ReactNode; renderDialog: (s: LogoutDialogState, a: DialogActions) => ReactNode }) { ... }
// App.tsx: <AuthProvider renderDialog={(s, a) => <LogoutDialog state={s} {...a} />}>
```

## 3. Open/closed

Can the system be extended without editing existing code? For Rust the compiler turns an exhaustive `match` into a checklist, which is a mitigation, not a closed design. A `_ =>` arm removes even that.

### O1. Adding a backup type is 11 Rust edits and about 8 web edits (7/10)

**Evidence.** The sites a new vendor touches at this commit, counted:

Rust, compiler-checked (exhaustive matches or new items):

1. `Exporter` enum, `crates/core/message-crate-core/src/exporters.rs:23-39`.
2. `Form::to_config` match, `exporters.rs:269-281`.
3. A new `to_<vendor>_config` function beside the seven at `:310`, `:365`, `:425`, `:453`, `:480`, `:505`, `:538`.
4. `SourceConfig` enum, `config.rs:189-205`.
5. A new `<Vendor>Config` struct beside those at `config.rs:210-290`.
6. `use <vendor>_exporter::run as run_<vendor>`, `src-tauri/src/commands/extract.rs:28-34`.
7. `run_exporter` match, `extract.rs:524-534`.
8. The exporter crate in `src-tauri/Cargo.toml:34-40`.

Rust, not compiler-checked (strings and lists):

9. The source-string match in `build_exporter_config`, `extract.rs:431-500` (nine arms: `:432`, `:438`, `:448`, `:453`, `:458`, `:464`, `:468`, `:480`, `:490`; the fallthrough at `:500` is a runtime error).
10. `programs_run_by`, `extract.rs:511-516`, only if the vendor needs an external tool; the `_ => &[]` arm absorbs a vendor that does (see O4).
11. The `source` choice list in `crates/server/server/src/search/fields.rs:157-165`, whose comment at `:154-156` says "A new exporter adds its id here and nothing else."

Web, not compiler-checked:

12. `EXPORT_SOURCES`, `web/src/lib/exportSources.ts:8-16`.
13. `useImportJob.ts:1447-1491` `extractFieldsFor`, an if/else chain over `isImessageMethod`, `isWhatsappMethod`, `form.isAndroidSms`, `IMAZING_SOURCE_ID`.
14. A `lib/<vendor>ExtractFields.ts` module (three exist: `imessageExtractFields.ts`, `sbrExtractFields.ts`, `whatsappExtractFields.ts`).
15. `ImportFormFields.tsx:306-312` predicates (`isIos`, `isImazing`, `isAndroidSms`, `wantsEmails`, `imessageMethod`, `whatsappMethod`) and the conditional sections at `:828` and `:837`.
16. `ImportScreen.tsx` state and probe effects per source (`:530` iMessage, `:564` WhatsApp).
17. `formSnapshot.ts:31-33` secret rules by source-id string literal.
18. `importSource.ts:11` `showsAttachmentOptions`, `:27` `importRunCreateBody`; and `androidSmsSources.ts:9-17`, `:54-75` for an Android-like source.

The Rust and web halves are joined by the source-id strings (`"sms-backup-plus"` and the like) that appear in `extract.rs:458`, `exportSources.ts`, `androidSmsSources.ts:11`, `search/fields.rs:164` and `tauri.ts:409`. Nothing compiles across that boundary.

**Why it hurts.** F3 says core enumerates its consumers; this count says what that costs. A vendor added in Rust and forgotten in `search/fields.rs:157` is a searchable `source:` that the server refuses as a bad word; forgotten in `programs_run_by` it runs without its tool and fails mid-run.

**Fix.** Two parts.

Rust: F3's trait and registry, so sites 1, 2, 4, 7 collapse to one registry entry in the composition root (the desktop host already imports every exporter, `extract.rs:28-34`). Site 9 becomes a lookup over the registry by id; site 11 reads the same list through `api-types`:

```rust
// crates/libs/api-types/src/lib.rs: one list of source ids both the server and the desktop read
pub const SOURCE_IDS: &[&str] = &["imessage", "whatsapp", "sms-backup-restore", "imazing", "openextract", "go-sms-pro", "sms-backup-plus"];
// crates/server/server/src/search/fields.rs:157: values: message_crate_api_types::SOURCE_IDS,
```

Web: one descriptor per source, registered once, so sites 12 to 18 become one new file:

```ts
// web/src/lib/importSources/types.ts
export type ImportSourceDescriptor = {
  id: string;
  label: string;
  methods?: readonly string[];               // "imessage-ios", "whatsapp-android", ...
  showsAttachmentOptions: boolean;
  extractFields(form: ImportJobFormValues): Record<string, unknown>;
  snapshotSecret?(form: ImportJobFormValues): SnapshotSecret | null;
  backupField?: BackupField;
  Fields: React.ComponentType<{ form: ImportJobFormValues; dispatch: Dispatch<FormAction> }>;
};
// web/src/lib/importSources/index.ts
export const IMPORT_SOURCES: readonly ImportSourceDescriptor[] = [apple, whatsapp, smsBackupRestore, goSmsPro, imazing, smsBackupPlus, openextract];
export const sourceFor = (id: string) => IMPORT_SOURCES.find((s) => s.id === id || s.methods?.includes(id));
// useImportJob.ts:1447 becomes: return sourceFor(form.source)?.extractFields(form) ?? {};
// exportSources.ts:8 becomes: export const EXPORT_SOURCES = IMPORT_SOURCES.map(({ id, label }) => ({ id, label }));
```

### O2. Adding a `/v1` route is five edits and is well gated (3/10)

**Evidence.** A handler with `#[utoipa::path]` (97 at this commit) and one `.routes(routes!(...))` line in `crates/server/server/src/openapi.rs:115-230` (92 lines). `declared_query.rs:36` `from_spec` derives the allowed query names from the OpenAPI document, so no edit; `openapi/shared_parts.rs:176` `failures()` infers each route's problem types from its path, method and credential, so no per-route list; `openapi/credential_matrix.rs:281` `operations()` walks the spec, so a new route is tested without being named. On the web, one function in `web/src/lib/serverApi.ts` (89 at this commit) and one key in `web/src/lib/queryKeys.ts`, with `serverApi.types.ts` regenerated and checked by `scripts/check-generated-api-types.sh` (named in `serverApi.ts:9-12`).

**Why it is acceptable.** Each edit is a registration, not a modification of existing logic, and the two spec-derived tables mean the usual third and fourth tables do not exist.

**Fix.** None needed. The one small gap: a handler written but not registered is silently absent. A test that every `#[utoipa::path]` function is reachable would close it, but the credential matrix would not notice the missing route either, so this is cosmetic.

### O3. Adding an `ApiError` variant is 12 edits across two modules and a generated docs directory (4/10)

**Evidence.** `ApiError` enum `crates/server/server/src/server.rs:498-587`; `problem_type()` `:604`; `to_problem()` `:645`; `Display` `:737`; `ProblemType` enum `problem.rs:22-71`; `ALL: [Self; 23]` `:75-99` (the length literal is edited by hand); `slug` `:103`; `status` `:133`; `title` `:162`; `page` `:199`; the `named!` list `:283-299` (opt-in, so a forgotten entry is a missing utoipa response, not a compile error); then `cargo run -p message-crate-server -- dump-error-docs` (`error_docs.rs:5`) regenerates the 24 pages under `docs/src/content/docs/docs/developer/reference/errors/`. `error_docs.rs:7-8` says a drift test fails the build when a page is missing.

**Why it hurts.** Five `match self` tables in `problem.rs` say the same thing five ways; the compiler checks each, but a reader checks consistency between them by eye. The drift test covers the docs; nothing covers `named!`.

**Fix.** One table instead of five:

```rust
// crates/server/server/src/problem.rs
pub struct ProblemSpec { pub slug: &'static str, pub status: StatusCode, pub title: &'static str }
impl ProblemType {
    pub const fn spec(self) -> ProblemSpec {
        match self {
            Self::ValidationFailed => ProblemSpec { slug: "validation-failed", status: StatusCode::UNPROCESSABLE_ENTITY, title: "Validation failed" },
            // one arm per type; slug(), status(), title() become self.spec().slug etc.
        }
    }
}
// ALL: derive with strum::EnumIter, or keep the array and add
const _: () = assert!(ProblemType::ALL.len() == 23, "ALL lists every ProblemType");  // the match above is the real check
```

### O4. Wildcard arms that absorb a new variant where a wrong default is a defect (4/10)

**Evidence.**

- `src-tauri/src/commands/extract.rs:512-515` `programs_run_by`: `SourceConfig::Whatsapp(_) => &[Program::Wtsexporter], _ => &[]`. A new vendor that needs a tool gets none and the Tools Directory check is skipped.
- `crates/server/server/src/db/handles.rs:110-115` `check_service_carries`: `(IdentityService::Whatsapp, IdentityType::Email) => Err(...), _ => Ok(())`. A new service or identity type is carried everywhere by default; the memory rule "service constrains identity type" says the service should say what it carries.
- `crates/libs/reexport/src/lib.rs:91-94`: `SourceConfig::Format(format) => format.run_started, _ => None`. Benign today (reexport only ever sees `Format`), but it is the arm that lets `SourceConfig::Format` live in the exporter enum at all (F16 lists the arm).
- The service match in `impl IrMessage` at `crates/libs/ir/src/lib.rs:556-560` (`IrService::Sms => true, IrService::Unknown => matches!(...), _ => false`), called as `is_sms_or_mms` from `sms-backup-plus-exporter/src/write.rs:69`. A new SMS-like service (`Rcs` is already there and falls to `false`) is silently left out of an SMS Backup+ export.

The `parse` fallthroughs (`IdentityType::parse` `:251` to `Other`, `IrService::parse` `:359` and `IrMessageKind::parse` `:470` to `Unknown`) are intended wire tolerance and are not findings. `crates/libs/export/src/project.rs:330-343` matches `IrService` exhaustively, which is the model to follow.

**Fix.** Spell the arms out where a default is a decision:

```rust
// src-tauri/src/commands/extract.rs:511
fn programs_run_by(source: &SourceConfig) -> &'static [Program] {
    match source {
        SourceConfig::Whatsapp(_) => &[Program::Wtsexporter],
        SourceConfig::GoSmsPro(_) | SourceConfig::SmsBackupRestore(_) | SourceConfig::SmsBackupPlus(_)
        | SourceConfig::OpenExtract(_) | SourceConfig::Imazing(_) | SourceConfig::Apple(_) | SourceConfig::Format(_) => &[],
    }
}
// crates/server/server/src/db/handles.rs:110: list (service, type) pairs the service carries and refuse the rest.
```

With O1's registry, `programs_run_by` becomes a field of the descriptor and the match disappears.

### O5. Source-id string literals as branch conditions in the web form (3/10)

**Evidence.** `ImportFormFields.tsx:306` `props.source === "imessage-ios"`, `:307` `=== IMAZING_SOURCE_ID`, `:828` `isIos || isAndroidSms`, `:837` `isImazing`; `formSnapshot.ts:31-33` `"imessage-ios"`, `"whatsapp-ios"`, `"whatsapp-android"`; `androidSmsSources.ts:54-75` `backupField` switch with a `default` arm that means SMS Backup & Restore.

**Why it hurts.** These are the if/else chains the skill names; each is one more place O1's list grows.

**Fix.** Folded into O1's descriptor (`Fields`, `snapshotSecret`, `backupField` per source).

## 4. Liskov substitution

In Rust terms: does every implementor of a trait honour the contract the trait's doc states, so a caller holding `&dyn Trait` or `impl Trait` behaves the same whichever it was given? In React terms: does every value a prop type admits behave as the component assumes?

### L1. `ProjectionHooks` is honoured five ways and bypassed two ways; one implementor empties the method the dedupe step depends on (6/10)

**Evidence.**

- The trait: `crates/libs/ir/src/projection.rs:85-205`. Its doc at `:80-84` says only `export`, `service` and `source` are required and "every other method has a default matching the common exporter behavior." `attachment_digests` (`:102-110`) is documented as "What tells the message's attachments apart, for its `MessageGuid` and for the dedupe step (`one_copy_per_message`)"; `pending_to_document` (`:239`) feeds it into `MessageCopy` (`:267-276`) and `MessageGuid::new` (`:296`).
- Five production implementors: `SbpProjection` (`sms-backup-plus-exporter/src/emit.rs:246`), `ImazingProjection` (`imazing-exporter/src/emit.rs:895`), `GoSmsProjection` (`go-sms-pro-exporter/src/emit.rs:332`), `WhatsappProjection` (`whatsapp-exporter/src/emit.rs:634`), `OpenExtractProjection` (`openextract-exporter/src/emit.rs:576`), plus three test implementors in `projection.rs` (`:516`, `:558`, `:763`).
- `WhatsappProjection::attachment_digests` returns `Vec::new()` (`:656-658`) with the comment "`key_id` already tells WhatsApp messages apart", and `vendor_key` (`:662-664`) is `trimmed(msg.extra_str("key_id"))`. But `key_id_string` (`:579-585`) returns an empty string when the JSON has no `key_id`, and `parse.rs:110` declares `key_id: Option<Value>`. With no `key_id`, `vendor_key` is `None` and `attachment_digests` is empty, so two WhatsApp messages with the same text in the same millisecond and different media are one copy to `one_copy_per_message`. Whether wtsexporter ever omits `key_id` is **Unable to verify** (section 9).
- `GoSmsProjection::normalize_handle` (`:342-344`) and `SbpProjection::normalize_handle` (`:256-258`) override the default with the default's own body. Not a break; noise that suggests the default was added after the overrides.
- `ImazingProjection::role` (`:904-912`) adds `ProjectedRole::Notification`, within the contract.
- Two exporters do not implement the trait at all. `imessage-ir-exporter/src/convert.rs:707` has its own `pending_to_document` (same name as `message_ir::projection::pending_to_document` `:239`, different signature), builds `ConversationDocument` by hand (`:733`), keeps the database's own `guid` (`:400`) and runs no `one_copy_per_message`. `sms-backup-restore-exporter/src/read.rs:520` `to_document` builds the document by hand (`:558`), with its own `attachment_digests` on its own `PendingMessage` type (`:449-451`), its own `dedupe` (`:457-480`) that calls `one_copy_per_message` (`:476`), and `MessageGuid::new` (`:622`).

**Why it hurts.** "Projection" is one contract honoured three ways, so a fix to the dedupe or GUID rule is made in `projection.rs`, `read.rs` and `convert.rs` and tested three times (F8 lists the copy-paste). The WhatsApp override changes the meaning of a method the pipeline relies on, which is the LSP failure mode: the trait's caller cannot tell.

**Fix.** Make the identity inputs one method with a stated invariant, give it a shared conformance test, and have the two hand-built exporters either implement the trait or say in the trait doc why they cannot:

```rust
// crates/libs/ir/src/projection.rs
/// What the source can tell two messages apart by. Invariant: two messages the source
/// knows to be different must differ here, or the dedupe step keeps one of them.
pub struct MessageIdentityParts { pub attachment_digests: Vec<String>, pub vendor_key: Option<String> }
pub trait ProjectionHooks {
    fn identity_parts(&self, msg: &PendingMessage) -> MessageIdentityParts {
        MessageIdentityParts { attachment_digests: msg.attachments.iter().filter_map(|a| a.digest_sha256.clone()).collect(), vendor_key: None }
    }
    // attachment_digests and vendor_key are removed; pending_to_document reads identity_parts.
}
// crates/libs/ir/src/testutil.rs: one test every implementor runs
pub fn two_messages_that_differ_only_in_media_stay_two<H: ProjectionHooks>(hooks: &H, a: PendingMessage, b: PendingMessage) { ... }
```

For WhatsApp, `identity_parts` returns `vendor_key` when `key_id` is present and falls back to the digests when it is not, which is the behaviour the comment at `:655-658` wants without losing the fallback.

### L2. `MergedArchive::write` returns a file for one implementor and a directory for the other, and `file_names` means two different things (4/10)

**Evidence.** The trait (`crates/libs/ir-format/src/format_sink.rs:21-47`) says `write` returns "the path of what was written" and `file_names` returns "The names of the files `write` creates in the output directory, partial files included", recorded before writing "so the next fresh export into the directory removes them". `SbrArchive::write` (`sms-backup-restore-exporter/src/write.rs:551-570`) returns the `smses.xml` path from `session.finish()` (`:569`) and `file_names` (`:572`) returns `sbr::backup_file_names()`. `SmsBackupPlusArchive::write` (`sms-backup-plus-exporter/src/write.rs:58-95`) returns `output_dir` itself (`:94`) and `file_names` (`:100-102`) returns `Vec::new()`, with a comment that the EML cleaner removes its directories. The one caller, `FormatSink::finish` (`format_sink.rs:200-221`), discards the returned path (`archive.write(&self.output_dir, &self.docs, report)?;` at `:208`) and records `file_names` at `:207`.

**Why it hurts.** A second caller that used the return value to, say, log "wrote `X`" would log a directory for one archive and a file for the other. The sentinel records nothing for SMS Backup+, so correctness rests on the EML cleaner's naming agreeing with this archive's, which is a contract between two crates nothing states.

**Fix.**

```rust
// crates/libs/ir-format/src/format_sink.rs
pub enum ArchiveOutput { File(String), Directory(String) }
pub trait MergedArchive: std::fmt::Debug + Send {
    fn write(&self, output_dir: &Path, documents: &[ConversationDocument], report: &mut ExportReport) -> Result<()>;
    /// Every output `write` creates under `output_dir`, partial ones included; the sink records them before writing.
    fn outputs(&self, documents: &[ConversationDocument]) -> Vec<ArchiveOutput>;
    fn format_name(&self) -> &'static str;
}
```

`record_archive_files` then removes directories as well as files, and SMS Backup+ lists its conversation directories.

### L3. `From<X> for ApiError`: the typed impls preserve their inputs; the `anyhow` funnel turns a refusal that travelled through `?` into a 500 (4/10)

**Evidence.** Fourteen impls. Seven in `server.rs:812-899`, seven beside their error types (`identity_country.rs:73`, `search/error.rs:60`, `accounts_api.rs:586`, `db/saved_searches.rs:66`, `contacts_api/edit.rs:57`, `db/named_membership.rs:38`, `contacts_api/address_book.rs:82`). Each typed impl maps every variant to a status-bearing `ApiError` and keeps the detail: `ImportLookupError::NotFound { import_id }` becomes `NotFound` with the id (`:812-823`); `AssetError::Mismatch | Invalid` become `AssetUploadInvalid` with `to_string()` (`:841-857`); `QueryError` keeps `word` and `did_you_mean` (`search/error.rs:60-68`); `CountryError::Exists { detail, holder }` keeps `holder` (`identity_country.rs:73-82`). Two blanket impls, `From<sqlx::Error>` (`:889`) and `From<anyhow::Error>` (`:895`), make `Internal`. `contacts_api/edit.rs:49-50` states the consequence as a rule: "Anything a helper hands back through `?` is a failure, not a refusal." `From<LoadError>` alone adds `.context("load address book")` (`address_book.rs:86`); the other impls add no context, so the 500 log line names the step for one route and not the others.

Two `db/` modules decide an HTTP kind: `SavedSearchError::Conflict => NameTaken` (`db/saved_searches.rs:71`) and `MembershipError::Conflict => NameTaken` (`db/named_membership.rs:43`). That is F2.

**Why it hurts.** A `db/` function that returns `anyhow::Result` and `bail!`s on a missing row answers 500 wherever a handler uses `?`. Whether any does is **Unable to verify** (section 9); the rule at `edit.rs:49` says the team has accepted the risk by convention rather than by type.

**Fix.** F2's `DbError` with one `From<DbError> for ApiError`, after which `From<anyhow::Error> for ApiError` can be narrowed to the handler modules that still need it. Add the context the one impl has to the rest:

```rust
// crates/server/server/src/server.rs:895
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self { Self::Internal(e) }   // keep
}
// and in each typed impl's Internal arm: Self::Internal(cause.context("<step>")) as address_book.rs:86 does.
```

### L4. Three `ProgressEvent` enums and two `ProgressFn` aliases that differ in `Send` (5/10)

**Evidence.** F6. At this commit: `crates/core/message-crate-core/src/progress.rs:23`, `crates/libs/import/src/progress.rs:45`, `crates/libs/export/src/run.rs:92`; `import/progress.rs:94` `dyn FnMut(ProgressEvent) + Send + 'a`, `export/run.rs:114` `dyn FnMut(ProgressEvent) + 'a`. A callback written for Export cannot be handed to Import, and a caller that wants to run Export on a worker thread must add `Send` itself.

**Fix.** F6's: one `ProgressEvent` in core with `Import(..)`/`Export(..)` variants and one `ProgressFn<'a> = dyn FnMut(ProgressEvent) + Send + 'a`.

### L5. Sink contracts are documented and asymmetric; one substitution changes what a test sees (2/10)

**Evidence.** `ProgressSink::new` (`core/progress.rs:158-164`) paces and so drops events except stage boundaries (`emit_at` `:191-205`, returns `false` when held back); `ProgressSink::unpaced` (`:167-174`) delivers every event. `emit_progress(None, ..)` returns `true` ("every event is due", `:214-220`), while `emit_log(None, ..)` prints to stderr (`process.rs:71-79`). `LogSink::warn` with no warning callback sends the warning to the line callback (`:56-61`). Each is stated in its doc; a test that swaps `unpaced` for `new` sees fewer events and may conclude a stage never reported.

**Fix.** None required beyond the docs that exist. A `#[cfg(test)]`-only `ProgressSink::unpaced` would make the swap impossible in production code, if that is wanted.

## 5. Interface segregation

Does a client depend on more than it uses?

### I1. `AppState` goes whole to 91 handlers; most read `db` alone (6/10)

**Evidence.** `crates/server/server/src/server.rs:385-423`: ten fields (`cfg`, `db`, `account_import_locks`, `asset_complete_locks`, `auth_rate_limits`, `asset_part_size`, `demo_build`, `demo_bundle_generator`, `media_link_key`, `media_queue`). `State(state)` appears at 91 sites in 19 non-test files. Field reads outside tests: `db` 99, `cfg` 29, `demo_build` 8, `auth_rate_limits` 5, `media_queue` 3, `account_import_locks` 2, `media_link_key` 2, `asset_part_size` 1, `demo_bundle_generator` 1, `asset_complete_locks` 1 (`assets_api.rs:1340`). The struct names `crate::server_api::DemoBuild`, `crate::reset_demo::BundleGenerator`, `crate::assets_api::media_links::MediaLinkKey`, `crate::media_queue::MediaQueue`, `crate::credentials::AuthRateLimits` (`:402-422`), which is why `server.rs` is the cycle hub (F1).

**Why it hurts.** A handler for contacts compiles against the demo build and the media queue; a test that needs `AppState` must build all ten (hence `test_support.rs` and the second `test_state()` at `server/tests.rs:142`, F10).

**Fix.** axum's `FromRef` lets a handler ask for the slice it uses, with no change to the router:

```rust
// crates/server/server/src/server.rs
#[derive(Clone, axum::extract::FromRef)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: Db,                 // #[derive(Clone)] pub struct Db(pub sqlx::SqlitePool);
    pub(crate) imports: ImportState,   // account_import_locks
    pub(crate) assets: AssetState,     // asset_complete_locks, asset_part_size, media_link_key
    pub(crate) auth: AuthState,        // auth_rate_limits
    pub(crate) demo: DemoState,        // demo_build, demo_bundle_generator
    pub(crate) media_queue: MediaQueue,
}
// a contacts handler: async fn get_contact(State(db): State<Db>, ...) and tests build Db alone.
```

### I2. `ExporterConfig` carries every vendor's config to every exporter and every sink to every lower crate (5/10)

**Evidence.** `crates/core/message-crate-core/src/config.rs:74-118`: thirteen fields, among them three sinks (`log`, `progress`, `issues`), `cancel`, `media`, `obfuscate`, `timezone`, `output_format`, `resume`, and `source: SourceConfig` with eight variants (`:189-205`). Each exporter opens with a `let SourceConfig::X(source) = &config.source else { bail!(...) }` guard: `go-sms-pro-exporter/src/run.rs:14-15`, `imazing-exporter/src/run.rs:14-15`, `imessage-ir-exporter/src/run.rs:221-222`, `openextract-exporter/src/run.rs:14-15`, `whatsapp-exporter/src/run.rs:28-29`, `sms-backup-plus-exporter/src/run.rs:19-20`, `sms-backup-restore-exporter/src/run.rs:15-16`. The lower crates take only the plumbing: `staging/src/export_writer.rs:12`, `counted_attachments.rs:8`, `spool.rs:24`, `write_queue.rs:35`, `transcode.rs:98`, `headroom.rs:15`; `ir-format/src/format_sink.rs:11`, `export_transforms.rs:7`, `read_mail.rs:5`, `write.rs:6`; `ios-backup/src/identity.rs:18`, `backup_domain.rs:13`, `helper.rs:27` (`LogSink`, `ProgressSink`, `ProgressEvent`, `ScratchDir`, `OutputFormat`, `ExportReport`, `ExportTransforms`, `discover_files`, `emit_log`, `emit_progress`).

**Why it hurts.** Seven copies of a runtime check for a mistake the type system could refuse; and `ios-backup`, which has no output, links the output format and the form.

**Fix.** Split by who reads what; the exporter signature then names its own config:

```rust
// crates/core/message-crate-core/src/config.rs
pub struct RunContext { pub scratch_dir: PathBuf, pub cancel: Option<CancelFlag>, pub log: Option<LogSink>, pub progress: Option<ProgressSink>, pub issues: Option<IssueSink> }
pub struct OutputConfig { pub output: PathBuf, pub output_format: OutputFormat, pub resume: bool, pub media: MediaConfig, pub obfuscate: ObfuscateConfig, pub timezone: Option<String> }
// whatsapp-exporter/src/run.rs
pub fn run(ctx: &RunContext, out: &OutputConfig, inputs: &[PathBuf], source: &WhatsappConfig) -> Result<RunResult>
// The let-else guards at the seven run.rs sites disappear; the registry from O1/F3 does the dispatch.
```

### I3. `ImportFormFieldsProps` is 57 props for a 21-field form (5/10)

**Evidence.** `web/src/screens/import/ImportFormFields.tsx:66-132`: 57 members, mostly `value` and `onXChange` pairs, plus readiness inputs (`pathStats`, `whatsappStats`, `profilePhones`, `profilePhonesReady`, `profilePhonesError`) and UI toggles (`formatOpen`, `processingOpen`, `showBackupPassword`, `showWhatsappKey`). `ImportJobFormValues` has 21 fields (`useImportJob.ts:255`, counted); `ImportScreen.tsx` holds them as 33 `useState` calls (`:203-245`). `ImportFormFields.test.tsx:54` builds all 57 to render the component. F4 records the form held three ways.

**Why it hurts.** Every source's fields are the concern of every caller; a test of the iMazing time-zone field supplies WhatsApp paths.

**Fix.** F4's reducer, with the props grouped by source so a section depends on its slice:

```ts
// web/src/screens/import/ImportFormFields.tsx
export type ImportFormFieldsProps = {
  form: ImportJobFormValues;
  dispatch: Dispatch<FormAction>;
  readiness: { imessage: ImessagePathStats; whatsapp: WhatsappPathStats; profilePhones: ProfilePhonesState };
  ui: { formatOpen: boolean; processingOpen: boolean; showBackupPassword: boolean; showWhatsappKey: boolean; toggle(key: keyof ImportFormFieldsProps["ui"]): void };
  phoneCountries: readonly PhoneCountryChoice[];
  running: boolean;
  onImport(ownerPhones?: string[]): void;
};
// Each source section (AppleFields, WhatsappFields, AndroidSmsFields, ImazingFields) takes { form, dispatch } and its own readiness slice.
```

### I4. `web/src/lib/tauri.ts`: 61 exports, 28 `invoke` calls, 24 importers, 12 tests that mock it (4/10)

**Evidence.** 796 lines; `export` appears 61 times and `invoke(` 28 times. Importers outside tests: 24 files. `useImportJob.ts:31-56` imports 25 names; `screens/settings/SystemSection.tsx` imports 6; `screens/ExportScreen.tsx` 5; `lib/auth.tsx:21` one (`invokeDeleteRunDir`); `lib/types.ts:2` one type (`ToolName`), which is the `types ↔ tauri` type-only cycle the design audit lists. Twelve test files `vi.mock` the module (for example `screens/import/useImportJob.test.tsx:75`, `screens/ExportScreen.test.tsx`, `screens/settings/SystemSection.test.tsx`, `lib/downloadAttachment.test.ts`).

**Why it hurts.** A mock of "the desktop" is a mock of 61 names; a change to the export commands recompiles the import hook's tests.

**Fix.** Split by job and end the type cycle:

```
web/src/lib/desktop/extract.ts    invokeExtract, invokeCancel, onExtractEvents, awaitTauriJob, parseTauriJobResult
web/src/lib/desktop/runDir.ts     invokeCreateRunDir, invokeDeleteRunDir, invokeStagingRoot, invokeSetStagingRoot, RunDirConfig
web/src/lib/desktop/runLog.ts     invokeStartImportRunLog, invokeListImportRunLogs, invokeReadImportRunLog*, RunLogEntry, RunLogLevel
web/src/lib/desktop/staging.ts    invokeSummarizeStaging, invokeTranscodeStaging, StagingSummary, AttachmentForecast, SizeVerdict
web/src/lib/desktop/upload.ts     invokeUpload, UploadConfig, UploadFinishedReport
web/src/lib/desktop/export.ts     invokeExport, invokeFormat, EXPORT_FORMATS, ExportFormat, ExportDir, invoke*ExportDir
web/src/lib/desktop/tools.ts      invokeToolsStatus, invokeRetryToolDownloads, ToolsStatus, ToolStatus, toolsDownloading, ffmpegMissing, toolUsable
web/src/lib/desktop/fs.ts         invokeHomeDir, invokePathStat, invokeSaveDownload, invokeIosBackupEncrypted, invokeImessageBackupIdentities
```

Move `ToolName` and `MediaToolName` into `lib/types.ts` so `types.ts` no longer imports `tauri.ts`.

### I5. `test_support.rs`: 58 helpers, 53 users, one module (3/10)

**Evidence.** `crates/server/server/src/test_support.rs` (1,331 lines) is used by 53 files. Its 58 `pub fn`s fall into: fixtures (`test_fixture`, `fixture_with_account`, `demo_account`, `account_with_id`, `turn_off_delete`); auth flows (`register_via_api`, `claim_as_owner`, `log_in`, `login_status`, `demo_account_session`); 28 HTTP verb helpers (`get_json`, `post_json`, `put_status`, `patch_raw`, `delete_json_with_body` and the rest); problem assertions (`problem`, `expect_problem`, `patch_failure`); raw SQL seeding (`SeedConversation` `:783`, `MessageRow` `:827`, `insert`, `insert_in`, `try_insert_in`, `seed_conversation`, `seed_one_message`, `attach_stored_file`); import helpers (`import_jsonl_text`, `import_ada_conversation`); log helpers (`small_log_files`, `write_log_line`, `log_event`). A second fixture `test_state()` lives at `server/tests.rs:142`. F10.

**Fix.** `test_support/{fixture.rs, http.rs, seed.rs, imports.rs, logs.rs}` with `pub use` in `test_support.rs`, and fold `test_state` into `TestFixture`.

### I6. `message-crate-api-types` is one crate for every wire type; the import client uses one enum (2/10)

**Evidence.** `crates/libs/api-types/src/lib.rs` (1,043 lines) defines `AppKind` `:47`, `ImportMode` `:81`, `RunIssueKind` `:146`, `ExportQueryList` `:251`, `ExportScope` `:284`, `Page<T>` `:317`, `ExportStatus` `:333`, the export page types, `TimePrecision` `:556`, `Deletion` `:586`, `Problem` `:898`, `IdentityHolder` `:948`. `message-crate-import` uses only `ImportMode` (`import/src/lib.rs:27`, `run.rs:37`, `prepare.rs:10`, `http.rs:8`, `report.rs:8`); `message-crate-core` only `RunIssueKind` (`core/src/pipeline.rs:15`); `message-crate-http` only `Problem`, `AppKind` and the two header constants (`http/src/lib.rs:57-62`, `response.rs:13`, `retry.rs:8`); `message-crate-export` the export page types (`export/src/lib.rs:15`, `project.rs:18`). The crate doc (`:1-19`) gives the reason for one definition, which stands.

**Why it is minor.** The cost is compile time and a dependency arrow, not behaviour. Keep one crate; gate the sections:

```toml
# crates/libs/api-types/Cargo.toml
[features]
export = []     # ExportQueryList, ExportScope, Page, ExportStatus, Message, Attachment, Tapback, EarlierVersion, Deletion, TimePrecision
# core, import and http depend on api-types without features; export and server turn `export` on.
```

## 6. Dependency inversion

Does the high-level policy depend on an abstraction, or on the low-level detail?

### D1. The persistence layer depends on the HTTP error type and on handler modules (7/10)

**Evidence.** F2. At this commit: `use crate::server::ApiError` in `db/conversations.rs:16` (7 uses), `db/exports.rs:19` (2), `db/conversation_messages.rs:31` (16, with `ApiError::validation` constructed at `:392`, `:402`, `:964`), `db/contacts/read.rs:16` (5); `impl From<…> for crate::server::ApiError` in `db/saved_searches.rs:66` and `db/named_membership.rs:38`; `db/identity_country.rs:22` imports `crate::dedupe::HAS_CONTENT_KEY_SQL`; `db/session_tokens.rs:38` calls `crate::assets_api::sha256_hex`.

**Why it hurts.** The arrow points from `db/` to the router module and to two handler groups, which is the inversion of the layering `docs/architecture/http-api.md:882-893` describes, and it is the reason the CLI import path and the Demo Account build (which share `import_jsonl_files_on_conn`, S1) speak HTTP errors.

**Fix.** F2's `DbError` with one `From<DbError> for ApiError` in the error module; `sha256.rs` for the hash helper; `HAS_CONTENT_KEY_SQL` into `db/dedupe.rs`.

### D2. Three pipeline crates depend upward on `message-crate-core` for plumbing (6/10)

**Evidence.** F3. `message-staging`, `message-ir-format` and `ios-backup` import from core at the fourteen sites listed under I2. What they want is the run plumbing: `LogSink`, `ProgressSink`, `ProgressEvent`, `IssueSink`, `CancelFlag`, `ScratchDir`, `ATTACHMENT_SPOOL_DIRECTORY`, `OutputFormat`, `ExportReport`, `ExportTransforms`, `discover_files`. What they get is a crate whose module doc at `exporters.rs:1-4` says "Backup-type forms, dropdown labels, and validation used by the desktop app", which depends on `message-crate-api-types`, `message-crate-log-lines`, `media`, `obfuscate`, `phone` and optionally `message-csv` (`core/Cargo.toml:11-17`). ADR 0012 (`docs/adr/0012-four-crates-in-the-export-pipeline.md:12-14`) accepts the form in core on purpose.

**Why it hurts.** `ios-backup`, whose job is reading an iPhone backup, recompiles when a dropdown label changes; the sinks that are the pipeline's one abstraction live in the crate with the most policy.

**Fix.** F3 step two, stated as crate layout:

```toml
# crates/libs/run/Cargo.toml (new): message-crate-run
# holds process.rs (LogSink, CancelFlag, check_cancel, parallel_for_each), progress.rs, pipeline.rs (IssueSink, RunIssue, ExportReport),
# scratch.rs, counter.rs, transforms.rs (ExportTransforms), attachment_jobs.rs, discover_files, OutputFormat.
[dependencies]
anyhow = { workspace = true }
message-crate-api-types = { path = "../api-types" }   # RunIssueKind only
media = { path = "../media" }                          # MediaConfig

# crates/core/message-crate-core keeps config.rs (ExporterConfig, SourceConfig), exporters.rs (Form), run_log.rs, run.rs,
# and `pub use message_crate_run::*;` so the exporters and src-tauri change nothing in step one.
# staging, ir-format, ios-backup swap `message-crate-core` for `message-crate-run` in their Cargo.toml.
```

This needs an ADR 0012 amendment, which F3 already says.

### D3. Leaf crates depend on `message-ir` for one or two helpers; `message-ir` depends on a transport crate (4/10)

**Evidence.** F12. `crates/libs/phone/src/lib.rs:20 use message_ir::IdentityType` (54 arms use it in that file); `crates/libs/media/src/lib.rs:280 message_ir::file_sha256` and `media/src/process.rs:736`, `:743 message_ir::rename_into_place`; `crates/libs/csv/Cargo.toml:13` and `csv/src/lib.rs:18`, `:66`, `zone.rs:31` for `AttachmentMeta`, `format_local_ts`, `trimmed`; `crates/libs/ir/Cargo.toml:14-18` on `imessage-reader-protocol` for the five re-exports at `lib.rs:53-88`. `src-tauri/src/tool_downloads.rs:342` re-implements `file_sha256` rather than depend on the model crate for it.

**Why it hurts.** A phone-number crate depends on the conversation model for a four-variant enum; the model crate depends on a crate named after one source's transport. The arrows read backwards.

**Fix.** F12's: `fs-util` for `durable.rs` and `file_sha256`; `IdentityType` defined in `phone` and re-exported by `message-ir`; the five shared types in `message-ir-shared-types`.

### D4. ffmpeg and ffprobe are `std::process::Command` behind a process-global location, with no port (5/10)

**Evidence.** `crates/libs/media/src/tools.rs`: `Command::new(bin)` for the `-version` probe at `:237`, `Command::new(ffmpeg)` at `:424`, `Command::new(ffprobe)` in `ffprobe_command` at `:526-530`; the location is process state, `set_tools_dir` `:98`, `tools_dir` `:105`. No trait names "a thing that probes and transcodes". The consequence in tests: `media/src/testutil.rs:1-13` says "Every test in the workspace that needs the real tools gates on `real_ffmpeg_test_guard`" (56 uses), which returns `None` without ffmpeg so the test returns early (`process/tests.rs:47-49`, `:89-91`), and panics when `CI` is set (`testutil.rs:91-96`); CI installs ffmpeg for that reason (`.github/workflows/ci.yml:161-174`). Because the location is global, tests that change it hold a write lock and every real-ffmpeg test a read lock (`testutil.rs:9-13`, `tools_test_lock` `:56`).

**Why it hurts.** The decision logic (which files to re-encode, size floors, `skip_efficient`) is testable only beside a binary, and the whole media test suite serialises on one lock.

**Fix.** A port for the two programs, with the real one built from today's `resolve_tools`:

```rust
// crates/libs/media/src/tools.rs
pub trait MediaTools: Send + Sync {
    fn probe(&self, path: &Path) -> Result<Probe>;
    fn transcode(&self, job: &TranscodeJob) -> Result<TranscodeOutcome>;
    fn versions(&self) -> Result<Versions>;
}
pub struct Ffmpeg { ffmpeg: PathBuf, ffprobe: PathBuf }     // built once from resolve_tools(); Command::new lives only here
impl MediaTools for Ffmpeg { /* bodies of :424 and :530 */ }
// MediaConfig gains `tools: Arc<dyn MediaTools>`; process.rs and estimate.rs call tools.probe()/transcode().
// media/src/testutil.rs: pub struct FakeTools(pub HashMap<PathBuf, Probe>); impl MediaTools for FakeTools { ... }
```

The real-ffmpeg tests stay for `Ffmpeg` itself; everything above it runs without the binary and without the lock.

### D5. `message-crate-http` exposes `reqwest` types in its public surface (3/10)

**Evidence.** `crates/libs/http/src/lib.rs:83 build_client() -> Result<reqwest::blocking::Client>`; `session.rs:48-62` `server_request` and `request_url` return `reqwest::blocking::RequestBuilder`; `import/src/http.rs:18 use reqwest::Method`, `:87 reqwest::StatusCode`, `:111 reqwest::Url`; `export/src/http.rs:18`, `:184`. The three crates test against `httpmock` (`import/Cargo.toml:25`, `export/Cargo.toml:26`, `http/Cargo.toml:17`), a real socket per test.

**Why it is minor.** The blocking client fits the desktop's worker-thread model and `httpmock` tests are honest. The cost is that `message-crate-import` and `message-crate-export` cannot be tested against a scripted transport, and a change of HTTP client is a change in three crates.

**Fix.**

```rust
// crates/libs/http/src/lib.rs
pub struct Request { pub method: Method, pub url: Url, pub bearer: Option<String>, pub body: Body }
pub struct Response { pub status: u16, pub headers: HeaderMap, pub body: Vec<u8> }
pub trait Transport: Send + Sync { fn send(&self, req: Request) -> Result<Response, TransportError>; }
pub struct ReqwestTransport(reqwest::blocking::Client);
impl Transport for ReqwestTransport { ... }
// HttpSession holds Box<dyn Transport>; import and export never name reqwest.
```

### D6. `useImportJob.ts` depends on seven concrete modules; its test mocks all seven (6/10)

**Evidence.** `web/src/screens/import/useImportJob.ts` imports 25 names from `../../lib/tauri` (`:31-56`), `createImport`, `completeImport`, `getServerState` from `../../lib/serverApi` (`:29`), `useAuth` (`:11`), `getAccountId`, `getBaseUrl`, `getToken` from `../../lib/api` (`:9`), `useFetchAccountProfile` (`:68`), `isTauri` (`:57`), and the run store. Its state is module-level: `let scratch` `:472`, `let work` `:489`, `let recordWrites` `:633`, `let queuedRecordWrite` `:636` (F4). `useImportJob.test.tsx` declares 21 `vi.fn()` stubs (`:38-68`) and `vi.mock`s seven modules: `../../lib/tauri` `:75`, `../../lib/useAccountProfile` `:100`, `../../lib/api` `:110`, `../../lib/serverApi` `:124`, `../../lib/auth` `:136`, `../../lib/tauri-check` `:140`, `../../lib/importRun` `:144`.

**Why it hurts.** Module mocking is the symptom of a missing port: the test is coupled to import paths, the stubs must mirror 61 exports (I4), and two mounted hooks share one `scratch`.

**Fix.** F4's service factory with ports, so the hook is a thin adapter and the test passes fakes:

```ts
// web/src/screens/import/importRunService.ts
export type DesktopPort = Pick<typeof import("../../lib/desktop/extract"), "invokeExtract" | "onExtractEvents" | "awaitTauriJob"> & /* runDir, staging, upload, tools, fs slices */;
export type ServerPort = { createImport: typeof createImport; completeImport: typeof completeImport; getServerState: typeof getServerState };
export function createImportRunService(ports: { desktop: DesktopPort; server: ServerPort; store: ImportRunStore; now: () => number }) {
  let scratch = freshScratch(); let work: Promise<void> | null = null; /* the module-level lets become closure state */
  return { start, resume, cancel, discard, ... };
}
// useImportJob.ts: const service = useMemo(() => createImportRunService({ desktop, server: serverApi, store: importRunStore, now: Date.now }), []);
// useImportJob.test.tsx: createImportRunService({ desktop: fakeDesktop, server: fakeServer, store, now }) and no vi.mock.
```

### D7. Web `lib/` imports `components/`; `components/` import `screens/` (3/10)

**Evidence.** F14. `web/src/lib/auth.tsx:12-13` (`LogoutDialog`, `PathList`); `lib/conversationSort.ts:1` and `lib/messageSearchSort.ts:1` (`SortOrder` from `components/SortMenu`); `lib/namedSetCopy.ts:1` (`NavEntityCopy` from `components/NavEntityList`); `components/AppLayout.tsx:22-23` (`ContactList`, `ConversationList` from `screens/`).

**Fix.** F14's: move `SortOrder` and `NavEntityCopy` into `lib/`, pass the logout UI into `AuthProvider` (S8), give each browse route its own element.

## 7. Findings

| ID | Importance | Principle | Finding |
|---|---|---|---|
| O1 | 7 | O | A new backup type is 11 Rust edits and about 8 web edits joined by unchecked source-id strings |
| S1 | 7 | S | `imports_api/mod.rs` holds DTOs, orchestration, twelve handlers and run control |
| D1 | 7 | D | `db/` depends on `crate::server::ApiError` and on two handler modules (F2) |
| L1 | 6 | L | `ProjectionHooks` honoured by five exporters, bypassed by two; WhatsApp empties `attachment_digests` |
| I1 | 6 | I | `AppState` passed whole to 91 handlers; most read `db` alone |
| D2 | 6 | D | `staging`, `ir-format`, `ios-backup` depend upward on `message-crate-core` for sinks (F3) |
| D6 | 6 | D | `useImportJob.ts` depends on seven concrete modules; its test mocks all seven (F4) |
| S2 | 6 | S | `reset_demo.rs` is a build, a verifier, a file swap and a console (F13) |
| S8 | 6 | S | `auth.tsx`, `AppLayout.tsx`, `ImportScreen.tsx` each carry four to seven jobs (F4, F14) |
| S4 | 5 | S | `message-ir` carries file I/O, time formatting, naming policy and a projection pipeline (F12) |
| S6 | 5 | S | `sbr/src/read.rs` is tokeniser, decoder, address model, keying and owner inference |
| L4 | 5 | L | Three `ProgressEvent` enums; `ProgressFn` aliases differ in `Send` (F6) |
| I2 | 5 | I | `ExporterConfig` carries every vendor's config; seven `let-else` guards refuse the wrong one |
| I3 | 5 | I | `ImportFormFieldsProps` is 57 props for a 21-field form |
| D4 | 5 | D | ffmpeg behind `Command::new` and a process-global location; 56 tests gate on the binary |
| S3 | 4 | S | `models.rs` mixes the import's row shapes with the time text that search uses |
| S7 | 4 | S | `tool_downloads.rs` mixes manifest, hashing, HTTP, permissions, state, lock and threads (F11) |
| O3 | 4 | O | A new `ApiError` variant is 12 edits; five `match self` tables in `problem.rs` |
| O4 | 4 | O | `_ =>` arms absorb new variants in `programs_run_by`, `check_service_carries`, `is_sms_or_mms` |
| L2 | 4 | L | `MergedArchive::write` returns a file or a directory; `file_names` means two things |
| L3 | 4 | L | `From<anyhow::Error> for ApiError` turns a refusal that travelled through `?` into a 500 |
| I4 | 4 | I | `lib/tauri.ts`: 61 exports, 24 importers, 12 tests mock it |
| D3 | 4 | D | `phone`, `media`, `csv` depend on `message-ir` for one or two helpers; `ir` on a transport crate (F12) |
| O2 | 3 | O | A new `/v1` route is five registrations; two tables are spec-derived |
| O5 | 3 | O | Source-id string literals as branch conditions in the web form |
| S5 | 3 | S | `message-csv` holds a time-zone model one exporter uses |
| I5 | 3 | I | `test_support.rs`: 58 helpers, 53 users, one module (F10) |
| D5 | 3 | D | `message-crate-http` exposes `reqwest` types; import and export name them |
| D7 | 3 | D | Web `lib/` imports `components/`; `components/` import `screens/` (F14) |
| L5 | 2 | L | Sink contracts are documented and asymmetric; `unpaced` vs `new` changes what a test sees |
| I6 | 2 | I | `api-types` is one crate for every wire type; the import client uses one enum |

## 8. What is done well

- The sinks are the right abstraction at the right level: `LogSink` (`core/src/process.rs:22`), `ProgressSink` (`core/src/progress.rs:150`) and `IssueSink` (`core/src/pipeline.rs:155`) are closures, so no pipeline crate links Tauri, and `ExporterConfig::emit_log`/`emit_progress`/`emit_issue` (`config.rs:122-136`) give every exporter one way to report.
- `MergedArchive` is a real port: `format_sink.rs:17-20` says "this crate knows no archive format by name", and the two vendor crates supply their own implementor. L2 is about the contract's wording, not its existence.
- Two tables that would usually be hand-kept are derived: `declared_query.rs:36 from_spec` reads allowed query names from the OpenAPI document, and `openapi/shared_parts.rs:176 failures()` infers each route's problem types from rules. `credential_matrix.rs:281` tests every operation the spec lists.
- `error_docs.rs` ships the error pages with the registry and has a drift test; `api-types` makes the compiler the check between server and client (`api-types/src/lib.rs:17-19`); `serverApi.ts:1-12` is one function per route over generated types, pinned by CI.
- `real_ffmpeg_test_guard` (`media/src/testutil.rs:83`) is one rule for a missing ffmpeg, and CI turns a skip into a failure (`testutil.rs:91-96`).
- `src-tauri/src/local_server.rs:332 step()` is a pure state machine with effects separate; `export/src/project.rs:330-343` matches `IrService` exhaustively; the GPL boundary is a process boundary enforced by `cargo deny` (ADR 0014).

## 9. Unable to verify

| Claim | Proving file(s) |
|---|---|
| Whether wtsexporter output ever omits `key_id`, which decides how often L1's WhatsApp collapse can happen | wtsexporter's JSON schema; `crates/exporters/whatsapp-exporter/src/parse.rs:110` declares `key_id: Option<Value>`, so the parser allows it |
| Whether any `db/` function returns a not-found or a refusal as `anyhow::Error` and so answers 500 through `From<anyhow::Error> for ApiError` (L3) | each `crates/server/server/src/db/*.rs` function returning `anyhow::Result` that a handler reaches with `?` |
| How many of the 91 `State(state)` handlers read only `state.db` (I1 gives field counts, not per-handler sets) | the 19 handler files that bind `State(state)` |
| Whether `src-tauri` depends on `message-ir` directly, which decides whether `tool_downloads::file_sha256` (S7, D3) can simply call `message_ir::file_sha256` | `src-tauri/Cargo.toml` (the exporter lines at `:34-40` were read; a direct `message-ir` line was not looked for) |
| Whether `ImportFormFields.test.tsx` exercises each source section or only renders with all 57 props (I3) | `web/src/screens/import/ImportFormFields.test.tsx` beyond line 54 |

## 10. Suggested order of work

1. D1 then I1 (F2 and F1's split): a `DbError` and a `FromRef` state remove the HTTP type from `db/` and the whole-state coupling, and are mechanical.
2. O1 step one with D2 (F3): the exporter trait and registry in `src-tauri/src/commands/extract.rs`, the `SOURCE_IDS` constant, and the `message-crate-run` crate with an ADR 0012 amendment. This is the largest closed-design win.
3. L1: `identity_parts` with the shared conformance test, then bring `sms-backup-restore-exporter` onto the trait (its `dedupe` already calls `one_copy_per_message`); `imessage-ir-exporter` follows or documents why not.
4. D6 and I3 and I4 together (F4): `importRunService` with ports, the form reducer, and the `lib/desktop/` split; the seven `vi.mock`s go with them.
5. O1 web half (the source descriptors), which O5 and the rest of I3 fall out of.
6. L4 (F6), O4, L2, S3: each is small and each removes a divergence.
7. S1, S2, S6, S7, D4, D5, S4, S5, I5, I6, D3, D7, O3, O2, L5 as time allows; none blocks the others.
