---
title: Storage
description: What the Storage tab of Settings shows, the account's usage, its Import Runs and Export Runs, and its largest attachments.
---

The **Storage** tab of **Settings** describes what one account holds and how it got there.
Nothing on it can be changed.
It has four sections, in this order: **Usage**, **Import history**, **Export history**, and **Largest attachments**.

The Owner's own Settings has no **Storage** tab, because the Owner holds no messages.

## Usage

**Usage** shows one large figure, the total size of the account's attachments.
The figure counts original file sizes, not the size of any converted or compressed copy.

Two lines under it give four counts: messages, attachments, conversations, and contacts.

## Import history

**Import history** lists the account's Import Runs, newest first.
A run is listed whether it completed, failed, or was cancelled, and a person can't delete one.

The list shows 50 runs to a page.
**Back** and **Next** move between pages.

| Column | Shows |
|---|---|
| **Date** | When the run finished, or when it started if it has not finished |
| **Import type** | The source, such as `imessage`, `whatsapp`, or `sms-backup-restore` |
| **Messages** | Messages counted for the run |
| **Attachments** | Attachments counted for the run |
| **Uploaded size** | Bytes the run uploaded |

### What an Import Run's record shows

Selecting a row opens **Import details** under it.
Selecting the row again, or the **×** button, closes it.

Three labels sit at the top: **Type**, the source, **Mode**, `Append` or `Replace`, and **Status**.

Six figures follow: **Started**, **Finished**, **Messages**, **Attachments**, **Bytes uploaded**, and **Issues**.

**Backup** names the backup the run read, as the desktop app recorded it, and when that backup was made, so two imports of one phone can be told apart.
The date comes from the backup itself: an iPhone backup's own date, the date an SMS Backup & Restore file records, or the date its files were last written where the backup records none.
When the run read two backups, it shows the later.
"The backup does not say when it was made" appears for a backup that records no date.
The owner, reading another account's runs, does not see this line.

A list of four steps comes next, each with the time it took: **Parse backup**, **Attachments**, **Preparing messages**, and **Upload to Message Crate**.

The **Messages** table accounts for every message the run read:

| Row | Counts |
|---|---|
| **Parsed** | Messages read from the backup |
| **Skipped** | Parsed messages the run did not try to upload |
| **Attempted** | Messages the run tried to upload |
| **New uploaded** | Attempted messages that were new to the account |
| **Duplicate** | Attempted messages the account already held |
| **Failed** | Attempted messages that could not be stored |

A count the run did not record shows as a dash.

**Import Errors** appears beside the table when the run recorded problems.
Identical errors are grouped, and selecting a row shows the full message and the files it applies to.

**Notes** appears under the table when the run kept something with a caveat, such as a message filed under a name because it recorded no phone number.
A note is not an error.
Identical notes are grouped, and selecting a row shows the full note and the items it applies to.

**Contacts** closes the record.
It states how many contacts the run made and how many it changed, then lists each one with what the run did to it:

| Label | Meaning |
|---|---|
| **New** | The run created the contact |
| **New, replaces a trashed contact** | The run created the contact in place of one in the Trash |
| **Named** | The run gave the contact a name |
| **Identity added** | The run added an Identity to the contact |

A contact with no name yet is listed as `(unknown)`.
A run that touched no contact reads "This import changed no contacts."

**Log** closes the record in the desktop app on the computer that ran the import.
**Open this run's log** shows the run's log under it, newest line first, with **Show**, **Search the log**, and **Download** as on Owner Home's [Logs](/docs/user/features/owner/owner-home/#logs).
The log stays on that computer, so a browser, or the desktop app on another computer, shows no **Log**.
An account sees its own runs' logs, and the Owner sees every run's.

How a run proceeds is covered in [Import](/docs/user/features/messages/import/).

## Export history

**Export history** lists the account's Export Runs, newest first.
The record holds what was asked for and how much matched, never what the messages said.

The list shows 50 runs to a page.
**Back** and **Next** move between pages.

| Column | Shows |
|---|---|
| **Date** | When the run finished, or when it started if it has not finished |
| **Scope** | What was asked for |
| **Status** | **Running**, **Completed**, **Failed**, or **Cancelled** |
| **Messages** | Messages that matched when the run started |
| **Delivered** | Messages the run handed over |
| **Attachments** | Distinct attachments among the matching messages |
| **Size** | Total size of those attachments |

**Scope** takes one of four forms:

- `Everything`, for all the account holds.
- `Conversations found by:` followed by the search text, for an export of whole conversations.
- `Messages found by:` followed by the search text, for an export of the matching messages alone.
- `Picked:` followed by a count of conversations and messages chosen by hand.

A run that never finished shows how far it got in **Delivered**.

How an export is started is covered in [Export](/docs/user/features/messages/export/).

## Largest attachments

**Largest attachments** lists the account's 100 largest attachments by file size, 20 to a page.
The line above the list says how many it holds, such as **Top 100 attachments by file size · 20 per page**.
**Back** and **Next** move between pages.
An account with no attachments of known size sees **No attachments with sizes yet** instead.

| Column | Shows |
|---|---|
| **Name** | The file's original name. Without one, its media type |
| **Conversation** | The conversation the attachment is in |
| **Size** | The file size |

Attachments themselves are covered in [Attachments and media](/docs/user/features/messages/attachments-and-media/).

## What the Owner reads

The Owner reads another account's **Storage** tab from [Owner Home](/docs/user/features/owner/owner-home/).
Two parts are held back, because they say who the account talks to.
The **Contacts** part of an Import Run's record gives the two counts without the list.
**Largest attachments** has no **Conversation** column.

The Owner's view of an Import Run's record has one part the account's own does not: **Thumbnails and Previews**, between **Contacts** and **Log**.
It lists the run's attachments whose Thumbnail or Preview the server could not make, so the Owner can see which files a damaged backup or a slow computer left without them.
The server makes them after the run's data arrives, so the part fills in once the server has tried.

A line first says how many attachments have no Thumbnail or Preview.
A table then counts them by kind of failure, with one column for **No Thumbnail** and one for **No Preview**:

| Kind of failure | Meaning |
|---|---|
| **Conversion failed** | ffmpeg could not read or convert the file |
| **Stopped making progress** | ffmpeg made no progress for the stall wait, so the server stopped it |
| **Damaged, ran past its length** | ffmpeg ran past twice the file's length, as it does looping on a damaged file, so the server stopped it |
| **Took too long** | Reading one frame of the file did not finish in time |
| **Original file missing** | The server holds no copy of the file |
| **Server problem** | Anything else that went wrong on the server |

Under the counts, each such attachment is listed with its **Name**, **Type** and **Size**, and under **Thumbnail** and **Preview** the kind of failure and the reason the server recorded, or a dash for a version that was made or that the attachment does not need.
A run whose attachments all got what they need reads "No attachment of this import is missing a Thumbnail or Preview the server tried to make."

The list names each attachment as a file, by the metadata the Owner may read.
The Owner never sees the attachment, its Thumbnail or its Preview, nor the message or conversation it belongs to.
Nothing on the part tries again: the server tries a file again only when someone runs `message-crate-server process-assets`.
