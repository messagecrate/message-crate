# Readability and naming audit

**Date:** 2026-10-10
**Commit:** `530dc1791`
**Scope:** the whole codebase: `crates/`, `src-tauri/src`, `web/src`, the naming contract in `CONTEXT.md`, `docs/architecture/http-api.md` ("Code") and `docs/agents/writing-style.md`, and the last 30 commit subjects. Excluded: `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`, and the generated `web/src/lib/serverApi.types.ts`.
**Method:** read-only. Every count below comes from `grep`, `find`, `awk` or `sed -n` over the working tree at this commit; nothing was compiled or run. Where a count depends on a heuristic, the heuristic is stated with it.

**Assumptions.** The repo's own documents are the standard, not generic taste: a word `CONTEXT.md` lists under _Avoid_ is a finding wherever a reader meets it, and an identifier is judged by the glossary when it names a product thing. British spelling is taken as the house standard because `CONTEXT.md` writes "cancelled", "labelled" and "Uncategorised" and `writing-style.md` writes "Neighbouring"; no document states the rule. Rust library vocabulary (`serialize`, `normalize`) is not counted as spelling. Vendor vocabulary inside a reader for that vendor's format (Apple's `chat` table in `imessage-reader`, WhatsApp's `ChatStorage.sqlite` in `whatsapp-exporter`, the `pdu`/`wsp` names of the MMS standards) is not counted as an avoided word.

A design audit exists at `audits/2026-10-09-initial-software-design-analysis.md`; its findings are cited as F1 to F19 where they overlap and are not repeated here.

## 1. Summary

The code is named with unusual care. Every one of the 66 wire types the server publishes follows the `http-api.md` naming rule, every routed handler is `verb_noun`, 3,163 Rust tests and 1,924 web tests are sentences (none start with `test_` or "should"), 504 of 506 intradoc links resolve to a definition in the repo, doc comments explain why rather than what, nesting is shallow, and the sampled commit subjects follow the voice guide without exception.

The findings are therefore about consistency at the edges, in order of weight:

1. The glossary's avoided words survive in a few reader-facing strings and in identifiers that name product things: "chat" in two Import Run summary lines, "Thread" as the web app's name for the Conversation on screen, "labels" as demo-seed's name for Contact Groups, and "direct" as a fourth name for the one-to-one conversation beside `individual`, `Individual` and "one-to-one".
2. Two words for one thing across a boundary: `Extract*Event` in Rust against `Import*Event` in TypeScript under an `extract:` channel that also carries Upload events; "Tauri job" against "desktop job" inside the web app; `IrService` documented as a transport beside `IdentityService` documented as the service.
3. Rules that exist in practice but are not written down, so each new file picks its own: the `Ir` prefix in `message-ir` (9 types carry it, 14 do not), where a hook lives (6 in `hooks/`, 23 in `lib/`, 13 elsewhere), the exporters' module names (`xml`/`parse`/`read`/`parse_emit`/`convert`), the crate-name prefix (`message-` beside `message-crate-`), and the register of an error's `detail` sentence.
4. The voice guide's dash rule: 228 Rust comment lines and 5 reader-facing Rust strings join clauses with an em dash.

| Importance | Count | IDs |
|---|---|---|
| 6 | 1 | N1 |
| 5 | 6 | N2, N3, N4, N11, N17, N18 |
| 4 | 7 | N19, N20, N21, N26, N28, N33, N34 |
| 3 | 13 | N5, N6, N8, N9, N12, N14, N22, N23, N24, N27, N29, N32, N35 |
| 2 | 6 | N7, N15, N16, N25, N30, N36 |
| 1 (compliant, recorded) | 3 | N10, N13, N31 |

## 2. Domain glossary (`CONTEXT.md`)

The glossary's _Avoid_ lists were searched as whole words in web copy (`web/src/**/*.tsx`, `web/src/lib/*Copy.ts`), Rust string literals, `tracing` log lines, `ApiError` `detail` sentences, doc comments, and exported identifiers. `folder` appears nowhere in code or copy. No log line (`info!`/`warn!`/`error!`/`debug!`) uses an avoided word. No `ApiError` `detail` uses one.

### N1. "chat" in two Import Run summary lines (6/10)

Reader-facing copy in the run summary and the note an exporter attaches to a conversation.

- `crates/core/message-crate-core/src/counter.rs:154-158`: `NAME_ONLY_CHAT` is the counter whose lines read "Kept 1 chat under a name alone, with no phone number or email address" and "Kept {n} chats under a name alone, ...". Copy and identifier.
- `crates/core/message-crate-core/src/pipeline.rs:121-122`: `NAME_ONLY_CHAT_NOTE` is "This chat names its person with no phone number or email address, so the conversation is kept under the name alone." Copy; the sentence switches to "conversation" in its own second clause.
- `counter.rs:151` (doc): "Chats that name their person ...". Comment.

Fix:

```rust
// crates/core/message-crate-core/src/counter.rs
/// Conversations that name their person with no phone number or email
/// address, each kept under the name alone and sent as a
/// [`crate::NAME_ONLY_CONVERSATION_NOTE`].
pub const NAME_ONLY_CONVERSATION: Counter = Counter::new(
    "name_only_conversation",
    "Kept 1 conversation under a name alone, with no phone number or email address",
    "Kept {n} conversations under a name alone, with no phone number or email address",
);

// crates/core/message-crate-core/src/pipeline.rs
pub const NAME_ONLY_CONVERSATION_NOTE: &str = "This conversation names its person with no phone \
     number or email address, so it is kept under the name alone.";
```

The counter's key `name_only_chat` is written into run summaries; the repo's no-compatibility rule (`CLAUDE.md`, "No backwards compatibility, anywhere") allows renaming it.

### N2. "Thread" is the web app's name for the Conversation on screen (5/10)

`CONTEXT.md:46` avoids "Thread" for Conversation. In `web/src` (non-test) the word appears in 49 `thread` locals and these exported names:

- `web/src/screens/message/MessageThread.tsx` (the component that shows a conversation) and `web/src/screens/message/threadLayout.ts:33,52` (`ThreadRow`, `threadRows`). Identifiers and file names.
- `web/src/screens/message/useConversationMessages.ts:54` `threadQueryFor`; `web/src/components/AppLayout.tsx:249` `threadListQuery` (it is built by `conversationListQuery` on the same line); `web/src/components/contactDrawer/contactDrawerTypes.ts:79,92` `ThreadParticipantPreviewSource`, `contactPreviewFromThreadParticipants`. Identifiers.
- `web/src/test/bubbles.tsx:57` "as a thread does"; `MessageThread.tsx:11,14,18` "the thread". Comments.
- `crates/server/demo-seed/src/personas.rs:69` `EMPTY_THREAD_HANDLE`. Identifier.
- `CONTEXT.md:186`, the Time Zone entry: "in search and in the thread alike". The glossary uses its own avoided word (see N16).

The component is the conversation view, and the rest of the code calls the thing a conversation (`conversationListQuery`, `useConversationMessages`). Fix: `MessageThread` to `ConversationView`, `threadLayout.ts` to `conversationLayout.ts` (`ConversationRow`, `conversationRows`), `threadQueryFor` to `conversationQueryFor`, `threadListQuery` to `conversationListQuery` (the local), `ThreadParticipantPreviewSource` to `ConversationParticipantPreviewSource`, `EMPTY_THREAD_HANDLE` to `EMPTY_ONE_TO_ONE_HANDLE`. "Reply in a thread" for Apple's reply chains (`CONTEXT.md:98`) stays, because there the word is Apple's.

### N3. demo-seed calls Contact Groups "labels" (5/10)

`CONTEXT.md:19` avoids "Label" for Contact Group. In `crates/server/demo-seed/src/config.rs:64-65` the field is documented "Contact labels and the share of contacts that get each one." and typed `pub labels: LabelsConfig` (struct at `:104`); the TOML key is `[labels]` (`:386`) with `labels.names` (`:237-254`, whose error says "labels.names must have exactly 4 entries (family, work, college, inactive)"). `personas.rs:706` names a test `group_labels_come_from_config_names`. Identifiers, a config key, and a sentence.

Fix:

```rust
// crates/server/demo-seed/src/config.rs
/// Contact Groups and the share of contacts that join each one.
pub contact_groups: ContactGroupsConfig,
// ...
pub struct ContactGroupsConfig { pub names: Vec<String>, pub family: f64, pub work: f64, pub college: f64 }
```

with `[contact_groups]` in the embedded TOML. The generator's inputs are compiled into the server (`CLAUDE.md`), so no file outside the repo reads the key.

### N4. Four names for the one-to-one conversation (5/10)

`CONTEXT.md:40-46` fixes "one-to-one conversation" for what a person reads, `kind:direct` for the search word, and avoids "Direct conversation". The code carries two more:

| Layer | Name | Where |
|---|---|---|
| Conversation file | `Individual` | `crates/libs/ir/src/lib.rs:187-189` (`IrConversationType::Individual`, doc "One-on-one chat with a single peer.") |
| HTTP wire | `"individual"` | `crates/libs/api-types/src/lib.rs:620-623` (`conversation_type` doc: "`individual`, `group`, or `orphaned`") |
| Search word | `direct` | `crates/server/server/src/search/fields.rs:138`; `search/emit.rs:1057` maps `"direct" => IrConversationType::Individual` |
| Wire field | `direct_messages` | `crates/server/server/src/db/handles.rs:396-398` on `Identity` (struct at `:377`) |
| UI copy | "N direct messages" | `web/src/screens/settings/identities.ts:7,14` builds "12 direct messages and 30 group messages" |
| Doc comment | "Direct, group, and orphaned conversations" | `crates/server/server/src/db/handles.rs:393` |
| UI copy | "one-to-one" | everywhere else a person reads |

The search word is settled by the glossary. The fix is at the two reader-facing points and the two internal names:

```ts
// web/src/screens/settings/identities.ts:14
if (identity.direct_messages > 0) parts.push(count(identity.direct_messages, "one-to-one"));
```

```rust
// crates/server/server/src/db/handles.rs:393
/// One-to-one, group, and orphaned conversations: for a contact, ...
```

and, when the conversation file's schema next changes for another reason, `IrConversationType::OneToOne` serialised as `one-to-one` with the api-types doc following. `check_schema_version` refuses the old file, which is the agreed cost.

### N5. "chat" in the shared model's own documentation (3/10)

`crates/libs/ir/src/lib.rs:3`: "A [`ConversationDocument`] is the in-memory form of one chat". `:188` "One-on-one chat with a single peer." `:190` "Chat with multiple peers." `crates/libs/import/src/report.rs:76`: "why was this chat slow?". `crates/server/demo-seed/src/personas.rs:89` seeds a group named "Family Chat" (made-up data; acceptable, since a person named it). Comments. Fix: "conversation" in all four doc lines. The UI widget family `ChatBubbleRow`, `chatBubbleShared.tsx` (`web/src/components/messages/`) names a bubble style, not a Conversation; `MessageBubbleRow` would still remove the word.

### N6. "Blob" for attachment bytes (3/10)

`CONTEXT.md:111` avoids "Blob" for Asset. `crates/libs/sbr/src/read.rs:65` `struct AttachmentBlob` and `:460` `fn attachment_blob`; `crates/exporters/sms-backup-plus-exporter/src/types.rs:8` `struct AttachmentBlob` and `assets.rs:195` `fn attachment_blob` (the duplication is F8); `crates/server/server/src/assets_api.rs:289` `fn install_blob`; `crates/server/demo-seed/src/assets.rs:84,118` `write_attachment_blobs`, `record_blob`. Identifiers. In the server, where the thing is an Asset, `install_asset`; in the exporters, where it is a decoded attachment's bytes, `AttachmentBytes`.

### N7. "filter" as a noun in copy (2/10)

`web/src/screens/ContactList.tsx:478`: "No contacts match this filter". `CONTEXT.md:25` avoids "Filter" as a name for a Saved Search; here it names the list's narrowing. **Unable to verify** from the one line whether the message shows under a Saved Search or under the search bar's own words; `ContactList.tsx` around `:478` would say. Either way "No contacts match this search" fits both.

### N8. A wire field named `label` beside `tags` (3/10)

`crates/server/server/src/db/conversations.rs:92` and `crates/libs/api-types/src/lib.rs:639` carry `pub label: Option<String>`, documented at `conversations.rs:86-91` as "The title the conversation is shown by". The same struct has `tags: Vec<String>` (`:94`) and the message's conversation has `group_title` (`api-types:631`). A reader of the generated TypeScript meets `label` next to `tags` on a product whose glossary reserves "Label" for the thing a Message Tag is not. The API token's `label` (`accounts_api/api_tokens.rs:34`) is a different case: it is the token's own name and the glossary has no word for it. Fix: `shown_title` (or `display_title`), since the doc already says "shown by".

### N9. `Item` and `Row` endings on web types for wire things (3/10)

`http-api.md:868-871` forbids `Item`, `Row` and the like on wire types because "these names become the web app's type names". The server obeys; the web app adds them back:

- `web/src/screens/settings/apiTokensUtils.ts:22`: `export type ApiTokenItem = components["schemas"]["ApiToken"];` An alias that adds the forbidden ending to the wire type.
- `web/src/screens/settings/storage/storageUtils.ts:36`: `ExportRow`.
- `web/src/lib/messageRowText.ts`: the module and `messageRowText` name a Message's text "row text"; `CONTEXT.md:55` avoids "Row" for Message.

Fix: `type ApiTokenItem` is removable (use `ApiToken` from `types.ts`); `ExportRow` to `ExportRun`'s own name or `ExportSummary`; `messageRowText.ts` to `messageListText.ts`. UI layout rows (`IdentityRow`, `FactRow`, `ColorRow`, `PopupMenuItem`, `ChoiceItem`) name rows and items of a table or menu, which is what they are, and are fine.

## 3. HTTP API code rules (`docs/architecture/http-api.md`, "Code")

### N10. Compliant (1/10, recorded)

- **Route group modules.** Every `*_api` module is named for its first path segment: `accounts_api`, `assets_api`, `audit_trail_api`, `contacts_api`, `conversations_api`, `exports_api`, `imports_api`, `messages_api`, `named_set_api` (the sanctioned shared one), `phone_countries_api`, `saved_searches_api`, `search_fields_api`, `server_api`, `session_api`, `trash_api`. Nested collections are submodules (`accounts_api::api_tokens`, `assets_api::media_links`, `contacts_api::address_book`, `server_api::log_files`, `server_api::log_lines`).
- **Handlers.** Of the handlers registered in `crates/server/server/src/openapi.rs` (`routes!(...)` at `:117-250`), every name is `verb_noun` with one of the listed verbs or the action's own (`claim_server`, `trash_contact`, `restore_conversation`, `complete_import`, `discard_import`, `cancel_export`, `head_asset`). No `_handler` suffix exists; the only `_handler` in the crate is a test name, `crates/server/server/src/server/tests.rs:559`. The bare `list`/`create`/`get`/`update`/`delete`/`members_list`/`members_update` at `named_set_api.rs:81-188` are the shared bodies the macro at `:253-381` wraps into `list_contact_groups`, `update_message_tag_members` and so on; they are not handlers.
- **Wire types.** 66 `ToSchema` types; none ends in `Body`, `Payload`, `Input`, `Patch`, `Item`, `Info`, `Detail` or `Row`. The projections and action types follow the three forms (`ContactSummary`, `ImportRunSummary`, `OwnerImportRun`, `CreateApiTokenRequest`, `DeleteMessagesResponse`, `ListContactsQuery`). `ApiIdentityService` (`db/handles.rs:140`) is the one sanctioned `Api` prefix; `ApiToken*` names the API Token itself.
- **`db/` row types** `ApiTokenRow` (`db/api_tokens.rs:11`), `ImportRow`, `ImportNoteRow`, `ImportIssueRow`, `ImportIssueInput` (`db/imports.rs:155,276,287,261`), `KeyedRow` (`db/handles.rs:197`) carry the endings but are not on the wire, and the rule is written for wire types. Consistent as written. If the rule is meant to reach `db/`, it should say so; `Row` there is accurate.

## 4. Writing style (`docs/agents/writing-style.md`)

### N11. Em dashes join clauses in comments and in five reader-facing strings (5/10)

`writing-style.md:38-39`: "A compound sentence splits in two rather than joining clauses with a dash or a semicolon." Counts (non-test):

- Rust comment lines containing `—`: **228**. Samples: `crates/core/message-crate-core/src/transforms.rs:63`, `crates/libs/ir-format/src/format_sink.rs:133`, `crates/libs/staging/src/transcode.rs:625,650`, `crates/server/server/src/server.rs:524`, `crates/libs/media/src/process.rs:219`, `src-tauri/src/commands/export.rs:1`, `crates/libs/import/src/report.rs:76`, `crates/server/server/src/named_set_api.rs:217`.
- Rust string literals: **6**, of which 5 reach a reader: `crates/helpers/imessage-reader/src/error.rs:21` and `crates/exporters/whatsapp-exporter/src/ios_backup.rs:127,213` ("The backup is encrypted — fill Encryption password."), `crates/server/server/src/process_assets.rs:287` ("no accounts found — create an account or run reset-demo first"), `crates/server/server/src/cli.rs:499` ("ambiguous numbers — fix them in Message Crate"), and `crates/server/server/src/error_docs.rs:36`, which writes "`{status}` — `{slug}`" into each generated error page's description.
- `web/src` lines: **174**, of which 10 sit in JSX text or copy strings; 9 of those are the empty-cell placeholder `—` (`CheckedContactsPanel.tsx:121`, `IdentityTable.tsx:26,291`, `ImportRunView.tsx:59` and the like), a typographic use the rule does not cover. The remaining one is a comment (`LoginScreen.tsx:252`).

Fix for the five strings:

```rust
// crates/helpers/imessage-reader/src/error.rs:21 (and the two whatsapp sites)
"The backup is encrypted. Fill in Encryption password.";
// crates/server/server/src/process_assets.rs:287
bail!("no accounts found; create an account or run reset-demo first");
// crates/server/server/src/error_docs.rs:36
description: \"`{status}` `{slug}`\"
```

The 228 comment lines are a one-time `sed` over `—` to `,` or `:` with a read of each; the guide applies to "every file someone outside the conversation opens", and rustdoc is published.

### N12. Spelling: British standard with American leaks (3/10)

Whole-word counts over Rust and TypeScript, excluding the generated types: `cancelled` 379 / `canceled` 9 (all 9 are `cancelEdit` or a negative test of `"canceled"`); `labelled` 18 / `labeled` 12 (the 12 are one prop, `GroupsMenu.tsx:28,46`); `recognise*` 10 / `recognize*` 9; `behaviour` 5 / `behavior` 2; `honour*` 2 / `honor*` 2; `colour` 22 / `color` 72 (CSS, excluded). The American forms:

- `recognize*`: `crates/libs/phone/src/lib.rs:161`, `crates/libs/media/src/estimate.rs:125`, `crates/libs/media/src/mime.rs:10,197`, `crates/libs/media/src/process.rs:217`, `crates/libs/http/src/auth_error.rs:120`, `crates/server/server/src/db/contacts/read.rs:547`, `web/src/screens/settings/ApiTokenForms.tsx:157`.
- `behavior`: `crates/libs/ir/src/projection.rs:84`, `crates/libs/ir-format/src/util.rs:56`.
- `honoring`: `crates/server/server/src/dedupe.rs:894`.
- `labeled` (prop name): `web/src/components/GroupsMenu.tsx:28,46`, `TagsMenu.tsx:32`.

No document states the rule. Fix: one line in `writing-style.md` under "Fixed product vocabulary" ("British spelling: cancelled, labelled, recognise, behaviour, honour; library names such as `serialize` keep their own"), then the eleven sites above.

### N13. Commit subjects and register: compliant (1/10, recorded)

The 30 most recent subjects (`git log --format=%s -30`) are each one plain declarative sentence under a type and scope, with the PR number as the guide requires; none contains an em dash, "folder", "you" or a marketing opener. In 30,885 Rust doc-comment lines, "we" appears 0 times and "you" 6 times, all inside a quoted phrase ("did you mean", `search/fields.rs:303,319`; `http/session.rs:141`).

### N14. Doc-comment accuracy: one stale of twenty checked (3/10)

Twenty doc comments that name another file or symbol were checked against the tree. Nineteen are accurate: `serve-protocol/src/lib.rs:42` (`tests/exit_with_parent.rs` exists in the server crate), `staging_summary.rs:148` (`pending_in` at `transcode.rs:365`), `mail/headers.rs:3` (`build_eml` at `mail/lib.rs:744`, `parse.rs` exists), `ios-backup/testutil.rs:7` (`tests/helper_process.rs`), `export/part_file.rs:21` (`note_asset_refs` at `run.rs:682`), `media/process.rs:986` (`estimate.rs`), `named_set_api.rs:216-217` (`db/named_membership.rs`; `openapi.rs` has the 14 named-set route lines), `test_support.rs:8-9` (the three test files), `imports_api/with_yourself.rs:5` (`FileStaging` at `imports_api/staging.rs:402`), `logging/files.rs:6` (`logging/read.rs`), `lib.rs:4` (`build.rs`), `test_support/lines.rs:6` (`tests/common/mod.rs:20` has the `#[path]` include), `db/schema.rs:20` (`tests/schema_column_comments.rs`), `imessage-reader/main.rs:12,20,66` (`backup.rs`, `error.rs`, `domain.rs`, `tests/temporary_files.rs`), `src-tauri/src/lib.rs:14` ("Both declare the same modules": the eight `mod` lines match, differing only in `pub`), `src-tauri/src/commands/mod.rs:6` (`generate_handler!` is in `main.rs`), `tool_downloads/pins.rs:13` (`WTSEXPORTER_TROUBLESHOOTING` at `wtsexporter.rs:25`), `events.rs:5` (the payload structs do match `types.ts:89-164`, see N28 for the names), `queryKeys.ts:14-15` (`routeQueryKey.ts:30` does put the account in front).

The stale one is the one the brief named: `crates/server/server/src/test_support.rs:5-7` says "Distinct from `server.rs`'s `test_state()`, which returns a four-tuple". `test_state()` is at `crates/server/server/src/server/tests.rs:142`; `server.rs:1807` holds `test_app_state(pool, data_dir)`, a different function with a different signature. F10 already proposes moving `test_state()` into `test_support`; until then:

```rust
//! Distinct from `server/tests.rs`'s `test_state()`, which returns a four-tuple
```

Of 506 unique snake-case intradoc link targets (`` [`name`] ``), 504 resolve to a definition in the repo; the two that do not are `mms_parts` (a crate) and `spawn_blocking` (tokio). No stale links.

### N15. Product vocabulary in one doc comment (2/10)

`crates/libs/ir/src/lib.rs:362`: "UI labels: `Phone` → "Text message", `Whatsapp` → "WhatsApp"". The fixed name is "Text Message" (`writing-style.md:197`), and "labels" is the avoided word for what a screen shows. Fix: "On screen, `Phone` is "Text Message" and `Whatsapp` is "WhatsApp"."

### N16. The glossary uses its own avoided word (2/10)

`CONTEXT.md:186` (Time Zone): "in search and in the thread alike". Fix: "in search and in the conversation alike".

## 5. Rust conventions

### N17. The `Ir` prefix in `message-ir` has no rule (5/10)

In `crates/libs/ir/src/lib.rs`, nine public types carry the prefix: `IrConversationType` (`:187`), `IrParticipant` (`:294`), `IrService` (`:306`), `IrMessageKind` (`:421`), `IrMessage` (`:477`), `IrDirection` (`:539`), `IrAttachment` (`:615`), `IrSource` (`:646`), `IrImessage` (`:668`). Fourteen do not: `ConversationDocument` (`:148`), `ExportMeta` (`:164`), `IdentityType` (`:224`), `ConversationMeta` (`:259`), `ConversationStats` (`:275`), `IdentityService` (`:365`), `AttachmentMeta` (`:578`), `ConversationHeader` (`:1009`), `PendingMessage` (`:1050`), `PendingAttachment` (`:1087`), `PendingConversation` (`:1130`), `UnknownDeletion` (`:126`), plus `MessageIdentity`, `MessageGuid`, `MessageCopy` in `identity.rs`. The crate doc (`lib.rs:1-14`) states no rule.

The likely origin is a clash with `message-crate-api-types`, which has `Message`, `Participant`, `Attachment` (`api-types/src/lib.rs:437,414,647`). That explains three of the nine, not `IrDirection`, `IrSource`, `IrImessage`, `IrService`, `IrMessageKind`, `IrConversationType`. `CONTEXT.md:233-238` already records the one place a prefix is deliberate (`ApiIdentityService`). Fix, either way, written into `lib.rs`'s crate doc:

```rust
//! Types are named for what they are, with no prefix: the crate path tells
//! `message_ir::Message` from `message_crate_api_types::Message`, and a file
//! that needs both imports one of them `as`.
```

or, if the prefix stays, "every type that appears in the conversation file carries `Ir`", which would add it to `ConversationDocument`, `ExportMeta`, `ConversationMeta`, `ConversationStats`, `AttachmentMeta`, `IdentityType` and `IdentityService`. The second rule conflicts with `CONTEXT.md:234`, which names `IdentityType` and `IdentityService` without it, so the first is the one that fits.

### N18. `IrService` is a transport named a service (5/10)

`crates/libs/ir/src/lib.rs:303`: "Transport a message arrived on." introduces `enum IrService { Sms, Imessage, ... }` (`:306`), the type of `IrMessage.service` (`:492`). `lib.rs:360`: "Platform identity stored on `handles.service`" introduces `enum IdentityService { Phone, Whatsapp }` (`:365`). `CONTEXT.md:241-249` fixes the words: Text Message and WhatsApp are **services**; SMS, MMS, RCS and iMessage are the **transports** a message took. One of the two enums is named against the glossary, and it is the one whose doc comment says so. `writing-style.md:205-208` pins the on-disk key: "Each message's `service` value still records its transport."

Fix that changes nothing on disk:

```rust
/// The transport a message took: SMS, MMS (recorded as `sms`), RCS, iMessage,
/// or another service's own. Written to the file under the key `service`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IrTransport { Sms, #[serde(rename = "imessage")] Imessage, /* ... */ }

pub struct IrMessage { /* ... */ pub service: IrTransport, /* ... */ }
```

F12 covers what else is wrong with `message-ir`'s contents; this is only the name.

### N19. Crate names against directory names, and two prefixes (4/10)

Twelve of 36 packages are not named for their directory: `api-types` → `message-crate-api-types`, `csv` → `message-csv`, `export` → `message-crate-export`, `http` → `message-crate-http`, `import` → `message-crate-import`, `ir` → `message-ir`, `ir-format` → `message-ir-format`, `log-lines` → `message-crate-log-lines`, `reexport` → `message-reexport`, `serve-protocol` → `message-crate-serve-protocol`, `staging` → `message-staging`, `server` → `message-crate-server` (each `crates/**/Cargo.toml`, `name =`). Two prefix families exist, `message-` (ir, ir-format, csv, reexport, staging) and `message-crate-` (the rest), and 24 packages have no prefix (`phone`, `media`, `mail`, `journal`, `sbr`, `contacts`, ...). `cargo` is unaffected; a reader is: a `use message_staging::` line sends them looking for a `message-staging` directory.

Fix: one rule in `AGENTS.md` or `CLAUDE.md`: "A crate's package name is its directory name; a prefix is used only where crates.io would otherwise collide, and that prefix is `message-crate-`." Then rename `message-ir`, `message-ir-format`, `message-csv`, `message-reexport`, `message-staging` to the `message-crate-` family or drop the prefix. Twelve `Cargo.toml` `name` lines and the matching `use` paths; nothing else refers to a package name.

### N20. Exporter module names differ per crate (4/10)

F8 lists this in one bullet under copy-paste; the naming fix stands on its own. The step that reads the vendor's files is `xml.rs` (go-sms-pro), `parse.rs` (imazing, openextract, whatsapp), `read.rs` (sms-backup-restore), `parse_emit.rs` (imazing, sms-backup-plus) or `convert.rs` (imessage-ir); the step that writes IR is `emit.rs` in six crates and `convert.rs` in the seventh. The entry point is `ConvertExportArgs` + `convert_export` in five (`go-sms-pro/emit.rs:388,414`, `imazing/emit.rs:88,113`, `openextract/emit.rs:32,54`, `sms-backup-plus/emit.rs:414,449`, `sms-backup-restore/emit.rs:74,97`) and `ConvertRequest` + `convert_json` in whatsapp (`emit.rs:35,66`). Fix: `read.rs` (vendor format in), `emit.rs` (IR out), `run.rs` (the exporter's `run`), and `convert_export(ConvertExportArgs)` everywhere; whatsapp's `ConvertRequest` becomes `ConvertExportArgs`.

### N21. Shadowed names from `message-ir` (4/10)

`PendingConversation` is defined three times: `crates/libs/ir/src/lib.rs:1130` (public), `crates/exporters/imessage-ir-exporter/src/convert.rs:60` (private, different fields), `crates/exporters/sms-backup-restore-exporter/src/read.rs:262` (private), the last with its own `PendingMessage` (`:243`) and `PendingAttachment` (`:235`) beside `message_ir`'s (`lib.rs:1050,1087`). `pending_to_document` is `crates/libs/ir/src/projection.rs:239` (public) and `crates/exporters/imessage-ir-exporter/src/convert.rs:707` (private). Inside either exporter a reader cannot tell which type a signature means without the import list. Fix: name the locals for what differs (`ChatInProgress`, `SbrMessageDraft`), or use `message_ir`'s and delete the locals where the fields allow (F8's remedy).

### N22. Abbreviations (3/10)

Whole-word identifier counts (non-test Rust): `cfg` 951 (as a variable for a config, e.g. `crates/libs/import/src/run.rs:235-238`), `msg` 943, `tx` 338, `convo` 185, `ctx` 165, `req` 100, `idx` 90, `msgs` 69, `buf` 55, `ts` 38, `conv` 34, `pdu` 32, `wsp` 21, `sbr` 18, `num` 13. Judgement:

- `convo` (`crates/libs/ir/src/projection.rs:94,187,241`; `crates/core/message-crate-core/src/pipeline.rs:415-429`) abbreviates the product's central word; `conversation` costs seven letters.
- `ts` (`crates/core/message-crate-core/src/attachment_jobs.rs:278`, `crates/server/server/src/dedupe.rs:901`, `crates/exporters/whatsapp-exporter/src/parse.rs:293,302`) hides the unit; the field it comes from is `timestamp_unix_ms`. `timestamp_ms` / `rfc3339` say what the value is.
- `pdu`, `wsp`, `sbr`, `sbp` are the standards' and products' own names (WAP PDU, WSP, SMS Backup & Restore, SMS Backup+) and stay.
- `cfg`, `msg`, `tx`, `ctx`, `buf`, `idx` are Rust idiom and consistent; no change.

### N23. Boolean fields that read as nouns (3/10)

151 `pub ... : bool` fields (non-test). The house style names the state (`disabled`, `trashed`, `is_group`, `undated`) rather than prefixing `is_` everywhere, and request flags are imperatives (`resume`, `force`, `confirm`, `merge`), which read well on a request. The ones that read as nouns or adjectives of something else:

- `business: bool` (`crates/exporters/whatsapp-exporter/src/wtsexporter.rs:58`) and `whatsapp_business: bool` (`crates/core/message-crate-core/src/exporters.rs:215`): `is_business_app`.
- `gzip: bool` (`src-tauri/src/tool_downloads.rs:160`): `gzipped`.
- `advanced: bool` (`exporters.rs:177`): `shows_advanced`.
- `obfuscate: bool` (`crates/core/message-crate-core/src/transforms.rs:17`, `exporters.rs:173`): an imperative on a config is fine; keep.
- `unknown: bool` on a contact (`crates/server/server/src/contacts_api.rs:100`, `db/contacts/read.rs:27`): the wire name of the Unknown group membership; keep, it is the glossary's word.

### N24. The two long parameter lists (3/10)

- `crates/libs/staging/src/transcode.rs:652-662`: `apply_transcode(run_dir, jsonl, doc, recorded_rel, src, is_heal: bool, options, issues, report)`, nine parameters, with a bool whose doc (`:647-651`) takes five lines to explain. Fix: group the target and replace the bool with an enum.

```rust
/// One attachment the Media stage is deciding about.
struct TranscodeTarget<'a> { run_dir: &'a Path, jsonl: &'a Path, recorded_rel: &'a str, src: &'a Path, origin: Origin }
/// Whether `recorded_rel` names a file this run made, or a `-mv` name a
/// crashed earlier run wrote into the document without producing the file.
enum Origin { Fresh, CrashHeal }
fn apply_transcode(target: &TranscodeTarget<'_>, doc: &mut ConversationDocument, options: &TranscodeOptions, issues: Option<&IssueSink>, report: &mut TranscodeReport) -> Result<()>
```

- `crates/exporters/sms-backup-restore-exporter/src/write.rs:164-172`: `restore_mms(attrs, parts, addrs, doc, msg, owner, output_dir)`, seven parameters, three of them `BTreeMap`s and `Vec<BTreeMap>`s whose names are abbreviations. Fix: `struct OriginalMms { attributes, parts, addresses }` as the first parameter.

51 functions take one `bool` parameter; none takes two.

### N25. A lowercase constructor beside PascalCase variants (2/10)

`ApiError::validation("...")` (`crates/server/server/src/server.rs:591`) is an associated function that builds `ApiError::ValidationFailed`; at a call site it sits beside `ApiError::NotFound("...")` (a variant). A reader must open `server.rs` to learn which is which. Fix: name constructors as verbs the variants are not (`ApiError::refuse_field(sentence)`), or expose `ValidationFailed` with a `From<&str>`.

## 6. TypeScript conventions

### N26. Where a hook lives is not decided (4/10)

`use*` modules: 6 in `web/src/hooks/` (`useAssetObjectUrl`, `useMouseHistoryNavigation`, `useNearScreen`, `useStreamedMedia`, `useTauriJob`, `useThumbnail`), 23 in `web/src/lib/` (`useAccountProfile` through `useWindowWidth`), 5 in `web/src/components/` (`useColumnResize`, `useIdentityCountryPick`, `useOrphanedColumn`, `useRightToolbar`, `contactDrawer/useHandleMutations`), 8 beside their screen (`screens/import/useImportJob`, `screens/owner/useOwnerAccounts`, ...). No `use*` name is anything but a hook, which is right. Fix: write the rule in `CLAUDE.md`'s `web/` paragraph and move files once: "a hook one screen uses sits beside that screen; a hook two or more use sits in `hooks/`; `lib/` holds no hooks". That empties `lib/use*` into `hooks/` (the ADR-0002 wrappers `useAccountProfile`, `useContactGroups`, `useMessageTags` included).

### N27. File-name casing (3/10)

- `web/src/lib/`: `auth.tsx`, `ThemeProvider.tsx`, `TimeZoneProvider.tsx`, `highlightText.tsx` sit among camelCase `.ts` modules; two of the four are PascalCase components, two are camelCase files that export JSX.
- `web/src/components/`: 96 PascalCase `.tsx`, 5 lowercase `.tsx` (`chatBubbleShared.tsx`, `contactDrawer/handleTableHelpers.tsx`, `contactDrawer/handleTableLogic.tsx`, `icons.tsx`, `phoneCountryItems.tsx`), 21 camelCase `.ts`, 0 PascalCase `.ts`.
- `web/src/lib/system-settings.ts` and `web/src/lib/tauri-check.ts` are the only kebab-case files in `web/src`.

`web/biome.json` sets no `useFilenamingConvention`. Fix: `PascalCase.tsx` exports a component; `camelCase.ts` is a module; a `.tsx` that only exports helpers is still camelCase (so `icons.tsx` is fine, `ThemeProvider.tsx` is fine, `auth.tsx` should be `AuthProvider.tsx` if it exports the provider); rename `system-settings.ts` to `systemSettings.ts` and `tauri-check.ts` to `tauriCheck.ts`; then

```json
"style": { "useFilenamingConvention": { "level": "error", "options": { "filenameCases": ["camelCase", "PascalCase"] } } }
```

### N28. Two names for the desktop events, and a channel named for one stage (4/10)

- Rust payloads: `ExtractProgressEvent`, `ExtractIssueEvent`, `ExtractFileDoneEvent`, `ExtractFileWrittenEvent`, `ExtractErrorEvent` (`src-tauri/src/commands/events.rs:82,158,192,206,228`). TypeScript twins: `ImportProgressEvent`, `ImportIssueEvent`, `ImportFileDoneEvent`, `ImportFileWrittenEvent`, `ExtractErrorEvent` (`web/src/lib/types.ts:100,118,147,164,89`). Four of five differ by prefix across one serialisation boundary; `events.rs:5` says they "match", which is true of the fields and not of the names.
- Channel names: `extract:log`, `extract:progress`, `extract:issue`, `extract:file-done`, `extract:file-written`, `extract:finished`, `extract:error` (`events.rs:16-31`; listened for at `web/src/lib/tauri.ts:646-653`). `events.rs:22-24` documents `extract:file-done` as "The Upload finished with one conversation file", so the `extract:` prefix names the Staging stage on an Upload-stage event. `CONTEXT.md:568-569` keeps "extract" alive only as "the internal name of the desktop command that reads a backup".

F6 covers the three Rust `ProgressEvent` types; this is the wire names. Fix: one prefix for the job's whole life and one set of names on both sides:

```rust
pub const LOG: &str = "import-run:log";          // and progress, issue, file-done, file-written, finished, error
pub struct ImportProgressEvent { /* ... */ }      // ImportIssueEvent, ImportFileDoneEvent, ImportFileWrittenEvent, ImportErrorEvent
```

`docs/src/content/docs/docs/developer/rustdoc-style.md:23-24` quotes the `extract:` names as an example and would follow.

### N29. "Tauri job" and "desktop job" for one thing (3/10)

`awaitTauriJob` (42 uses), `TauriJobResult` (22), `parseTauriJobResult` (13), `TauriJobFormShell` (9), `hooks/useTauriJob.ts` against `currentDesktopJob` (24), `holdDesktopJob` (18), `desktopJob` (17), `DesktopJobName` (13), `useDesktopJob` (9), `lib/desktopJob.ts`. `useTauriJob.ts:2-3` imports from both. The product word is "desktop" (`desktopFeatures.ts`, `CLAUDE.md` "Desktop app"); Tauri is the framework. Fix: `awaitDesktopJob`, `DesktopJobResult`, `parseDesktopJobResult`, `DesktopJobFormShell`, `useDesktopJob` (merging with the existing one if they overlap; **Unable to verify** without reading both hooks' bodies).

### N30. Tailwind class-string constants under three suffixes (2/10)

`*Class` (7: `thClass`, `tdClass`, `mutedClass`, `selectTriggerClass`, `contactStackClass`, `dataCardHeaderRowClass`, `labelClass`), `*Style` (2: `hintStyle` `web/src/screens/import/ImportFormUi.tsx:19`, `tdStyle` `web/src/screens/settings/storage/storageUtils.ts:15`), none (5: `thSeparator` `ownerTableStyles.ts:10`, `sectionTitle`, `sectionHint`, `tableCard` `storageUtils.ts:10-12`, `tdMuted` `apiTokensUtils.ts:26`, `sectionGap` `ImportFormUi.tsx:21`). `thClass` is defined twice (`ownerTableStyles.ts:7`, `apiTokensUtils.ts:24`; F15). Fix: `*Class` for every string of utility classes; `Style` is the word React reserves for inline styles.

### N31. Compliant (1/10, recorded)

- **Query keys.** `web/src/lib/queryKeys.ts` holds every key under one documented rule (namespace `all` prefix, builders nested, account prefixed by `routeQueryKey.ts:30`); `queryKeys.test.ts` exists. The header's claims were checked (N14).
- **Tests.** 1,924 web test names, 0 begin "should", 13 begin with an article (a noun-phrase form, e.g. "is the participant's name for their identity, else the identity", which still reads as a sentence with `it`). 3,163 Rust tests, 0 named `test_*`, 1 contains "should"; the 30 containing "case" all mean letter case. Names are sentences about behaviour in both languages.
- **Copy modules.** `namedSetCopy.ts`, `toolStatusCopy.ts`, `attachmentProgressCopy.ts`, `attachmentStepCopy.ts` share the `*Copy.ts` suffix and the one-place-for-the-words doc (`namedSetCopy.ts:4-7`). Three more suffixes hold copy too: `contactLabel.ts`, `serviceLabel.ts`, `missingAttachmentLabel.ts`, `deletionMarkText.ts`, `messageRowText.ts`, `unknownGroup.ts` (`UNKNOWN_GROUP_LABEL`). A one-line rule ("a module that exists to hold words a screen shows ends in `Copy`") would settle future ones; the existing six are fine to leave.

### N32. `tauri.ts` is not the one wrapper `CLAUDE.md` says it is (3/10)

`CLAUDE.md` ("`web/src/lib/tauri.ts` wraps desktop-only commands"). 29 of 35 exports in `tauri.ts` are `invoke*`; the six that are not are derived helpers (`toolsDownloading`, `ffmpegMissing`, `toolUsable`, `onExtractEvents`, `awaitTauriJob`, `parseTauriJobResult`), which is consistent. But 6 of the 34 Rust `#[tauri::command]` functions are invoked from other files: `start_local_server`, `set_open_to_network`, `local_server_status`, `open_data_directory` (`web/src/lib/localServer.ts:37,66,71,76`), `open_path` (`web/src/lib/openPath.ts:16`), `save_file` (`web/src/lib/saveFile.ts:21`). The 34 command names match on both sides exactly (snake_case strings). Fix: either move the six into `tauri.ts` as `invokeStartLocalServer` and so on, or change the `CLAUDE.md` sentence to "wraps the Import, Export, Convert and tool commands; `localServer.ts`, `openPath.ts` and `saveFile.ts` wrap their own".

## 7. Readability

### N33. `ApiError` `detail` sentences have no written register (4/10)

68 literal `detail` strings at `ApiError::...("...")` call sites (non-test): 61 start lowercase, 7 uppercase, 2 end with a period (`crates/server/server/src/accounts_api.rs:1076,1084` "New passwords do not match."), the rest do not. One condition has two sentences: "the request body is too large" (`server.rs:1001,1060`) and "request body too large" (`server.rs:1721,1738,1756,1787`). `problem.rs:204-236` writes the problem-type descriptions as full paragraphs with capitals and periods, a different register from `detail`, which is right for a page and wrong to mix into `detail`. `http-api.md` "Failures" (`:422`) does not say what a `detail` looks like. Fix: one sentence in `http-api.md` ("`detail` is one lowercase clause without a final period, naming the thing and what is wrong with it: `asset not found`") and one const per repeated condition:

```rust
const REQUEST_BODY_TOO_LARGE: &str = "the request body is too large";
```

### N34. Public items without a doc comment (4/10)

Counted with an `awk` pass that skips multi-line `#[...]` attribute blocks (a naive pass over-counts the handlers, whose `///` sits above a `#[utoipa::path]` block) and ignores `pub mod` lines (a module's `//!` satisfies `missing_docs`). Non-test, non-`tests.rs`, non-`test_support.rs`:

| Crate | Undocumented pub items | The ones that matter |
|---|---|---|
| `crates/server/server` | 15 | `db/handles.rs:140 ApiIdentityService` and `db/account_profile.rs:29 AccountPhone` (both `ToSchema`: utoipa puts the doc into the OpenAPI reference), `messages_api.rs:31 ListMessagesResponse`, `search/lex.rs:12,35 TokenKind, Token`, `credentials.rs:23 AUTH_RATE_MAX`; 7 are the macro-made handlers at `named_set_api.rs:253-381`, 2 are macro struct heads (`problem.rs:273`, `server.rs:326`) |
| `crates/server/demo-seed` | 15 | `config.rs:91,104,113,129,154,178` (six config structs), `personas.rs:20,27,42,49,67-70`, `assets.rs:19` |
| `crates/exporters/whatsapp-exporter` | 4 | `parse.rs:14,96 ChatJson, MessageJson`; `wtsexporter.rs:29,45` |
| `crates/libs/api-types` | 3 | `lib.rs:81 ImportMode`, `lib.rs:146 RunIssueKind` (both on the wire), `:241` macro head |
| 8 others | 1 to 2 each | `obfuscate/names.rs:3,25`, `imessage-reader/body.rs:21`, `data_source.rs:47`, `ir-format/export_transforms.rs:241`, `media/tools.rs:504`, `sms-backup-plus/types.rs:8,18`, `openextract/parse.rs:9,25`, `imazing/parse.rs:28,34`, `go-sms-pro/xml.rs:14,43` |
| 22 others | 0 | |

`missing_docs = "warn"` (`Cargo.toml:67`, `src-tauri/Cargo.toml:82`) reaches only items exported from the crate root, so `pub(crate)` items and the server's private-module `pub` items never warn. **Unable to verify** which of the listed items the lint reports without `cargo doc`/`cargo clippy`, which this run did not execute. The four wire types are the ones to document first.

### N35. Numeric literals outside a named constant (3/10)

Production code (checked against the file's `#[cfg(test)]` boundary):

- `crates/server/server/src/accounts_api.rs:1252`: `top_attachments_by_size(&mut conn, target, 100)`. The storage screen's list length. `const TOP_ATTACHMENTS: usize = 100;`
- `crates/libs/export/src/http.rs:189`: `.timeout(Duration::from_secs(300))` on an asset GET. `const ASSET_READ_TIMEOUT: Duration`.
- `crates/server/server/src/search/value.rs:155-158`: `* 7`, `* 31`, `* 365` turn `w`/`m`/`y` into days; 31 for a month is a policy choice worth a name and a doc line (`DAYS_PER_MONTH_LOOKBACK`).
- `crates/libs/phone/src/lib.rs:700`: `if id.len() > 180`, documented in the function's doc (`:681`) and repeated in `crates/libs/sbr/src/read.rs` (F9).
- `web/src/components/ThemeSettings.tsx:208`: `setTimeout(..., 1500)` for the "Copied" flash. `1024` appears in two byte formatters (`web/src/screens/settings/storage/storageUtils.ts:120`, `web/src/lib/attachmentProgressCopy.ts:9`), a duplication F15 covers.

Nesting is shallow throughout: 2 Rust files have a line indented 40 spaces or more (`crates/server/server/src/openapi.rs`, 3 lines; `db/conversations.rs`, 1), and the deepest TypeScript is JSX in `web/src/screens/TrashScreen.tsx:393-396` (23 lines at 28 spaces or more). No finding.

### N36. Comment quality: explains why; density is high but earns it (2/10)

30,885 doc lines in Rust. The 50-line sample (every 400th `///`) and the 20 spot-checks (N14) found comments that state the reason, the rule, or the failure mode (`queryKeys.ts:4-16`, `transcode.rs:647-651`, `search/bridge.rs:223`, `db/imports.rs:1214`) rather than restating the code. The one habit to drop is the em dash (N11). Vendor-format readers carry the vendor's words on purpose and say so (`imessage-reader/src/fields.rs:449 parse_thread_part` for Apple's thread reply; `whatsapp-exporter/src/parse.rs:11 ChatStoreFile` for `ChatStorage.sqlite`).

## 8. Naming convention guide

What the code does today where it is consistent, plus the rules the findings propose, so a reader has one list.

**Words.** `CONTEXT.md` is the dictionary for anything a person reads and for any identifier that names a product thing: Conversation (never thread, chat), one-to-one conversation and group conversation (never direct), Contact Group and Message Tag (never label, group alone), Asset (never blob), Import Run and Export Run (never job, session), Message (never item, row, post, text). A vendor's own word stays inside the reader for that vendor's format and is said to be the vendor's in the doc comment. A UI widget may be a row, an item or a bubble, because that is what it is.

**Rust.**
- A package is named for its directory; a prefix exists only to avoid a crates.io collision and is `message-crate-` (N19).
- In `message-ir`, no `Ir` prefix; the crate path disambiguates (N17). If the prefix stays, every type in the file carries it.
- An exporter has `read.rs`, `emit.rs` and `run.rs`, and its entry point is `convert_export(ConvertExportArgs)` (N20).
- A local type never reuses a `message_ir` name (N21).
- A bool field names the state (`disabled`, `trashed`, `undated`) or, on a request, the imperative (`force`, `merge`); nouns and adjectives of another thing get a predicate form (`is_business_app`, `gzipped`) (N23).
- Beyond five parameters, group the ones that travel together into a struct; a bool that means one of two origins is an enum (N24).
- `convo` is `conversation`; `ts` says its unit; the standards' names (`pdu`, `wsp`, `sbr`) stay (N22).
- Constants are `UPPER_SNAKE` (0 exceptions found); a literal that encodes a policy (a list length, a timeout, a lookback) is a named const (N35).
- A `detail` is one lowercase clause without a final period, and a repeated one is a const (N33).
- Every `ToSchema` type and every `pub` item has a `///` (N34).
- British spelling in prose and identifiers; library names keep theirs (N12).
- No em dash; a compound sentence is two sentences (N11).

**HTTP wire.** As `http-api.md` "Code" writes it, and as the server follows it (N10). The web app does not add `Item`/`Row` back on an alias (N9).

**TypeScript.**
- `PascalCase.tsx` exports a component; `camelCase.ts` is a module; a `.tsx` of helpers is camelCase; no kebab-case (N27).
- `use*` is a hook and nothing else is; a hook one screen uses lives beside the screen, one that several use lives in `hooks/`, none in `lib/` (N26).
- Desktop command wrappers are `invoke*` and live in `tauri.ts`, or `CLAUDE.md` names the other wrapper files (N32).
- The desktop boundary uses one name on both sides: payload types identical, channel prefix named for the whole job, not one stage (N28); the job is a "desktop job" (N29).
- A string of utility classes ends in `Class` (N30); a module that exists to hold screen words ends in `Copy` (N31).
- Query keys are built in `queryKeys.ts` only (already the rule, ADR 0002).
- Tests are sentences; no "should", no `test_` (already the practice, both languages).

## 9. What is done well

- The `http-api.md` "Code" section is followed to the letter across 66 wire types and every routed handler (N10).
- Test names in both languages are sentences about behaviour, 5,087 of them, with no `test_` or "should" (N31).
- 504 of 506 intradoc links resolve; 19 of 20 cross-file doc claims are true (N14).
- Query keys, copy modules and the `invoke*` wrappers each have one home and one documented rule.
- Commit subjects and doc comments keep the third-person register without exception in the sample (N13).
- Constants are uniformly `UPPER_SNAKE` in Rust (0 exceptions) and 96 of 119 primitive exports in TypeScript.
- Nesting is shallow everywhere (N35).

## 10. Unable to verify

- Which of the undocumented items in N34 `missing_docs` actually reports: needs `cargo doc` or `cargo clippy` on the workspace and `src-tauri`, which this run did not execute.
- Whether `ContactList.tsx:478` shows under a Saved Search or the search bar (N7): `web/src/screens/ContactList.tsx` around line 478.
- Whether `useTauriJob` (`web/src/hooks/useTauriJob.ts`) and `useDesktopJob` (`web/src/lib/desktopJob.ts`) overlap enough to merge (N29): both bodies.
- Whether anything outside this repo reads the `name_only_chat` counter key (N1) or the `extract:` channel names (N28): the repo's own docs reference the channel names only as an example (`docs/src/content/docs/docs/developer/rustdoc-style.md:23-24`); the no-compatibility rule makes the question moot for clients.
- The 228 em-dash comment lines were counted, not each read; a few may be a range or a quoted title rather than a joined clause (N11).

## 11. Cross-references to the design audit

| This audit | Design audit |
|---|---|
| N6 duplicate `AttachmentBlob`, N20 exporter module names, N21 shadowed `Pending*` | F8 |
| N17, N18 `message-ir` naming | F12 |
| N14 stale `test_support.rs:5` | F10 |
| N28 event names | F6 |
| N35 `180` in `phone` and `sbr` | F9 |
| N30 duplicate `thClass`, N35 two byte formatters | F15 |
