---
title: Export
description: What the desktop app's Export screen writes, what an export covers, and the formats it offers.
---

**Export** writes an account's messages and their attachments to a directory on the computer.
It reads the Message Crate, never a phone backup.

Export is in the desktop app's sidebar, under **Messages**.
The browser doesn't show it, because writing a directory of files needs the desktop app.

Exporting needs the account's export permission, which the Owner sets under **Message Permissions**.
**Export** stays in the sidebar for an account without it.
Opening it shows a message in place of the form: the Owner has not allowed this account to export.

Every export is recorded as an Export Run.
[**Settings → Storage**](/docs/user/features/settings/storage/) lists them under **Export history**.
Each row has the date, the scope, the status, the count of messages that matched, the count delivered, the count of attachments, and their size.
The record holds what was asked for, never what the messages said.

## The form

The Export screen has five fields.

| Field | What it sets |
|---|---|
| **Scope** | **Everything** or **Search**. |
| **Search in** | **Conversations** or **Messages**: the list the search runs on. Shown only when **Scope** is **Search**. |
| **Search** | The search an export is limited to. Shown only when **Scope** is **Search**. |
| **Save to** | The directory the export is written into. Left empty, the export gets a directory of its own in the Export Directory. |
| **Format** | One of six formats. **JSON Lines (.jsonl)** is the default. |

**Export** starts the run and **Cancel** stops it.
Under **Search**, the **Export** button stays disabled until the search box holds a search.
The desktop app runs one job at a time, so Export waits from the start of an Import Run to its end, its Reviews included, and **Export** stays disabled while **Convert** in Settings runs. A Review holds it back only for the account that started the run; another account logged in on the same desktop app can use it. The screen names the job it waits for.
A log under the buttons shows what the run is doing.
A finished run reads `Export complete.` followed by the format and the directory.

## Scope

**Everything** writes every conversation the account holds.

**Search** writes only what a search finds.
The box takes the [search language](/docs/user/features/messages/search/), and **Search in** sets which list the search runs on.
The line under the box says what the export will hold.

| **Search in** | The export holds | Example |
|---|---|---|
| **Conversations** | Every message of each conversation the search finds | `messages:>100` exports each conversation of more than 100 messages, whole |
| **Messages** | Only the messages the search finds | `from:me last year` exports the messages the account sent last year and no others |

Each list has its own search words.
`messages:` is a Conversations word, and `from:`, `to:`, and `in:` are Messages words.
A search that uses a word its list doesn't have is refused, and the log names the word.

Under **Messages**, `in:` names particular conversations by title, by identity, or by id: `in:#19,#22` exports those two conversations and nothing else.

Does a search that both lists accept export the same thing on each? No.
`date:2024` under **Messages** exports the messages sent in 2024.
Under **Conversations** it exports every conversation with a message in 2024, including what those conversations hold from other years.

Export opened from the conversation list starts in **Search**, with the search that list is showing already in the box and **Search in** set to **Conversations**.
The export then holds every message of the conversations the list showed.
Opened from any other screen, or with no search running, it starts in **Everything**, and choosing **Search** there starts in **Messages**.

An Export Run hands over the messages that matched when it started.
A message imported or trashed while the run is being read does not change what the run writes.

## Formats

| Format | What is written |
|---|---|
| **JSON Lines (.jsonl)** | One `.jsonl` file per conversation, attachments in an `attachments/` directory |
| **JSON (.json)** | One indented `.json` file per conversation, attachments in an `attachments/` directory |
| **CSV (.csv)** | One `.csv` file per conversation, attachments in an `attachments/` directory. Columns: [CSV columns](/docs/developer/reference/csv-columns/) |
| **EML (one file per message)** | One directory per conversation, one `.eml` file per message, attachments embedded |
| **MBOX (.mbox)** | One `.mbox` file per conversation, attachments embedded |
| **Android XML (smses.xml)** | A single `smses.xml` holding only SMS and MMS, attachments embedded. The log says how many other messages were left out |
| **EML (SMS Backup+)** | One directory per conversation, one `.eml` file per SMS or MMS in the mail SMS Backup+ writes, attachments embedded. Other messages are left out, and the run's log says how many |

Export always fetches the messages as JSON Lines first.
Any other format is written by converting that JSON Lines copy, as part of the same run.

The JSON Lines layout is described in [Export structure](/docs/developer/reference/export-structure/).

## The Export Directory

Every export gets a directory of its own in the Export Directory, named `export-` followed by the date, the time and the format, such as `export-2026-10-04-1430-mbox`.
With **Save to** left empty, the export is written there.
[**Settings → System**](/docs/user/features/settings/system/) names the Export Directory under **Exports**, and opens it.

The desktop app keeps the Export Directory in the operating system's app-data directory, under `exports`:

| System | Export Directory |
|---|---|
| Linux | `~/.local/share/app.messagecrate.desktop/exports` |
| macOS | `~/Library/Application Support/app.messagecrate.desktop/exports` |
| Windows | `%APPDATA%\app.messagecrate.desktop\exports` |

An export in any format other than JSON Lines first fetches the messages as JSON Lines into its directory, then converts them.
When it finishes, the JSON Lines copy is deleted and the directory holds only the result, with the hidden `.message-crate-export` file that marks it as an export, so a later export or conversion into it replaces what it holds.
A failed or cancelled export deletes its directory, so no copy of the messages is left behind.
An export the app did not see to its end, because the app was closed or stopped, is deleted the next time the app starts.
The disk that holds the Export Directory needs room for a second copy of the exported messages and attachments while such an export runs.

## Saving somewhere else

A directory chosen under **Save to** gets the result instead, and the export's directory in the Export Directory holds only the JSON Lines copy while the conversion runs.

A JSON Lines export writes straight into the chosen directory.
It also keeps a file named `.message-crate-pull-state.jsonl` there, which records the attachments already downloaded from each server and account.
A later JSON Lines export into the same directory, from the same server and account, skips those attachments.
An export into its own directory in the Export Directory deletes that file when it finishes, since nothing exports into that directory again.

An export in any other format first deletes the files of an earlier export from the chosen directory.
It refuses a directory that holds other files and no export, so that nothing unrelated is deleted.
An empty directory, or one a previous export wrote, is accepted.
