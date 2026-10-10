# Testing implementation audit

- Audit: testing-implementation (code-audit plugin, skill `testing-implementation`)
- Date: 2026-10-10
- Commit: `530dc1791` (branch `docs/initial-software-design-analysis`)
- Scope: the whole repository less `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`
- Method: static only. Nothing was compiled or executed (`cargo test`, `npm test`, `coverage.sh` and `mutants.sh` were not run because the working tree is shared). Every number below comes from a command listed in Appendix A, referenced as `[C<n>]`.
- Companion: the design audit `audits/2026-10-09-initial-software-design-analysis.md` is cited by finding id (`F10` for the `test_support.rs` fixture module).

## Assumptions

- A file named `tests.rs`, `test_support.rs`, `testutil.rs`, or anything under a `tests/` or `test_support/` directory is test code; everything else under `src/` is source, less the lines inside an inline `#[cfg(test)] mod … { }` block (brace-matched). This is the same split `scripts/coverage.sh` uses (`IGNORE='(^|/)tests/|/tests\.rs$'`), plus the inline blocks.
- `src-tauri/` is counted beside the workspace crates although it is not a member; `web-next/` is not counted.
- GitHub Actions sets `CI=true` in every job; `crates/libs/media/src/testutil.rs:61-63` reads that variable.

## 1. Summary

| Measure | Value | Source |
| --- | --- | --- |
| Rust test functions | 3,162 (2,108 `#[test]`, 1,054 `#[tokio::test]`) in 275 files | [C1] |
| of which `src-tauri` | 210 in 15 files | [C1] |
| of which the server crate | 1,384 (330 + 1,054) | [C1] |
| Rust source lines (non-test) | about 107,900 | [C1] |
| Rust test lines (inline + `tests.rs` + `tests/*.rs`) | about 117,900 (24,279 inline, 80,034 `tests.rs`-style, 13,552 `tests/*.rs`) | [C1] |
| Rust crates with zero tests | 1 of 36: `crates/libs/serve-protocol` (48 lines of constants) | [C1] |
| Web test files / sources | 193 test files, 349 `.ts`/`.tsx` sources (the 14,079-line generated `serverApi.types.ts` and 11 `src/test/` helpers included) | [C3] |
| Web test cases | 1,931 `it`/`test` calls; 0 without an `expect`; 0 `.skip`/`.only`/`.todo` | [C4] |
| Automated browser (end-to-end) tests | none. `playwright` is in `web/package-lock.json` only as a peer dependency of vitest (`@vitest/browser-playwright`), not installed; `AGENTS.md` names the Playwright MCP for manual checks | [C5] |
| `#[ignore]` | 3, each with a reason string | [C6] |
| `#[should_panic]` / `serial_test` | 0 / 0 | [C6] |
| Tests gated on real ffmpeg | 56 `real_ffmpeg_test_guard()` sites in 13 files; a missing ffmpeg panics under `CI`, skips with a message elsewhere | [C7] |
| Mocking | web: 220 `vi.mock` calls in 80 of 193 files, no MSW; Rust: `httpmock` 0.8 (116 `MockServer::start` sites) in `http`, `import`, `export`, `src-tauri`; the server tests bind a real axum listener | [C4][C8] |
| Property or fuzz testing | none (`proptest`, `quickcheck`, `arbitrary`, `insta`, `fast-check`: 0 in both lockfiles; no `fuzz/`). One hand-rolled deterministic sweep exists: `crates/libs/go-sms-mms/src/robustness_tests.rs` | [C8][C9] |
| Coverage percentage at HEAD | **Unable to verify** (no committed report; running it is out of scope). Dated proxy in section 9 | [C10] |
| Mutation score | **Unable to verify** (no committed `mutants.out`; the workflow runs by hand only) | [C10] |

The suite is large, named in sentences, almost free of skipped or assertion-less tests, and backed by contract tests that pin the OpenAPI document, the generated TypeScript, the search language and the sidecar protocol. The gaps are narrow and specific: no robustness sweep for user-typed search queries, no test of the import concurrency bound, a handful of tests that read the wall clock to build day-relative data, a few fixed sleeps used as negative assertions, a few unreferenced web components, and no automated browser test at all.

## 2. Test pyramid

| Layer | What | Size | Where |
| --- | --- | --- | --- |
| Unit (inline `#[cfg(test)]` and `<module>/tests.rs`) | handler-level and function-level tests; the server's "unit" tests drive handlers over a real in-process axum listener via `test_support::serve` (`crates/server/server/src/test_support.rs:71-83`) | about 104,300 lines, about 3,000 functions | every crate; `src-tauri` |
| Integration (`crates/*/tests/*.rs`) | 14 crates have a `tests/` directory; the server's five files start the built binary (`crates/server/server/tests/exit_with_parent.rs:105-108` uses `env!("CARGO_BIN_EXE_message-crate-server")`); `crates/libs/import/tests/import_mock.rs` (3,752 lines) and `crates/libs/export/tests/export_mock.rs` run the client libraries against `httpmock`; six exporter `convert_smoke.rs` files run a backup fixture end to end into JSONL | about 13,550 lines, 14 files plus `src-tauri/tests/window_csp.rs` | [C1][C11] |
| End-to-end (browser) | none automated | 0 | [C5] |
| Web component and hook tests (Vitest + Testing Library, jsdom for `.tsx`) | 1,931 cases; `serverApi` route functions and `lib/auth` are mocked per file; `web/src/test/providers.tsx` supplies a fresh `QueryClient` per mount | 193 files | [C3][C4] |

The shape is right for this product: wide at the unit layer, a real HTTP boundary inside the unit layer for the server, a thin integration layer that exercises the process seams (binary start, exit-with-parent, log files, sidecar protocol), and no browser layer. Whether the missing browser layer matters is finding T7.

## 3. Per-crate test presence

Source lines exclude inline test blocks; "`tests.rs` lines" counts sibling `tests.rs`, `test_support*.rs`, `testutil.rs` and files under `src/**/tests/`. [C1]

| Crate path | Package | Source lines | Inline test lines | `tests.rs`-style lines | `tests/*.rs` lines | `#[test]` | `#[tokio::test]` | `tests/fixtures/` |
|---|---|---:|---:|---:|---:|---:|---:|---|
| `crates/core/message-crate-core` | message-crate-core | 3,223 | 1,640 | 1,576 | 0 | 106 | 0 | no |
| `crates/exporters/go-sms-pro-exporter` | go-sms-pro-exporter | 1,129 | 405 | 0 | 793 | 32 | 0 | yes |
| `crates/exporters/imazing-exporter` | imazing-exporter | 2,395 | 391 | 1,695 | 375 | 81 | 0 | yes |
| `crates/exporters/imessage-ir-exporter` | imessage-ir-exporter | 1,336 | 453 | 1,024 | 849 | 50 | 0 | yes |
| `crates/exporters/openextract-exporter` | openextract-exporter | 800 | 85 | 0 | 611 | 21 | 0 | yes |
| `crates/exporters/sms-backup-plus-exporter` | sms-backup-plus-exporter | 2,541 | 515 | 902 | 1,963 | 94 | 0 | yes |
| `crates/exporters/sms-backup-restore-exporter` | sms-backup-restore-exporter | 1,586 | 56 | 1,241 | 636 | 52 | 0 | yes |
| `crates/exporters/whatsapp-exporter` | whatsapp-exporter | 2,209 | 1,296 | 0 | 984 | 71 | 0 | yes |
| `crates/helpers/chat-db-fixture` | chat-db-fixture | 831 | 157 | 0 | 0 | 4 | 0 | no |
| `crates/helpers/imessage-reader` | imessage-reader | 3,355 | 3,014 | 201 | 68 | 109 | 0 | yes |
| `crates/helpers/imessage-reader-protocol` | imessage-reader-protocol | 581 | 147 | 0 | 0 | 4 | 0 | no |
| `crates/libs/api-types` | message-crate-api-types | 762 | 281 | 0 | 0 | 8 | 0 | no |
| `crates/libs/build-version` | build-version | 112 | 20 | 0 | 0 | 2 | 0 | no |
| `crates/libs/contacts` | contacts | 178 | 46 | 0 | 0 | 4 | 0 | no |
| `crates/libs/csv` | message-csv | 267 | 163 | 0 | 0 | 12 | 0 | no |
| `crates/libs/export` | message-crate-export | 1,766 | 1,429 | 0 | 1,437 | 69 | 0 | no |
| `crates/libs/go-sms-mms` | go-sms-mms | 1,423 | 9 | 1,341 | 0 | 43 | 0 | no |
| `crates/libs/http` | message-crate-http | 736 | 558 | 0 | 338 | 42 | 0 | no |
| `crates/libs/import` | message-crate-import | 4,518 | 1,275 | 0 | 3,752 | 99 | 0 | no |
| `crates/libs/ios-backup` | ios-backup | 908 | 337 | 381 | 484 | 41 | 0 | yes |
| `crates/libs/ir` | message-ir | 2,323 | 1,098 | 434 | 0 | 67 | 0 | no |
| `crates/libs/ir-format` | message-ir-format | 2,186 | 371 | 2,134 | 0 | 76 | 0 | no |
| `crates/libs/journal` | journal | 189 | 227 | 0 | 0 | 9 | 0 | no |
| `crates/libs/log-lines` | message-crate-log-lines | 225 | 117 | 0 | 0 | 6 | 0 | no |
| `crates/libs/mail` | mail | 1,678 | 387 | 1,442 | 0 | 48 | 0 | no |
| `crates/libs/media` | media | 2,617 | 372 | 2,437 | 0 | 106 | 0 | no |
| `crates/libs/mms-parts` | mms-parts | 202 | 117 | 0 | 0 | 6 | 0 | no |
| `crates/libs/obfuscate` | obfuscate | 731 | 0 | 471 | 0 | 24 | 0 | no |
| `crates/libs/phone` | phone | 1,143 | 674 | 0 | 0 | 42 | 0 | no |
| `crates/libs/reexport` | message-reexport | 768 | 0 | 1,806 | 0 | 57 | 0 | yes |
| `crates/libs/sbr` | sbr | 1,269 | 679 | 0 | 0 | 43 | 0 | no |
| `crates/libs/serve-protocol` | message-crate-serve-protocol | 48 | 0 | 0 | 0 | **0** | 0 | no |
| `crates/libs/staging` | message-staging | 3,094 | 521 | 3,059 | 0 | 91 | 0 | no |
| `crates/server/demo-seed` | demo-seed | 3,901 | 368 | 1,285 | 0 | 49 | 0 | no |
| `crates/server/server` | message-crate-server | 49,671 | 5,105 | 55,140 | 1,221 | 330 | 1,054 | yes |
| `src-tauri` | (not a member) | 7,169 | 1,966 | 3,465 | 41 | 210 | 0 | no |

`serve-protocol` holds four constants and is tested from both sides that use them (`crates/server/server/tests/serve_start_report.rs`, `src-tauri/src/local_server/tests.rs`), so its zero is not a gap.

### 3.1 Server modules (`crates/server/server/src`)

118 non-test source files; 31 have neither an inline test nor a sibling `tests.rs`. [C2] Every `*_api.rs` route group but three has a sibling `tests.rs`:

| Route group | Source lines | Own tests |
|---|---:|---|
| `accounts_api.rs` | 1,513 | `tests.rs` 3,018 lines, 64 tests |
| `assets_api.rs` | 1,453 | 2,203 lines, 49 tests |
| `contacts_api.rs` | 450 | 3,806 lines, 71 tests |
| `conversations_api.rs` | 341 | 2,829 lines, 59 tests |
| `messages_api.rs` | 225 | 2,236 lines, 43 tests |
| `exports_api.rs` | 562 | 1,611 lines, 26 tests |
| `server_api.rs` | 813 | 1,453 lines, 36 tests |
| `session_api.rs` | 292 | 551 lines, 17 tests |
| `audit_trail_api.rs` | 155 | 532 lines, 10 tests |
| `named_set_api.rs` | 427 | 449 lines, 10 tests |
| `trash_api.rs` | 51 | 641 lines, 11 tests |
| `imports_api/mod.rs` | 1,861 | 6,545 lines, 113 tests (plus `imports_api/tests/*.rs`: 37 more) |
| `saved_searches_api.rs` | 305 | 4 inline tests |
| `search_fields_api.rs` | 168 | 1 inline test |
| `phone_countries_api.rs` | 68 | **none**; no test file names `phone-countries` [C12] |

The 31 files without own tests, by size: `search/emit.rs` 1,298, `db/conversation_messages.rs` 1,070, `db/contacts/read.rs` 607, `db/exports.rs` 592, `db/conversations.rs` 500, `db/attachment_versions.rs` 417, `search/bridge.rs` 391, `contacts_api/edit.rs` 391, `search/fields.rs` 356, `db/identity_country.rs` 330, `test_support/lines.rs` 299, `contacts_api/address_book.rs` 278, `logging/files.rs` 237, `identity_country.rs` 193, `logging/read.rs` 155, `exit_with_parent.rs` 154, `openapi/response_fields.rs` 143, `imports_api/with_yourself.rs` 140, `db/participants.rs` 114, `db/media_queue.rs` 111, `db/free_name.rs` 99, `phone_countries_api.rs` 68, `search/error.rs` 68, `search/fts.rs` 65, `progress.rs` 52, `db/maintenance.rs` 49, `db/demo_account_build.rs` 48, `request_id.rs` 41, `db/mod.rs` 39, `main.rs` 25, `media_options.rs` 16.

Most are reached through the route tests of their callers rather than by name: `search/tests.rs` has 76 tests and mentions trash 96 times (so `emit.rs`'s `not_trashed_*` is exercised); `exports_api/tests.rs` hits the exports routes 44 times; the conversation and message routes are hit 84 times; "set aside" participants appear in 19 test lines; the case-insensitive free-name rule appears in 74 lines of the named-set and saved-search tests [C12]. The dated coverage proxy (section 9) says 98.1% of functions were called by some test. The one route with no test of its own content or paging is `GET /v1/phone-countries` (finding T5).

### 3.2 `src-tauri/src` (25 files) [C2]

Nine files have no test: `commands/exports.rs` (3 commands), `commands/local_server.rs` (3 commands), `commands/run_logs.rs` (4 commands), `commands/mod.rs`, `lib.rs`, `main.rs`, `state.rs`, `local_server/process_tree.rs`, `tool_downloads/pins.rs`. The last two are reached: `pins` is named 99 times in `tool_downloads/tests.rs`, and `local_server/tests.rs:685` `stop_ends_the_processes_the_server_started_too` is the process-tree test. The three command files are thin wrappers over modules that are tested (`export_directories/tests.rs` 11 tests, `local_server/tests.rs` 25, `run_logs/tests.rs` 11). `commands/extract.rs` (32 tests), `commands/paths.rs` (28 inline), `commands/jobs.rs` (12), `commands/tools.rs` (8), `commands/events.rs` (7), `commands/download.rs` (7), `commands/staging.rs` (7), `commands/upload.rs` (6) are tested. Finding T13.

### 3.3 `web/src/screens` (96 sources, 57 with a sibling test) [C3]

Of the 39 without a sibling test, these are reached by a parent screen's test: the auth forms (`LoginScreen.test.tsx` names Create Account or Claim 19 times), the owner panels (`OwnerHome.test.tsx` names accounts, audit or dashboard 110 times), `StorageSection.tsx` and `useStorageData.ts` (rendered in 2 test files), `conversationWindow.ts` (24 `offset` assertions in `useConversationMessages.test.tsx`), `importRunStore.ts` (3 test files), `formSnapshot.ts` (2), `useAuditTrail.ts` (1). [C13]

Not referenced by any test file: `screens/message/MessageFindBar.tsx` (98 lines; `MessageThread.test.tsx:47` only passes `findTerm: ""`), `screens/MessageView.tsx` (140), `screens/import/RunFacts.tsx` (166), `screens/import/MissingProgramNotice.tsx` (147), `screens/import/importRunCopy.ts` (63), `screens/auth/useCreateAccountForm.ts` (78, reached only through the Login screen), `screens/settings/ApiTokenForms.tsx` (182), `screens/settings/NewAccountPanel.tsx` (104), `screens/settings/storage/ImportDetailPanel.tsx` (158), `screens/settings/storage/TopAttachmentsTable.tsx` (78). Finding T4.

Outside `screens/`, the largest untested sources are `components/messages/chatBubbleShared.tsx` (527), `components/VirtualList.tsx` (264), `components/advancedSearch/ContactsSearchFields.tsx` (214), `components/ResultsColumn.tsx` (157), `components/IdentityCountryDialog.tsx` (146), `components/advancedSearch/ChoiceMultiSelect.tsx` (146), `hooks/useAssetObjectUrl.ts` (125), `lib/localServer.ts` (77, named in 3 tests), `lib/desktopJob.ts` (68), `lib/phoneCountries.ts` (63). The core modules `lib/api.ts`, `lib/serverApi.ts`, `lib/tauri.ts`, `lib/desktopFeatures.ts`, `lib/auth.tsx`, `lib/searchQuery.ts`, `lib/importRun.ts` all have tests. [C13]

## 4. Test quality

### 4.1 Assertions [C6][C4]

- Rust: 3,161 test bodies parsed. 13 contain no `assert*!`, `panic!` or `expect_*` call. Two are print-only halves of a child-process time-zone check and say so (`crates/core/message-crate-core/src/attachments/tests.rs:67`, `crates/libs/ir/src/projection.rs:874`, the latter `#[ignore]`d for that reason); one asserts through a named helper (`crates/server/server/src/db/address_book/tests.rs:1570` via `contact_named`); ten assert by `unwrap`/`expect` or a mock's `.assert()` on the thing under test (for example `crates/libs/http/tests/app_headers.rs:11-31`, where `named.assert()` is the assertion). None is an empty smoke test.
- Web: 0 of 1,931 cases lack an `expect`.
- Server route tests: 155 `expect_problem(` calls (status, `type` and `request_id` together, as `docs/architecture/http-api.md:903-905` requires) beside 214 `assert_eq!(…StatusCode::…)` lines. How many of the 214 check a failure rather than a success is **Unable to verify** without reading each; finding T11 asks for the sweep.

### 4.2 Naming and structure [C6][C14]

- 0 of 3,161 Rust tests use a `test_` prefix. Names are sentences: `the_exported_file_loads_back_through_the_route_and_changes_nothing` (`contacts_api/tests.rs:2882`), `a_helper_on_another_protocol_version_is_refused_by_both_numbers` (`ios-backup/src/helper/tests.rs:128`). Web: `it("keeps the Groups menu's checkmark live after unticking the group the page is filtered to")`.
- Table-driven: 132 `for (…) in [...]` loops in Rust test files; 20 `it.each`/`test.each` in web tests. The search tests pin refusals in a table (`search/tests.rs:1552-1553`).
- One test name appears in six files: `jsonl_drains_the_write_queue_and_a_second_run_resumes_it`, once per exporter `tests/convert_smoke.rs` (168 to 825 lines each, 2,615 in all). Finding T9; it is the test-side face of design finding F8.
- `CLAUDE.md` says a test module over a few hundred lines lives beside its source as `<module>/tests.rs`. 19 inline modules are 300 lines or more; 7 are 500 or more [C1]:

| File | Module starts | Lines |
|---|---:|---:|
| `crates/helpers/imessage-reader/src/emit.rs` | 745 | 928 |
| `crates/exporters/whatsapp-exporter/src/wtsexporter.rs` | 532 | 667 |
| `crates/libs/phone/src/lib.rs` | 762 | 647 |
| `crates/libs/export/src/project.rs` | 390 | 605 |
| `crates/libs/sbr/src/read.rs` | 958 | 601 |
| `crates/core/message-crate-core/src/exporters.rs` | 711 | 599 |
| `crates/helpers/imessage-reader/src/fields.rs` | 453 | 516 |
| `crates/exporters/imessage-ir-exporter/src/convert.rs` | 933 | 453 |
| `crates/server/server/src/accounts_api/api_tokens.rs` | 389 | 421 |
| `crates/server/server/src/db/account_profile.rs` | 907 | 413 |
| `crates/libs/ir/src/projection.rs` | 510 | 400 |
| `crates/libs/mail/src/parse.rs` | 483 | 387 |
| `crates/helpers/imessage-reader/src/contacts.rs` | 451 | 384 |
| `src-tauri/src/commands/paths.rs` | 449 | 355 |
| `crates/server/server/src/db/api_tokens.rs` | 403 | 334 |
| `crates/server/server/src/asset_uploads.rs` | 519 | 327 |
| `src-tauri/src/run_directories.rs` | 513 | 319 |
| `crates/server/server/src/import_cli.rs` | 304 | 313 |
| `crates/server/server/src/models.rs` | 541 | 303 |

Finding T8. At the other end, the largest `tests.rs` files are `imports_api/tests.rs` 6,545, `search/tests.rs` 4,112, `contacts_api/tests.rs` 3,806, `import/tests/import_mock.rs` 3,752, `accounts_api/tests.rs` 3,018, `reset_demo/tests.rs` 2,907 (finding T12). `imports_api/tests/` already holds four themed files (`backup_dates.rs`, `phone_country.rs`, `changed_content_dedupe.rs`, `time_precision.rs`), so the split pattern exists.

### 4.3 Time dependence [C6][C4]

Rule under test: unit tests never depend on time (memory `tests-not-time-bound`).

- 43 clock reads in Rust test bodies. 30 are `Instant::now()` deadlines around a bounded poll with an `assert!(Instant::now() < deadline, …)` (`assets_api/tests.rs:98-104`, `media_queue/tests.rs:387-393`, `local_server/tests.rs:636-642`) or `SystemTime::now()` to make a unique temp name or an old mtime (`process_assets/tests.rs:1765`, `exporters.rs:1150`). Those are fine. Four build day-relative data from the wall clock and compare it with a date the product computes from its own clock later:
  - `crates/server/server/src/messages_api/tests.rs:1764-1776`: `today` in the account's zone, then `early_today` at 00:30 and `late_yesterday` at 23:30, and `now` a few lines later. Across local midnight between the seed and the request, "today" moves and the assertion flips.
  - `crates/server/server/src/imports_api/tests.rs:3799-3800`: names sets `whatsapp import {today}` and `{tomorrow}` so the import must pick the next free name; the product names its run from its own `today`. Across midnight the test and the server disagree.
  - `crates/server/server/src/audit_trail_api/tests.rs:215`, `:460-461`: 91, 31 and 1 days ago; safe margins.
  - `crates/server/server/src/assets_api/media_links/tests.rs:72`: asserts a lifetime between 55 and 60 minutes; safe margin.
  Finding T3. `search::parse` already takes `today: NaiveDate` (`search/parse.rs:432-436`) and `search::today_in(zone)` exists (`search/mod.rs:81`), so a clock-free seam is one parameter away.
- Web: 8 `vi.useFakeTimers`/`setSystemTime` lines in 4 files (`OnboardingScreen.test.tsx:60,432,444,468`, `logoutDuringUpload.test.tsx:275`, `serverHealth.test.ts:107`, `useServerHealth.test.tsx:15`, `useLocalServer.test.ts:17`), used the right way (`toFake: ["setTimeout", "clearTimeout", "Date"]` with a comment at `OnboardingScreen.test.tsx:426-432`). `Date.now()` is read for real in `ExportScreen.test.tsx:180-182`, as a before/after bracket around a value the code stamps, which is sound.

### 4.4 Sleeps and waits [C6][C4]

- Rust: 31 sleep calls in test bodies across 16 files. 25 are 1 to 50 ms ticks inside a deadline-bounded poll. Six are fixed waits that exist so that something can happen, or to prove it did not:
  - `crates/server/server/src/accounts_api/tests.rs:1661`: `sleep(300 ms)` then `assert!(!delete.is_finished(), "the delete finished while a batch held the account's import lock")`. A negative assertion over a fixed wait: slow hardware cannot make it fail wrongly, but a lock that is released after 310 ms passes it.
  - `src-tauri/src/local_server/tests.rs:758`: `sleep(1 s)` after a second `ensure_started` while a start is under way, then reads what was bound.
  - `crates/libs/import/src/progress.rs:476`: `sleep(150 ms)`; `crates/libs/import/tests/import_mock.rs:2086`: `sleep(UPLOAD_HOLD)`.
  - `src-tauri/src/tool_downloads/tests.rs:836,844,879,947,1128`: `sleep(50 ms)` between steps.
- Web: 13 `setTimeout` lines; 9 are `setTimeout(resolve, 0)` microtask flushes. Fixed waits: `LoginScreen.test.tsx:353` 600 ms (a negative: the card did not go back to "disconnected"), `ConversationList.test.tsx:102` 100 ms (lets React's nested-update guard trip), `useConversationMessages.test.tsx:333` 50 ms. 18 explicit `timeout:` options (2,000 to 5,000 ms, `LoginScreen.test.tsx:577-902`, `ImportFormFields.test.tsx:941-1093`, `SystemSection.test.tsx:382,397`), plus `SLOW_STATE_WAIT` (4,000 ms, `web/src/test/waits.ts`) used 11 times in 5 files, with the reason recorded in the file (#1654, polling intervals) and the sentence "A longer wait is never the fix for a test that failed for another reason."
Finding T6 covers the negative-assertion sleeps.

### 4.5 Isolation and shared state [C15]

- No `serial_test`. Isolation is by construction: `db::engine::test_pool()` gives each test a database in its own `TempDir`; `web/src/test/providers.tsx` builds one `QueryClient` per mount with retries off; `setup.ts` runs Testing Library's `cleanup` after each test because Vitest globals are off.
- Process-wide statics in the server (`crates/server/server/src`): `logging.rs:40 SERVER_LOG` (`init()` ignores a second call "so tests that build a server in-process do not panic"; the unit tests use `LogFiles::open` on a temp directory and the binary is tested by `tests/server_log.rs`), `imports_api/mod.rs:1769-1772` the import semaphore (`OnceLock<Semaphore>` with `MAX_CONCURRENT_IMPORTS = 2`), `credentials.rs:25 DUMMY_PASSWORD_HASH`, `assets_api.rs:482 COUNTS`, `openapi/credential_matrix.rs:391 HASH`, `db/sqlite_functions.rs:35` and `db/write_guard.rs:75` `Once` registrations. None is reset between tests, and none needs to be: the semaphore is never touched by a test (section 6, finding T2), and the rest are idempotent.
- The media tool location is process-wide (`set_tools_dir`, `PATH`), so `crates/libs/media/src/testutil.rs:44-58` serialises it with one `RwLock`: readers share, a test that moves the tools holds it alone. `real_ffmpeg_test_guard()` takes the lock and answers availability in one call so a test cannot be answered by another test's mock directory (#308). That is the right design for a global.
- The test-only SQLite authorizer `db/write_guard.rs` (registered from `db/engine.rs:55`) fails any test that writes `messages`, `attachments`, `message_versions` or their FTS tables outside a transaction. It names its own blind spots (`write_guard.rs:38-48`: statements prepared earlier in a transaction and re-run; a hand-written `BEGIN`; not in the integration tests).

### 4.6 Skips [C6][C7]

- `#[ignore]`: `crates/libs/ir/src/projection.rs:875` and `crates/server/server/src/assets_api/tests.rs:251` are child halves started by a named parent test; `crates/exporters/go-sms-pro-exporter/tests/real_backup.rs:21` needs `GO_SMS_PRO_BACKUP` and `GO_SMS_PRO_OWNER` (a personal backup, which the rules say never to commit). All three carry a reason.
- ffmpeg: 56 `real_ffmpeg_test_guard()` call sites in 13 files (`staging/src/transcode/tests.rs` 24, `media/src/process/tests.rs` 13, `media/src/versions/tests.rs` 5, `reexport/src/tests.rs` 4, the server's `media_queue`, `server_api`, `process_assets`, `imports_api` tests 1 to 2 each, `demo-seed/src/assets.rs` 1, `message-crate-core/src/attachment_jobs/tests.rs` 1). Under `CI` a missing ffmpeg panics (`testutil.rs:61-63, 72-74`), and the `test`, `coverage` and `mutants` jobs install it (`ci.yml:174`, `coverage.yml:28`, `mutants.yml:46`). No `src-tauri` test gates on ffmpeg (0 sites), so the `check-tauri` job needs none. The ADR's "twenty-six tests passed without running" hole is closed.

### 4.7 Fixtures and golden files [C16]

- 82 fixture files across 11 crates (48 in `imazing-exporter`, 8 in the server, 7 in `sms-backup-plus-exporter`, 6 in `whatsapp-exporter`); 5 in the root `tests/fixtures/`. 19 `include_str!`/`include_bytes!` uses in test code.
- No `expected*`/golden output files and no snapshot library: the exporters assert on fields of the parsed IR rather than on a frozen JSONL. That keeps a format change from invalidating every golden, at the cost that a field nobody asserts can drift silently. The design is defensible; the mutation workflow is the check on it.
- `crates/helpers/chat-db-fixture` builds a made-up `chat.db`, a contacts database and an iPhone backup (plain or encrypted) with rusqlite alone, so the exporter's test binary links no GPL code (`lib.rs:1-18`). Three crates use it as a dev-dependency (`ios-backup`, `imessage-ir-exporter`, `imessage-reader`).
- `crates/server/server/src/test_support.rs` (1,331 lines) plus `test_support/lines.rs` (299) is the server's fixture module; design finding F10 covers its shape. Its doc (`:5-10`) still points the handler-level `test_state()` at `server.rs`; it is at `server/tests.rs:142`.

## 5. Mocking strategy [C4][C8]

- Web: no network layer mock (no MSW). Tests mock the route-function module with `vi.mock("…/lib/serverApi", async (importOriginal) => ({ ...(await importOriginal()), listConversationMessages: vi.fn() }))` (`useConversationMessages.test.tsx:12-16`) and fake the session with `vi.mock("…/lib/auth", () => ({ useAuth: () => mockedAuth }))`. Counts by target, summing relative paths: `lib/auth` 51, `lib/serverApi` 48, `lib/tauri-check` 22, `lib/tauri` 9, `@tauri-apps/plugin-dialog` 8, `lib/useAccountProfile` 8, `@tauri-apps/api/core` 3. The fetch wrapper itself is tested in `lib/api.test.ts`, and `serverApi.test.ts` tests the route functions. One consistent seam, which matches ADR 0002's one way to fetch data.
- Rust client libraries: `httpmock` 0.8 is the only HTTP fake (`http` 3 files, `import` 2, `export` 2, `src-tauri` 4; 116 `MockServer::start` sites). `crates/libs/http/tests/` also has `os_trust.rs` and `classify_reqwest.rs` with `rcgen`/`rustls` for TLS cases.
- Server: no mocks. `test_support::serve` binds `127.0.0.1:0` and spawns the app (`test_support.rs:79-83`); helpers issue one `reqwest` request each. The database is a real SQLite file per test. The sidecar is a real process started against `chat-db-fixture`. This is the most faithful arrangement available short of the browser.

## 6. Contract tests [C17]

| Contract | Pinned by | Checked in CI |
|---|---|---|
| OpenAPI document equals the committed JSON | `crates/server/server/src/openapi.rs:429-433` (`include_str!("../../../../docs/src/assets/openapi.json")`, tells you to run `dump-openapi`) | `test` job |
| Generated TypeScript equals the JSON | `scripts/check-generated-api-types.sh` regenerates with `openapi-typescript@7.13.0` and diffs | `web` job (`ci.yml:265`) |
| Every operation accepts and refuses each credential kind as declared | `openapi/credential_matrix.rs` walks the in-process document, so a new route is covered when registered | `test` job |
| Document rules (status taxonomy, shared parts, field descriptions) | `openapi/document_rules.rs`, `shared_parts.rs`, `field_descriptions.rs`, `response_fields.rs` (18 tests in `openapi/`) | `test` job |
| Search language, web to server | `web/src/lib/searchQuery.test.ts:616-634` writes `tests/fixtures/search/web-queries.txt` under `UPDATE_FIXTURES=1` and fails when the builders drift; `crates/server/server/src/search/tests.rs:3604-3660` `include_str!`s the same file and parses every line | both jobs |
| Sidecar protocol version | `PROTOCOL_VERSION = 13` (`imessage-reader-protocol/src/lib.rs:80`), refused at `ios-backup/src/helper.rs:196-208`, tested by `helper/tests.rs:128` with `PROTOCOL_VERSION + 41` | `test` job |
| `serve` start report | `crates/libs/serve-protocol` constants, read by `server/tests/serve_start_report.rs`, `tests/exit_with_parent.rs` and `src-tauri/src/local_server/tests.rs` | `test`, `check-tauri` |
| Window CSP | `src-tauri/tests/window_csp.rs` reads `tauri.conf.json` | `check-tauri` |
| Schema column comments | `crates/server/server/tests/schema_column_comments.rs` | `test` |

Note: `tests/fixtures/search/parse-cases.json` (27 KB) and `parity-messages.json` have no reader in `web/` or the server; only `web-next/src/lib/searchQuery.test.ts` and `scripts/deprecated/regen-search-goldens-worker.ts` read them (finding T15; not a proposal to delete anything, `web-next` is kept on purpose).

## 7. Property and fuzz testing [C8][C9]

None of `proptest`, `quickcheck`, `arbitrary`, `cargo-fuzz`, `insta` or `fast-check` is present in either lockfile; there is no `fuzz/` directory. One deterministic sweep exists and is the template: `crates/libs/go-sms-mms/src/robustness_tests.rs` cuts every seed PDU at every length and applies 2,000 fixed-seed LCG mutations per seed, asserting the decoder never panics and its invariants hold (`:173-230`). It needs no dependency and reruns identically.

Where the same sweep would pay, in order:
1. `crates/server/server/src/search/lex.rs:79 tokenize` and `search/parse.rs:432 parse`: the input is what a person types into the search box and reaches the server on every search. The existing refusals are a two-entry table (`search/tests.rs:1552-1553`). Finding T1, skeleton in section 11.
2. `crates/libs/sbr/src/read.rs:800 parse_file_with` (SMS Backup & Restore XML, from a file the person supplies).
3. `crates/libs/phone/src/lib.rs:73 sanitize_number`, `:191 normalize_checked`, `:439 normalize_typed_handle` (every identity passes through).
4. `crates/libs/mail/src/parse.rs` (EML in).

## 8. Mutation testing configuration

`.cargo/mutants.toml` excludes four crates (`demo-seed`, `chat-db-fixture`, `build-version`, `api-types`), every `testutil.rs`, `Display`/`Debug` impls, a few log-line builders, arithmetic in top-level constants, and `rand_factor`; `nextest` stops at the first failure; `timeout_multiplier = 3.0`; `scripts/mutants-test-runner.sh` caps each test process at 3 GiB. Each exclusion carries a reason, and the regexes are narrow (`replace [-+*/%] with [-+*/%]$` cannot match a mutant inside a function because those end in ` in <function>`).

Two judgements:
- `demo-seed` (3,901 source lines, 49 tests) is excluded because "its output is checked by the demo import test". `serve` seeds the Demo Account into every new Message Crate, so a wrong generator ships in every install. The end-to-end import check catches a bundle that fails to import, not one that imports the wrong thing. Finding T10.
- `api-types` is serde-only and `testutil.rs` is test-only; those exclusions hide nothing.

The score and the missed list are **Unable to verify**: no `mutants.out` or `summary.md` is committed, and `mutants.yml` runs only by hand. The proving artifact is `mutants-summary-<sha>` on a run of that workflow.

## 9. Coverage

**Unable to verify at HEAD.** No coverage report is committed; `coverage.yml` keeps `lcov.info`, `summary.txt`, `functions.txt` and `uncovered-functions.txt` as a 30-day artifact on each push to `main`, and running `scripts/coverage.sh` here is out of scope.

Dated proxy, not from this commit [C10]: `.worktrees/in-depth-code-review/target/llvm-cov/summary.txt` (uncommitted build output in another worktree at `d8d92f466`, 2026-10-02, before many commits on `main`) reports for the workspace less test code and `src-tauri`: regions 91.50% (81,290, 6,906 missed), functions 89.72% (5,612, 577 missed), lines 94.17% (51,816, 3,021 missed); `functions.txt`: "functions called by a test: 3288 of 3353 (98.1%), 65 never called", led by `build-version/src/lib.rs` 4/7, `message-crate-core/src/exporters.rs` 3/47, `imessage-reader/src/backup.rs` 3/14, `ir-format/src/format_sink.rs` 3/21, `ir/src/projection.rs` 3/78. Treat it as the order of magnitude, not the current number.

Structural proxies at HEAD: 1.09 test lines per source line over the Rust tree; 1,384 server tests over 49,671 server source lines; 193 web test files for 349 sources (155 sources without a sibling test, most of them small components or reached through a parent).

## 10. Findings

Importance is 1 to 10, 10 highest. "Bug it would catch" names what the missing or weak test lets through.

| Id | Importance | Finding |
|---|---:|---|
| T1 | 7 | The search lexer and parser have no malformed-input sweep; a panic on a typed query is a dropped connection on every search for that text |
| T2 | 6 | The import concurrency bound (`MAX_CONCURRENT_IMPORTS`) has no test; every import test bypasses the semaphore |
| T3 | 5 | Two server tests build day-relative data from the wall clock and compare it with the product's own "today" |
| T4 | 5 | `MessageFindBar.tsx` and nine other web components are referenced by no test |
| T5 | 4 | `GET /v1/phone-countries` has no route test beyond the credential matrix |
| T6 | 4 | Six fixed sleeps serve as negative assertions ("it did not finish in N ms") |
| T7 | 4 | No automated browser test; the Playwright MCP is manual |
| T8 | 3 | 7 inline test modules of 500 or more lines (19 of 300 or more) against the `tests.rs` rule |
| T9 | 3 | Six copy-paste `convert_smoke.rs` files share test names and 2,615 lines |
| T10 | 3 | `demo-seed` is excluded from mutation testing although it seeds every new Message Crate |
| T11 | 3 | 214 raw `assert_eq!(…StatusCode::…)` beside 155 `expect_problem` in server tests |
| T12 | 2 | `imports_api/tests.rs` is 6,545 lines in one file; `import_mock.rs` 3,752; `search/tests.rs` 4,112 |
| T13 | 2 | Three `src-tauri` command files (10 commands) have no test of their own |
| T14 | 2 | 31 server modules have no test of their own; reached by route tests |
| T15 | 1 | Two JSON fixtures in the root `tests/fixtures/search/` are read only by `web-next/` and a deprecated script |
| T16 | 1 | No performance tests or benches; nothing to add while tests must not be time-bound |

### T1. Search query robustness sweep (7/10)

**Evidence.** `crates/server/server/src/search/lex.rs:79 tokenize(input)` and `search/parse.rs:432 parse(list, tokens, today)` are reached by every `GET /v1/messages`, `/v1/conversations` and `/v1/contacts` with a `q`. The refusal tests are two table rows (`search/tests.rs:1552-1553`, `(unbalanced` and `"unterminated`) and a web-queries fixture of well-formed queries (`search/tests.rs:3604-3660`). No test feeds truncated or corrupted queries. The pattern that would exists in `crates/libs/go-sms-mms/src/robustness_tests.rs:173-230` and costs no dependency.

**Bug it would catch.** An index or slice panic in the lexer or parser on an odd byte sequence, an unclosed quote at a multi-byte boundary, or a field with an empty value; a panic in a handler drops the connection and the person sees a failed search for anything containing that text.

**Remediation.** Section 11, skeleton 1.

### T2. Import concurrency bound untested (6/10)

**Evidence.** `crates/server/server/src/imports_api/mod.rs:1764-1772` declares `MAX_CONCURRENT_IMPORTS = 2` and `import_semaphore()`; `run_import_path` takes a permit at `:1785` "so they can never drain the pool". No test names `import_semaphore`, `MAX_CONCURRENT_IMPORTS` or `available_permits` [C15]; the two grep hits for overlapping imports are about unrelated things (`imports_api/tests.rs:712`, `:4722`). The 113 tests in `imports_api/tests.rs` import through `test_support::import_jsonl_text` (`test_support.rs:1091-1123`), which calls `import_jsonl_files_on_conn` and never passes the semaphore.

**Bug it would catch.** A third concurrent import admitted while two run (pool drained; auth, search and export requests time out), or a permit not returned after an import ends, which after two runs leaves the server refusing every import until restart.

**Remediation.** Section 11, skeleton 2.

### T3. Wall-clock dates in two server tests (5/10)

**Evidence.** `crates/server/server/src/messages_api/tests.rs:1764-1776` and `crates/server/server/src/imports_api/tests.rs:3799-3800` (section 4.3). The rule: unit tests never depend on time.

**Bug it would catch.** None in the product; it is a false failure when the test crosses local midnight between setup and request, which is the kind of red that gets "fixed" by a rerun.

**Remediation.** Section 11, skeleton 3: assert the date logic at the clock-free seam that already exists (`parse(list, tokens, today)`), and give the Import Run naming a `today` parameter so the route passes `search::today_in(zone)` and the test passes a fixed date.

### T4. Web components no test references (5/10)

**Evidence.** Section 3.3; the list [C13]. `screens/message/MessageFindBar.tsx` carries the Find keyboard contract (`:51-56`: Enter steps older, Shift+Enter newer, Escape closes; `:62-66`: the "n of m" count is `matchPosition + 1`; `:70-72, 79-81`: the step buttons are disabled at `matchCount === 0`).

**Bug it would catch.** An off-by-one in "n of m", Enter stepping when there is no match, Escape not closing, or the buttons staying enabled with no matches. The other nine are mostly presentational; `RunFacts.tsx` (166 lines) and `MissingProgramNotice.tsx` (147) carry copy and branching worth one test each.

**Remediation.** Section 11, skeleton 4.

### T5. `GET /v1/phone-countries` route test (4/10)

**Evidence.** `crates/server/server/src/phone_countries_api.rs:1-68`: a paged list over `phone::COUNTRIES` with `limit` default 40, max 500, `offset` max 50,000 through `page_params`. No test file names `phone-countries` [C12]; the credential matrix calls it only to check who may.

**Bug it would catch.** A paging change that drops the last country, a duplicate entry, `XK` missing after a table edit, or a `limit` cap that no longer matches the documented 500.

**Remediation.** Section 11, skeleton 5.

### T6. Fixed sleeps as negative assertions (4/10)

**Evidence.** `accounts_api/tests.rs:1661` (300 ms, then `!delete.is_finished()`), `src-tauri/src/local_server/tests.rs:758` (1 s), `web/src/screens/LoginScreen.test.tsx:353` (600 ms), `web/src/screens/ConversationList.test.tsx:102` (100 ms), `web/src/screens/message/useConversationMessages.test.tsx:333` (50 ms), `crates/libs/import/src/progress.rs:476` (150 ms).

**Bug it would miss.** A lock, poll or effect that releases just after the wait passes the test; and a slower machine turns a positive wait into a flake, which the rule forbids fixing by widening.

**Remediation.** Replace "wait N then assert nothing happened" with a signal the code under test emits: for the account-delete case, have the delete task report that it is waiting on the lock (a oneshot sent before `lock().await`), await that, then assert `!is_finished()` and release the batch. For `local_server/tests.rs:758`, poll `binds` until it has two lines with the existing deadline loop (`:752-756`) rather than sleeping one second. On the web, `vi.useFakeTimers` plus `vi.advanceTimersByTime(600)` makes `LoginScreen.test.tsx:353` deterministic, as `OnboardingScreen.test.tsx:432` already does.

### T7. No automated browser test (4/10)

**Evidence.** [C5]. `AGENTS.md` "Tools" says to verify a `web/` UI change by hand with the Playwright MCP. ADR 0007 argues against gates that cost more than they return.

**Bug it would catch.** The one class the current suite cannot: `web/` and the server disagree at runtime in a way the generated types and route mocks both accept (a route function mocked in 48 files is never called against the server in CI), or the served `static/` bundle fails to boot.

**Remediation.** Not a gate (ADR 0007). One report-only workflow on push to `main`, like `coverage.yml`: build the server, run `serve` on a temp data directory (the Demo Account is seeded on first start), run one Playwright script that logs in as `demo` with an empty password, searches `hello`, opens the first conversation, and fails on any console error or a 5xx in `page.on("response")`. It would have no `pull_request` trigger and would open an issue like `nightly.yml` does.

### T8. Inline test modules over the size rule (3/10)

**Evidence.** Table in 4.2. `CLAUDE.md`: "A test module over a few hundred lines lives beside its source as `<module>/tests.rs`."

**Remediation.** For each of the seven files of 500 or more lines, move the `mod tests { … }` body to `<module>/tests.rs` and leave `#[cfg(test)] mod tests;`. The imports at the top of the moved file become `use super::*;` plus what the module imported. No test changes.

### T9. Copy-paste smoke tests (3/10)

**Evidence.** `jsonl_drains_the_write_queue_and_a_second_run_resumes_it` in all six `crates/exporters/*/tests/convert_smoke.rs` (168 to 825 lines; 2,615 lines in all) [C14].

**Remediation.** Lift the shared body into `message-crate-core`'s existing `testutil` feature (the crates already depend on it as a dev-dependency): `pub fn assert_second_run_resumes(run: impl Fn(&Path) -> anyhow::Result<()>, fixture: &Path)`. Each exporter keeps a two-line test that passes its own run function. The design audit's F8 covers the source side.

### T10. `demo-seed` excluded from mutants (3/10)

**Evidence.** `.cargo/mutants.toml` `exclude_globs` entry and its reason; `crates/server/demo-seed` has 3,901 source lines and 49 tests of its own.

**Remediation.** Remove the glob and run `./scripts/mutants.sh --file crates/server/demo-seed/src/lib.rs` once by hand; keep the exclusion only if the missed list is all text and shuffling, and write that finding into the toml comment.

### T11. Raw status assertions beside `expect_problem` (3/10)

**Evidence.** 155 `expect_problem(` and 214 `assert_eq!(…StatusCode::…)` in server tests [C6]; `docs/architecture/http-api.md:903-905`.

**Remediation.** A sweep: `grep -rn "assert_eq!(.*StatusCode::\(BAD_REQUEST\|NOT_FOUND\|FORBIDDEN\|UNAUTHORIZED\|CONFLICT\|UNPROCESSABLE\)" crates/server/server/src --include=*.rs` lists the failure checks; each becomes `expect_problem(response, StatusCode::…, "<type>").await`, which also asserts `type` and `request_id`.

### T12. Very large test files (2/10)

**Remediation.** `imports_api/tests.rs` already has four themed siblings under `imports_api/tests/`; move the next themes (dedupe, contact names, trash, assets) the same way. `import_mock.rs` can split by scenario into `tests/import_mock/*.rs` with a shared `mod common;`.

### T13. `src-tauri` command wrappers (2/10)

**Evidence.** `commands/exports.rs` (3 commands), `commands/local_server.rs` (3), `commands/run_logs.rs` (4), `state.rs`. The modules they wrap are tested.

**Remediation.** One test per file that the command's error string is the module's error (`Err(e) => e.to_string()`), so a wrapper that swallows a reason is caught. Low priority; the wrappers are a few lines each.

### T14. Server modules reached only through routes (2/10)

**Evidence.** Section 3.1 and [C12]. Of `db/conversation_messages.rs`'s 13 public functions, 8 are not named in any test file, though the message and conversation routes are hit 84 times; `search/bridge.rs` 8 of 19; `db/exports.rs` 8 of 13.

**Remediation.** Nothing by itself: a route test is the right level for `db/` (http-api.md:903). Use the mutants report on these files to find the branches the routes do not drive, and add route tests for those.

### T15. Orphaned search fixtures in the root directory (1/10)

**Evidence.** `tests/fixtures/search/parse-cases.json` and `parity-messages.json`; readers only in `web-next/` and `scripts/deprecated/` [C17]. `CLAUDE.md` says the root `tests/fixtures/` holds fixtures that are not one crate's own, which these now are (web-next's).

**Remediation.** Record it; nothing to delete (`web-next` is kept on purpose, and its drift is fixed on its side).

### T16. No performance tests (1/10)

**Evidence.** No `benches/` directory, `criterion` absent from `Cargo.lock` [C8].

**Remediation.** None. A timing assertion in a unit test is what the time rule forbids. If a regression needs watching, a report-only workflow like `coverage.yml` is the place.

## 11. Drop-in test skeletons for the top five

Helper names marked `// adapt:` are placeholders where the exact `test_support.rs` verb helper (`:380-765`) was not enumerated in this audit.

### 1. Search query sweep (T1), in `crates/server/server/src/search/tests.rs`

```rust
/// Every query a person can type is answered or refused, never a panic.
/// Mirrors crates/libs/go-sms-mms/src/robustness_tests.rs: every seed is cut
/// at every char boundary, then mutated with a fixed-seed generator, so a
/// failure names the input and reruns the same way every time.
#[test]
fn truncated_and_corrupted_queries_never_panic() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
    let seeds = [
        r#"from:"Ada Lovelace" has:attachment before:2024-01-01 (hello OR "good night") -spam"#,
        "in:#12 kind:group larger:10mb filetype:jpg after:yesterday",
        "\"unterminated (unbalanced tag:a,b, from: -",
        "émoji 🙂 \u{200b} \u{ffff} ::: \"\"\"",
    ];
    let mut rng = Lcg(0x9e37_79b9_7f4a_7c15); // copy Lcg and mutate from go-sms-mms robustness_tests.rs:17-41
    let mut failures = Vec::new();
    for seed in seeds {
        for (end, _) in seed.char_indices().chain([(seed.len(), ' ')]) {
            check(&seed[..end], today, &mut failures);
        }
        for case in 0..2_000u64 {
            let mut chars: Vec<char> = seed.chars().collect();
            for _ in 0..1 + rng.below(4) {
                mutate_chars(&mut rng, &mut chars); // delete, duplicate, swap or replace one char
            }
            let input: String = chars.into_iter().collect();
            check(&input, today, &mut failures);
            let _ = case;
        }
    }
    assert!(
        failures.is_empty(),
        "{} inputs panicked:\n{}",
        failures.len(),
        failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );

    fn check(input: &str, today: chrono::NaiveDate, failures: &mut Vec<String>) {
        for list in [ListKind::Contacts, ListKind::Conversations, ListKind::Messages] {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let tokens = super::lex::tokenize(input)?;
                super::parse::parse(list, &tokens, today).map(|_| ())
            }));
            if outcome.is_err() {
                failures.push(format!("{list:?}: {input:?}"));
            }
        }
    }
}
```

`tokenize` and `parse` are `pub(crate)`, so the test must stay in the server crate, which `search/tests.rs` does. A second assertion worth adding once the sweep passes: every `Err` is one of the `QueryErrorKind` variants with a non-empty message and a span inside the input.

### 2. Import concurrency bound (T2), in `crates/server/server/src/imports_api/tests.rs`

```rust
/// A third import waits while two run, and runs once one of them ends; the
/// bound keeps auth, search and export connections free (mod.rs:1764-1772).
/// Yields, never sleeps: under the default current-thread test runtime, the
/// spawned task runs to its first `.await` on each yield.
#[tokio::test]
async fn a_third_import_waits_until_one_of_two_running_imports_ends() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let semaphore = super::import_semaphore();
    assert_eq!(semaphore.available_permits(), super::MAX_CONCURRENT_IMPORTS);

    // Two imports "run": hold both permits the way run_import_path does.
    let held = semaphore
        .acquire_many(super::MAX_CONCURRENT_IMPORTS as u32)
        .await
        .unwrap();

    let third = tokio::spawn({
        let token = user.token.clone();
        let base = server.base().to_string();
        async move {
            // adapt: the test_support helper that POSTs a JSON Lines body to the import route
            crate::test_support::post_import_jsonl(&base, &token, ADA_CONVERSATION_JSONL).await
        }
    });
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert!(!third.is_finished(), "a third import ran while two held the bound");

    drop(held);
    let status = third.await.unwrap();
    assert_eq!(status, StatusCode::OK, "the import ran once a permit was free");
    assert_eq!(
        semaphore.available_permits(),
        super::MAX_CONCURRENT_IMPORTS,
        "the permit came back after the import"
    );
}
```

The semaphore is process-wide, so while this test holds both permits any other test that goes through the import route in the same binary waits; none asserts on timing, so they are delayed, not broken. A second test, `an_import_that_fails_returns_its_permit`, posts a body the route refuses after the permit is taken and asserts `available_permits()` is back to the bound.

### 3. Clock-free date tests (T3)

For `messages_api/tests.rs:1764-1776`, assert the day logic where `today` is a parameter:

```rust
/// `date:today` and `date:yesterday` split at the account's local midnight,
/// not UTC's. Clock-free: the parser takes `today` (search/parse.rs:432).
#[test]
fn today_and_yesterday_split_at_the_accounts_local_midnight() {
    let zone: chrono_tz::Tz = "America/New_York".parse().unwrap();
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(); // the DST change day
    let tokens = super::lex::tokenize("date:today").unwrap();
    let expr = super::parse::parse(ListKind::Messages, &tokens, today).unwrap().unwrap();
    // adapt: whatever the Expr exposes for the resolved bounds
    let (from, to) = expr.date_bounds_in(zone).expect("date:today has bounds");
    assert_eq!(from, zone.with_ymd_and_hms(2026, 3, 8, 0, 0, 0).unwrap().with_timezone(&chrono::Utc));
    assert_eq!(to, zone.with_ymd_and_hms(2026, 3, 9, 0, 0, 0).unwrap().with_timezone(&chrono::Utc));
}
```

Then keep one route test that cannot cross midnight: seed `now - 2h` and `now - 26h` and assert `date:today OR date:yesterday` returns both. For `imports_api/tests.rs:3799`, give the function that names an Import Run's sets a `today: NaiveDate` argument, pass `search::today_in(zone)` from the route and a fixed date from the test; the test then asserts the exact name `whatsapp import 2026-10-10 3`.

### 4. `MessageFindBar` (T4), new file `web/src/screens/message/MessageFindBar.test.tsx`

```tsx
/** @vitest-environment jsdom */
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { setupUser } from "../../test/user";
import MessageFindBar from "./MessageFindBar";

type Props = React.ComponentProps<typeof MessageFindBar>;

function renderBar(over: Partial<Props> = {}) {
  const props: Props = {
    findTerm: "ada",
    onFindTermChange: vi.fn(),
    matchCount: 3,
    matchPosition: 0,
    searching: false,
    onPrevMatch: vi.fn(),
    onNextMatch: vi.fn(),
    onClose: vi.fn(),
    ...over,
  };
  render(<MessageFindBar {...props} />);
  return props;
}

describe("MessageFindBar", () => {
  it("counts the current match from one", () => {
    renderBar({ matchPosition: 0, matchCount: 3 });
    expect(screen.getByText("1 of 3")).toBeInTheDocument();
  });

  it("Enter steps to the older match and Shift+Enter to the newer one", async () => {
    const user = setupUser();
    const props = renderBar();
    const input = screen.getByRole("textbox", { name: "Find in conversation" });
    await user.type(input, "{Enter}");
    expect(props.onPrevMatch).toHaveBeenCalledTimes(1);
    expect(props.onNextMatch).not.toHaveBeenCalled();
    await user.type(input, "{Shift>}{Enter}{/Shift}");
    expect(props.onNextMatch).toHaveBeenCalledTimes(1);
  });

  it("does not step while there is no match, and says so", async () => {
    const user = setupUser();
    const props = renderBar({ matchCount: 0 });
    expect(screen.getByText("No matches")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Older match" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Newer match" })).toBeDisabled();
    await user.type(screen.getByRole("textbox"), "{Enter}");
    expect(props.onPrevMatch).not.toHaveBeenCalled();
  });

  it("says it is finding while the matches are still being read", () => {
    renderBar({ matchCount: 0, searching: true });
    expect(screen.getByText("Finding…")).toBeInTheDocument();
  });

  it("Escape closes Find", async () => {
    const user = setupUser();
    const props = renderBar();
    await user.type(screen.getByRole("textbox"), "{Escape}");
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });
});
```

If `PlainButton` renders `aria-disabled` rather than `disabled`, use `toHaveAttribute("aria-disabled", "true")`.

### 5. `GET /v1/phone-countries` (T5), new `crates/server/server/src/phone_countries_api/tests.rs` with `#[cfg(test)] mod tests;` added to `phone_countries_api.rs`

```rust
use axum::http::StatusCode;
use crate::test_support::{expect_problem, fixture_with_account};

/// Every country the server keys numbers by is offered once; the page shape
/// follows "Lists" in docs/architecture/http-api.md.
#[tokio::test]
async fn the_list_offers_every_country_the_server_keys_numbers_by() {
    let (fixture, user) = fixture_with_account().await;
    // adapt: the test_support GET-as-JSON helper
    let page = crate::test_support::get_json(&fixture.state, "/v1/phone-countries?limit=500", &user.token).await;
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), phone::COUNTRIES.len());
    assert_eq!(page["total"].as_u64().unwrap() as usize, phone::COUNTRIES.len());
    assert!(items.iter().any(|c| c["code"] == "XK"), "Kosovo is offered under XK");
    let codes: std::collections::BTreeSet<&str> =
        items.iter().map(|c| c["code"].as_str().unwrap()).collect();
    assert_eq!(codes.len(), items.len(), "no country is offered twice");
}

/// The default page is 40, an offset past the end is empty, and a limit over
/// the cap is refused as a problem, not clamped.
#[tokio::test]
async fn paging_follows_the_documented_default_and_cap() {
    let (fixture, user) = fixture_with_account().await;
    let first = crate::test_support::get_json(&fixture.state, "/v1/phone-countries", &user.token).await;
    assert_eq!(first["items"].as_array().unwrap().len(), 40);
    let past = crate::test_support::get_json(&fixture.state, "/v1/phone-countries?offset=50000", &user.token).await;
    assert_eq!(past["items"].as_array().unwrap().len(), 0);
    // adapt: the raw-response helper and the problem type name page_params uses
    let response = crate::test_support::get_raw(&fixture.state, "/v1/phone-countries?limit=501", &user.token).await;
    expect_problem(response, StatusCode::BAD_REQUEST, "invalid-paging").await;
}
```

## 12. What is done well

- 3,162 tests named as sentences, 0 `test_` prefixes, 0 `.skip`/`.only`, 3 `#[ignore]` each with a reason, and effectively no assertion-free test.
- The ffmpeg guard turns a missing tool into a red job under CI and a labelled skip elsewhere, from one function; the ADR's "26 tests passed without running" hole is closed by design, not by convention.
- Four contract seams are pinned by tests, not by review: the OpenAPI JSON, the generated TypeScript, the shared search-language fixture, and the sidecar protocol version.
- The credential matrix walks every operation with every credential kind from the in-process document, so a new route is covered the day it is registered.
- The test-only SQLite authorizer makes a transactional rule a failing test and documents its own blind spots.
- The server tests drive a real listener and a real database per test; the GPL seam is tested with a made-up database so no test binary links GPL code.
- One deterministic robustness sweep already exists for the binary PDU decoder and can be copied without a dependency.
- The web test helpers are small and explicit (`providers.tsx`, `waits.ts`, `setup.ts`), each with the reason it exists written at the top.

## 13. Unable to verify

- Coverage percentage at HEAD (no committed report; the proxy in section 9 is from `d8d92f466`). Proving artifact: `coverage-<sha>` from `coverage.yml` on `main`.
- Mutation score and the missed-mutant list. Proving artifact: `mutants-summary-<sha>` from a hand-started `mutants.yml` run.
- How many of the 214 raw `StatusCode` assertions check a failure rather than a success (T11).
- Whether every `limit`/`offset` problem type is named `invalid-paging` (skeleton 5 marks it `adapt`).
- Whether `PlainButton` renders `disabled` or `aria-disabled` (skeleton 4).
- The exact names of the `test_support.rs` GET and POST helpers (`:380-765`) used in skeletons 2 and 5.
- Whether the `check-tauri` job's apt line includes ffmpeg (immaterial: no `src-tauri` test gates on it).

## 14. Suggested order of work

1. T1 search sweep (one test, no dependency, highest payoff).
2. T2 import bound test (two tests).
3. T3 the two wall-clock tests, with the `today` seam.
4. T4 `MessageFindBar.test.tsx`, then `RunFacts` and `MissingProgramNotice`.
5. T5 phone-countries route tests.
6. T6 replace the six negative-assertion sleeps with signals or fake timers.
7. T8 move the seven 500-line inline modules to `tests.rs`; T9 lift the shared smoke test.
8. T10 one hand run of mutants over `demo-seed`, then decide the exclusion on evidence.
9. T7 a report-only browser smoke on `main`, if and when the team wants it.

## Appendix A. Commands

All run from the repository root at `530dc1791`. Python ran with `-I` from stdin; the scripts are reproduced in condensed form.

```bash
# [C1] Per-crate source lines, inline test block lines, tests.rs lines, tests/*.rs lines,
#      #[test] and #[tokio::test] counts, fixtures directory, inline modules >= 300 lines.
cargo metadata --no-deps --format-version 1 --offline   # member list; src-tauri appended by hand
python3 -I - <<'EOF'
# for each crate: walk src/, classify a file as test code when its name is tests.rs,
# test_support.rs or testutil.rs or its path has /tests/ or /test_support/; otherwise
# brace-match each "#[cfg(test)]\nmod x {" block and subtract its lines from source;
# count r'^\s*#\[test\]' and r'^\s*#\[tokio::test' per file; walk tests/ (not fixtures/).
EOF

# [C2] Server and src-tauri modules: inline tests, sibling <stem>/tests.rs, line counts.
python3 -I - <<'EOF'
# for each .rs under crates/server/server/src (and src-tauri/src) that is not a test file:
# count r'^\s*#\[(tokio::)?test\b'; check os.path.exists(dir/<stem>/tests.rs); report.
EOF
ls -la crates/server/server/tests/; grep -n "CARGO_BIN_EXE" crates/server/server/tests/*.rs

# [C3] Web sources vs test files, per directory and per screen.
python3 -I - <<'EOF'
# walk web/src for .ts/.tsx (not .d.ts); a test file matches r'\.(test|spec)\.(ts|tsx)$';
# a source "has a test" when <stem>.test.ts(x) or <stem>.spec.ts(x) exists beside it.
EOF

# [C4] Web test quality: cases, cases without expect, vi.mock targets, skips, timers, waits.
python3 -I - <<'EOF'
# per test file: count r'^(\s*)(it|test)\(' cases and the body up to the matching "});" at
# the same indent; flag bodies without r'\bexpect\(|\bexpect\.|assert'; collect
# vi.mock("...") targets; grep .skip/.only/.todo, useFakeTimers|setSystemTime,
# Date.now()|new Date(), setTimeout(, timeout: <n>, it.each|test.each.
EOF
grep -n '"msw"\|fast-check\|"playwright"' web/package.json
grep -c '"node_modules/msw"\|"node_modules/fast-check"\|"node_modules/playwright"' web/package-lock.json

# [C5] Playwright: who pulls it into the web lockfile.
python3 -I -c 'import json;d=json.load(open("web/package-lock.json"));[print(k,"->",x) for k,v in d["packages"].items() for x in {**v.get("dependencies",{}),**v.get("peerDependencies",{})} if "playwright" in x]'
grep -rn "playwright" web/package.json .github/ AGENTS.md

# [C6] Rust test bodies: assertion density, naming, ignore, should_panic, serial, sleeps, clock reads.
python3 -I - <<'EOF'
# for each .rs under crates/ and src-tauri/: find r'^\s*#\[(tokio::)?test[^\n]*\]' followed by
# attributes and "fn name"; brace-match the body; flag bodies with no
# r'\bassert(_eq|_ne|_matches)?!|\bexpect_problem\b|\bexpect_\w+\(|\bpanic!\(|\bunreachable!\(';
# count names starting with test_; grep #[ignore, #[serial, should_panic;
# in bodies grep thread::sleep|tokio::time::sleep|\bsleep\( and
# Utc::now\(\)|SystemTime::now\(\)|Instant::now\(\)|Local::now\(\)|now_utc\(\).
EOF
grep -rn "expect_problem(" crates/server/server/src --include=*.rs | grep -v test_support.rs | wc -l   # 155
grep -rn "assert_eq!(.*StatusCode::" crates/server/server/src --include=*.rs | wc -l                  # 214

# [C7] ffmpeg gating.
sed -n 1,80p crates/libs/media/src/testutil.rs
grep -rn "real_ffmpeg_test_guard()" crates --include=*.rs | grep -v testutil.rs | awk -F: '{print $1}' | sort | uniq -c | sort -rn
grep -rn "real_ffmpeg_test_guard" src-tauri/src --include=*.rs | wc -l
grep -n "ffmpeg" .github/workflows/ci.yml .github/workflows/coverage.yml .github/workflows/mutants.yml
grep -rn 'env::var("CI")' crates src-tauri/src --include=*.rs

# [C8] Dev-dependencies and test-tool crates.
cargo metadata --no-deps --format-version 1 --offline | python3 -I -c 'import json,sys;d=json.load(sys.stdin);[print(p["name"],"->",sorted(x["name"] for x in p["dependencies"] if x["kind"]=="dev")) for p in d["packages"]]'
awk '/^\[dev-dependencies\]/,/^\[lints/' src-tauri/Cargo.toml
for c in wiremock httpmock mockito mockall proptest quickcheck insta rstest serial_test arbitrary test-case pretty_assertions assert_cmd tempfile criterion; do printf "%-18s " "$c"; grep -c "^name = \"$c\"" Cargo.lock; done
grep -rn "MockServer::start" crates src-tauri/src --include=*.rs | wc -l    # 116
ls -d crates/*/*/benches 2>/dev/null

# [C9] Property/fuzz tooling anywhere in scope.
grep -rniE "insta::|proptest|quickcheck|fuzz_target|cargo-fuzz|fast-check|arbitrary::Arbitrary" --include=*.rs --include=*.ts --include=*.tsx --include=*.toml --include=*.json . | grep -v node_modules | grep -v /target/ | grep -v web-next | grep -v "^./.worktrees"
ls fuzz 2>/dev/null
sed -n 1,30p crates/libs/go-sms-mms/src/robustness_tests.rs; sed -n 173,230p crates/libs/go-sms-mms/src/robustness_tests.rs

# [C10] Committed or stray coverage and mutants artifacts.
find . -path ./target -prune -o -path ./node_modules -prune -o -path ./web-next -prune -o -type f \( -name lcov.info -o -name mutants.out -o -name summary.txt -o -name uncovered-functions.txt \) -print
git ls-files | grep -iE 'lcov|mutants\.out|uncovered'
git -C .worktrees/in-depth-code-review log -1 --format='%h %ci %s'
tail -6 .worktrees/in-depth-code-review/target/llvm-cov/summary.txt
cat .worktrees/in-depth-code-review/target/llvm-cov/functions.txt

# [C11] Integration test directories and sizes.
ls crates/libs/import/tests crates/libs/export/tests crates/libs/http/tests src-tauri/tests
wc -l crates/exporters/*/tests/convert_smoke.rs

# [C12] Reach of server modules without their own tests.
python3 -I - <<'EOF'
# concatenate every server test file; for each target module list r'^\s*pub(\(crate\))?\s+(async\s+)?fn\s+(\w+)'
# and report which names appear in that text.
EOF
grep -rln "phone-countries" crates/server/server/src --include=tests.rs | wc -l            # 0
grep -c -i "trash" crates/server/server/src/search/tests.rs                                 # 96
grep -c "/v1/accounts/.*/exports\|/v1/exports" crates/server/server/src/exports_api/tests.rs   # 44
grep -rn "set_aside\|set aside" crates/server/server/src --include=tests.rs | wc -l         # 19
grep -rn -i "conflict\|409\|letter case\|case-insensitive\|lower case\|upper case\|ALICE\|same name" crates/server/server/src/named_set_api/tests.rs crates/server/server/src/db/named_membership/tests.rs crates/server/server/src/db/saved_searches/tests.rs crates/server/server/src/saved_searches_api.rs | wc -l   # 74

# [C13] Web modules without a sibling test: referenced by any test file?
for f in importRunStore formSnapshot conversationWindow useStorageData MessageFindBar MessageView useAuditTrail localServer logSource useCreateAccountForm RunFacts MissingProgramNotice; do printf "%-24s %s\n" $f "$(grep -rl "$f" web/src --include=*.test.ts --include=*.test.tsx | wc -l)"; done
grep -c "offset" web/src/screens/message/useConversationMessages.test.tsx                   # 24
grep -c -i "create account\|claim" web/src/screens/LoginScreen.test.tsx                       # 19
grep -c -i "accounts\|audit\|dashboard" web/src/screens/OwnerHome.test.tsx                    # 110
grep -rn "MessageFindBar\|matchPosition\|onPrevMatch" web/src --include=*.test.tsx | wc -l    # 0

# [C14] Table-driven loops, largest test files, duplicate test names.
grep -rn "for (" crates src-tauri/src --include=*.rs | grep -E "tests\.rs|/tests/|_tests\.rs" | grep -E "in \[|in &\[|in cases|in CASES" | wc -l   # 132
find crates src-tauri -name tests.rs -o -name '*_tests.rs' -o -path '*/tests/*.rs' | grep -v target | grep -v fixtures | xargs wc -l | sort -rn | head -16
grep -rln "fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it" crates

# [C15] Shared static state and the import semaphore.
grep -rn "^\s*static \|^\s*pub static \|OnceLock<\|LazyLock<" crates/server/server/src --include=*.rs | grep -v "tests.rs\|test_support"
sed -n 1760,1795p crates/server/server/src/imports_api/mod.rs; sed -n 30,45p crates/server/server/src/logging.rs
grep -rn "import_semaphore\|MAX_CONCURRENT\|available_permits" crates/server/server/src --include=*.rs
grep -rn -i "MAX_CONCURRENT\|overlap\|at the same time\|third import\|two imports run" crates/server/server/src/imports_api/tests.rs crates/server/server/tests/*.rs

# [C16] Fixtures and golden files.
find crates -path '*/tests/fixtures/*' -type f | awk -F/ '{print $1"/"$2"/"$3}' | sort | uniq -c | sort -rn
find crates tests -path '*fixtures*' -type f \( -iname '*expected*' -o -iname '*golden*' -o -iname '*.snap' \) | wc -l   # 0
grep -rn "include_str!\|include_bytes!" crates src-tauri/src --include=*.rs | grep -i "test\|fixture" | wc -l            # 19

# [C17] Contract tests.
grep -rn "assets/openapi.json" crates/server/server/src --include=*.rs
cat scripts/check-generated-api-types.sh
grep -n "web-queries\|writeFile\|UPDATE_FIXTURES" web/src/lib/searchQuery.test.ts
grep -rn "web-queries" crates --include=*.rs
grep -n "PROTOCOL_VERSION" crates/helpers/imessage-reader-protocol/src/lib.rs crates/libs/ios-backup/src/helper.rs crates/libs/ios-backup/src/helper/tests.rs
grep -rln "parse-cases\|parity-messages" --include=*.ts --include=*.tsx --include=*.rs --include=*.md --include=*.sh --include=*.py . | grep -v node_modules | grep -v /target/ | grep -v "^./tests/fixtures/search/"
```
