---
title: System
description: What the System tab of Settings holds, the Staging Directory, remembered importer paths, the ffmpeg directory, the Export Directory, the data directory of the app's own Message Crate, and the app's version.
---

The **System** tab of **Settings** holds the settings of one installed desktop app.
They are stored on the computer, not in the account, so a second computer has its own.

In a browser the tab shows **About** and nothing else to change.
A line under it says the other settings are available in the desktop app, because they name directories on the computer and a web page can't reach those.

The Owner's own Settings has no **System** tab, because every setting here serves Import and Export and the Owner holds no messages.

## Staging

### Staging directory

**Staging directory** is the Staging Directory, where Import keeps the backup it prepares.
The default is `message-crate` in the person's home directory, shown as `~/message-crate`.

Each Import Run writes into a directory of its own inside it.
The line under the field gives an example, `~/message-crate/staging-iphone-ios-260809-143022`.
Export and Convert don't use it: they work in the Export Directory, under [Exports](#exports).

The field takes a typed path or a directory chosen with the picker.
A change is saved as it is typed.

Two kinds of path are not saved, and the default stays in force:

- A relative path, because it would resolve against wherever the app happened to start.
- The root of a file system, such as `/` or `C:`, because an Import Run would then write beside every other directory on the disk.

Emptying the field returns to the default.

A change applies to the Import Runs started after it.
An import already staged keeps the directory it was staged in, so it can still be resumed, discarded or cleaned up after the setting moves.

When Message Crate can't delete the directory of a finished, cancelled or discarded import, the Import screen names the directory and the reason, so the directory can be deleted by hand.

### Remember importer paths

**Remember importer paths** is a checkbox, off by default.
When it is on, Import restores the last backup path used for each import source.

## Media

### ffmpeg directory

**ffmpeg directory** names a directory that holds both `ffmpeg` and `ffprobe`.
The field is empty by default, and the app then finds both on the system `PATH`.

The app checks the directory as soon as a path is entered.
Two lines under the field report the result, one per tool:

- A check mark with `Found ffmpeg` and the full path of the program.
- A cross with `ffmpeg not found`.

The directory is saved only when both tools are found in it, because one without the other can't convert media.
A saved directory is applied again each time the app starts.

**Install help** opens [Attachments and media](/docs/user/features/messages/attachments-and-media/), which covers what the two tools are used for and how to install them.

## Exports

**Exports** names the Export Directory and opens it.
Each [Export](/docs/user/features/messages/export/), and each [Convert](/docs/user/features/settings/convert/) with no output directory chosen, gets a directory of its own there, named for what it is, when it started and its format, such as `export-2026-10-04-1430-mbox` or `convert-2026-10-04-1502-csv`.
The result is written there unless the Export form names another directory.
The directory holds only the result once the run finishes; a run that fails or is cancelled deletes it, and one the app did not see to its end is deleted the next time the app starts.

The Export Directory is `exports` in the operating system's app-data directory, such as `~/.local/share/app.messagecrate.desktop/exports` on Linux.
Message Crate never deletes a finished export from it.

## Where the app keeps its files

The desktop app keeps everything that is not a phone backup or a chosen destination in the operating system's app-data directory, `~/.local/share/app.messagecrate.desktop` on Linux, `~/Library/Application Support/app.messagecrate.desktop` on macOS and `%APPDATA%\app.messagecrate.desktop` on Windows.

| Directory | In the app-data directory | Holds |
|---|---|---|
| Data Directory | `data` | The database, each account's attachments and the server's log, of the Message Crate this app starts |
| Export Directory | `exports` | One directory per Export, and per Convert with no output directory chosen. See [Exports](#exports) |
| Logs Directory | `logs` | Each Import Run's log, named for the run, such as `import-iphone-ios-261004-143000.log`. Never deleted |
| Scratch Directory | `scratch` | Decrypted iPhone backup databases and the attachments read out of SMS backups, while a run needs them. What a stopped run left is deleted the next time the app starts |

The Staging Directory is the one directory outside it: see [Staging directory](#staging-directory).

## Message Crate on this computer

This part is about the Message Crate the desktop app starts for itself.

### Open data directory

**Open data directory** opens the directory where that Message Crate keeps its database and attachments.
A copy of the directory, made while the app is closed, is a backup.

### Let other devices on this network connect

The checkbox is off by default, and the Message Crate then answers this computer only.

When it is on, a phone or another computer on the same network reaches the Message Crate in a browser, at this computer's address on port 8080.
That works only while the app is open.
The connection is plain HTTP, so anyone on the network can read what is sent, passwords included.

Changing the checkbox restarts the Message Crate, which takes a moment.
During an import, the restart waits until the import's current step ends, so the import isn't cut off.
It has no effect on a Message Crate the app didn't start, such as one in Docker on the same computer, and a line under the checkbox says so.
While the app uses a Message Crate at another address, the checkbox starts nothing, and the setting applies the next time the app starts its own.

## About

**Version** shows the Build of the app.
A Build made from a release is the Product Version alone, such as `0.9.0`.
Any other Build adds the commit it was built from, such as `0.9.0+343fe0d8`.
A browser tab left open across a server upgrade keeps reporting the Build it loaded.

## Third-party software

This section appears in the desktop app only.

It states that Apple Messages are read by the Apple Messages reader (imessage-reader), a separate program installed beside the app.
That program is free software under the GNU General Public License, version 3 or later.

**Source** and **License** link to the program's source and license for the installed Build.
The browser does not show the section, because the website ships no such program.
