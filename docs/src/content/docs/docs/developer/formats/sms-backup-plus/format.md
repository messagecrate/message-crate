---
title: "SMS Backup+ format"
description: "Layout of the SMS Backup+ EML files the exporter reads."
---

Input messages come from [SMS Backup+](https://github.com/jberkel/sms-backup-plus) syncing Android SMS/MMS to Gmail/IMAP, then archived as `.eml` (this project does **not** talk to IMAP).

## Flat single-message EML

Typical headers:

| Header | Meaning |
|--------|---------|
| `X-smssync-datatype` | `SMS`, `MMS`, `CALLLOG` or `WHATSAPP`; a `CALLLOG` or `WHATSAPP` mail is skipped (below) |
| `X-smssync-type` | Android SMS type; sent ≈ `{2,128,4,135,6,5}`, received ≈ `{1,132,130}` |
| `X-smssync-address` | The other party's number. For an MMS, only the first address the phone stored for it, so a group MMS names one of its participants here. Older exports may list several, separated by `~` (or `;`, `,`, `\|`) |
| `X-smssync-date` | The message time as Android stores it: epoch **milliseconds** for an SMS or a call, epoch **seconds** for an MMS (below) |
| `X-smssync-id` | Stable sync id (optional) |
| `Subject` | `SMS with {contact name}` |
| `From` / `To` | The owner is their Gmail address. Anyone else is their contact's email address when the contact has one, else `<number>@unknown.email`. A sent MMS names every recipient in `To`; a received MMS names its sender in `From` and every other recipient, the owner included, in `To` |

The `X-smssync-address`, `From` and `To` rows come from SMS Backup+ at commit
`fd33c32`: `messageFromMapMms` in
`app/src/main/java/sms/backup/plus/mail/MessageGenerator.java` (lines 144–150
and 182), `getDetails` in `MmsSupport.java` (lines 91–153), and `getAddress`
and `getUnknownEmail` in `PersonRecord.java` (lines 36–52 and 83–86).

SMS Backup+ copies `X-smssync-date` straight from Android's database, so its
unit is the one Android stores for the message's kind: milliseconds for an
SMS and a call, seconds for an MMS. The source is `setSmsHeaders`,
`setMmsHeaders` and `setCallLogHeaders` in
`app/src/main/java/sms/backup/plus/mail/HeaderGenerator.java` at commit
`fd33c32` (lines 64–88). The exporter reads the date in the unit
`X-smssync-datatype` gives, by the date rule every Android reader shares: a
whole number, at most the last millisecond of the year 9999.

`WHATSAPP`, which SMS Backup+ wrote for WhatsApp messages from 2013 to 2015
(commits `e77c56d` and `24b549e`), is read in milliseconds too. A mail with no
`X-smssync-datatype` comes from a version older than the header, which SMS
Backup+ added in November 2010 (commit `33f87f2`), a month after it began
backing up MMS. Its date is read in the unit that agrees with the `Date`
header to the second, and in milliseconds when it has no `Date`.

A date that cannot be read, or one in a mail of any other datatype, leaves the
time to the `Date` header, which SMS Backup+ writes on every mail to the
second. A mail with neither is skipped and counted as a message with an
invalid date.

A received MMS names the owner in `To` under the address on the owner's own
contact card, which need not be the account the backup went to. The run
knows the owner only by the phone numbers and email addresses it is given, so
they should include every number the archive spans and any email address on
the owner's own contact card. A received MMS whose `To` names two or more
addresses and none of the owner's is not read as a group. It goes to the
one-to-one conversation of the address in `From`, who wrote it. When `From`
gives no address, it goes to the `X-smssync-address` conversation with no
sender, since that number is not known to have written it. Either way it is
counted as `group_messages_owner_not_named` in the run summary. The counter
counts stored messages, after duplicates are dropped.

The body is the `text/plain` part. SMS Backup+ writes every message body as
plain text — zero of 20,000 sampled carry a `text/html` part — so there is
nothing else to read. Non-text MIME parts are exported as attachments.

## Call-log mails

SMS Backup+ can also back up the phone's call log, one mail per call, into a label of its own ("Call log").
Such a mail carries `X-smssync-datatype: CALLLOG`, and its `X-smssync-type` holds the call's type, not a message type.
Message Crate has no model for a call, so the exporter skips every `CALLLOG` mail and counts it as `skipped_call_log`, which the run summary prints as `Skipped N call log entries`.

## WhatsApp mails

From SMS Backup+ 1.5.0 (June 2013) to 1.5.9-BETA5 (2015), SMS Backup+ also backed up WhatsApp messages, one mail per message, into a label of its own ("WhatsApp"), between commits `e77c56d` ("Adding whatsapp support") and `24b549e` ("Remove whatsapp for real") of jberkel/sms-backup-plus.
Such a mail carries `X-smssync-datatype: WHATSAPP`, and its `X-smssync-type` holds WhatsApp's own status number, not an Android message type.
The exporter skips every `WHATSAPP` mail and counts it as `skipped_whatsapp`, which the run summary prints as `Skipped N WhatsApp messages`.
It is not read as an SMS, because its type gives no direction, and not as WhatsApp either: the feature never covered group chats, SMS Backup+ removed it because it did not work for most people (jberkel/sms-backup-plus#564), and no real backup is at hand to test a mapping against.

## Import mapping and deduplication

Source-field mapping and online cover-key deduplication: [SMS Backup+ mapping](/docs/developer/formats/sms-backup-plus/mapping/).

## Writing SMS Backup+ mail

The **EML (SMS Backup+)** export format writes SMS and MMS back out as this mail, through `SmsBackupPlusArchive` in `sms-backup-plus-exporter` (ADR 0021). A message that is not SMS or MMS is left out and counted, and the run's log says how many.
A message whose service is unknown counts as SMS or MMS when its kind says so, as it does for the Android XML export.

Each conversation is one directory, named as the EML archive names its directories, holding one `.eml` per message, named as the EML archive names its files.

Every mail carries `Subject` (`SMS with <name>`: the sender of a received message, the other person of a sent one-to-one message, and otherwise a member's number, since the import takes the name as the sender's), `From`, `To`, `Date`, `Message-ID` and `References` (`<…@sms-backup-plus.local>`), `MIME-Version`, `Content-Type`, `Content-Transfer-Encoding`, and:

| Header | Value |
|---|---|
| `X-smssync-datatype` | `SMS`, or `MMS` for an MMS or a message with an attachment |
| `X-smssync-address` | The other person's address; in a group, every other person's, joined by `~`. Empty for a conversation known only by a name, which the subject then names, and for the conversation that names nobody, whose subject is plain `SMS` |
| `X-smssync-date` | The message time in epoch milliseconds for an SMS, and in epoch seconds for an MMS, as SMS Backup+ writes it, so an MMS goes out to the second |
| `X-smssync-type` | `1` received or `2` sent for SMS; `132` received or `128` sent for MMS |
| `X-smssync-backup-time` | The start of the Export Run that wrote the file, or of the Convert run, in epoch milliseconds |

`From` and `To` name a phone number as `<number>@unknown.email` and an email address as itself, as SMS Backup+ does, so the import reads each identity back as it went out.
A received group message whose sender is unknown names an address no member has, and comes back with no sender. So does a received message in the conversation that names nobody, which comes back as that conversation, with no participant.

An SMS is `text/plain`. An MMS is `multipart/mixed`: the text, then each stored attachment with its content type and file name.
Every `text/plain` part of an MMS is message text to the import, as it is on the phone, so a text file attached to a message is written as `application/octet-stream` under its own name, and comes back as a file of that type.
An attachment whose file is gone is left out of the mail and counted as missing in the run's log.
A carriage return in the text comes back from the import as a newline, because the import reads a mail's line breaks as newlines.

The database does not keep the phone's row and thread ids, read and status flags, protocol, or the app's build, so `X-smssync-id`, `-thread`, `-read`, `-status`, `-protocol` and `-version` are never written. `X-GM-THRID` and `X-Gmail-Labels` are Gmail's, not the app's, and are never written either.
