# Design pattern implementation audit

Date: 2026-10-10. Commit audited: `530dc1791` (branch `docs/initial-software-design-analysis`).
Scope: the whole repository except `web-next/`, `vendor/`, `target/`, `node_modules/` and `docs/dist`.
Method: read-only. `grep`, `sed -n` and `ls` over the server crate, the pipeline crates, the exporters, the desktop host and the web app; no build, test or lint was run. Every claim cites a `path:line` that was read at this commit. A claim that could not be confirmed is marked **Unable to verify** with the file that would prove it.

This audit is the pattern-level companion to `audits/2026-10-09-initial-software-design-analysis.md`. That report covers layering, God modules, cycles and modularity; where a finding here overlaps one of its findings, the earlier one is cited by its ID (F1 to F19) rather than repeated. The question here is narrower: which design patterns the code uses, where each is applied well, where one is misapplied or hand-rolled inconsistently, and where a missing pattern would remove duplication or coupling.

Assumptions: the ADRs in `docs/adr/` state deliberate decisions, so a structure an ADR prescribes is not a finding, only its implementation is. "No backwards compatibility, anywhere" (`CLAUDE.md`) means a remediation may change any interface; none below adds a compatibility layer.

Every path is relative to the repository root.

## 1. Summary

The codebase uses a small set of patterns and uses most of them well. The two best examples are a Template Method with hook defaults (`message_ir::ProjectionHooks`, implemented by five exporters) and a pure state machine whose effects are listed, not performed (`local_server.rs` `step`). Strategy appears as a trait object where a vendor format must be supplied from above (`MergedArchive`), as a data table where two tables differ only in names (`MembershipSpec`), and as a plain `match` on an enum where the variants are closed (`OutputFormat`). Axum's extractor trait is used as a newtype guard (`auth_guard!`), so a route cannot compile without its capability check. The search language is a textbook Interpreter: lexer, parser to an `Expr` tree, a registry, and an emitter that walks the tree into a hand-rolled SQL builder.

The weaknesses are of three kinds:

- **A pattern the code knows but stops short of.** The exporter `run` convention has no trait and the desktop dispatches by `match` (F3). `run_pipeline` is a Template Method with one hook, so three of seven exporters bypass it and one copies its tail. The server records Import Run stages but neither side owns a transition table, while the desktop host shows exactly how one is written.
- **The same pattern hand-rolled several times.** Four `Option<&Sink>` parameters where a Null Object would do; three external stores in the web app with the same `Set<() => void>` shape; two journal adapters that are the same adapter over `jsonl_journal`; five sites that build the desktop's one progress wire type by hand.
- **A global where an instance exists.** The import semaphore (F7), and the `media` crate's process-wide Tools Directory that `Form::to_config` reads without being told.

Nothing here blocks change today. The highest-value work is DP1 and DP2 together (one exporter registry and one convert context), because they turn seven parallel implementations into one skeleton with seven short specialisations.

## 2. Pattern inventory

| Pattern | Where | Verdict |
|---|---|---|
| Strategy (trait object) | `MergedArchive` `crates/libs/ir-format/src/format_sink.rs:21`; impls `SbrArchive` `sms-backup-restore-exporter/src/write.rs:548`, `SmsBackupPlusArchive` `sms-backup-plus-exporter/src/write.rs:39`; held as `Option<Box<dyn MergedArchive>>` `format_sink.rs:58` | Correct. The lower crate names no vendor format; the owner supplies it (`sms-backup-restore-exporter/src/emit.rs:111`, `reexport/src/lib.rs:193`). |
| Strategy (convention, no trait) | `pub fn run(&ExporterConfig) -> Result<RunResult>` in seven exporters and `reexport/src/lib.rs:36`; `run_exporter` `src-tauri/src/commands/extract.rs:524-537` | Missing abstraction, F3. See DP1. |
| Strategy (data table) | `MembershipSpec` `crates/server/server/src/db/named_membership.rs:61`, `tag_spec` `:92`, `group_spec` `:133` | Correct and the right weight: two tables, same SQL, names in a `static`. |
| Strategy (closed enum match) | `write_format` `crates/libs/ir-format/src/write.rs:81-99`; `read_artifact` `crates/libs/reexport/src/lib.rs:465-475`; `OutputFormat` `crates/core/message-crate-core/src/config.rs:18` | Appropriate; the format set is closed and owned by one crate. The `Xml`/`SmsBackupPlus` arms bail or `unreachable!` because those go through `MergedArchive`, which is the right split. |
| Template Method (hooks with defaults) | `ProjectionHooks` `crates/libs/ir/src/projection.rs:85-200`, driver `pending_to_document` `:239`; impls in `imazing-exporter/src/emit.rs:895`, `whatsapp-exporter/src/emit.rs:634`, `openextract-exporter/src/emit.rs:576`, `sms-backup-plus-exporter/src/emit.rs:246`, `go-sms-pro-exporter/src/emit.rs:332` | Correct and well-shaped: three required methods, fourteen with defaults. |
| Template Method (run skeleton) | `run_pipeline` `crates/core/message-crate-core/src/run.rs:20`, `finish_run` `:35`; used by four exporters, bypassed by three | Hook set too narrow. See DP2. |
| Template Method (convert skeleton) | `convert_export` in `go-sms-pro-exporter/src/emit.rs:414`, `imazing-exporter/src/emit.rs:113`, `sms-backup-plus-exporter/src/emit.rs:449`, `openextract-exporter/src/emit.rs:54`, `sms-backup-restore-exporter/src/emit.rs:97`; `convert_json` `whatsapp-exporter/src/emit.rs:66` | Missing, F8. See DP2. |
| Observer (sinks) | `LogSink` `crates/core/message-crate-core/src/process.rs:22`, `ProgressSink` `progress.rs:150`, `IssueSink` `pipeline.rs:155`, `CancelFlag` `process.rs:9`; closures built in `src-tauri/src/commands/events.rs:51,63` | Correct design (callbacks, no Tauri in any pipeline crate). Carried as `Option` everywhere; see DP4. Three progress vocabularies, F6; see DP3. |
| Observer (web external stores) | `web/src/lib/api.ts:34-46`, `web/src/lib/desktopJob.ts:21-57`, `web/src/screens/import/importRunStore.ts:115-140`, `web/src/lib/useWindowWidth.ts:14` | Same shape hand-rolled three times. See DP5. |
| State machine (pure step) | `step(State, Event) -> (State, Vec<Action>)` `src-tauri/src/local_server.rs:332`; `apply` loop `:768-787` | Correct. The model for the other two machines. |
| State machine (web import phases) | `ImportPhase` `web/src/screens/import/importProgressState.ts:18`; phase written at `useImportJob.ts:562,1147,1221,1320,1503,1833,1988` | No transition function. See DP8. |
| State machine (server stages) | `ImportStage` `crates/server/server/src/db/imports.rs:21`, `ALL` `:39`; `set_import_stage` `:579` | Order declared, not enforced. See DP8. |
| Builder (library) | router assembly `http_app` `crates/server/server/src/server.rs:1248-1345`; `OpenApiRouter` `openapi.rs:126-132`; `ExportWriter::open/with_spool/with_archive` `crates/libs/staging/src/export_writer.rs:84,127,138`; `FormatSink::with_archive` `format_sink.rs:96` | Correct. `sqlx::QueryBuilder` is used nowhere (grep); dynamic SQL goes through one hand-rolled builder, below. |
| Builder (hand-rolled SQL) | `Sql` `crates/server/server/src/search/bridge.rs:12-50` with `SqlParam`/`bind_all` `db/sql.rs:14,42` | Correct and small. Used consistently by the emitter. |
| Interpreter | `Expr` `search/parse.rs:34`, `FieldSpec`/`FIELDS` `search/fields.rs:31,45`, `emit_expr` `search/emit.rs:161`, `compile` `search/mod.rs:203` (ADR 0004); TypeScript twin `web/src/lib/searchQuery.ts` tested against `tests/fixtures/search/web-queries.txt` (`searchQuery.test.ts:617`, `search/tests.rs:3617`) | Correct. One weak joint: the registry and the emitter are linked by a string match. See DP13. |
| Repository | `crates/server/server/src/db/*`, 270 functions taking `conn: &mut SqliteConnection` first (grep), none taking a pool | Consistent signatures. Returns the HTTP error type, F2; row structs double as API schemas, DP6. |
| Unit of work | `WriteTx` `db/write_tx.rs:36`, `begin_write` `:57` | Correct: the only way to open a transaction, with `BEGIN IMMEDIATE` and a Clippy ban on `Connection::begin`. |
| Newtype guard | `auth_guard!` `server.rs:323-381`, seven guards; `OptionalFromRequestParts` for the one optional route `:307` | Correct. No handler calls a `require_*` function directly (grep over `*_api.rs`). |
| Decorator / middleware | `request_id::layer` `crates/server/server/src/request_id.rs`; `declared_query::refuse_undeclared_query` `declared_query.rs:142` reading the OpenAPI spec `:36`; `extract::{Query,Path,Json}` `extract.rs:23,43,62` mapping rejections to `ApiError` | Correct. The declared-query layer is a good example of one source of truth driving a check. |
| Chain of responsibility | `resolve_auth_on_conn` `server.rs:1612`: session, then API token, then `Credential` `:1595` | Two links; an if/else is the right size. |
| Command | `start_job`/`spawn_job`/`Job: Drop` `src-tauri/src/commands/jobs.rs:70,118,49`; five commands composed the same way: `extract` `extract.rs:161`, `export` `export.rs:49`, `format` `format.rs:29`, `upload` `upload.rs:99`, `transcode_staging` `staging.rs:243` | Correct and consistent. `Result<_, String>` at the command edge is F11. |
| Adapter | core progress to window: `From<ProgressEvent> for WindowEvent` `events.rs:126`; journal adapters `crates/libs/import/src/journal.rs`, `crates/libs/export/src/journal.rs` over `jsonl_journal`; `reexport` over the format readers | The window adapter is one of five mapping sites, DP3. The two journal adapters are one adapter twice, DP9. |
| Facade | `web/src/lib/serverApi.ts` (106 exports over `apiClient`, `api.ts:255`); `web/src/lib/tauri.ts` (61 exports, 29 `invoke` calls); `useImportJob` `useImportJob.ts:1766-2084` over the import store | `serverApi.ts` holds. `tauri.ts` is bypassed in four files, DP10. `useImportJob` is F4. |
| Provider / Context | `AuthContext` `web/src/lib/auth.tsx:64`, `useAuth` `:554`; `ThemeContext` `ThemeProvider.tsx:42`, `useTheme` `:134`; `TimeZoneContext` `timeZone.ts:41`, `useTimeZone` `:44` | Correct. `useAuth` and `useTheme` throw outside their provider; `useTimeZone` falls back to the browser zone (`:44-46`) and says so. |
| Custom hooks | `useRouteQuery` `routeQuery.ts:114`, `useRouteInfiniteQuery` `:134`, `useRoutePagedList` `:234`, `useRouteCache` `:368`; key factories `routeQueryKey` `routeQueryKey.ts:30`, `keys` `queryKeys.ts:31` | Correct (ADR 0002). No `useRouteMutation`; 20 raw `useMutation` sites invalidate by hand, DP10. |
| Factory | `createQueryClient` `routeQuery.ts:68` (a factory so each test gets its own client); `Form::to_config` `crates/core/message-crate-core/src/exporters.rs:260` (F3); `run_with(config, spawn)` `imessage-ir-exporter/src/run.rs:182` injecting the process spawner | `createQueryClient` and `run_with` are the right shape. `Form::to_config` is the enum fan-out F3 describes. |
| Singleton / global | `import_semaphore` `imports_api/mod.rs:1769-1771` (F7); `tools_state` `crates/libs/media/src/tools.rs:84`; `JOURNAL_WRITE_LOCK` `crates/libs/journal/src/lib.rs:49`; `SERVER_LOG` `crates/server/server/src/logging.rs:40`; `DESKTOP_BUILD` `crates/libs/http/src/lib.rs:35`; web module-level `let` in `useImportJob.ts:472,489,633,636` (F4) | Two are justified by a process-wide fact (the tracing subscriber, the app's Build). The import semaphore and the Tools Directory are instance state in a static, DP7. |
| Service layer | `import_jsonl_files_on_conn` `imports_api/mod.rs:314`, shared by HTTP `:1776`, the CLI `import_cli.rs:210` and the demo build `reset_demo.rs:881` | Correct: one entry point, three callers. |
| DTO / value object | `Created<T>` `server.rs:472`; `Page<T>` (`paging.rs`); `ProblemType` `problem.rs:22` with `ApiError::problem_type` `server.rs:604` | `ProblemType` is a clean registry. Row structs as schemas, DP6. |

## 3. Findings

Importance is 1 to 10: 10 means it causes defects or blocks change now; 1 is cosmetic.

| ID | Importance | Area | Finding |
|---|---|---|---|
| DP1 | 6 | core / desktop | Strategy by convention: no exporter trait, hand `match`, per-exporter facts in two places (with F3) |
| DP2 | 6 | core / exporters | `run_pipeline` has one hook so three exporters bypass it and one copies its tail; `convert_export` skeleton and seven argument structs with no shared context (with F8) |
| DP3 | 5 | desktop / pipeline | Five hand-written mappings onto the one progress wire type (with F6) |
| DP4 | 5 | pipeline | Null Object missing: `Option<&LogSink>` at 21 signatures, `Option<&ProgressSink>` 11, `Option<&CancelFlag>` 15, `Option<&IssueSink>` 7, each with an `emit_*` unwrapper |
| DP5 | 4 | web | Three hand-rolled external stores with one shape; the generic factory exists but is private to the import feature |
| DP6 | 3 | server | 23 persistence row structs derive `utoipa::ToSchema`; the API document is generated from the storage layer |
| DP7 | 4 | pipeline / server | Process-wide Tools Directory read implicitly by form validation; import semaphore in a static (with F7) |
| DP8 | 4 | web / server | Import Run phases and stages have no transition function on either side; the desktop host shows how |
| DP9 | 3 | pipeline | Two journal adapters are the same adapter; `jsonl_journal` has no "event with a target" abstraction |
| DP10 | 3 | web | `tauri.ts` facade bypassed in four files; no `useRouteMutation`, so 22 hand `invalidateAccount()` calls |
| DP11 | 3 | desktop / web | Desktop event structs and their TypeScript twins are kept in step by hand; the API side has a generator and a CI gate |
| DP12 | 2 | core | `parallel_for_each` carries `Err("cancelled")` strings beside the typed `Cancelled` (with F19) |
| DP13 | 3 | server | Search registry and emitter joined by a string `match` with a runtime "has no emitter" arm |

### DP1. Strategy by convention; the registry is split across two files (6/10)

**Evidence.** F3 has the structural case: `Exporter` and `SourceConfig` enumerate the vendors in core (`crates/core/message-crate-core/src/exporters.rs:23`, `config.rs:189`), every exporter exposes `pub fn run(config: &ExporterConfig) -> Result<RunResult>` by convention (`go-sms-pro-exporter/src/run.rs:13`, `imazing-exporter/src/run.rs:13`, `imessage-ir-exporter/src/run.rs:177`, `openextract-exporter/src/run.rs:13`, `sms-backup-plus-exporter/src/run.rs:18`, `sms-backup-restore-exporter/src/run.rs:14`, `whatsapp-exporter/src/run.rs:27`, and `reexport/src/lib.rs:36`), and the desktop renames them with `use … as run_*` (`src-tauri/src/commands/extract.rs:28-34`) to dispatch in `run_exporter` (`:524-537`) with a dead `SourceConfig::Format` arm.

The pattern-level addition: the facts about one exporter are already spread over two `match` statements in the same file. `run_exporter` (`:524`) picks the function; `programs_run_by` (`:511-516`) picks the programs it needs (`Whatsapp → [Program::Wtsexporter]`). Each `run` also re-checks its own variant (`sms-backup-plus-exporter/src/run.rs:19-21` `let SourceConfig::SmsBackupPlus(source) = &config.source else { bail!(…) }`, `whatsapp-exporter/src/run.rs:28-30`, `imessage-ir-exporter/src/run.rs:221-223`), which is the check a registry would make once.

Meanwhile the codebase already has the right shape for this in `ProjectionHooks` (`crates/libs/ir/src/projection.rs:85`) and `MergedArchive` (`format_sink.rs:21`): a trait the lower crate defines and the owner implements.

**Why it matters.** Adding a backup type touches `Exporter`, `SourceConfig`, `Form::to_config`, `run_exporter`, `programs_run_by` and the new crate, and nothing checks that the two `match` statements agree with each other or with the form.

**Remediation.** F3's trait, plus one table that holds every per-exporter fact the desktop needs.

```rust
// src-tauri/src/commands/extract.rs: one row per exporter, replacing run_exporter and programs_run_by
struct ExporterEntry {
    run: fn(&ExporterConfig) -> anyhow::Result<RunResult>,
    /// The programs the Tools Directory holds that this exporter runs.
    programs: &'static [Program],
}

fn exporter_for(source: &SourceConfig) -> Option<ExporterEntry> {
    Some(match source {
        SourceConfig::GoSmsPro(_) => ExporterEntry { run: go_sms_pro_exporter::run, programs: &[] },
        SourceConfig::SmsBackupRestore(_) => ExporterEntry { run: sms_backup_restore_exporter::run, programs: &[] },
        SourceConfig::SmsBackupPlus(_) => ExporterEntry { run: sms_backup_plus_exporter::run, programs: &[] },
        SourceConfig::OpenExtract(_) => ExporterEntry { run: openextract_exporter::run, programs: &[] },
        SourceConfig::Imazing(_) => ExporterEntry { run: imazing_exporter::run, programs: &[] },
        SourceConfig::Apple(_) => ExporterEntry { run: imessage_ir_exporter::run, programs: &[] },
        SourceConfig::Whatsapp(_) => ExporterEntry { run: whatsapp_exporter::run, programs: &[Program::Wtsexporter] },
        // Format conversion goes through commands/format.rs; no "not yet wired" string.
        SourceConfig::Format(_) => return None,
    })
}
```

Then `extract` (`:161`) reads `entry.programs` for `wait_for_downloads` and `entry.run` in `run_staging`, and the `use … as run_*` aliases at `:28-34` go. Plain function pointers suffice here; F3's `dyn Exporter` is only needed if something must hold a boxed exporter.

### DP2. Two Template Methods that stop short: the run skeleton and the convert skeleton (6/10)

**Evidence.**

*The run skeleton.* `run_pipeline` (`crates/core/message-crate-core/src/run.rs:20-27`) is cancel check, `convert(transforms)`, then `finish_run` (`:35-45`), which appends media lines and summary lines unconditionally. Four exporters use it (`imazing-exporter/src/run.rs:19`, `go-sms-pro-exporter/src/run.rs:19`, `sms-backup-restore-exporter/src/run.rs:21`, `openextract-exporter/src/run.rs:19`). Three bypass it: `sms-backup-plus-exporter/src/run.rs:9-11` says "The shared `run_pipeline` cannot apply `SmsBackupPlusConfig::include_summary` (it appends the summary lines unconditionally)", calls `finish_run` only when `include_summary` is set (`:47-49`), and otherwise re-implements the tail by hand (`:50-53`: `report.check_media(...)?; Ok(RunResult::new(report.media_lines(), &report))`). `imessage-ir-exporter/src/run.rs:216` and `whatsapp-exporter/src/run.rs` call `finish_run` directly because their middle differs (`run.rs:29-31` names the three). So the template's single hook (`convert`) is too narrow for one boolean, and the tail has been copied once already.

*The convert skeleton.* F8 lists the ~30-line `prepare_outputs → ExportWriter::open → ingest with check_cancel → export_meta → project_conversation loop → finish` sequence in three exporters (read here at `go-sms-pro-exporter/src/emit.rs:414-495`). What F8 does not list is that each exporter also defines its own argument struct for that skeleton, and the seven differ in shape while carrying the same six config fields: `ConvertExportArgs<'a>` in `go-sms-pro-exporter/src/emit.rs:388` (`output_dir, scratch_dir, transforms, output_format, cancel, resume, issues` plus `input_dir, owner_phones`), `imazing-exporter/src/emit.rs:88` (same six plus `input, timezone`), `sms-backup-restore-exporter/src/emit.rs:74`, `openextract-exporter/src/emit.rs:32`, `sms-backup-plus-exporter/src/emit.rs:414` (generic over `P: AsRef<Path>` and with `log`, `verbose`, `phone_country` besides), `ConvertRequest<'a>` in `whatsapp-exporter/src/emit.rs:35` (`output, transforms, output_format, cancel, resume, issues` plus `json_path, media_search_roots, owner_identity, backup_taken_at_unix_ms`), and `ExportOptions` in `imessage-ir-exporter/src/run.rs:75`. Every `run` copies the same fields out of `ExporterConfig` into its struct (`sms-backup-plus-exporter/src/run.rs:33-46` is the pattern).

**Why it matters.** A field added to the shared tail (a new sink, a new transform) is added to seven structs and seven copy sites. The one divergence that exists (`include_summary`) already cost a hand-copied tail, and the next one will cost another.

**Remediation.** Give the run skeleton the one hook it lacks, and give the convert skeleton a shared context built from the config.

```rust
// crates/core/message-crate-core/src/run.rs
pub fn run_pipeline(
    config: &ExporterConfig,
    convert: impl FnOnce(ExportTransforms) -> anyhow::Result<ExportReport>,
) -> anyhow::Result<RunResult> {
    run_pipeline_with(config, true, convert)
}

/// `run_pipeline`, with the summary lines left out when `summary` is false.
pub fn run_pipeline_with(
    config: &ExporterConfig,
    summary: bool,
    convert: impl FnOnce(ExportTransforms) -> anyhow::Result<ExportReport>,
) -> anyhow::Result<RunResult> {
    check_cancel(config.cancel.as_ref())?;
    let report = convert(ExportTransforms::from_config(config))?;
    finish_run(config, &report, config.media.mode.needs_tools(), summary)
}

pub fn finish_run(config: &ExporterConfig, report: &ExportReport, needs_tools: bool, summary: bool) -> anyhow::Result<RunResult> {
    report.check_media(needs_tools)?;
    let mut messages = report.media_lines();
    if summary {
        report.summary_lines(config.output_format, &config.output, &mut messages);
    }
    Ok(RunResult::new(messages, report))
}
// sms-backup-plus-exporter/src/run.rs: delete :47-53 and call
//   message_crate_core::run_pipeline_with(config, source.include_summary, |transforms| convert_export(…))
```

```rust
// crates/core/message-crate-core/src/convert.rs (new): what every convert_export takes from the config
pub struct ConvertContext<'a> {
    pub output: &'a Path,
    pub scratch_dir: &'a Path,
    pub output_format: OutputFormat,
    pub transforms: ExportTransforms,
    pub resume: bool,
    pub cancel: Option<&'a CancelFlag>,
    pub issues: Option<&'a IssueSink>,
}

impl<'a> ConvertContext<'a> {
    pub fn from_config(config: &'a ExporterConfig, transforms: ExportTransforms) -> Self {
        Self {
            output: &config.output,
            scratch_dir: &config.scratch_dir,
            output_format: config.output_format,
            transforms,
            resume: config.resume,
            cancel: config.cancel.as_ref(),
            issues: config.issues.as_ref(),
        }
    }
}

// go-sms-pro-exporter/src/emit.rs:388: the struct keeps only what is GO SMS Pro's
pub(crate) struct ConvertExportArgs<'a> {
    pub ctx: ConvertContext<'a>,
    pub input_dir: &'a Path,
    pub owner_phones: &'a [String],
}
```

With the context in place, F8's `run_convert_export(ctx, ingest)` in core is a mechanical extraction: the ingest closure is the one hook the five SMS-and-CSV exporters differ in.

### DP3. Five mappings onto one progress wire type (5/10)

**Evidence.** F6 names the three `ProgressEvent` enums (`crates/core/message-crate-core/src/progress.rs:23`, `crates/libs/import/src/progress.rs:45`, `crates/libs/export/src/run.rs:92`) and the `Send` mismatch between their `ProgressFn` aliases (`import/src/progress.rs:93` is `+ Send`, `export/src/run.rs:114` is not). The desktop host then flattens all of them onto one `ExtractProgressEvent` (`src-tauri/src/commands/events.rs:82`), and does so in five places with three styles:

- the Adapter proper: `impl From<ProgressEvent> for WindowEvent` (`events.rs:126-152`), used by `progress_sink` (`:63`), with a `counts(step, done, total)` constructor (`:109`);
- a free function: `forward_upload_event` (`commands/upload.rs:200-260`) builds `ExtractProgressEvent { … }` literals at `:211` and `:25`;
- inline closures with struct literals: `commands/export.rs:71-87` (maps `Page` to a log line, no progress event at all), `commands/staging.rs:294-303` (`step: "media".into(), … waiting: None` written out), `commands/tools.rs:251-252`.

The `step` string (`"setup" | "parse" | … | "upload"`) is a `String` in Rust (`events.rs:84-86`) and a union in TypeScript (`web/src/lib/types.ts:101`); each literal site types it by hand.

**Why it matters.** The wire type has one shape and the web reads it in one place (`importProgressState.ts:62` `STEP_ROW_INDEX`), but a new field (as `waiting` was, #1053) is added at five construction sites, and nothing but the TypeScript union says which `step` strings are legal.

**Remediation.** One `From` per source event type, next to the existing one, and a `Step` enum so the string is typed once.

```rust
// src-tauri/src/commands/events.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Step { Setup, Parse, Attachments, Prepare, Check, Media, Upload }

impl ExtractProgressEvent {
    fn counts(step: Step, done: usize, total: usize) -> Self { /* as :109, with `step` typed */ }
}

impl From<message_staging::TranscodeProgress> for ExtractProgressEvent {
    fn from(p: message_staging::TranscodeProgress) -> Self { Self::counts(Step::Media, p.done, p.total) }
}
// commands/staging.rs:294: events::emit(&app_handle, events::PROGRESS, ExtractProgressEvent::from(progress));

impl ExtractProgressEvent {
    /// The Upload's count for one conversation file (`upload.rs:211`).
    pub fn upload_file(index: usize, total: usize) -> Self { Self::counts(Step::Upload, index, total) }
}
```

With F6 done (one `ProgressEvent` with `Import`/`Export` variants), the three `From` impls collapse into the one at `:126`. **Unable to verify** the exact name of the progress struct `transcode_staged` hands its callback (`crates/libs/staging/src/transcode.rs`, the `&mut |progress|` parameter at `staging.rs:291`); the snippet names it `TranscodeProgress`.

### DP4. Null Object missing for the four sinks (5/10)

**Evidence.** Every sink is optional at every level. `ExporterConfig` carries `cancel: Option<CancelFlag>`, `log: Option<LogSink>`, `progress: Option<ProgressSink>`, `issues: Option<IssueSink>` (`crates/core/message-crate-core/src/config.rs:96-108`); `ExportTransforms` copies two of them as `Option` again (`transforms.rs:21-23`, `:50-51`). Below that, by grep over `crates/` and `src-tauri/src`: `Option<&LogSink>` in 21 signatures, `Option<&ProgressSink>` in 11, `Option<&CancelFlag>` in 15, `Option<&IssueSink>` in 7, and `Option<LogSink>` owned in 10 more. Five free functions exist only to unwrap them: `emit_log` (`process.rs:71`, 58 call sites), `emit_warning` (`:81`), `is_cancelled`/`check_cancel` (`:92,107`), `emit_progress` (`progress.rs:218`), `emit_issue` (`pipeline.rs:179`). `emit_log` and `emit_warning` fall back to `eprintln!` (`process.rs:75,85`), so a library crate decides where a missing sink's lines go. `emit_progress` returns `sink.is_none_or(|s| s.emit(event))` so that pacing works with no sink (`:218-220`).

`ExportWriter` then exposes the same options as accessors (`log()`, `progress()`, `export_writer.rs:179,184`) for the write queue to unwrap again.

**Why it matters.** This is the Observer pattern with the "no observer" case handled at every call site instead of once. The `None → stderr` rule is a policy the desktop app never wants (its sinks are always set, `extract.rs:215-217`) and the server never sees, yet every pipeline function pays for it in its signature. The two-level `Option` (config and transforms) is why `ExportTransforms::from_config` has to copy sinks at all.

**Remediation.** Give each sink a null value and pass sinks by reference.

```rust
// crates/core/message-crate-core/src/process.rs
impl LogSink {
    /// Every line to stderr: what a run gets when nothing is listening.
    pub fn stderr() -> Self { Self::new(|line| eprintln!("{line}")) }
    /// Drops every line; for tests that assert nothing about the log.
    pub fn silent() -> Self { Self::new(|_| {}) }
}
impl Default for LogSink { fn default() -> Self { Self::stderr() } }

// progress.rs
impl ProgressSink {
    /// A run with no bar to move. Unpaced, so `emit` always answers `true`
    /// and a caller that writes a log line beside each count still writes it.
    pub fn none() -> Self { Self::unpaced(|_| {}) }
}
impl Default for ProgressSink { fn default() -> Self { Self::none() } }

// pipeline.rs
impl IssueSink { pub fn none() -> Self { Self::new(|_| {}) } }
impl Default for IssueSink { fn default() -> Self { Self::none() } }

// config.rs: `pub log: LogSink, pub progress: ProgressSink, pub issues: IssueSink, pub cancel: CancelFlag`
// (a never-set `Arc<AtomicBool>` is the "cannot be cancelled" value). Then
//   emit_log(sink, line)        -> sink.emit(line)
//   emit_progress(sink, event)  -> sink.emit(event)
//   check_cancel(cancel)        -> cancel.check()   // the Result<(), Cancelled> stays
// and the 54 `Option<&…Sink>` parameters become `&…Sink`.
```

The change is wide but mechanical; it can go one sink at a time, `LogSink` first because it has the stderr policy.

### DP5. Three hand-rolled external stores with one shape (4/10)

**Evidence.** The web app subscribes React to non-React state three ways that are the same code:

- `web/src/lib/api.ts:3-5` module `let`s for `baseUrl`, `authToken`, `accountId`; `accountIdListeners = new Set<() => void>()` `:34`; `setAccountId` notifies `:28-32`; `onAccountIdChange` `:40-46` returns the unsubscribe.
- `web/src/lib/desktopJob.ts:21-22` `let holds` and `const listeners = new Set<() => void>()`; `notify` `:24`; `holdDesktopJob` `:32` returns the release; `useDesktopJob` `:55` is `useSyncExternalStore(subscribe, currentDesktopJob, currentDesktopJob)`.
- `web/src/screens/import/importRunStore.ts:115-138` `createImportRunStore(initial)` with `get`, `set(patch)`, `subscribe`, `reset`, a `Set<() => void>` and a notify loop; `importRunStore` `:140` is its one instance; `useImportRunState` `:193` is `useSyncExternalStore(importRunStore.subscribe, read, read)`.
- `web/src/lib/useWindowWidth.ts:14` is a fourth `useSyncExternalStore` over `window` resize, which is the one legitimate stand-alone.

`createImportRunStore` is a generic store factory in everything but its type parameter and its location: it is private to the import feature and typed to `ImportRunState`. The import store also wires itself to the other two at module load (`importRunStore.subscribe(followRun)` `:175`, `onAccountIdChange(followRun)` `:176`), so three singletons observe each other through three private listener sets.

F4 covers the module-level mutable state in `useImportJob.ts` and its remedy (a service factory); this finding is the store shape.

**Why it matters.** Three listener sets means three places where a forgotten `notify` or a leaked subscription can hide, and none of the three can be constructed fresh for a test except the import one (which has `reset`). The pattern the codebase wants is `useSyncExternalStore` over a store object; it has written the store object once and copied the mechanism twice.

**Remediation.** Lift the factory out of the import feature and make the other two its instances.

```ts
// web/src/lib/externalStore.ts
import { useSyncExternalStore } from "react";

export type ExternalStore<T> = {
  get: () => T;
  set: (next: T | ((state: T) => T)) => void;
  subscribe: (listener: () => void) => () => void;
};

export function createExternalStore<T>(initial: T): ExternalStore<T> {
  let state = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => state,
    set: (next) => {
      state = typeof next === "function" ? (next as (state: T) => T)(state) : next;
      for (const listener of listeners) listener();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** `select` must return a stable value (a primitive or the state itself), as `useSyncExternalStore` requires. */
export function useExternalStore<T, S>(store: ExternalStore<T>, select: (state: T) => S): S {
  const read = () => select(store.get());
  return useSyncExternalStore(store.subscribe, read, read);
}

// desktopJob.ts: const holds = createExternalStore<{ name: DesktopJobName }[]>([]);
// api.ts:       export const accountId = createExternalStore<number | null>(null);  // onAccountIdChange = accountId.subscribe
// importRunStore.ts: createImportRunStore becomes createExternalStore<ImportRunState> plus the `patch` and `reset` helpers it adds.
```

### DP6. Persistence row structs are the API schema (3/10)

**Evidence.** 23 structs under `crates/server/server/src/db/` derive `utoipa::ToSchema` (grep): `SavedSearch` `db/saved_searches.rs:38` ("One row of `saved_searches`"), `db/conversations.rs:68,402`, `db/imports.rs:19,68,120,1083`, `db/exports.rs:23`, `db/handles.rs:136,376`, `db/account_profile.rs:28`, `db/address_book.rs:42`, `db/import_contacts.rs:22,125`, eight in `db/audit_trail.rs`, `db/contacts/read.rs`. Handlers return them directly (`saved_searches_api.rs:61` `Result<Json<Page<SavedSearch>>, ApiError>`), and only 6 `*Response` structs exist across the `*_api.rs` files (grep). The OpenAPI document, and from it `web/src/lib/serverApi.types.ts`, is therefore generated from the storage layer's row types.

**Why it matters.** This is the DTO question: the repository's row type and the interface's response type are one struct. The cost is the coupling F2 names from the other side (`db/` knows HTTP): here, a column rename is an API change and a response cannot carry a derived field without the row carrying it. The benefit is real too: no mapping layer, no drift between row and response, and the no-compatibility rule means an API change costs nothing but a rebuild.

**Remediation.** For this codebase the simpler solution is the right one: keep the row-as-schema rule, but write it down in `docs/architecture/http-api.md` beside the "a handler holds no SQL" rule, so a reviewer does not add a parallel `Response` struct for one route and not another. Revisit only when F2 is done, because then `db/` has its own error type and the question of whether it should also have its own types is live. No code change recommended now.

### DP7. Globals where instance state exists (4/10)

**Evidence.**

- `media::tools_state()` (`crates/libs/media/src/tools.rs:84-95`) is a `OnceLock<Mutex<ToolsState>>` holding the Tools Directory and a cache of which binaries answered `-version`. It is set from the server (`crates/server/server/src/server.rs:1358`) and the desktop app (`src-tauri/src/app_directories.rs:53`), and read through `media::ffmpeg_available()` (`tools.rs:177`) by `Form::to_config` (`crates/core/message-crate-core/src/exporters.rs:318,596`) and `reset_demo.rs:164`. So the form's validation result depends on a process global that nothing in its signature names. Tests must save and restore it (`media/src/testutil.rs:128-150`, `src-tauri/src/commands/tools.rs:402-410`).
- `import_semaphore()` (`crates/server/server/src/imports_api/mod.rs:1769-1771`) is a static `Semaphore` while `AppState` (`server.rs:385`) already owns the auth rate limiter precisely so that "tests in one binary cannot rate-limit each other" (`server.rs:398-401`). F7 has the remedy.
- `JOURNAL_WRITE_LOCK` (`crates/libs/journal/src/lib.rs:49`) is one `Mutex<()>` for every journal file in the process, so an Upload and an Export writing two different files serialise on it. Harmless at today's volume; noted as coarse.
- `SERVER_LOG` (`logging.rs:40`) and `DESKTOP_BUILD` (`crates/libs/http/src/lib.rs:35`) are process-wide facts (the tracing subscriber is global; the Build is one per process) and are correctly statics.

**Why it matters.** A Singleton is right when the thing is one per process by nature. The Tools Directory is one per app, which is the same count, but the hidden read in `Form::to_config` makes a pure validation function depend on ambient state, which is why the tests need a save-and-restore dance.

**Remediation.** Pass the tools lookup to the one function that validates against it; leave the cache where it is.

```rust
// crates/core/message-crate-core/src/exporters.rs
pub struct FormContext<'a> {
    pub scratch_dir: &'a Path,
    /// Whether ffmpeg answers, asked once by the caller (`media::ffmpeg_available()`).
    pub ffmpeg_available: bool,
}
pub fn to_config(&self, exporter: Exporter, ctx: &FormContext<'_>) -> Result<ExporterConfig, Vec<String>> { /* :260, reading ctx.ffmpeg_available at :318 and :596 */ }
// src-tauri/src/commands/extract.rs:406 build_exporter_config asks media once and passes the answer.
```

The media crate's cache stays a static (it is a cache of a process-wide fact), but no validation reads it implicitly.

### DP8. No transition function for the Import Run on either side (4/10)

**Evidence.**

*Server.* `ImportStage` (`crates/server/server/src/db/imports.rs:21-36`) has six variants and declares their order (`ALL` `:39-46`, "in the order a run passes through them"). `set_import_stage` (`:579-603`) writes any stage for any running run: the `UPDATE … WHERE … status = 'running'` is the only guard. `update_import` (`imports_api/mod.rs:1627-1647`) passes `body.stage` straight through.

*Web.* `ImportPhase` (`web/src/screens/import/importProgressState.ts:18-24`) has six phases. The phase is set by `store.set({ phase: … })` at eight sites in `useImportJob.ts` (`:562, 1147, 1221, 1320, 1503, 1833, 1988`, plus `waitAtReview` `:997`), and the server stage is moved by `moveStage` (`:945`) and `moveStageAtReview` (`:967`) from the same functions. No function says which phase may follow which; the rule lives in the control flow of ~60 module-level functions (F4).

*The model that exists.* `src-tauri/src/local_server.rs:332` `fn step(mut state: State, event: Event) -> (State, Vec<Action>)`: "The only place the state changes; it does no I/O, so every rule is tested without a process." Its caller `apply` (`:768-787`) performs the actions and feeds `SpawnFailed` back in as an event.

**Why it matters.** The web and the server each hold half a state machine, and the half they hold is implicit. A resume lands on `staging_review` from `media` (`useImportJob.ts:2039` `landOn("staging_review", true)`), so the machine is not forward-only, and that fact is recorded nowhere a reader or a test can find it. The server accepts a move from `upload` back to `parse` today.

**Remediation.** A pure transition function on the web side, in the module that already owns `ImportPhase`, and a `may_move_to` table on the server side.

```ts
// web/src/screens/import/importProgressState.ts
export type ImportEvent =
  | "start" | "identityStop" | "identityContinue" | "stagingDone" | "approveStaging"
  | "mediaDone" | "approveMedia" | "uploadDone" | "cancel" | "resumeAtStagingReview" | "resumeAtMediaReview";

/** The one place the phase changes. Every arm is a row the tests assert. */
export function nextPhase(phase: ImportPhase, event: ImportEvent): ImportPhase {
  switch (event) {
    case "start": return phase === "form" ? "running" : phase;
    case "identityStop": return phase === "running" ? "identity_stop" : phase;
    case "identityContinue": return phase === "identity_stop" ? "running" : phase;
    case "stagingDone": return phase === "running" ? "staging_review" : phase;
    case "approveStaging": return phase === "staging_review" ? "running" : phase;
    case "mediaDone": return phase === "running" ? "media_review" : phase;
    case "approveMedia": return phase === "media_review" ? "running" : phase;
    case "uploadDone": return phase === "running" ? "done" : phase;
    case "cancel": return "form";
    case "resumeAtStagingReview": return "staging_review";
    case "resumeAtMediaReview": return "media_review";
  }
}
// useImportJob.ts: the eight `store.set({ phase: "…" })` become `store.set((s) => ({ phase: nextPhase(s.phase, "…") }))`.
```

The event names above are illustrative; the real list is read off the eight sites. The server side:

```rust
// crates/server/server/src/db/imports.rs
impl ImportStage {
    /// Whether a running run at `self` may be moved to `next`: forward by one stage,
    /// or back to a Review on a resume (`web/src/screens/import/useImportJob.ts`, `landOn`).
    pub fn may_move_to(self, next: Self) -> bool {
        use ImportStage::*;
        matches!((self, next),
            (Parse, Write) | (Write, StagingReview) | (StagingReview, Media) | (StagingReview, Upload)
            | (Media, MediaReview) | (MediaReview, Upload)
            | (Media, StagingReview) | (Upload, MediaReview) | (Upload, StagingReview))
            || self == next
    }
}
// set_import_stage: read the current stage inside begin_write, refuse with ImportLookupError::InvalidRun when !current.may_move_to(stage).
```

**Unable to verify** the full set of backward moves the resume paths make; `web/src/screens/import/useImportJob.ts` (`landOn`, `resumeAtReview`, the `resumeDecision.ts` module) and `crates/server/server/src/imports_api/tests.rs` would list them, and the table must be built from that list, not from the illustration above.

### DP9. Two journal adapters are one adapter (3/10)

**Evidence.** `crates/libs/import/src/journal.rs` and `crates/libs/export/src/journal.rs` each adapt `jsonl_journal` (`crates/libs/journal/src/lib.rs`: `ServerTarget` `:30`, `append` `:63`, `load_events` `:112`, `compact_with` `:165`) with the same six items: an event enum with a private `target(&self) -> &ServerTarget` (`import:33,67`; `export:21,153`), a state struct with `assets: HashSet<String>` (`import:79`; `export:47`), `journal_path` (`import:108`; `export:55`), `unreadable_line_sentence` (`import:97`; `export:63`), `load` with an identical shape (`import:122-156`, `export:82-105`: call `load_events`, `filter(|e| e.target() == target)`, fold into the state), `append` (`import:158`; `export:109`) and `compact` (`import:171`; `export:123`). The two differ only in the event variants, the sentence wording, and the fold arms.

**Why it matters.** The Adapter is the right pattern: each side owns its event vocabulary and the generic crate knows no vocabulary. But the "filter by target then fold" rule is a property of every journal, and it is written twice because `jsonl_journal` offers no way to say "an event that belongs to a target".

**Remediation.**

```rust
// crates/libs/journal/src/lib.rs
/// An event recorded against one server and account.
pub trait TargetedEvent {
    fn target(&self) -> &ServerTarget;
}

/// Load `path` and fold the events for `target` into a fresh `S`.
pub fn load_for_target<E, S>(
    label: &str,
    path: &Path,
    target: &ServerTarget,
    on_unreadable: &mut dyn FnMut(usize, &serde_json::Error),
    mut fold: impl FnMut(&mut S, E),
) -> Result<S>
where
    E: DeserializeOwned + TargetedEvent,
    S: Default,
{
    let mut state = S::default();
    for event in load_events::<E>(label, path, on_unreadable)? {
        if event.target() == target {
            fold(&mut state, event);
        }
    }
    Ok(state)
}
// import/src/journal.rs:122 becomes: load_for_target("journal", path, target, &mut |i, e| on_unreadable(unreadable_line_sentence(path, i, e)), |state, event| match event { … })
```

Each side keeps its sentence and its fold arms; the filter loop is written once.

### DP10. The `tauri.ts` facade is bypassed; mutations have no route hook (3/10)

**Evidence.** `web/src/lib/tauri.ts` wraps 29 `invoke` calls behind 61 exports. Four files import Tauri directly: `web/src/lib/openPath.ts:1`, `saveFile.ts:1`, `localServer.ts:1` (`invoke` from `@tauri-apps/api/core`) and `auth.tsx:2` (`getCurrentWindow` from `@tauri-apps/api/window`). F15 lists the first three as copy-paste; here the point is that the facade exists and nothing enforces it, although `web/biome.json:53-62` already enforces two other import rules with `style/noRestrictedImports` `paths`.

On the query side, `useRouteQuery`/`useRouteInfiniteQuery`/`useRoutePagedList`/`useRouteCache` (`routeQuery.ts:114,134,234,368`) exist so that no call site remembers the account prefix. There is no `useRouteMutation` (grep): 20 `useMutation` calls (`web/src/lib/nameCollection.ts:204,215,229,254`, `savedSearches.ts:47`, `trash.ts:26`, `contactDetail.ts:65`, `useAccountProfile.ts:55`, `useSettingsAccount.ts:46`, `useToolsStatus.ts:48`, `screens/owner/useOwnerAccounts.ts:28,45`, `screens/settings/useApiTokens.ts:21`, `screens/owner/ServerSettingsPanel.tsx:36,41`, `DemoAccountCard.tsx:43`, `components/DownloadAttachmentButton.tsx:27`, `hooks/useStreamedMedia.ts:35`) each decide for themselves whether to call `invalidateAccount()` (22 sites by grep). `useServerState.ts:31` uses a raw `useQuery` with its own key on purpose and says why (`:18-22`: it runs before login).

**Why it matters.** A facade that is bypassed in four files is a convention, not a seam. The mutation side has the same gap the query side closed in ADR 0002: the rule ("every write marks the whole account's cache stale", `queryKeys.ts:9-11`) is applied by hand per call.

**Remediation.**

```json
// web/biome.json, inside "style"."noRestrictedImports"."options"."paths" (:56)
"@tauri-apps/api/core": "Call the desktop through src/lib/tauri.ts, the one module that knows the command names.",
"@tauri-apps/api/window": "Call the desktop through src/lib/tauri.ts."
// and an override for ["src/lib/tauri.ts"] setting "noRestrictedImports": "off", like the one for src/test/user.ts at :95-104.
```

```ts
// web/src/lib/routeQuery.ts
/** `useMutation`, marking the whole account's cache stale when it settles, as every write must. */
export function useRouteMutation<TData, TVariables, TContext = unknown>(
  options: UseMutationOptions<TData, Error, TVariables, TContext>,
): UseMutationResult<TData, Error, TVariables, TContext> {
  const cache = useRouteCache();
  return useMutation({
    ...options,
    onSettled: (data, error, variables, context) => {
      cache.invalidateAccount();
      return options.onSettled?.(data, error, variables, context);
    },
  });
}
```

The optimistic writes in `nameCollection.ts:254` keep their own `onMutate`/`onError` and only replace `useMutation` with `useRouteMutation`.

### DP11. Desktop event structs and their TypeScript twins are kept in step by hand (3/10)

**Evidence.** `src-tauri/src/commands/events.rs:3-5`: "These structs match the TypeScript types in `web/src/lib/types.ts`." The pairs: `ExtractProgressEvent` `events.rs:82` and `ImportProgressEvent` `types.ts:100`; `ExtractIssueEvent` `:158` and `ImportIssueEvent` `:118`; `ExtractFileDoneEvent` `:192` and `:147`; `ExtractFileWrittenEvent` `:206` and `:164`; `ExtractErrorEvent` `:228` and `:89`. The API side has a generator and a CI gate (`serverApi.ts:8-12`, `scripts/check-generated-api-types.sh`); the event side has neither: no `ts-rs`, `specta` or `tauri-specta` in `src-tauri/Cargo.toml` or `web/package.json` (grep), and no Rust test names `types.ts`. The `step` field is `String` in Rust and a seven-member union in TypeScript (`types.ts:101`).

**Why it matters.** The same DTO contract is enforced on one of the two process boundaries and trusted on the other. `useImportJob.test.tsx` tests against the TypeScript shape, so a Rust-side rename would pass every test and fail at runtime.

**Remediation.** The cheapest gate is the one the search language already uses (`tests/fixtures/search/web-queries.txt`, written by `web/src/lib/searchQuery.test.ts:617` and read by `crates/server/server/src/search/tests.rs:3617`): a JSON fixture of one serialised value per event struct, written by a `src-tauri` test and asserted against by a `web` test with the TypeScript types. A `ts-rs` derive with a checked-in output and a `check-generated-*.sh` is the heavier alternative. Either way, `Step` as an enum (DP3) makes the union a generated fact rather than a hand-typed one.

### DP12. Core carries `Err("cancelled")` strings beside its typed `Cancelled` (2/10)

**Evidence.** `Cancelled` (`crates/core/message-crate-core/src/process.rs:100`) is the typed error and `check_cancel` (`:107`) returns it. `parallel_for_each` in the same file (`:121`) returns `Vec<Result<T, String>>` and its doc says a job not yet started "records `Err("cancelled")`" (`:120`). F19 notes the exporter doc that promises the message text (`go-sms-pro-exporter/src/emit.rs:420`); this is the same inconsistency inside core.

**Remediation.** `parallel_for_each<J, T, E, F>(…) -> Vec<Result<T, E>>` with `E: From<Cancelled>`, so a cancelled job records `Err(Cancelled.into())` and the caller that wants a string calls `.to_string()` at its edge. **Unable to verify** whether any caller matches on the `"cancelled"` text; `src-tauri/src/commands/jobs.rs` and `commands/extract.rs` would show it (F19 names the same two files).

### DP13. Search registry and emitter joined by a string match (3/10)

**Evidence.** `FieldSpec` (`crates/server/server/src/search/fields.rs:31-40`) holds `word`, `value_type`, `lists`, `values`, `help`, `example`. `emit_one` (`emit.rs:360-382`) dispatches on `term.spec.word` by string: `"body" | "subject" | … => emit_text_word`, `"with" | "from" | … => emit_people_word`, `"kind" | … => emit_kind_word`, `"date" | … => emit_measure_word`, `"deleted"`, `"unsent"`, and a final `other => Err(QueryError … "{other}: has no emitter; add one in emit.rs")`. ADR 0004 says "Adding a filter is one entry in the module's field registry plus one SQL emitter"; the code makes that two edits with a runtime error between them.

**Why it matters.** The Interpreter is otherwise exact: a word cannot exist for the parser and not for `describe` because both read `FIELDS` (`fields.rs:1-2`). The emitter is the one reader that does not, so a word added to `FIELDS` without an arm in `emit_one` compiles and fails on the first query that uses it.

**Remediation.** Put the emitter in the registry so the match is exhaustive over an enum.

```rust
// crates/server/server/src/search/fields.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emitter { Text, People, Kind, Measure, Deletion(message_ir::Deletion) }

pub(crate) struct FieldSpec {
    pub word: &'static str,
    pub emitter: Emitter,
    /* value_type, lists, values, help, example as before */
}
// each FIELDS entry gains `emitter: Emitter::Text` and so on.

// emit.rs:360
fn emit_one(ctx: &ListCtx, out: &mut Sql, term: &FieldTerm, v: &Value) -> Result<(), QueryError> {
    match term.spec.emitter {
        Emitter::Text => emit_text_word(ctx, out, term, v),
        Emitter::People => emit_people_word(ctx, out, term, v),
        Emitter::Kind => emit_kind_word(ctx, out, term, v),
        Emitter::Measure => emit_measure_word(ctx, out, term, v),
        Emitter::Deletion(kind) => emit_deletion_word(ctx, out, term, v, kind),
    }
}
```

The `other =>` arm and its `QueryErrorKind::BadValue` misuse disappear.

## 4. Patterns applied well

These are listed so they are not "fixed", and because they are the templates the findings above point at.

- **`ProjectionHooks`** (`crates/libs/ir/src/projection.rs:85-200`): three required methods (`export`, `service`, `source`), fourteen with defaults (`normalize_handle`, `attachment_digests`, `vendor_key`, `reply_key`, `reply`, `reactions`, `attachment_to_ir`, `role`, `subject`, `sort_key_unit`, `message_order`, `message_kind`, `participants`, `group_title`, `packaging_stem_suffix`), one driver (`pending_to_document` `:239`), five production implementations and three test ones (`:516,558,763`). This is the Template Method done as the pattern intends.
- **`MergedArchive`** (`format_sink.rs:21`): the lower crate defines the trait and "knows no archive format by name" (`:19-20`); the two owners implement it; `FormatSink::finish` records the archive's files before writing (`:206-208`) and refuses a merged format with no archive (`:401` test). `ExportWriter::with_spool`/`with_archive` (`export_writer.rs:127,138`) are consuming builders, the idiomatic Rust shape.
- **`local_server.rs` `step`** (`:332`): the state and the actions are data; `apply` (`:768`) runs the actions and feeds a failed spawn back as an event (`:780`). F11 notes the spawn happens under the `Inner` mutex; the design is still the one to copy for DP8.
- **`auth_guard!`** (`server.rs:323-381`): seven newtypes, each an Axum extractor wrapping one `require_*` check, so the guard is in the handler's signature and visible in the OpenAPI `security` attribute. Zero direct `require_*` calls in handlers.
- **`MembershipSpec`** (`db/named_membership.rs:61`): where two tables differ only in names and reserved words, a `static` table is the whole Strategy; no trait, no generics.
- **Search module** (ADR 0004): lexer (`lex.rs`), parser to `Expr` (`parse.rs:34`), registry (`fields.rs:45`), emitter (`emit.rs:161`), `Sql` builder (`bridge.rs:12`), `ListCtx` for the three lists, `compile` pure of I/O (`emit.rs:53`), and a TypeScript twin tested against a shared fixture. DP13 is the one joint that is not type-checked.
- **Middleware as data**: `declared_query::DeclaredQueries::from_spec` (`declared_query.rs:36`) reads the assembled OpenAPI document to refuse undeclared query parameters, so the check "needs no list of its own to keep in step with the handlers" (`:9-10`). `extract::{Query, Path, Json}` (`extract.rs:23,43,62`) wrap Axum's to answer as problem documents. `WriteTx` (`write_tx.rs:36`) makes the only transaction kind a type.
- **Command**: `start_job`/`spawn_job` (`jobs.rs:70,118`) with `Job: Drop` (`:49`) so an early return ends the job; a panic is caught and reported (`:140-155`); five commands compose config, sinks, job and closure in the same order.
- **Dependency injection by parameter**: `run_with(config, spawn)` (`imessage-ir-exporter/src/run.rs:182`) injects the process spawner for tests; `ProgressSink::unpaced` (`progress.rs:167`) injects the pacing policy; `createQueryClient` (`routeQuery.ts:68`) is a factory "so each test gets a client of its own".
- **Service layer**: `import_jsonl_files_on_conn` (`imports_api/mod.rs:314`) is the one import entry for HTTP, CLI and demo build.

## 5. Unable to verify

| Claim | Proving file(s) |
|---|---|
| The name of the struct `transcode_staged` hands its progress callback (DP3 snippet calls it `TranscodeProgress`) | `crates/libs/staging/src/transcode.rs`, the callback parameter of `transcode_staged` |
| The full set of backward stage moves a resume makes, which fixes the `may_move_to` table (DP8) | `web/src/screens/import/useImportJob.ts` (`landOn`, `resumeAtReview`), `web/src/screens/import/resumeDecision.ts`, `crates/server/server/src/imports_api/tests.rs` |
| Whether any caller matches cancellation by the `"cancelled"` text (DP12, as F19) | `src-tauri/src/commands/jobs.rs`, `src-tauri/src/commands/extract.rs` |
| Whether `Form::to_config`'s two `ffmpeg_available()` reads are the only implicit reads of `tools_state()` outside the media crate (DP7) | `grep -rn "media::" crates/core src-tauri/src` beyond the sites listed; `crates/libs/media/src/lib.rs` for other public readers |
| Whether Biome 2.5's `noRestrictedImports` `paths` accepts a subpath such as `@tauri-apps/api/core` as written (DP10); the existing rules use a bare package and an `importNames` form | `web/biome.json:53-62`, Biome's rule documentation for the installed version (`web/package.json:33`) |

## 6. Suggested order of work

1. **DP2 run tail** (`run_pipeline_with`, one afternoon): removes the one hand-copied tail and lets all seven exporters start the same way.
2. **DP1 with F3**: the exporter registry in the desktop, then the trait. DP2's `ConvertContext` follows and makes F8's shared skeleton a mechanical extraction.
3. **DP4, `LogSink` first**: ends the `None → stderr` policy in a library and shortens 21 signatures; the other three sinks follow the same recipe.
4. **DP13 and DP9**: small, each removes a runtime failure mode or a duplicated loop, and each is contained in one crate.
5. **DP8**: write the web `nextPhase` first (it is tested like `step`), then the server table once the resume moves are listed.
6. **DP5, DP10, DP3, DP7, DP11, DP6, DP12** in that order; DP6 is a documentation change only.
