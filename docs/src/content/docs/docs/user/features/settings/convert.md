---
title: Convert
description: What the Convert tab of Settings does, rewriting a directory of exported files into another format without reading a backup or Message Crate.
---

**Convert** rewrites a directory of already-exported files into a different format.
It reads files and writes files.
It never opens a phone backup, and it never reads or changes anything in Message Crate.

The **Convert** tab of **Settings** appears in the desktop app only, because the conversion runs on the computer, not on the server.
A browser does not show the tab.
The Owner's own Settings has no **Convert** tab either, because the Owner holds no messages.

Convert sits under Settings, not beside Import and Export, because most people never need it.
[Import](/docs/user/features/messages/import/) never requires it, and [Export](/docs/user/features/messages/export/) already writes every format Convert does.
It serves the case where an export exists in one format and a copy in another is wanted, such as JSON Lines rewritten as MBOX for a mail client.

## The form

The tab holds three fields and two buttons.

| Field | Holds |
|---|---|
| **Input directory** | The directory holding one existing export |
| **Output directory** | A different directory to write into. Left empty, the conversion gets a directory of its own in the Export Directory |
| **Output format** | The format to write. The default is **JSON Lines (.jsonl)** |

**Convert** starts the conversion.
It stays disabled until **Input directory** is filled in, and while **Output directory** names the same directory.
The desktop app runs one job at a time, so **Convert** also stays disabled from the start of an Import Run to its end, its Reviews included, and from the start of an export to its end. A Review holds it back only for the account that started the run; another account logged in on the same desktop app can use it. The screen names the job it waits for.
**Cancel** stops a running conversion.

A log under the buttons fills in as the conversion runs.
Its first line names the format found in the input directory, such as `Detected input format: json`.
Its second line gives the number of conversations, such as `Conversations: 384`.

A finished conversion shows a green panel that names the format and the output directory, such as "Conversion complete. MBOX (.mbox) written to /home/sam/exports/mbox."
A failed one shows the reason in a red panel and as an `Error:` line in the log.

With **Output directory** left empty, the conversion writes into a directory of its own in the Export Directory, named `convert-` followed by the date, the time and the format, such as `convert-2026-10-04-1502-mbox`.
[**Settings → System**](/docs/user/features/settings/system/#exports) names the Export Directory and opens it.
A conversion that fails or is cancelled deletes that directory.

## What it reads

The input format is detected from the directory, not chosen.
Convert reads the six formats Export writes:

| Format | Recognised by |
|---|---|
| JSON Lines | `.jsonl` files |
| JSON | `.json` files |
| CSV | `.csv` files |
| MBOX | `.mbox` files |
| EML | Directories that hold `.eml` files |
| Android XML | A file named `smses.xml`, or an `.xml` file that starts with `<smses` |

A `.json`, `.jsonl`, or `.csv` file counts only when its contents look like a Message Crate export, so an unrelated file with the same extension is passed over.

Detection ignores the `attachments` directory, any name that starts with a dot, and names that end in `.meta.json`, `.tmp`, or `.xml.sbrbody`.

A directory must hold exactly one format.
A directory that holds more than one is refused with `unsupported input: mixed formats`, followed by the formats and files found, because Convert can't tell which export to read.
A directory that holds none is refused with `unsupported input: no Message Crate IR export found`.

A `.json` or `.jsonl` export of another schema version, such as an export written before version 9, is refused with "This file is schema version 8; Message Crate reads version 9" and the file's name.
Nothing is upgraded: export the conversations again with the current app.
Convert reads every file before it writes, so a refused file stops the whole run and the output directory is left as it was.

## What it writes

| **Output format** | Shape | Media |
|---|---|---|
| **JSON Lines (.jsonl)** | One `.jsonl` per conversation | `attachments/` directory |
| **JSON (.json)** | One indented `.json` per conversation | `attachments/` directory |
| **CSV (.csv)** | One `.csv` per conversation | `attachments/` directory. Columns: [CSV columns](/docs/developer/reference/csv-columns/) |
| **EML (one file per message)** | One directory per conversation, one `.eml` per message | Embedded |
| **MBOX (.mbox)** | One `.mbox` per conversation | Embedded |
| **Android XML (smses.xml)** | One `smses.xml` holding only SMS and MMS | Embedded |
| **EML (SMS Backup+)** | One directory per conversation, one `.eml` per SMS or MMS | Embedded. Messages that are not SMS or MMS are left out, and the log says how many |

The directory layout is described in [Export structure](/docs/developer/reference/export-structure/).

## Why the two directories must differ

Convert clears an earlier export out of the output directory before it writes.
A second run into the same output directory therefore replaces the first instead of mixing with it.
Writing into the input directory, or into a directory that holds it, would delete the files being read.

Two checks prevent that.
The screen keeps **Convert** disabled while both fields name the same directory, and says "Choose a different output directory."
The conversion itself refuses an output directory that resolves to the input directory or to a directory above it, with `output <directory> must not be the same as, or contain, the input <directory>`.
The second check also catches a symbolic link that points at the input directory.

## What the output directory may hold

An output directory that does not exist is created.
An empty directory, or one an earlier export wrote, is used as it is.
A directory that holds only the hidden files an operating system leaves behind, such as `.DS_Store`, `Thumbs.db` or `desktop.ini`, counts as empty.

Any other directory is refused and nothing in it is touched, even when it holds files named like an export, such as a `.csv` or a `.json` file.
A file's name cannot show that an export wrote it, and the clean-up would delete a person's own `budget.csv` along with an earlier export.
The message names the directory and ends "Choose an empty directory or one an earlier export wrote."

An export leaves a hidden file named `.message-crate-export` in its output directory, which marks the directory as one Convert may clear on a later run.
Only a directory with that file is cleaned.
When Convert writes an SMS Backup & Restore file, that hidden file lists `smses.xml` and its two temporary files.
The clean-up removes earlier export files, the whole `attachments` directory, and the files that hidden file lists.
Every other file stays, other XML files included.

## Limits

- Attachments reach the output only when they are present in the input directory, because Convert copies them from its `attachments` directory and never fetches them from the server.
- Android XML holds only SMS and MMS, because SMS Backup & Restore cannot describe an iMessage, a WhatsApp message or any other kind. Convert leaves every other message out and its log says how many: `Left out 3 messages that are not SMS or MMS, because SMS Backup & Restore holds only SMS and MMS`. JSON or JSON Lines is the format to choose when those messages matter.
- EML (SMS Backup+) holds only SMS and MMS. Its mail is read back by the SMS Backup+ import, not by Convert.
