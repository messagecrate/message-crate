---
title: Export structure
description: The JSONL format Message Crate imports — schema version 14, one file per conversation.
---

Message Crate imports JSONL (JSON Lines) exports at schema version 14. Version 13 and older are refused, never upgraded. This page describes the format for CLI users and tool authors.

## Happy path

**Phone backup → JSONL export → server CLI `import` or `POST /v1/imports/{id}/batches` → SQLite**

The JSONL files are plain text — one JSON object per line. The format is the same whether you import through the desktop app or post to the import API directly.

## File shape

One `*.jsonl` per conversation, plus media files under `attachments/`:

1. **Line 1** — Conversation header (`schema_version`, `export`, `conversation`)
2. **Following lines** — One message per line (`timestamp_unix_ms`, `time_precision`, `direction`, `service`, `text`, `attachments`, and optional fields)

`time_precision` is required: `milliseconds` when the source recorded the time below the second, `seconds` when it recorded whole seconds. A millisecond time can end in `000` and is still `milliseconds`. Within one source, the server shows a whole-second message once when the source also holds it with milliseconds in the same second, with the milliseconds, unless another source holds it too and that source's copy is the one shown.

`service` is the channel (`sms`, `imessage`, `rcs`, …). A message's reactions are its `reactions` list, one shape for every source: each names the part reacted to (`part_index`), what the reaction is (`kind`, and `emoji` for an emoji reaction), whether the owner reacted (`is_from_me`), and who did (`reactor_identity`, `reactor_display_name`). The server stores each reaction under the person who reacted. A message deleted in the source app before the backup carries `"deletion": "deleted_in_source_app"`, and one its sender unsent carries `"deletion": "unsent"`; a message with neither leaves `deletion` out. The server stores the mark and imports the message like any other. An edited message carries its earlier versions in `edits`, oldest first within each part, each with its `part_index`, its `text` and the time it was written; `text` is absent when the source recorded the edit without the text it replaced, as iMazing does, and such a version is shown as "Earlier version not in the backup" and never searched (`edited_at_unix_ms`); `text` is the final version, and a message never edited leaves `edits` out. The server stores each version, and search finds the message by any of them. A reply carries `reply_to`, naming the quoted message's `guid` when the source names it and the part replied to (`part_index`) when the source records one; a reply whose source names no quoted message has a `null` `guid`, and a message that is not a reply leaves `reply_to` out. A named message can still be missing from the export, such as an Apple Messages thread's first message deleted before the backup was made, which the backup does not hold. The server counts each message's replies when it reads it. Apple-specific fields such as message effects live under an optional `imessage` object.

Attachment records may include `digest_sha256` so clients can upload by hash (`PUT /v1/assets/{sha256}`) before importing the JSONL.

## Clients

- **Same machine**: CLI `import` against a local export directory
- **Remote**: The desktop app **Import** screen posts JSONL to the import API. Batched requests may concatenate multiple conversations (header + messages, repeated) in one body.

## Schema compatibility

The server reads one schema version, currently 14. Version 14 lets an earlier version of an edited message carry no `text`, for a source that records the edit and not the text it replaced, as iMazing does, where version 13 required it. Version 13 had given a participant no `identity_type`, because the server works out every identity's type from the service and the address, where version 12 stated one. Version 12 had said whether each message's time has milliseconds, in its `time_precision`, where version 11 did not. Version 11 had said when the backup was made, in `export.backup_taken_at_unix_ms`, where version 10 did not. Version 10 had kept the message a reply quotes in the message's own `reply_to`, for every source, where version 9 kept the Apple Messages reply link in `imessage.is_reply` and `imessage.in_reply_to_guid`. Version 9 had given orphaned messages, ones the backup holds without recording which conversation they were said in, conversations of type `orphaned`: one for each sender, keyed `orphaned:` and the sender's address, and one with no participants, keyed `orphaned:`, for the ones the account holder sent; version 8 put them all in one `individual` conversation named `orphaned`. Version 8 had kept an edited message's earlier versions in its own `edits`, where version 7 kept the Apple Messages edit history in `imessage.edits`. Version 7 had moved a message's mark, Deleted in the source app or Unsent, in its own `deletion`, where version 6 kept the Apple Messages deleted mark in `imessage.is_deleted`. Version 6 had moved a message's reactions into its own `reactions` list, where version 5 kept Apple Messages reactions in `imessage.tapbacks`. Version 5 had called every address an identity (`identity`, `identity_type`, `owner_identity`, `sender_identity`, `reactor_identity`) where version 4 said `handle`. A file written at any other version is refused, with an error naming both the file's version and the version the server expects. To import an older export, re-export it with the current desktop app.

## Related

- [CSV columns reference](/docs/developer/reference/csv-columns/)
- [Import API reference](/docs/developer/reference/api/)
