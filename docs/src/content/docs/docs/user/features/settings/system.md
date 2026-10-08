---
title: System
description: What the System tab of Settings holds, the Staging Directory, remembered importer paths, where ffmpeg, ffprobe and wtsexporter were found and how their download stands, the Export Directory, the data directory of the app's own Message Crate, and the app's version.
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

**Media** shows where the desktop app found the programs it runs, and has nothing to change.
`ffmpeg` and `ffprobe` convert and compress attachments, and are looked for on the system `PATH`, then in the Tools Directory.
`wtsexporter` reads WhatsApp backups, and is looked for in the Tools Directory only.

Each time it starts, the desktop app downloads into the Tools Directory the programs that are missing there, in the background.
Login and browsing don't wait for it.
`ffmpeg` and `ffprobe` are not downloaded when both are on `PATH`, because the copy installed there is the one used.
`ffmpeg` and `ffprobe` come from a pinned release of `eugeneware/ffmpeg-static`, and `wtsexporter` from a pinned release of Message Crate's fork, `messagecrate/WhatsApp-Chat-Exporter`.
The app carries the SHA-256 checksum of every file it downloads, and refuses and deletes a file that doesn't match.

The Tools Directory belongs to the app, and one rule holds for all three programs.
A program in the Tools Directory whose checksum is the pinned program's is kept, whoever put it there, and anything else under its name is replaced by the pinned program.
The old file is replaced only after the new one has arrived and passed its checksum, so with no internet, or after a failed download, the old one stays in use.

**Tools directory** shows the full path of the Tools Directory, `tools` in the Message Crate Directory, such as `/home/sam/message-crate/tools` on Linux.

One line per program reports the result:

- A check mark with `Found ffmpeg` and the full path of the program.
- A cross with `<program> not found` and where to put it.
  A missing ffmpeg or ffprobe goes beside the other one, or both go in the Tools Directory, because the two are used only from one place.
  A missing wtsexporter goes in the Tools Directory.
- A cross with `<program> not used` and the reason.
  For ffmpeg and ffprobe, the reason is that they were found in two places, one only on `PATH` and the other only in the Tools Directory.
  For wtsexporter, the reason is that it has no permission to run.
- A download arrow with `Downloading <program>` and how much has arrived, such as `12 MB of 29 MB (41%)`.
  The line updates each second until the download ends.
- A cross with `<program> download failed` and the reason: no connection to the download's server, the status the server answered, a checksum that didn't match, a file that couldn't be written to the Tools Directory, or a program that was downloaded but doesn't run on this computer.
  A failed download is tried again the next time the app starts.
  A program that doesn't run is not downloaded again, because the download would be the same file.
  For ffmpeg and ffprobe, the reason says to install them with a package manager, because the app uses the copy on `PATH`.
  While an older copy is in place, the line shows that copy as found instead, because the old copy is still the one used.

On macOS, an app opened from the Dock or Finder doesn't see the `PATH` Homebrew sets in a terminal, so it doesn't find ffmpeg and ffprobe installed with Homebrew.
It downloads its own copies into the Tools Directory instead, and nothing needs doing.

[Attachments and media](/docs/user/features/messages/attachments-and-media/) covers what ffmpeg and ffprobe are used for.

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
| Logs Directory | `logs` | Each Import Run's log, named for the run, such as `import-iphone-ios-261004-143000.log`. Never deleted. The run's account reads it from **Settings → Storage**, and the Owner from **Logs** on Owner Home |
| Scratch Directory | `scratch` | Decrypted iPhone backup databases, the WhatsApp database decrypted from an Android backup, WhatsApp's files read out of a phone backup, and the attachments read out of SMS backups, while a run needs them. What a stopped run left is deleted the next time the app starts |

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
