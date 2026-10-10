# Code quality metrics and standards audit

- Audit: `code-quality-metrics-standards`
- Date: 2026-10-10
- Commit: `530dc1791` (branch `docs/initial-software-design-analysis`)
- Scope: the whole codebase, less `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`, the generated `web/src/lib/serverApi.types.ts`, and the other checkouts under `.claude/worktrees/` and `.worktrees/`. Every file set below is a `find` over `crates/`, `src-tauri/src`, `src-tauri/build.rs` (Rust, 441 files) and `web/src` (TypeScript, 542 files).
- Companion: `audits/2026-10-09-initial-software-design-analysis.md` already measured file sizes, the test count, `unwrap` outside tests, `allow(clippy::…)`, `biome-ignore` and `any`. Those figures are cited, not repeated, except where this audit's method gives a different number and says why.

## 1. Summary

The repository holds to its written standards almost completely. Every rustc naming lint passes, there are no `TODO`/`FIXME`/`XXX`/`HACK` markers in 186,000 lines of source, no `@ts-ignore`, `biome-ignore` or `eslint-disable`, no non-null assertion and no type-position `any` in the web app's production code, 99.2% of public Rust items are documented, and there are 6 lint-suppression attributes in production Rust, 3 of them `#[expect]` with a reason. The test corpus is larger than the code it tests (1.13 test lines per production line in Rust, 0.91 in the web app).

The quality problems are concentrated, not spread. Nine Rust functions exceed 150 lines and 23 take more than 6 parameters; one function (`crates/libs/go-sms-mms/src/mms.rs:218`) carries a 65-arm `match` and 60 `?` operators. In the web app, 15 production functions exceed 200 lines and 7 exceed 300; `ImportScreen` is 780 lines with a cyclomatic proxy of 140 and 33 `useState` calls. Server row mapping is hand-written through 100 positional `try_get(n)` calls in 7 files although `sqlx::FromRow` is already in use. The web `tsconfig.json` has `strict` on but none of the extra strictness flags; 104 TypeScript exports are imported by no other file, 5 of them never used at all.

Headline numbers (methods in section 2, commands in section 17):

| Metric | Rust | TypeScript (`web/src`) |
|---|---|---|
| Production lines / test lines | 106,063 / 120,282 (ratio 1.13) | 42,393 / 38,753 (ratio 0.91) |
| Production functions measured | 3,564 | 1,263 |
| Functions > 60 lines / > 100 / > 150 | 146 / 31 / 9 | 58 / 29 / 17 |
| Longest production function | 200 lines (`db/address_book.rs:923 apply`) | 780 lines (`screens/ImportScreen.tsx:111 ImportScreen`) |
| Cyclomatic proxy > 10 / > 20 / > 30 | 221 / 35 / 8 | 77 / 27 / 16 |
| Functions with > 6 parameters | 23 | 0 |
| Brace nesting depth >= 7 inside a body | 7 functions | 3 functions (JSX inflates this) |
| `TODO|FIXME|XXX|HACK` | 0 | 0 |
| Lint suppressions in production code | 6 (`#[allow]` 3, `#[expect]` 3) | 0 |
| Documented `pub` items (outside tests and trait impls) | 1,928 of 1,944 (99.2%) | n/a |
| Type assertions / non-null assertions / `any` (production) | n/a | 31 / 0 / 0 |
| Exports or `pub fn` referenced nowhere else | 1 `pub fn` | 104 exports (5 fully unused) |

## 2. Method and assumptions

- **Line counts** come from `cloc 2.06` (code, comment, blank) and `wc -l` (raw). "Production" and "test" lines come from a scratch Python parser (section 17) that classifies as test: every file under a `tests/` directory; `tests.rs`, `testutil.rs`, `test_support.rs` and anything under `test_support/`; every file module declared under `#[cfg(test)]` (97 files, found by reading each `#[cfg(test)]` followed by `mod name;`, including `#[path = "lib_tests.rs"]`); and every inline `#[cfg(test)] mod … { … }` block. That is why `crates/server/server/src/openapi/credential_matrix.rs` and `document_rules.rs` (both `#[cfg(test)]` modules) and `db/write_guard.rs` count as test here. The parser counts one line more per file than `wc -l` (the trailing newline), so production plus test lines exceed the raw total by 441, one per file.
- **Functions** are `fn` items whose body was found by brace matching over source with strings, chars and comments blanked out. Length is from the signature line to the closing brace, inclusive. TypeScript functions are `function` declarations, `const x = (…) =>` arrows, and class or object methods; anonymous callbacks are not counted on their own (they add to their parent's length).
- **Cyclomatic proxy** = 1 + `if` + loops + `&&` + `||` + `?` + (match arms beyond one per `match`). For TypeScript: 1 + `if` + loops + `case` + `catch` + `&&` + `||` + `??` + ternary `?`. It is a proxy: `?` in Rust is one early return per use, so the figure "without `?`" is also given. Real cyclomatic numbers would need `rust-code-analysis` or `cargo-geiger`-class tooling, which the run rules exclude.
- **Nesting** is the maximum brace depth inside the body relative to the body's own brace. In `.tsx` every `{…}` JSX expression counts, so TSX depth overstates control-flow nesting.
- **Doc coverage** counts `pub`, `pub(crate)` and `pub(super)` `fn|struct|enum|trait|type|const|static|mod` items outside test code and outside `impl … for` blocks (trait impls are not linted by `missing_docs`). An item is documented when a `///` or `#[doc]` precedes it, skipping attributes and plain `//` comments; a `pub mod x;` is documented when `x.rs` or `x/mod.rs` opens with `//!`.
- **Magic numbers** are numeric literals other than 0, 1, 2 and single digits, outside `const`/`static` items, attributes, comments and strings. Two classes are then removed and reported apart: `status = NNN` arguments of `#[utoipa::path]` (declarative) and the phone country table `crates/libs/phone/src/countries.rs` (data).
- Nothing was compiled, built or type-checked. Where a claim would need `cargo clippy` or `tsc` to confirm, it is marked **Unable to verify**.

## 3. Standards in force and compliance

| Standard | Where | What it enforces | Compliance found |
|---|---|---|---|
| Toolchain pin | `rust-toolchain.toml` | Rust 1.98.1 with rustfmt, clippy, llvm-tools | In place |
| `[workspace.lints.rust] missing_docs = "warn"` | `Cargo.toml`; every one of the 35 crates has `[lints] workspace = true`; `src-tauri/Cargo.toml:81` repeats it | Public items documented | 99.2% of `pub` items documented; the 16 gaps are 15 in the `demo-seed` binary (section 8) and 1 test-only item |
| Clippy at `-D warnings` on `--all-targets` | `.github/workflows/ci.yml:158`, `:234` | Every rustc and clippy warning is an error, tests included | 6 suppressions in production code, 3 in test code (section 9.2) |
| `clippy.toml` `disallowed-methods` | `crates/server/server/clippy.toml` | No deferred `begin()`; `crate::db::begin_write` only | Not measured (needs clippy) |
| `[workspace.lints.clippy]` allows | `Cargo.toml` | `str_to_string`, `redundant_pub_crate` allowed, with reasons | In place |
| cargo-mutants config | `.cargo/mutants.toml` | Excludes `demo-seed`, `chat-db-fixture`, `build-version`, `api-types`, `testutil.rs`, Display/Debug impls | In place; no local run artifact (section 16) |
| Biome 2.5.14 | `web/biome.json` | `recommended` preset plus `useHookAtTopLevel`, `useExhaustiveDependencies`, `noRestrictedElements` (raw `<button>`), `noRestrictedImports`, `useComponentExportOnlyModules`; 100-column lines; `serverApi.types.ts` excluded | 0 `biome-ignore` (prior audit), 0 `eslint-disable`, 0 `@ts-ignore` |
| TypeScript | `web/tsconfig.json` | `strict: true`; tests type-checked through `tsconfig.test.json` | `noUncheckedIndexedAccess`, `noImplicitReturns`, `noFallthroughCasesInSwitch`, `noUnusedLocals`, `noUnusedParameters`, `exactOptionalPropertyTypes`, `noImplicitOverride`, `noPropertyAccessFromIndexSignature`, `verbatimModuleSyntax` are all absent (section 15, F7) |
| Vitest | `web/vite.config.ts:72` | `src/**/*.{test,spec}.{ts,tsx}`, jsdom for `.tsx` | No coverage configuration; coverage is a Rust-only report (`scripts/coverage.sh`) |
| Writing style | `docs/agents/writing-style.md` | "directory" never "folder"; reasons attached to restrictions | Comments are 33% of production Rust lines, 88% of them doc comments (section 8) |

## 4. Size per area

Raw lines are `wc -l`; code/comment are `cloc` (so code + comment + blank = raw). Production/test lines are the parser's classification of raw lines.

| Area | Files | Raw lines | Code | Comment | Production lines | Test lines | Test/prod |
|---|---|---|---|---|---|---|---|
| server (`crates/server/server`) | 166 | 111,143 | 87,773 | 16,454 | 47,324 | 63,985 | 1.35 |
| demo-seed (`crates/server/demo-seed`) | 12 | 5,554 | 4,308 | 816 | 3,917 | 1,649 | 0.42 |
| core (`crates/core`) | 16 | 6,439 | 4,825 | 1,150 | 3,246 | 3,209 | 0.99 |
| pipeline libs (`crates/libs`, 20 crates) | 121 | 55,340 | 42,421 | 9,122 | 27,299 | 28,162 | 1.03 |
| exporters (`crates/exporters`, 7) | 74 | 26,270 | 20,968 | 3,615 | 12,061 | 14,283 | 1.18 |
| helpers (`crates/helpers`, 3) | 21 | 8,354 | 6,120 | 1,555 | 4,805 | 3,570 | 0.74 |
| src-tauri | 31 | 12,804 | 9,232 | 2,593 | 7,411 | 5,424 | 0.73 |
| **Rust total** | 441 | 225,904 | 175,647 | 35,305 | 106,063 | 120,282 | 1.13 |
| web (`web/src`, generated file excluded) | 542 | 80,604 | 63,714 | 9,461 | 42,393 | 38,753 | 0.91 |

File size (production files only, those with no test code at all; files that hold an inline `#[cfg(test)]` module are not in these counts):

| Area | Production files | > 300 lines | > 500 | > 1,000 |
|---|---|---|---|---|
| server | 70 | 39 | 25 | 13 |
| demo-seed | 4 | 2 | 2 | 1 |
| core | 3 | 1 | 0 | 0 |
| libs | 36 | 13 | 11 | 2 |
| exporters | 25 | 8 | 4 | 0 |
| helpers | 1 | 0 | 0 | 0 |
| src-tauri | 15 | 5 | 3 | 1 |
| **Rust** | 154 | 68 | 45 | 17 |
| web components | 122 | 7 | 3 | 0 |
| web screens | 96 | 12 | 7 | 1 |
| web lib | 111 | 6 | 3 | 0 |
| web hooks | 6 | 0 | 0 | 0 |
| **web** | 338 | 25 | 13 | 1 |

The 17 Rust production files over 1,000 lines: `reset_demo.rs` 2,189, `db/staging.rs` 2,036, `imports_api/mod.rs` 1,862, `server.rs` 1,829, `accounts_api.rs` 1,514, `demo-seed/src/conversations.rs` 1,482, `search/emit.rs` 1,299, `db/imports.rs` 1,266, `process_assets.rs` 1,226, `db/address_book.rs` 1,221, `dedupe.rs` 1,185, `db/audit_trail.rs` 1,168, `imports_api/staging.rs` 1,097, `crates/libs/mail/src/lib.rs` 1,075, `db/conversation_messages.rs` 1,071, `src-tauri/src/tool_downloads.rs` 1,029, `crates/libs/media/src/process.rs` 1,019 (server paths are under `crates/server/server/src/`). The prior audit's section 4.3 covers the four largest; this list adds the rest. The web production files over 500: `screens/import/useImportJob.ts` 2,085, `lib/serverApi.ts` 982, `screens/import/ImportFormFields.tsx` 898, `screens/ImportScreen.tsx` 891, `lib/tauri.ts` 797, `components/InfiniteOffsetList.tsx` 676, `screens/import/ImportRunView.tsx` 673, `screens/ContactList.tsx` 569, `screens/settings/SystemSection.tsx` 565, `lib/auth.tsx` 559, `components/SearchBar.tsx` 553, `screens/TrashScreen.tsx` 540, `components/messages/chatBubbleShared.tsx` 528.

Test files are larger still: `crates/server/server/src/imports_api/tests.rs` 6,546 lines, `search/tests.rs` 4,113, `contacts_api/tests.rs` 3,807, `crates/libs/import/tests/import_mock.rs` 3,753, `web/src/screens/import/useImportJob.test.tsx` 3,569.

## 5. Function length

### 5.1 Rust, production code (3,564 functions)

| Area | Functions | > 50 | > 60 | > 100 | > 150 | Max | Median | 90th pct |
|---|---|---|---|---|---|---|---|---|
| server | 1,477 | 93 | 69 | 13 | 3 | 200 | 12 | 41 |
| demo-seed | 140 | 9 | 6 | 1 | 0 | 106 | 10 | 47 |
| core | 143 | 2 | 1 | 1 | 0 | 116 | 7 | 23 |
| libs | 981 | 43 | 30 | 11 | 5 | 164 | 9 | 33 |
| exporters | 395 | 35 | 25 | 3 | 1 | 181 | 11 | 46 |
| helpers | 162 | 8 | 5 | 1 | 0 | 126 | 11 | 38 |
| src-tauri | 266 | 12 | 10 | 1 | 0 | 143 | 8 | 33 |
| **all** | 3,564 | 202 | 146 | 31 | 9 | 200 | 10 | 38 |
| (test functions) | 4,367 | 371 | 223 | 43 | 14 | 390 | 17 | 47 |

Top 15 production functions by length (with the per-function proxy counts):

| Lines | Location | Function | Params | Depth | Proxy | Notes |
|---|---|---|---|---|---|---|
| 200 | `crates/server/server/src/db/address_book.rs:923` | `apply` | 5 | 5 | 37 | 9 loops, 5 `match`, 11 `?` |
| 181 | `crates/exporters/whatsapp-exporter/src/run.rs:27` | `run` | 1 | 4 | 43 | 22 `?`, 12 arms |
| 164 | `crates/libs/go-sms-mms/src/mms.rs:218` | `header` | 3 | 4 | 123 | 65 arms, 60 `?` |
| 161 | `crates/server/server/src/imports_api/staging.rs:447` | `write_rows` | 2 | 4 | 22 | |
| 158 | `crates/libs/import/src/run.rs:278` | `run` | 2 | 4 | 27 | |
| 157 | `crates/libs/staging/src/transcode.rs:653` | `apply_transcode` | 9 | 5 | 22 | `#[allow(too_many_arguments)]` |
| 156 | `crates/server/server/src/db/conversation_messages.rs:915` | `get_conversation_messages` | 6 | 3 | 16 | |
| 155 | `crates/libs/mail/src/parse.rs:37` | `mail_message_from_eml_bytes` | 1 | 3 | 29 | |
| 151 | `crates/libs/ir/src/projection.rs:239` | `pending_to_document` | 3 | 4 | 12 | |
| 143 | `src-tauri/src/local_server.rs:332` | `step` | 2 | 6 | 29 | 25 arms; pure state machine by design (`:330` doc) |
| 140 | `crates/server/server/src/db/address_book.rs:614` | `plan` | 2 | 6 | 29 | 19 arms, 7 `match` |
| 130 | `crates/libs/staging/src/write_queue.rs:506` | `drain_write_queue` | 6 | 8 | 17 | deepest nesting in the repo |
| 126 | `crates/helpers/chat-db-fixture/src/lib.rs:172` | `write_chat_db` | 1 | 1 | 1 | straight-line SQL, fixture builder |
| 126 | `crates/libs/staging/src/write_queue.rs:768` | `write_one_unit` | 7 | 4 | 14 | |
| 124 | `crates/libs/reexport/src/lib.rs:88` | `convert_export` | 2 | 3 | 34 | |

Test functions over 150 lines (14): the longest are `crates/server/server/src/search/tests.rs:298 seeded` (390 lines, a fixture builder), `crates/server/server/tests/server_log.rs:84` (245), `crates/server/server/src/imports_api/tests.rs:5499` (211), `search/tests.rs:2778` (193), `contacts_api/tests.rs:1037` (185) and `:689` (183).

### 5.2 TypeScript, production code (1,263 functions)

| Area | Functions | > 50 | > 60 | > 100 | > 150 | > 200 | > 300 | Max | Median |
|---|---|---|---|---|---|---|---|---|---|
| components | 286 | 16 | 14 | 7 | 4 | 3 | 1 | 344 | 9 |
| hooks | 11 | 1 | 1 | 0 | 0 | 0 | 0 | 87 | 9 |
| lib | 574 | 7 | 4 | 1 | 1 | 1 | 1 | 356 | 5 |
| screens | 390 | 39 | 38 | 21 | 12 | 11 | 5 | 780 | 8 |
| **all** | 1,263 | 64 | 58 | 29 | 17 | 15 | 7 | 780 | 7 |
| (test functions) | 322 | 2 | 2 | 1 | 1 | 0 | 0 | 183 | 7 |

Top 15 production functions by length:

| Lines | Location | Function | Proxy | Notes |
|---|---|---|---|---|
| 780 | `web/src/screens/ImportScreen.tsx:111` | `ImportScreen` | 140 | 52 `if`, 34 `&&`, 23 ternaries; 33 `useState` in the file |
| 593 | `web/src/screens/import/ImportFormFields.tsx:305` | `ImportFormFields` | 92 | 49 ternaries; props interface `:65-131` has 57 members |
| 385 | `web/src/screens/TrashScreen.tsx:100` | `TrashScreen` | 83 | depth 7 |
| 356 | `web/src/lib/auth.tsx:163` | `SessionProvider` | 37 | 20 `if` |
| 344 | `web/src/components/AppLayout.tsx:110` | `AppLayout` | 74 | |
| 319 | `web/src/screens/import/useImportJob.ts:1766` | `useImportJob` | 55 | the hook facade of a 2,085-line module (prior audit F4) |
| 304 | `web/src/screens/LoginScreen.tsx:142` | `LoginScreen` | 54 | |
| 300 | `web/src/components/ContactDrawer.tsx:143` | `OneContactDrawer` | 66 | |
| 286 | `web/src/screens/OnboardingScreen.tsx:114` | `OnboardingScreen` | 43 | |
| 272 | `web/src/screens/import/ImportRunView.tsx:401` | `stagingReviewContent` | 54 | 36 ternaries |
| 270 | `web/src/screens/settings/SystemSection.tsx:296` | `DataDirectory` | 7 | long but flat |
| 218 | `web/src/components/ThemeSettings.tsx:47` | `ThemeSettings` | 24 | |
| 218 | `web/src/screens/message/useConversationMessages.ts:119` | `useConversationMessages` | 44 | |
| 215 | `web/src/screens/ExportScreen.tsx:93` | `ExportScreen` | 30 | |
| 210 | `web/src/screens/import/useImportJob.ts:1491` | `runImport` | 26 | 5 positional params |

## 6. Complexity

### 6.1 Cyclomatic proxy

Rust production: 221 functions over 10, 35 over 20, 8 over 30; counting `?` as zero (it is one guarded return each) the figures are 105, 14 and 1. The outliers:

| Proxy | Location | Function | What drives it |
|---|---|---|---|
| 123 | `crates/libs/go-sms-mms/src/mms.rs:218` | `header` | 65 `match` arms, each arm decoding one WSP header with `?` |
| 48 | `crates/server/server/src/search/value.rs:130` | `parse_date_span` | 16 arms, 11 `&&`, 19 `?`; 77 lines |
| 43 | `crates/exporters/whatsapp-exporter/src/run.rs:27` | `run` | 22 `?`, 9 `if`, 12 arms over 181 lines |
| 37 | `crates/server/server/src/db/address_book.rs:923` | `apply` | 9 loops over 200 lines |
| 35 | `crates/libs/reexport/src/lib.rs:478` | `detect_ir_export` | 11 `if`, 16 arms in 75 lines |
| 34 | `crates/libs/reexport/src/lib.rs:88` | `convert_export` | 10 `if`, 14 `?` |
| 33 | `crates/server/server/src/db/conversation_messages.rs:499` | `fetch_message_page` | 31 `?`: one per `row.try_get(n)?` |
| 31 | `crates/exporters/go-sms-pro-exporter/src/xml.rs:189` | `read_sms_elements` | 13 `if`, 20 arms, depth 8 |
| 28 | `crates/server/server/src/db/imports.rs:455` | `import_from_row` | 27 `?` in 43 lines, all `row.try_get(n)?` |

TypeScript production: 77 over 10, 27 over 20, 16 over 30, 8 over 50. Everything over 50 is a screen or layout component: `ImportScreen` 140, `ImportFormFields` 92, `TrashScreen` 83, `AppLayout` 74, `OneContactDrawer` 66, `useImportJob` 55, `stagingReviewContent` 54, `LoginScreen` 54. Outside components, `lib/auditTrail.ts:100 describeAuditEntry` is a 20-`case` switch (proxy 35), `lib/tauri.ts:17 invokeExtract` has 18 ternaries in 28 lines (proxy 37), and `screens/settings/storage/storageUtils.ts:191 toImportSummaryView` has 22 ternaries in 48 lines (proxy 44).

### 6.2 Switch complexity (large `match` / `switch`)

Rust production functions with 15 or more arms: 19. Beyond `header` (65) and `step` (25), they are flat one-`match` lookups that are long but simple: `server.rs:604 problem_type` (24 arms, 30 lines), `problem.rs:103 slug`, `:162 title`, `:199 page` (23 arms each), `db/audit_trail.rs:91 as_str` (20), `crates/libs/http/src/auth_error.rs:140 kind` (15). The three `problem.rs` functions walk the same 23-variant enum three times; a single `fn parts(&self) -> (&'static str, &'static str, &'static str)` would hold the table once. TypeScript has one large switch, `describeAuditEntry` (20 cases).

### 6.3 Nesting depth

Rust production functions with body brace depth >= 5: 91; >= 6: 29; >= 7: 7:

| Depth | Location | Function | Lines |
|---|---|---|---|
| 8 | `crates/exporters/go-sms-pro-exporter/src/xml.rs:189` | `read_sms_elements` | 83 |
| 8 | `crates/libs/sbr/src/read.rs:923` | `infer_owner_phones` | 34 |
| 8 | `crates/libs/staging/src/write_queue.rs:506` | `drain_write_queue` | 130 |
| 7 | `crates/helpers/imessage-reader/src/fields.rs:108` | `build_part_records` | 75 |
| 7 | `crates/server/server/src/db/address_book.rs:467` | `row_identity` | 107 |
| 7 | `crates/server/server/src/import_cli.rs:267` | `files_by_source` | 36 |
| 7 | `src-tauri/src/local_server.rs:768` | `apply` | 19 |

TypeScript: 12 functions at depth >= 6, 3 at depth 7 (`components/import/VirtualizedImportIssuesTable.tsx:97 IssuesTable`, `components/StepProgress.tsx:109 StepProgress`, `screens/TrashScreen.tsx:100 TrashScreen`); JSX braces inflate these.

### 6.4 Recursion

31 production Rust functions call their own name; 23 of them are delegating wrappers (`emit_log` calling `self.inner.emit_log(…)`) and `Drop` impls. The genuinely recursive ones are directory and tree walks, bounded by the input: `crates/core/message-crate-core/src/pipeline.rs:47 discover_files_into`, `crates/exporters/sms-backup-plus-exporter/src/assets.rs:68 walk_parts`, `crates/libs/ir-format/src/placeholders.rs:47 remove_directory_contents`, `crates/libs/mail/src/parse.rs:450 collect_mime_attachments`, `crates/libs/reexport/src/lib.rs:751 copy_dir_recursive`, `crates/server/demo-seed/src/lib.rs:420 copy_dir_recursive`, `crates/server/server/src/openapi/response_fields.rs:117 name_refs`, `openapi/shared_parts.rs:79 for_each_schema`, `reset_demo.rs:1653 list_directory`. None recurses on user-controlled depth without a filesystem bound. The two `copy_dir_recursive` functions are near-duplicates (16 lines; they differ in one `create_dir_all` placement, an `is_dir` spelling, and the arrow in a message).

## 7. Parameter counts

Rust production functions with more than 6 parameters: 23 (182 have 5 or more). One has 9, the rest 7:

| Params | Location | Function |
|---|---|---|
| 9 | `crates/libs/staging/src/transcode.rs:653` | `apply_transcode` (`#[allow(clippy::too_many_arguments)]` at `:652`) |
| 7 | `crates/exporters/go-sms-pro-exporter/src/emit.rs:200` | `add_pdu_message` |
| 7 | `crates/exporters/sms-backup-restore-exporter/src/write.rs:165` | `restore_mms` (`#[allow(clippy::too_many_arguments)]` at `:164`) |
| 7 | `crates/libs/import/src/run.rs:630` | `submit_file` |
| 7 | `crates/libs/staging/src/transcode.rs:908` | `apply_too_large` |
| 7 | `crates/libs/staging/src/write_queue.rs:321`, `:483`, `:768` | `drain_write_queue_with_loader`, `drain_units`, `write_one_unit` |
| 7 | `crates/server/demo-seed/src/conversations.rs:115`, `:1066` | `write_all`, `write_conversation_header` |
| 7 | `crates/server/demo-seed/src/lib.rs:547` | `restore_previous_paths` |
| 7 | `crates/server/demo-seed/src/personas.rs:270`, `:427` | `sample_skewed_msgs_per_year`, `finish_group_spec` |
| 7 | `crates/server/server/src/assets_api.rs:242` | `store_verified_inner` |
| 7 | `crates/server/server/src/db/contacts/read.rs:165` | `list_contacts_sorted` |
| 7 | `crates/server/server/src/db/conversation_messages.rs:451` | `load_messages_from` |
| 7 | `crates/server/server/src/db/conversations.rs:128` | `list_conversations_sorted` |
| 7 | `crates/server/server/src/imports_api/contact_name.rs:132` | `resolve_incoming_sender_handle` |
| 7 | `crates/server/server/src/imports_api/staging.rs:76`, `:213` | `try_store_converted`, `prepare_attachments` |
| 7 | `crates/server/server/src/reset_demo.rs:723`, `:847` | `rebuild_demo_account`, `import_demo_sources_with` |
| 7 | `src-tauri/src/commands/format.rs:29` | `format` (Tauri command; 2 of the 7 are `State` and `AppHandle`) |

Clippy's `too_many_arguments` threshold is 7 and the lint reports a count above it, so the 22 seven-parameter functions pass without an attribute. `restore_mms` has exactly 7, which means the `#[allow]` at `write.rs:164` may no longer be doing anything. **Unable to verify** without running clippy; replacing `#[allow]` with `#[expect(clippy::too_many_arguments)]` would make the compiler say so (F5).

TypeScript: no production function takes more than 6 positional parameters; 7 take 5 or 6. The two 6-parameter functions are both named `checkOptionalPath` (`web/src/lib/imessageImport.ts:102`, `web/src/lib/whatsappImport.ts:122`) and differ only in the error-key type and two identifier names (`diff` in section 17).

## 8. Documentation density

Doc coverage of public items (production code, outside trait impls):

| Visibility | Items | Undocumented | Coverage |
|---|---|---|---|
| `pub` | 1,944 | 16 | 99.2% |
| `pub(crate)` / `pub(super)` | 841 | 24 | 97.1% |

The 16 undocumented `pub` items: 15 are in `crates/server/demo-seed` (`src/assets.rs:19 JPG_PHOTOS`; `src/config.rs:91 ContactsConfig`, `:104 LabelsConfig`, `:113 OneToOneConfig`, `:129 GroupsConfig`, `:154 MessagesConfig`, `:178 EdgeCasesConfig`; `src/personas.rs:20 MessageScope`, `:27 Contact`, `:42 Unassigned`, `:49 GroupSpec`, `:67-70 EMPTY_GROUP_HANDLE`, `EMPTY_GROUP_MEMBERS`, `EMPTY_THREAD_HANDLE`, `ORPHAN_SENDER`), and 1 is `src-tauri/src/run_directories.rs:487 Scratch::new`, inside a `#[cfg(test)] impl` (test code the parser's item pass did not exclude; not a gap). `demo-seed` is a binary crate; rustc's `missing_docs` reports only items reachable from a library's public surface, so this is the one crate where the workspace lint cannot fire (inference from the lint's definition; **Unable to verify** without a clippy run). The 24 undocumented restricted items are listed in section 17's output and are mostly parser-internal structs (`RawRow`, `ChatJson`, `MessageJson`, `XmlMessage`, `Token`, `TokenKind`, `Probe`, `PromoteStats`) plus `crates/server/server/src/credentials.rs:23 AUTH_RATE_MAX` and four `pub(super) fn` in `openapi/credential_matrix.rs` (test code).

Comment density (comment lines per production line, parser): Rust 0.33 overall; server 0.34, libs 0.34, core 0.36, exporters 0.30, helpers 0.33, src-tauri 0.35, demo-seed 0.21. By crate the low end is `contacts` 0.15, `ir-format` 0.29, `sbr` 0.23, `obfuscate` 0.21, `go-sms-pro-exporter` 0.21; the high end `staging` 0.41, `ios-backup` 0.43, `api-types` 0.43, `http` 0.40, `journal` 0.40. 30,933 of the 35,104 Rust comment lines (88%) are `///` or `//!`. TypeScript: 0.19 overall; `components/` 0.11, `lib/` 0.26, `screens/` 0.20.

## 9. Inventory of discipline markers

### 9.1 `TODO|FIXME|XXX|HACK`

Zero, in both languages, by two methods (parser and `grep -w`). A case-insensitive sweep of `//` comments for `todo`, `fixme`, `hack` also finds nothing.

### 9.2 Lint suppressions

| Location | Attribute | Code | Reason given |
|---|---|---|---|
| `crates/exporters/sms-backup-restore-exporter/src/write.rs:164` | `#[allow(clippy::too_many_arguments)]` | production | none; function has 7 params (section 7) |
| `crates/libs/staging/src/transcode.rs:652` | `#[allow(clippy::too_many_arguments)]` | production | none; 9 params |
| `crates/server/demo-seed/src/personas.rs:218` | `#[allow(clippy::if_same_then_else)]` | production | yes, in the comment above (`:216-217`) |
| `crates/exporters/whatsapp-exporter/src/parse.rs:39`, `:49`, `:56` | `#[expect(dead_code, reason = …)]` | production | yes, inline |
| `crates/libs/journal/src/lib.rs:272` | `#[allow(dead_code)]` | test | |
| `crates/server/server/tests/common/mod.rs:19`, `tests/exit_with_parent.rs:19` | `#[allow(dead_code)]` | test | |

No `#![allow(…)]` at crate or module level anywhere. No `allow(non_snake_case|non_camel_case_types|non_upper_case_globals)`. The prior audit's "3 `allow(clippy::…)`" matches the three above.

### 9.3 `unsafe`

22 production lines with `unsafe` in 5 files (plus 5 in the test-only `db/write_guard.rs` SQLite authorizer the prior audit named): `crates/helpers/imessage-reader/src/backup.rs:104-109` and `main.rs:76` (`std::env::set_var`, which is `unsafe` since Rust 2024), `crates/libs/ios-backup/src/reader_build.rs:63`, `crates/server/server/src/db/sqlite_functions.rs:40-84` (`sqlite3_auto_extension` and a `unicode_lower` C callback), `crates/server/server/src/exit_with_parent.rs:92-150` (Win32 `OpenProcess`), `src-tauri/src/local_server/process_tree.rs:98-206` (`libc::waitid`, Win32 job objects). Every site is an FFI or process-environment boundary; none is in data parsing.

### 9.4 `panic!` family in production code

12 lines: `crates/helpers/imessage-reader/src/data_source.rs:163` (`panic!("Database connection is closed!")`, the one panic reachable from a code path rather than a build or invariant), `crates/libs/reexport/src/lib.rs:472-473` and `crates/server/server/src/server.rs:649`, `:727` (`unreachable!` with a reason), `crates/server/server/src/openapi/shared_parts.rs:191`, `:198` (startup-time OpenAPI assembly), `src-tauri/build.rs:97-203` (5, a build script). No `todo!`, `unimplemented!` or `dbg!`.

### 9.5 `.unwrap()` / `.expect()`

The prior audit reported server 68, src-tauri 1, core 26, libs 51, exporters 15, helpers 29 outside `#[cfg(test)]`. This audit, with `credential_matrix.rs`, `document_rules.rs` and `write_guard.rs` reclassified as test and counting occurrences rather than lines, finds 158 in production: server 25, libs 51, src-tauri 24 (21 in `build.rs`), helpers 29 (24 in `crates/helpers/chat-db-fixture/src/ios_backup.rs`, a fixture builder), exporters 16, demo-seed 9, core 4. The dominant pattern is `.lock().expect("… mutex poisoned")` (`crates/libs/import/src/prepare.rs:148`, `:617`, `:626`, `:794`, `:893`, `:993`; `crates/libs/media/src/tools.rs:99-281`, 7 sites; `crates/core/message-crate-core/src/process.rs:154-163`), which is the accepted idiom for a poisoned `Mutex`.

### 9.6 `as` numeric casts (Rust)

269 production lines. Top files: `crates/server/demo-seed/src/personas.rs` 16 (`as f64` in sampling arithmetic), `crates/server/server/src/db/contacts/read.rs` 13, `demo-seed/src/conversations.rs` 10, `crates/libs/import/src/pipeline.rs` 9, `crates/core/message-crate-core/src/attachment_jobs.rs` 7, `crates/exporters/imessage-ir-exporter/src/convert.rs` 7, `crates/exporters/sms-backup-plus-exporter/src/emit.rs` 7, `crates/libs/staging/src/write_queue.rs` 7. 33 of the 269 are in `demo-seed`, which is test-data generation.

### 9.7 Magic numbers

Rust production: 640 literals by the rule in section 2. 102 are `status = NNN` arguments inside `#[utoipa::path]` responses (declarative API documentation) and 98 are the phone country table `crates/libs/phone/src/countries.rs` (data). The remaining 440 cluster on: `32` (29, `[u8; 32]` digest widths in `crates/exporters/imazing-exporter/src/parse_emit.rs:40-382`), `1000`/`1000.0` (31, millisecond conversions), `1024` (17, byte sizes), `60` (10), `30` (10). Top files: `demo-seed/src/phones.rs` 48, `demo-seed/src/conversations.rs` 45, `demo-seed/src/assets.rs` 31 (RGB triples), `crates/helpers/chat-db-fixture/src/lib.rs` 22, `crates/libs/import/src/http.rs` 19, `crates/server/server/src/assets_api.rs` 18. Hard-coded network timeouts: 16 `Duration::from_secs(N)` literals, 9 of them in `crates/libs/import/src/http.rs` (`:146` 15, `:202` 600, `:273` 600, `:315` 60, `:348` 60, `:391` 30, `:440` 600, `:473` 600, `:490` 30) and 4 in `crates/libs/export/src/http.rs` (`:54`, `:90`, `:143` 120; `:189` 300).

TypeScript production: 91 literals, 55 of them icon sizes (`16` x15, `15` x12, `14` x6, `18` x4, `12` x5) passed as `size={n}`; the rest are `60`/`1000`/`1024` time and size arithmetic and layout constants in `components/VirtualList.tsx` and `components/import/importIssuesTableLayout.ts`. `web/src/lib/newId.ts` holds 8 (an id alphabet).

### 9.8 TypeScript assertions and escapes (production, non-generated)

- `as` type assertions: 31 (import aliases and JSX prose excluded by hand). By file: `screens/import/useImportJob.ts` 5 (`:333`, `:340`, `:346`, `:367`, `:755`), `lib/api.ts` 5 (`:125`, `:192`, `:195`, `:215`, `:217`), `screens/import/formSnapshot.ts` 3, `screens/import/runRecord.ts` 3, `lib/nameCollection.ts` 2 (`:147-148`), `components/logs/LogViewer.tsx` 2, and one each in `components/FormField.tsx:51`, `components/NavEntityList.tsx:144`, `components/theme/ThemeColorRow.tsx:47`, `lib/backupIdentity.ts:63`, `lib/contactSort.ts:158`, `lib/conversationSort.ts:43`, `lib/routeQuery.ts:284`, `lib/system-settings.ts:83`, `lib/theme.ts:210`, `lib/useServerState.ts:37`, `screens/settings/storage/storageUtils.ts:197`. Fourteen of them are `x as Record<string, unknown>` at the edge of a JSON parse, which is the narrowing idiom rather than a type lie.
- Non-null assertions (`x!`): 0.
- `any` in type position: 0 (the prior audit's 4 came from a different method; a direct grep over production files finds `any` only in comments and the string `"trashed:any"`).
- `@ts-ignore`, `@ts-expect-error`, `@ts-nocheck`, `biome-ignore`, `eslint-disable`: 0.
- `console.log` / `console.debug`: 0.

## 10. Naming conventions

Rust: 0 functions with an uppercase letter, 0 types outside CamelCase, 0 `const`/`static` outside SCREAMING_CASE, 0 `allow(non_*)`. The rustc naming lints at `-D warnings` leave no room here.

TypeScript:
- snake_case locals: 5, all in `web/src/components/contactDrawer/contactDrawerTypes.ts:193-197` (`direct_messages`, `group_messages`, `orphaned_messages`, `start_date`, `end_date`), mirroring the API field names they accumulate into.
- `.tsx` files not in PascalCase: 7 (`components/contactDrawer/handleTableHelpers.tsx`, `handleTableLogic.tsx`, `components/icons.tsx`, `components/messages/chatBubbleShared.tsx`, `components/phoneCountryItems.tsx`, `lib/auth.tsx`, `lib/highlightText.tsx`); all export helpers or several small components rather than one component, so camelCase is defensible, but there is no written rule either way.
- kebab-case file names in an otherwise camelCase `lib/`: 2 (`lib/system-settings.ts`, `lib/tauri-check.ts`).
- Abbreviations in declared identifiers (production): Rust `dir` 314, `id` 204, `err` 129, `msg` 69, `tmp` 53, `db` 48, `ms` 47, `str` 42, `len` 40, `cfg` 38, `buf` 11; TypeScript `id` 58, `ms` 26, `dir` 19, `param` 17, `res` 16, `err` 15, `db` 10, `idx` 5, `msg` 4. These are the conventional short forms (`_ms` suffixes, `dir` for a path); nothing idiosyncratic (`cnt`, `btn`, `elem`, `pct`) appears at all.

## 11. Dead and unreferenced public symbols

- Rust `pub`/`pub(crate)` production functions whose name occurs nowhere else in the repository's Rust (common trait-method names excluded): 1, `crates/libs/ir/src/lib.rs:1002 parse_json_value`. It is documented and exported from `message-ir`, and nothing calls it.
- The `contacts` crate (`crates/libs/contacts`) has no dependents and no workspace dependencies (`cargo metadata`); its own `lib.rs:3-6` says so and names issue #916. Documented, so not a finding.
- TypeScript exports in production files that no other file references: 104 (section 17 lists them). 99 are types, constants or functions used inside their own file, where the `export` keyword is unnecessary and widens the module surface (`lib/serverApi.ts:41 RequestOptions` is used 52 times in its file and imported nowhere). 5 are declared and never used anywhere: `web/src/lib/types.ts:20 MessageParticipant`, `web/src/components/contactDrawer/handleTableStyles.ts:6 tdLeftClass`, `:14 rowActionsRevealClass`, `web/src/lib/uiStyles.ts:64 authInput`, `:66 mutedText`.
- Exports used only by test files: 80, including seven `serverApi.ts` route functions (`:774 listContactGroupMembers`, `:819 listMessageTagMembers`, `:966 getExport`, `:971 createExport`, `:975 completeExport`, `:979 cancelExport`) and `screens/import/useImportJob.ts:477 resetImportRun`. The export-route functions exist for the desktop Export flow through `lib/tauri.ts`, so test-only use in `web/src` is expected for some; the list is where to look, not a verdict.

## 12. Test-to-code ratio per crate

Rust (test lines / production lines, parser classification):

| Ratio | Crates |
|---|---|
| 0.00 - 0.30 | `serve-protocol` 0.00 (49 prod lines), `build-version` 0.17, `chat-db-fixture` 0.18, `imessage-reader-protocol` 0.25, `contacts` 0.25 |
| 0.30 - 0.70 | `api-types` 0.36, `demo-seed` 0.42, `log-lines` 0.51, `sbr` 0.53, `mms-parts` 0.57, `phone` 0.59, `csv` 0.59, `obfuscate` 0.64, `ir` 0.65 |
| 0.70 - 1.10 | `src-tauri` 0.73, `openextract-exporter` 0.87, `go-sms-mms` 0.95, `imessage-reader` 0.97, `message-crate-core` 0.99, `imazing-exporter` 1.02, `whatsapp-exporter` 1.02, `go-sms-pro-exporter` 1.06, `media` 1.07, `mail` 1.09 |
| 1.10 - 2.40 | `import` 1.11, `ir-format` 1.14, `staging` 1.15, `journal` 1.17, `http` 1.20, `sms-backup-restore-exporter` 1.22, `ios-backup` 1.31, `sms-backup-plus-exporter` 1.32, `server` 1.35, `export` 1.60, `imessage-ir-exporter` 1.74, `reexport` 2.35 |

`serve-protocol`, `build-version`, `api-types` and `chat-db-fixture` are type-only or fixture crates, and three of the four are also excluded from mutation testing by `.cargo/mutants.toml` for that reason. `sbr` (0.53) and `phone` (0.59) are parsing crates with real logic and the lowest ratios among them; `sbr::group_chat_key` is already named by the prior audit (F9).

Web: 204 test files against 338 production files; 155 production files (46%) have no sibling `*.test.ts(x)`. The largest without one: `components/messages/chatBubbleShared.tsx` (528 lines), `components/VirtualList.tsx` (265), `components/icons.tsx` (221), `components/advancedSearch/ContactsSearchFields.tsx` (215), `screens/import/importRunStore.ts` (203), `screens/settings/ApiTokenForms.tsx` (183). Many of these are exercised through a parent's test, so this is a map, not a coverage figure.

## 13. Coupling

### 13.1 Crate graph (`cargo metadata`, non-dev dependencies; `src-tauri` is outside the workspace and depends on 22 workspace crates by path)

| Crate | Ce (depends on) | Ca (depended on by) | Instability Ce/(Ca+Ce) |
|---|---|---|---|
| `message-ir` | 1 | 21 | 0.05 |
| `media` | 1 | 14 | 0.07 |
| `phone` | 1 | 12 | 0.08 |
| `message-csv` | 1 | 6 | 0.14 |
| `message-crate-api-types` | 0 | 5 | 0.00 |
| `imessage-reader-protocol` | 0 | 4 | 0.00 |
| `message-crate-core` | 7 | 13 | 0.35 |
| `message-ir-format` | 7 | 11 | 0.39 |
| `message-staging` | 4 | 8 | 0.33 |
| `ios-backup`, `obfuscate` | 2 | 2 | 0.50 |
| `sbr` | 3 | 1 | 0.75 |
| `sms-backup-restore-exporter` | 9 | 1 | 0.90 |
| `sms-backup-plus-exporter` | 8 | 1 | 0.89 |
| `message-crate-server` | 8 | 0 | 1.00 |
| the other 5 exporters, `message-crate-import`, `message-crate-export`, `message-reexport`, `imessage-reader` | 6 - 8 | 0 | 1.00 |

The shape is right for a layered system: the model crates are stable (I near 0) and the applications are unstable leaves (I = 1). The one crate in the middle that is both heavily used and heavily dependent is `message-crate-core` (13 dependents including every exporter, 7 dependencies); a change there rebuilds 13 crates, and the prior audit's F3 explains why it also names its consumers. `sms-backup-restore-exporter` and `sms-backup-plus-exporter` each have one dependent (`message-reexport`), which makes them the only exporters something else links.

### 13.2 Web module graph (relative imports among production files)

Fan-in (most imported): `lib/serverApi.ts` 53, `lib/types.ts` 50, `components/Button.tsx` 49, `components/PlainButton.tsx` 38, `lib/routeQuery.ts` 38, `lib/uiStyles.ts` 37, `lib/queryKeys.ts` 32, the generated `lib/serverApi.types.ts` 26, `lib/apiErrorMessage.ts` 25, `lib/tauri.ts` 25. Fan-out (most imports): `screens/import/useImportJob.ts` 33, `screens/ImportScreen.tsx` 29, `screens/ContactList.tsx` 27, `components/LeftPanel.tsx` 25, `screens/import/ImportFormFields.tsx` 24, `components/AppLayout.tsx` 23. External packages: `react` 151 import sites, `react-aria-components` 40, `react-router-dom` 21, `@tanstack/react-query` 19, `@tauri-apps/api` 6. The high fan-in of `serverApi.ts`, `routeQuery.ts` and `queryKeys.ts` is the single data path ADR 0002 asks for; the high fan-out files are the same screens that top the length table in 5.2.

## 14. Cohesion

Production functions per file (top): `crates/server/server/src/server.rs` 72, `reset_demo.rs` 71, `db/staging.rs` 62, `crates/server/demo-seed/src/conversations.rs` 52, `crates/libs/ir/src/lib.rs` 49, `search/emit.rs` 46, `crates/libs/mail/src/lib.rs` 44, `crates/libs/media/src/process.rs` 43, `crates/libs/import/src/prepare.rs` 42, `imports_api/mod.rs` 41, `assets_api.rs` 41, `src-tauri/src/tool_downloads.rs` 40. The server keeps its 14 route groups in 14 `*_api.rs` files as `docs/architecture/http-api.md` asks; `src-tauri` spreads its 37 `#[tauri::command]` functions over 12 files in `src/commands/` (`staging.rs` 10, `paths.rs` 6). On the web side, `screens/ImportScreen.tsx` owns 33 `useState` calls and passes them down through a 57-member `ImportFormFieldsProps` (`screens/import/ImportFormFields.tsx:65-131`); the prior audit's F4 describes the structure, and the figures here say how big it has become.

## 15. Findings

Importance is 1 to 10. Each finding names the evidence, the fix, and where it sits relative to the prior audit.

### F1. `ImportScreen` and `ImportFormFields` are the two largest and most complex functions in the repository (8/10)

**Evidence.** `web/src/screens/ImportScreen.tsx:111 ImportScreen` is 780 lines, proxy 140 (52 `if`), 33 `useState`. `web/src/screens/import/ImportFormFields.tsx:305 ImportFormFields` is 593 lines, proxy 92 (49 ternaries), with a 57-member props interface at `:65-131`. `web/src/screens/TrashScreen.tsx:100` (385 lines, proxy 83, depth 7) is third.

**Remediation.** The prior audit (F4) proposes the architectural shape. The metric-level step that pays first: move the 33 `useState` calls into one `useReducer` with a typed form state, and give `ImportFormFields` the state and dispatch instead of 57 props.

```ts
// web/src/screens/import/importForm.ts
export type ImportFormState = { source: ImportSource; inputPath: string; /* the other 31 fields */ };
export type ImportFormAction =
  | { type: "set"; field: keyof ImportFormState; value: ImportFormState[keyof ImportFormState] }
  | { type: "refill"; form: ImportFormState }
  | { type: "reset" };
export function importFormReducer(s: ImportFormState, a: ImportFormAction): ImportFormState { /* three cases */ }

// ImportScreen.tsx
const [form, dispatch] = useReducer(importFormReducer, initialImportForm);
<ImportFormFields form={form} dispatch={dispatch} />   // replaces ~60 value/onChange pairs
```

Then split `ImportFormFields` by source into `ImessageFields`, `WhatsappFields`, `AndroidSmsFields`, each under 150 lines, chosen by one `switch (form.source)`.

### F2. `go_sms_mms::mms::header` is a 65-arm `match` with a proxy of 123 (7/10)

**Evidence.** `crates/libs/go-sms-mms/src/mms.rs:218-381`: 164 lines, 3 `match`, 65 arms, 60 `?`, 8 `return`s. Every arm is `CODE => { msg.field = decoder(cur)?; return Ok(()); }`.

**Remediation.** A table from header code to decoder keeps one arm per *shape* rather than per header. The repetitive arms become data:

```rust
type Store = fn(&mut Cursor<'_>, &mut Message) -> Result<()>;
const SIMPLE: &[(u8, Store)] = &[
    (FROM, |cur, m| { m.from = from_value(cur)?; Ok(()) }),
    (TO,   |cur, m| { m.to.push(string(cur)?); Ok(()) }),
    (CC,   |cur, m| { m.cc.push(string(cur)?); Ok(()) }),
    (BCC,  |cur, m| { m.bcc.push(string(cur)?); Ok(()) }),
    (DATE, |cur, m| { m.date = Some(long_integer(cur)?); Ok(()) }),
    // ...
];
fn header(code: u8, cur: &mut Cursor<'_>, msg: &mut Message) -> Result<()> {
    if let Some((_, store)) = SIMPLE.iter().find(|(c, _)| *c == code) { return store(cur, msg); }
    match code { MESSAGE_TYPE => message_type(cur, msg), CONTENT_TYPE => content_type(cur, msg), other => unknown(other, cur, msg) }
}
```

The headers that need their own logic (`MESSAGE_TYPE` with its nested match, content type, the `other` fallthrough at `:379`) keep named functions under 30 lines each.

### F3. Four server functions over 150 lines, two of them at nesting depth 6 (7/10)

**Evidence.** `crates/server/server/src/db/address_book.rs:923 apply` (200 lines, 9 loops, proxy 37) and `:614 plan` (140 lines, depth 6, 7 `match`); `imports_api/staging.rs:447 write_rows` (161); `db/conversation_messages.rs:915 get_conversation_messages` (156). `apply` already names its phases in comments at body lines 24, 145, 167 and 190 ("First every contact takes what its rows list", "Edit: an identity a file contact still holds and no row of it lists comes off", "A contact this load left with neither a name nor an identity … goes", "The notes in the order of the file's rows").

**Remediation.** Cut `apply` at those four comments into `take_listed_identities`, `release_unlisted_identities`, `delete_empty_contacts`, `collect_notes`, each taking `&mut LoadState` (the `holder_of`, `changed`, `lost_identity`, `placed` locals at `:933-944` become its fields). The comments become the doc lines of the four functions. The same cut applies to `plan` (its `match` arms) and to `write_rows`.

### F4. Row mapping is hand-written through 100 positional `try_get(n)` calls (6/10)

**Evidence.** `grep -rn 'try_get(\d)'` over server production code: 100 calls in `db/conversation_messages.rs`, `db/exports.rs`, `db/imports.rs`, `db/ownership.rs`, `db/participant_names.rs`, `db/staging.rs`, `reset_demo.rs`. `db/imports.rs:455 import_from_row` is 27 `?` in 43 lines; `db/conversation_messages.rs:499 fetch_message_page` is 31. `#[derive(sqlx::FromRow)]` is already used in 4 places (`dedupe.rs:22`, `db/media_queue.rs:11`, `db/attachment_versions.rs:65`, `:409`). Positional indices also break silently when a column is added to the `SELECT` list.

**Remediation.** Derive `FromRow` on the row structs and select by name; keep a hand-written `TryFrom<RawRow>` only for the fields that need parsing (`ImportStatus::parse` at `:462`).

```rust
#[derive(sqlx::FromRow)]
struct ImportRowRaw { id: i64, account_id: i64, source: String, tool: String, mode: String, status: String, started_at: i64, finished_at: Option<i64>, /* … */ }
impl TryFrom<ImportRowRaw> for ImportRow {
    type Error = sqlx::Error;
    fn try_from(r: ImportRowRaw) -> Result<Self, sqlx::Error> {
        let status = ImportStatus::parse(&r.status)
            .ok_or_else(|| sqlx::Error::Decode(format!("imports.status holds unknown value '{}'", r.status).into()))?;
        Ok(ImportRow { id: r.id, account_id: r.account_id, status, /* … */ })
    }
}
// let rows: Vec<ImportRowRaw> = sqlx::query_as(SQL).fetch_all(conn).await?;
```

### F5. 23 functions over 6 parameters; one `#[allow]` sits on a function at the threshold (6/10)

**Evidence.** Section 7. `apply_transcode` (`crates/libs/staging/src/transcode.rs:653`) takes 9; `write_queue.rs` has three 7-parameter functions sharing most of their arguments (`:321`, `:483`, `:768`). `restore_mms` has 7 and carries `#[allow(clippy::too_many_arguments)]` at `write.rs:164`; clippy's default threshold is 7, so the attribute likely covers nothing. **Unable to verify** without clippy.

**Remediation.** Group the arguments that travel together. For `apply_transcode`, five of the nine describe the file being transcoded:

```rust
pub(crate) struct TranscodeTarget<'a> { run_dir: &'a Path, jsonl: &'a Path, recorded_rel: &'a str, src: &'a Path, is_heal: bool }
fn apply_transcode(target: &TranscodeTarget<'_>, doc: &mut ConversationDocument, options: &TranscodeOptions, issues: Option<&IssueSink>, report: &mut TranscodeReport) -> Result<()>
```

For the two `#[allow(clippy::too_many_arguments)]` sites, use `#[expect]` so the compiler reports the attribute the day it stops being needed, which is the convention `parse.rs:39` already follows:

```rust
#[expect(clippy::too_many_arguments, reason = "the write-back of one <mms> needs every original part")]
```

### F6. Seven functions nest 7 or 8 braces deep (5/10)

**Evidence.** Section 6.3. `crates/exporters/go-sms-pro-exporter/src/xml.rs:189 read_sms_elements` (depth 8 in 83 lines, proxy 31), `crates/libs/sbr/src/read.rs:923 infer_owner_phones` (depth 8 in 34 lines), `crates/libs/staging/src/write_queue.rs:506 drain_write_queue` (depth 8 in 130 lines).

**Remediation.** Flatten with `let … else` and `continue`, which the codebase already uses (`whatsapp-exporter/src/run.rs:28`). For a loop body of the shape `for e in events { match e { Ok(Event::Start(t)) => { if t.name() == b"sms" { for a in t.attributes() { match a { Ok(a) => { if … { … } } … } } } } } }`, each `if`/`match` that only guards becomes a `let Ok(a) = a else { continue };` or `if cond { continue; }` at the loop's top level.

### F7. `web/tsconfig.json` is `strict` and nothing more (5/10)

**Evidence.** `strict: true` is set; `noUncheckedIndexedAccess`, `noImplicitReturns`, `noFallthroughCasesInSwitch`, `noUnusedLocals`, `noUnusedParameters`, `exactOptionalPropertyTypes`, `noImplicitOverride`, `noPropertyAccessFromIndexSignature` and `verbatimModuleSyntax` are absent. A proxy grep finds 132 indexed reads (`x[0]`, `x[i]`) in 60 production files that `noUncheckedIndexedAccess` would type as `T | undefined`. The exact error count needs `tsc`: **Unable to verify**.

**Remediation.** Add the two flags that find real bugs, then the two that are free because Biome and the code already satisfy them:

```jsonc
// web/tsconfig.json  compilerOptions
"noUncheckedIndexedAccess": true,   // array[i] is T | undefined; ~132 sites to look at
"noFallthroughCasesInSwitch": true, // describeAuditEntry has 20 cases
"noImplicitReturns": true,
"noImplicitOverride": true
```

`noUnusedLocals`/`noUnusedParameters` are covered by Biome's `noUnusedVariables`/`noUnusedImports` if they are in the active `recommended` set for 2.5.14; **Unable to verify** which rules that preset enables without reading Biome's own rule listing.

### F8. 104 exports with no importer, 5 dead symbols, 1 dead `pub fn` (5/10)

**Evidence.** Section 11.

**Remediation.** Delete `web/src/lib/types.ts:20 MessageParticipant`, `web/src/components/contactDrawer/handleTableStyles.ts:6 tdLeftClass`, `:14 rowActionsRevealClass`, `web/src/lib/uiStyles.ts:64 authInput`, `:66 mutedText`, and `crates/libs/ir/src/lib.rs:1002 parse_json_value`. Drop the `export` keyword from the 99 own-file-only symbols, starting with `lib/serverApi.ts:41 RequestOptions`, `lib/tauri.ts` (7 types), `lib/nameCollection.ts` (8), `lib/contactSort.ts` (5). A one-off `npx knip` run in `web/` would confirm the list; keep it a report, not a gate (ADR 0007).

### F9. `demo-seed` is the one crate `missing_docs` cannot reach, and has 15 undocumented public items (4/10)

**Evidence.** Section 8. The binary's `pub` items in `src/config.rs`, `src/personas.rs`, `src/assets.rs` have no doc comment, and the lint that enforces docs everywhere else does not apply to a binary's items.

**Remediation.** Either document them (one line each; `ContactsConfig` through `EdgeCasesConfig` mirror `seed.toml` sections, so the line is the section name) or make them `pub(crate)`, which states the intent and matches how the rest of the crate is used. 24 `pub(crate)`/`pub(super)` items elsewhere also lack a line; the parser structs (`RawRow` x2, `ChatJson`, `MessageJson`, `XmlMessage`, `Token`, `TokenKind`) are the ones a reader meets first.

### F10. Timeouts are literals repeated across call sites (4/10)

**Evidence.** Section 9.7: 16 `Duration::from_secs(N)`; `crates/libs/import/src/http.rs` has 600 four times, 60 twice, 30 twice and 15 once; `crates/libs/export/src/http.rs` has 120 three times and 300 once. The same value in four places is four edits when it changes, and nothing says which operation each covers.

**Remediation.**

```rust
// crates/libs/import/src/http.rs
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const STAGE_TIMEOUT: Duration = Duration::from_secs(60);
const SHORT_CALL_TIMEOUT: Duration = Duration::from_secs(30);
```

`.cargo/mutants.toml` already excludes arithmetic in top-level constants, so the move costs nothing in mutation noise.

### F11. Small duplicates: `copy_dir_recursive` twice, `checkOptionalPath` twice (3/10)

**Evidence.** `crates/libs/reexport/src/lib.rs:751` and `crates/server/demo-seed/src/lib.rs:420` are the same 16-line function with three cosmetic differences (`diff` in section 17). `web/src/lib/imessageImport.ts:102` and `web/src/lib/whatsappImport.ts:122` are the same 24-line `checkOptionalPath` differing only in the error-key type and two identifier names.

**Remediation.** One `copy_dir_recursive` in `message-ir-format` (both crates depend on it) or in `message-crate-core`; one generic `checkOptionalPath<K extends string>(…, errors: Partial<Record<K, string>>, key: K, kindError: string, missing: string)` in `web/src/lib/pathChecks.ts`.

### F12. File-name case drift in `web/src` (3/10)

**Evidence.** Section 10: 7 camelCase `.tsx` files among 100-odd PascalCase ones, 2 kebab-case `.ts` files among camelCase ones; 5 snake_case locals at `contactDrawerTypes.ts:193-197`.

**Remediation.** Rename `lib/system-settings.ts` to `lib/systemSettings.ts` and `lib/tauri-check.ts` to `lib/tauriCheck.ts`; write the rule into `web/STYLE_GUIDE.md` (PascalCase when the file's default export is a component, camelCase otherwise), which makes the 7 `.tsx` files conforming. Rename the five locals to camelCase and convert at the return.

### F13. Test functions of 150 to 390 lines (3/10)

**Evidence.** 14 test functions over 150 lines, 43 over 100 (section 5.1). `crates/server/server/src/search/tests.rs:298 seeded` is 390 lines of fixture setup; `imports_api/tests.rs` is 6,546 lines in one file.

**Remediation.** `seeded` is a builder, not a test; `search/tests.rs` would read better with it in `search/test_fixture.rs` (the `test_support/` pattern the server already has). Split `imports_api/tests.rs` by the behaviour under test (`tests/staging.rs`, `tests/promote.rs`, `tests/with_yourself.rs` beside the modules they cover).

### F14. `message-crate-core` sits in the middle of the graph (2/10)

**Evidence.** Ca 13, Ce 7, instability 0.35 (section 13.1); every exporter and both pipelines depend on it while it depends on 7 libs. Not a defect by itself; it is the place a change fans out widest. The prior audit's F3 holds the design discussion; the number here is the cost.

## 16. What is done well

- No `TODO`/`FIXME`/`HACK` anywhere; problems are issues, not comments.
- Six lint suppressions in 106,000 production lines, three with a reason in the attribute and one with a reason in the comment above.
- 99.2% public documentation, 88% of all comment lines are doc comments, and `missing_docs` is enforced through `-D warnings` on every library crate and `src-tauri`.
- Zero non-null assertions, zero `any`, zero ignore pragmas in 42,000 lines of web code; 31 `as` casts, most at JSON-parse boundaries.
- Rust median function length 10, 90th percentile 38; only 9 functions over 150 lines.
- Every `unsafe` is an FFI or process-environment edge; every `panic!` outside build scripts is an invariant with a message.
- Tests outnumber code 1.13:1 in Rust and 0.91:1 in the web app, with test naming that reads as a sentence.

## 17. Unable to verify

- Exact cyclomatic and cognitive complexity: the figures are the proxy defined in section 2; a tool that parses Rust and TypeScript ASTs would move individual numbers, not the ranking.
- Whether `#[allow(clippy::too_many_arguments)]` at `crates/exporters/sms-backup-restore-exporter/src/write.rs:164` still suppresses anything (F5): needs `cargo clippy`.
- How many type errors `noUncheckedIndexedAccess` would raise (F7): needs `tsc`; 132 indexed reads is the upper bound by grep.
- Whether rustc's `missing_docs` is inert on the `demo-seed` binary (F9): inferred from the lint's definition; a clippy run on that crate would confirm.
- Which Biome rules the `recommended` preset enables in 2.5.14 (F7).
- Coverage and mutation figures: `target/llvm-cov/uncovered-functions.txt` and `target/mutants/summary.md` are absent locally; both are CI reports (`coverage.yml`, `mutants.yml`).
- TypeScript anonymous callbacks (`useEffect(() => …)`) are not counted as functions, and JSX braces inflate TSX nesting depth; the TS function count (1,263) is therefore a floor and the depth figures a ceiling.

## 18. Commands

All commands were run from the repository root at commit `530dc1791`. `S` is a scratch directory outside the repository; the two parsers (`rsfn.py`, 279 lines, and `tsfn.py`, 206 lines) were written there and not committed, per the read-only rule. Their behaviour is specified in section 2; the invocation and every shell command are below.

```bash
# File sets (worktrees, target, vendor, node_modules excluded by construction)
S=/tmp/cqm-audit-$(id -u); mkdir -p $S
find crates src-tauri/src src-tauri/build.rs -name '*.rs' -not -path '*/target/*' | sort > $S/rust.txt
find web/src -type f \( -name '*.ts' -o -name '*.tsx' \) -not -name 'serverApi.types.ts' | sort > $S/web.txt
for a in "server:crates/server/server" "demo-seed:crates/server/demo-seed" "core:crates/core" "libs:crates/libs" \
         "exporters:crates/exporters" "helpers:crates/helpers" "src-tauri:src-tauri"; do
  grep "^${a#*:}/" $S/rust.txt > $S/rust-${a%%:*}.txt; done

# Section 4: raw lines and cloc
for n in server demo-seed core libs exporters helpers src-tauri; do
  printf "%s %d\n" $n $(cat $(cat $S/rust-$n.txt) | wc -l); cloc --quiet --timeout 60 --list-file=$S/rust-$n.txt --csv | tail -1; done
cloc --quiet --list-file=$S/web.txt --csv | tail -1

# Sections 5-9, 12: parsers (write per-function and per-file TSVs into $S)
python3 -I $S/rsfn.py $S/rust.txt $S     # rs_fn.tsv rs_item.tsv rs_file.tsv rs_todo.tsv rs_allow.tsv rs_cast.tsv rs_num.tsv rs_panic.tsv rs_unsafe.tsv
python3 -I $S/tsfn.py $S/web.txt $S      # ts_fn.tsv ts_file.tsv ts_todo.tsv ts_num.tsv ts_assert.tsv
# rs_fn.tsv columns: path:line, crate, name, visibility, prod|test, length, params, max_depth, if, match, arms, loops, &&, ||, ?, return, proxy
# cfg(test)-gated file modules (97) were collected by reading each '#[cfg(test)]' followed by 'mod x;' and resolving x.rs / x/mod.rs / #[path]

# Section 9.1 cross-check
cat $S/rust.txt $S/web.txt | xargs grep -n -w -E 'TODO|FIXME|XXX|HACK' | wc -l
cat $S/rust.txt $S/web.txt | xargs grep -n -i -E '//.*\b(todo|fixme|hack)\b' | grep -v 'todo!' | wc -l

# Section 9.2 suppressions; 9.3 unsafe; 9.4 panics (also emitted by the parser)
cat $S/rust.txt | xargs grep -n -E '#!?\[(allow|expect)\('
grep -rn 'missing_docs' --include=*.rs crates src-tauri/src

# Section 9.7 magic numbers, split
grep -v -P 'status = \d' $S/rs_num.tsv | grep -v 'phone/src/countries.rs' | wc -l
grep -c -P 'status = \d' $S/rs_num.tsv; grep -c 'phone/src/countries.rs' $S/rs_num.tsv
grep -P 'from_(secs|millis)\(\d' $S/rs_num.tsv

# Section 9.8 web assertions
grep -v -E '\.test\.tsx?$|/src/test/' $S/web.txt | xargs grep -n -P '[\w\)\]]!(?=[\.\[\(;,\)])' | grep -v -P '!==?' | wc -l   # non-null: 0
grep -v -E '\.test\.tsx?$|/src/test/' $S/web.txt | xargs grep -n -P ':\s*any\b|<any>|as any|any\[\]|\bany\s*[,>)]' | grep -v -P '^\S+:\d+:\s*(//|\*)'   # any: comments and a string only
grep -v -E '\.test\.tsx?$|/src/test/' $S/web.txt | xargs grep -l -P '\w\[(\d+|[a-z]\w*)\](?!\s*=[^=])' | wc -l   # 60 files, 132 lines (F7 proxy)

# Section 3 standards files
cat rust-toolchain.toml Cargo.toml crates/server/server/clippy.toml .cargo/mutants.toml web/biome.json web/tsconfig.json web/tsconfig.test.json
for f in crates/*/*/Cargo.toml; do grep -A1 '^\[lints\]' "$f" | grep -q 'workspace = true' || echo "$f"; done   # prints nothing
grep -n -E 'clippy|cargo fmt|biome|npm (run|test)|-D warnings' .github/workflows/ci.yml
for k in strict noUncheckedIndexedAccess exactOptionalPropertyTypes noImplicitOverride noFallthroughCasesInSwitch noUnusedLocals noUnusedParameters noImplicitReturns noPropertyAccessFromIndexSignature verbatimModuleSyntax; do printf "%-32s %s\n" $k "$(grep -c "\"$k\"" web/tsconfig.json)"; done

# Section 13.1 coupling
cargo metadata --no-deps --format-version 1 --offline | python3 -I -c '
import json,sys; m=json.load(sys.stdin); ws=set(m["workspace_members"]); pk={p["id"]:p for p in m["packages"] if p["id"] in ws}
names=set(p["name"] for p in pk.values()); ca={n:set() for n in names}; ce={}
for p in pk.values():
    d=set(x["name"] for x in p["dependencies"] if x["name"] in names and x.get("kind") in (None,"build")); ce[p["name"]]=d
    for x in d: ca[x].add(p["name"])
for n in sorted(names,key=lambda n:-(len(ce[n])+len(ca[n]))): c,a=len(ce[n]),len(ca[n]); print(n,c,a,round(c/(a+c),2) if a+c else "nan")'
grep -c 'path = "../crates' src-tauri/Cargo.toml   # 22

# Section 14 cohesion
awk -F'\t' '$5=="prod"{split($1,a,":"); c[a[1]]++} END{for(f in c) print c[f], f}' $S/rs_fn.tsv | sort -rn | head -12
ls crates/server/server/src/*_api.rs | wc -l                       # 14
grep -r '#\[tauri::command' src-tauri/src | wc -l                  # 37
grep -c 'useState' web/src/screens/ImportScreen.tsx                # 33
sed -n '65,135p' web/src/screens/import/ImportFormFields.tsx | grep -c -E '^\s+\w+\??:'   # 57

# Section 11 dead symbols (parser-side token frequency; shell confirmation)
grep -rn 'parse_json_value' --include=*.rs crates src-tauri        # definition only
grep -rn -P 'try_get(::<[^>]*>)?\(\d+\)' crates/server/server/src --include=*.rs | grep -v tests | wc -l   # 100 (F4)
grep -rn 'FromRow' crates/server/server/src --include=*.rs | grep -v tests | wc -l                         # 4

# F11 duplicates
diff <(sed -n '751,766p' crates/libs/reexport/src/lib.rs) <(sed -n '420,435p' crates/server/demo-seed/src/lib.rs)
diff <(sed -n '102,125p' web/src/lib/imessageImport.ts) <(sed -n '122,145p' web/src/lib/whatsappImport.ts)

# Section 10 naming (Rust: regex over fn/struct/enum/const declarations; TS: declarations, file names)
# Rust: 0 / 0 / 0 by the regexes in rsfn.py's companion pass; TS:
grep -v -E '\.test\.tsx?$|/src/test/' $S/web.txt | xargs grep -n -P '^\s*(export\s+)?(const|let|function)\s+[a-z]+_[a-z_]+\b'
grep -E '/[a-z][A-Za-z]*\.tsx$' $S/web.txt | grep -v -E '/(main|index)\.tsx$'; grep -E '/[a-z]+-[a-z-]+\.ts$' $S/web.txt

# Verification of the longest functions (start and end lines)
sed -n '923p;1122p' crates/server/server/src/db/address_book.rs
sed -n '218p;381p' crates/libs/go-sms-mms/src/mms.rs
sed -n '111p;890p' web/src/screens/ImportScreen.tsx
sed -n '305p;897p' web/src/screens/import/ImportFormFields.tsx
```

The 104 unreferenced TypeScript exports, the 80 test-only exports and the 24 undocumented restricted Rust items were printed in full during the run; the report names the ones that matter and the files that hold the rest (`lib/tauri.ts`, `lib/nameCollection.ts`, `lib/contactSort.ts`, `lib/serverApi.ts`, `screens/import/*`), so a re-run of the parser reproduces the lists.
