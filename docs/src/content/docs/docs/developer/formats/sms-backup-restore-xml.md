---
title: "SMS Backup & Restore XML output"
description: "How Message Crate writes a SyncTech smses.xml file from the shared conversation structure."
---

Exporters can project the common message into a **single** SyncTech-style backup file:

`{output}/smses.xml`

Root structure:

```xml title="smses.xml"
<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
<smses count="N">
  <sms … />
  <mms …>…</mms>
</smses>
```

This is the same family of files that [SMS Backup & Restore](https://www.synctech.com.au/sms-backup-restore/) reads. The importer’s source-format reference is [SMS Backup & Restore input format](/docs/developer/formats/sms-backup-restore/input/).

**Motivation:** Android compatibility. Full-device Android backup/restore without third-party tooling requires root and often an unlocked bootloader; the SMS Backup & Restore app restores `smses.xml` without either, so this format is the practical path for putting messages back onto an Android phone.

## Layout

| Piece | Crate / API |
|-------|-------------|
| XML codec (streaming read/write, SMIL, MMS media) | [`sbr`](https://github.com/messagecrate/message-crate/blob/main/crates/libs/sbr/) |
| SBR → common message | [`sms_backup_restore_exporter::read_backup`](https://github.com/messagecrate/message-crate/blob/main/crates/exporters/sms-backup-restore-exporter/src/read.rs) |
| Common message → SBR | [`sms_backup_restore_exporter::SbrArchive`](https://github.com/messagecrate/message-crate/blob/main/crates/exporters/sms-backup-restore-exporter/src/write.rs), a `MergedArchive` the caller hands to [`message_ir_format::FormatSink::with_archive`](https://github.com/messagecrate/message-crate/tree/main/crates/libs/ir-format) |
| Desktop | `OutputFormat::Xml` |

The exporter crate owns the format in both directions; `message-ir-format` knows only that a merged archive exists. A caller that wants `smses.xml` opens a `FormatSink` (or `ExportWriter`), calls `with_archive(Box::new(SbrArchive))`, writes each conversation with `write_document`, and calls `finish`. Asking for XML without the archive is refused at `finish`, and `write_format(..., Xml, …)` returns an error, because a single shared file cannot be rewritten per chat.

## Mapping rules

- **SMS** in a one-to-one conversation with no attachments: `<sms>` with `type` `1`/`2`, `date` = `timestamp_unix_ms`, `body` = text.
- **MMS** when group and/or attachments (or `message_kind=mms`): `<mms>` with `<parts>` / `<addrs>`; attachment bytes base64 in `data` when available on disk or in memory.
- **`contact_name`**: in a group conversation, the display names of the participants the reader will find in the `address` attribute, joined by a comma and a space (such as `Ana, Lee`), as SMS Backup & Restore writes it, on every message in either direction. Those are the participants with an identity the reader can read, other than the owner, one per identity. A participant with no name is left out, and a group conversation with no named participant gets no value. That rule is the writer's own: what SMS Backup & Restore writes for a participant the phone has no contact for is not known. A group conversation the reader will find fewer than two participants in reads back as one-to-one, so it takes the one-to-one value. In a one-to-one conversation, the value is the sender's display name on an incoming message, else the peer's.
- If `source.fields` has `kind: "sms"|"mms"` (as produced by the SBR importer), attrs / parts / addrs are preferred and overlaid with common-message date/direction/body.
- **Dropped:** entire `imessage` bag (tapbacks, replies, balloons, send effects, edits, announcements, …). Text and media still export as SMS/MMS.

## Related

- [Message-ir architecture](/docs/developer/architecture/common-message/) — shared model and projectors
- [What’s inside an export](/docs/developer/reference/export-structure/) — end-user workflow
- [SMS Backup & Restore import mapping](/docs/developer/formats/sms-backup-restore/mapping/) — XML source → shared model
