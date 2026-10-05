---
title: Export structure
description: The JSONL format Message Crate imports — schema version 7, one file per conversation.
---

Message Crate imports JSONL (JSON Lines) exports at schema version 7. Version 6 and older are refused, never upgraded. This page describes the format for CLI users and tool authors.

## Happy path

**Phone backup → JSONL export → server CLI `import` or `POST /v1/imports/{id}/batches` → SQLite**

The JSONL files are plain text — one JSON object per line. The format is the same whether you import through the desktop app or post to the import API directly.

## File shape

One `*.jsonl` per conversation, plus media files under `attachments/`:

1. **Line 1** — Conversation header (`schema_version`, `export`, `conversation`)
2. **Following lines** — One message per line (`timestamp_unix_ms`, `direction`, `service`, `text`, `attachments`, and optional fields)

`service` is the channel (`sms`, `imessage`, `rcs`, …). A message's reactions are its `reactions` list, one shape for every source: each names the part reacted to (`part_index`), what the reaction is (`kind`, and `emoji` for an emoji reaction), whether the owner reacted (`is_from_me`), and who did (`reactor_identity`, `reactor_display_name`). The server stores each reaction under the person who reacted. A message deleted in the source app before the backup carries `"deletion": "deleted_in_source_app"`, and one its sender unsent carries `"deletion": "unsent"`; a message with neither leaves `deletion` out. The server stores the mark and imports the message like any other. Apple-specific fields such as replies and message effects live under an optional `imessage` object.

Attachment records may include `digest_sha256` so clients can upload by hash (`PUT /v1/assets/{sha256}`) before importing the JSONL.

## Clients

- **Same machine**: CLI `import` against a local export directory
- **Remote**: The desktop app **Import** screen posts JSONL to the import API. Batched requests may concatenate multiple conversations (header + messages, repeated) in one body.

## Schema compatibility

The server reads one schema version, currently 7. Version 7 keeps a message's mark, Deleted in the source app or Unsent, in its own `deletion`, where version 6 kept the Apple Messages deleted mark in `imessage.is_deleted`. Version 6 had moved a message's reactions into its own `reactions` list, where version 5 kept Apple Messages reactions in `imessage.tapbacks`. Version 5 had called every address an identity (`identity`, `identity_type`, `owner_identity`, `sender_identity`, `reactor_identity`) where version 4 said `handle`. A file written at any other version is refused, with an error naming both the file's version and the version the server expects. To import an older export, re-export it with the current desktop app.

## Related

- [CSV columns reference](/docs/developer/reference/csv-columns/)
- [Import API reference](/docs/developer/reference/api/)
