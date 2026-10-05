---
title: Common message
description: Shared ConversationDocument schema, JSON and JSONL layout, and format projectors used by every exporter.
---

**Common message** is the shared per-conversation structure after source parse and before packaging (CSV / EML / MBOX / JSON / JSONL / XML). End-user overview: [Export structure](/docs/developer/reference/export-structure/).

Four crates:

| Package | Path | Owns |
|---------|------|------|
| **`message-ir`** | [`crates/libs/ir/`](https://github.com/messagecrate/message-crate/tree/main/crates/libs/ir) | Schema types only (`ConversationDocument`, `Ir*` bags, helpers) and the attachment path check |
| **`message-ir-format`** | [`crates/libs/ir-format/`](https://github.com/messagecrate/message-crate/tree/main/crates/libs/ir-format) | `FormatSink`, readers/writers for JSON, JSON Lines, CSV, EML and MBOX, transforms, `CSV_HEADERS` |
| **`message-staging`** | [`crates/libs/staging/`](https://github.com/messagecrate/message-crate/tree/main/crates/libs/staging) | The resumable write path: `ExportWriter`, the write queue, the transcode pass, the staging summary |
| **`message-reexport`** | [`crates/libs/reexport/`](https://github.com/messagecrate/message-crate/tree/main/crates/libs/reexport) | Directory convert |

The run model (`ExporterConfig`, `ExportReport`, `ExportTransforms`, `run_pipeline`) is `message-crate-core`, below all four. Why the split: [ADR 0012](https://github.com/messagecrate/message-crate/blob/main/docs/adr/0012-four-crates-in-the-export-pipeline.md).

On-disk forms:

- **JSON** — one pretty-printed `<conversation-stem>.json` per chat
- **JSONL** (the desktop app's default) — one `<conversation-stem>.jsonl` per chat: header line, then one `IrMessage` per line

Stem rules match CSV filenames. Packaging-only suffixes (e.g. `__whatsapp`) affect the on-disk stem but are **not** serialized in the JSON body. When two conversations in one run reduce to the same stem, ignoring case (two groups with one title, or two untitled groups with the same people), each gets `__` and the first 8 hex digits of the SHA-256 of its `chat_identifier` appended, so neither file replaces the other. Two conversations with the same `chat_identifier` and stem stop the run instead.
Reading a file back gives the document whatever follows the stem it would have without a suffix, when that starts with `__`, so a converted file keeps its name. The readers know no suffix by name: the exporter that adds one owns it.

Pipeline: `backup → common message → FormatSink → user-picked format`.

## Status

- **Common-message path** (`ConversationDocument` → `message_ir_format::FormatSink`, one of json/jsonl/csv/eml/mbox/xml): all exporters, including iMessage (`imessage-ir-exporter`). Per-chat formats also accept `write_format`; XML uses a single `smses.xml` via the sink.
- **Media + obfuscate** run inside `FormatSink::finish` for every format (`message_crate_core::ExportTransforms`: none / copy / convert / compress, plus optional obfuscate). When obfuscate is on, exporters skip staging real attachment bytes and convert/compress is not run — only placeholder files are written. Exporters pass transforms from `ExporterConfig.media` / `.obfuscate`; there is no CSV-only post-step. EML / MBOX / XML embed media and drop the staged `attachments/` directory afterward.
- **Schema version 9 only** (breaking). Version 9 gives orphaned messages conversations of type `orphaned` (see [Orphaned messages](#orphaned-messages)), where version 8 put them all in one `individual` conversation named `orphaned`. Version 8 had kept an edited message's earlier versions in its own `edits`, for every source, where version 7 kept the Apple Messages edit history as a JSON value in `imessage.edits`. Version 7 had moved a message's mark, Deleted in the source app or Unsent, in its own `deletion`, for every source, where version 6 kept the Apple Messages deleted mark in `imessage.is_deleted`. Version 6 had moved a message's reactions into its own `reactions` list, one shape for every source, where version 5 kept Apple Messages reactions as a JSON value in `imessage.tapbacks`. Version 5 had named every address an identity (`identity`, `identity_type`, `owner_identity`, `sender_identity`, `reactor_identity`) where version 4 said `handle`. Version 8 and older are refused, never upgraded. Typed enums/bags, filled outgoing identity, conversation stats, stable null/`[]` keys. Older common-message JSON is not read — regenerate exports after schema changes.

## Document schema (`schema_version: 9`)

```json
{
  "schema_version": 9,
  "export": {
    "source": "sms-backup-restore",
    "tool": "SMS Backup & Restore",
    "tool_version": "10.26.003",
    "owner_identity": "+15555550100",
    "owner_display_name": "Me"
  },
  "conversation": {
    "chat_identifier": "+15555550101",
    "conversation_type": "individual",
    "group_title": null,
    "participants": [
      { "identity": "+15555550101", "display_name": "Sam", "identity_type": "phone" }
    ],
    "stats": {
      "message_count": 1,
      "attachment_count": 0,
      "first_timestamp_unix_ms": 1400773261000,
      "last_timestamp_unix_ms": 1400773261000
    }
  },
  "messages": [
    {
      "guid": "…",
      "timestamp_unix_ms": 1400773261000,
      "direction": "outgoing",
      "service": "sms",
      "message_kind": "sms",
      "sender_identity": "+15555550100",
      "sender_display_name": "Me",
      "subject": null,
      "text": "Hello",
      "attachments": [],
      "imessage": null,
      "source": {
        "android_type": 2,
        "fields": { "kind": "sms", "attrs": { "address": "+15555550101" } }
      }
    }
  ]
}
```

| Tier | Contents |
|------|----------|
| Core | typed conversation + message fields |
| `imessage` | typed Apple extensions (`IrImessage`); nested `parts` / `app` are JSON values |
| `source` | `android_type` (`i32` or null) + vendor `fields` object |

### Identity

- Outgoing rows set `sender_identity` / `sender_display_name` from `export.owner_*` (display defaults to `"Me"` when an identity is known).
- Incoming rows use the peer identity.
- `owner_identity` on a message is the owner's own address on it: the one it was sent from, or the one it was received at. Only sources that record the owner per message write it (iMessage, from `destination_caller_id`); everywhere else it is omitted and `export.owner_identity` stands for every message. An iMessage outgoing row keeps the address it was sent from, and takes `export.owner_identity` only when the database recorded none.
- Display names are not duplicated under `source`.
- `guid` is Apple's own id for Apple Messages. Every other source's `guid` is a `MessageGuid`: SHA-256 of the chat id, the direction, the sender of an incoming message, the UTC instant in milliseconds, the text with whitespace collapsed, the sorted attachment digests, and the source's own key where it has one (WhatsApp's `key_id`). It reads no time zone and no display format, so one backup gives the same ids on any computer. The server refuses a message whose `guid` is empty.
- Two records a backup cannot tell apart are one message, and the exporter keeps one (`message_ir::one_copy_per_message`). The server's content key, which matches one message across sources, is the same identity at whole seconds.

### Reactions

`reactions` is the list of reactions that stand on a message, the same shape for every source. Each one is a `Reaction`:

| Field | Meaning |
|-------|---------|
| `part_index` | The part of the message reacted to; `0` for the first or only part |
| `kind` | `loved`, `liked`, `disliked`, `laughed`, `emphasized`, `questioned`, `sticker`, or `emoji` |
| `emoji` | For an `emoji` reaction, the emoji; left out otherwise |
| `is_from_me` | `true` when the owner reacted |
| `reactor_identity` | The identity of the person who reacted, when someone other than the owner did and the source names them; left out otherwise |
| `reactor_display_name` | The name of the person who reacted, when the source knows one; left out otherwise |

Each reaction names its own reactor, who is rarely the author of the message, and the server stores it under that person. Adds and removes are resolved before the list is written, so a reaction has no action and a removed one is not in the list. A message with no reactions leaves `reactions` out of the file.

`Reaction` is defined once, in `imessage-reader-protocol`, and `message_ir::Reaction` is that type. It lives there because the Apple Messages Reader is GPL and runs as its own process, and the protocol crate (MIT OR Apache-2.0) is the one crate both it and the rest of Message Crate may link ([ADR 0014](https://github.com/messagecrate/message-crate/blob/main/docs/adr/0014-gpl-code-only-behind-a-process-boundary.md)). Apple Messages fills it. Every other source writes none yet.

Apple Messages also writes each reaction as a row of its own (`message_kind` `tapback` or `sticker_tapback`), with `imessage.associated_guid`, `tapback_kind` and `tapback_action`. The server reads reactions from `reactions` and skips those rows.

### Deleted in the source app and Unsent

`deletion` says why a message's content is gone in the app it came from:

| Value | Meaning |
|-------|---------|
| `deleted_in_source_app` | The person deleted the message in the source app before the backup was made, and the backup still holds it. `text` is whatever text the backup kept. |
| `unsent` | The sender took the message back for everyone: every part of it was unsent, or some part was and no part has text or an attachment left, so `text` is empty. |

A message with neither leaves `deletion` out of the file. A message only partly unsent, with text or an attachment left in another part, carries no mark. A marked message is imported, listed and searched like any other; the search word `deleted:` narrows to or away from marked messages.

`Deletion` is defined in `imessage-reader-protocol` beside `Reaction`, for the same reason, and `message_ir::Deletion` is that type. Apple Messages fills it: a message in a chat's recently deleted list is `deleted_in_source_app`, and a message whose every part was unsent is `unsent` rather than an announcement that someone unsent it. Every other source writes none yet.

### Earlier versions

`edits` holds the earlier versions of an edited message, oldest first within each part. `text` is always the final version, so the list holds only the versions before it. Each one is an `EarlierVersion`:

| Field | Meaning |
|-------|---------|
| `part_index` | The part of the message the version belongs to; `0` for the first or only part |
| `text` | The part's text in this version |
| `edited_at_unix_ms` | When this version was written, in milliseconds since 1970: the send time for the original, the time of the edit that wrote it for a later one. Left out when the source does not record it |

A message never edited leaves `edits` out of the file. The server stores each version and its time, and search finds the message by any of them as well as by its final text.

`EarlierVersion` is defined in `imessage-reader-protocol` beside `Reaction`, for the same reason, and `message_ir::EarlierVersion` is that type. Apple Messages fills it from each part's edit history: every entry but the last, which is the text the message holds now. An unsent part has no history and so no earlier version; `deletion` says what was unsent. Every other source writes none yet.

### Orphaned messages

A backup can hold a message without recording which conversation it was said in. Such messages sit in conversations of type `orphaned`, which are neither one-to-one nor a group. The ones one person sent sit in a conversation keyed `orphaned:` and that person's address, with that person as its only participant, apart from their one-to-one conversation. The ones the account holder sent record no recipient, and sit together in one conversation keyed `orphaned:` alone, with no participants. `message_ir::orphaned_chat_id` makes both keys; it is defined in `imessage-reader-protocol` beside `Reaction`, because the Apple Messages Reader writes these conversations.

Apple Messages writes them for messages in no chat. OpenExtract writes the account holder's: a sent row of the all-conversations CSV with no `Conversation` value, and every row of a per-chat file in which only the holder wrote. OpenExtract cannot tell such rows apart, so each carries a vendor key: a per-chat file's own key, or, in the all-conversations CSV, how many rows alike in every column came before it. The same text sent in the same second to several people is then several messages, never one copy kept as a duplicate, and the same rows give the same ids on every export.

### Attachments

Attachment **bytes** are never stored in JSON/JSONL (`#[serde(skip)]`). Paths + digests point at sidecar files under `attachments/`. For EML / MBOX / XML, FormatSink loads those files, embeds the bytes, then removes the staged `attachments/` directory so the output directory is the archive product.

### Vocabulary (enums)

- `conversation_type`: `individual` \| `group` \| `orphaned`
- `service`: `sms` \| `imessage` \| `whatsapp` \| `rcs` \| `discord` \| `signal` \| `telegram` \| `slack` \| `unknown`
- `message_kind`: `sms` \| `mms` \| `imessage` \| `tapback` \| `sticker_tapback` \| `announcement` \| `location_share` \| `balloon` \| `unknown`

### Serialization rules

- Optional strings / bags serialize as `null` when absent (stable keys).
- Empty `participants` / `attachments` serialize as `[]`. An empty `reactions` list, an empty `edits` list and a `deletion` of neither are left out.
- Packaging stem suffix is not part of the document (internal `packaging_stem_suffix` only).

### Conversation stats

`conversation.stats` is computed from messages at write time (`message_count`, `attachment_count`, first/last `timestamp_unix_ms`).

## JSONL layout

```text
{"schema_version":9,"export":{…},"conversation":{…}}
{"guid":"…","timestamp_unix_ms":…, …}
…
```

Line 1 is the header (includes `conversation.stats`; no `messages` array). Each following line is one `IrMessage`.

## Projectors

| Format | Writer | Reader |
|--------|--------|--------|
| JSON | pretty-printed `ConversationDocument` | `read_conversation_json` |
| JSONL | header + one message per line | `read_conversation_jsonl` |
| CSV | unified [`CSV_HEADERS`](https://github.com/messagecrate/message-crate/blob/main/crates/libs/ir-format/src/write.rs) (header from first data row on read) | `read_conversation_csv` |
| EML / MBOX | common message → `MailMessage` → [`mail`](https://github.com/messagecrate/message-crate/tree/main/crates/libs/mail) | `read_conversation_eml_dir` / `read_conversation_mbox` |
| XML | single `smses.xml` via [`sms_backup_restore_exporter::SbrArchive`](https://github.com/messagecrate/message-crate/tree/main/crates/exporters/sms-backup-restore-exporter) handed to `FormatSink::with_archive` | `sms_backup_restore_exporter::read_backup` (owner inferred when omitted) |
| SMS Backup+ | one directory of `X-smssync-*` mail per conversation, SMS and MMS only, via [`sms_backup_plus_exporter::SmsBackupPlusArchive`](https://github.com/messagecrate/message-crate/tree/main/crates/exporters/sms-backup-plus-exporter) handed to `FormatSink::with_archive` | `sms_backup_plus_exporter::run` (the SMS Backup+ import) |

**Directory convert:** [`message-reexport`](https://github.com/messagecrate/message-crate/tree/main/crates/libs/reexport) auto-detects one format in an export directory and writes another via `FormatSink`. Export calls it for any format other than JSON Lines.

**XML packaging differs:** one SyncTech backup for the whole export (not per conversation). iMessage-only fields are dropped. See [SMS Backup & Restore XML output](/docs/developer/formats/sms-backup-restore-xml/).

## Content round-trip

Library APIs support content-preserving cycles:

`ConversationDocument` → CSV \| EML \| MBOX \| JSON \| JSONL → `ConversationDocument`

[`message-reexport`](/docs/developer/formats/convert/) converts a whole export directory between formats.

XML is **lossy** for non-Android common messages (Apple bags omitted). SBR-origin `source.fields` can restore many SyncTech attrs on write-back.

After `normalize_document_for_compare`:

- Recomputes `conversation.stats`
- Collapses empty `source` / `imessage` to `null`
- Clears packaging stem suffix and attachment bytes (not part of JSON content)

**Preserved:** messages, attachment metadata (path/digest/mime when present in the format), `source`, `imessage`, export + conversation identity.

**Not required:** filename stem / pretty-print identity, packaging suffix in the JSON body, embedding attachment bytes in JSON. EML `X-ME-Attachment-Meta` currently omits on-disk `path` (bytes may still round-trip in memory for re-export).

CSV nested bags use empty string when absent (never literal `null`). See [CSV columns](/docs/developer/reference/csv-columns/).

## Related

- [Mail archive format](/docs/developer/formats/mail-archive/) — EML/MBOX packaging
- [SMS Backup & Restore XML output](/docs/developer/formats/sms-backup-restore-xml/) — SyncTech `smses.xml` output
- [CSV columns](/docs/developer/reference/csv-columns/) — user-facing CSV conventions
- [Converter capabilities](/docs/developer/formats/)
