---
title: Troubleshooting
description: What to check when the server can't be reached, a login is refused, the desktop app won't start, an import stops, or the desktop app can't download ffmpeg, ffprobe or wtsexporter.
---

Each entry names what is seen and what fixes it.

## Reaching the server

### The login card reads Disconnected

The server isn't answering at the address the browser or the desktop app is using.

For the Message Crate the desktop app starts, the card says what went wrong in place of the forms, with **Try again** and the server's own words under **Details**.

For a Message Crate in Docker:

1. `docker ps` lists the running containers. `message-crate` must be among them.
2. `docker logs message-crate` shows what the server printed, including why it stopped.
3. In the desktop app, **Server Address** must hold the server's address, and **Test** checks it. The app keeps the address after logging out and opens on the login card for it at the next start. It starts its own Message Crate only when the address is its own, `http://127.0.0.1:8080`.

A server on a different computer needs more than the address.
[Run on another machine](/docs/user/features/owner/run-on-another-machine/) covers it.

### The app says another program is using port 8080

The app's own server listens on port 8080 and something else holds it.
Closing that program and selecting **Try again** starts the server.

### Docker refuses to start the server because the port is in use

Another program holds port 8080.
Changing the first `8080` in the `docker run` command to a free port, such as `-p 127.0.0.1:8090:8080`, moves the server to `http://localhost:8090`.
The desktop app then needs `http://127.0.0.1:8090` under **Change server address**.

### The browser shows Create Owner after an update

The new release changed the database layout, and the server rebuilt the database empty.
[Update Message Crate](/docs/user/features/owner/update/#when-the-database-layout-changes) describes what is gone and the way back.

## Logging in

### "this account is disabled"

The Owner has set the account's **Status** to **Disabled**.
The Owner sets it back to **Active** under **User Accounts**, in the account's [User Settings](/docs/user/features/owner/owner-home/#account).

### An account's password is forgotten

The Owner sets a new one under **User Accounts**, in the account's [User Settings](/docs/user/features/owner/owner-home/#account), with **Change password**.
The Owner doesn't need the old password.

### The Owner's password is forgotten

Nothing in the browser or the desktop app can set the Owner's password, because no account stands above the Owner.
The server program sets it from a command line on the computer that runs Docker.

```bash title="Set a new Owner password"
docker exec message-crate \
  message-crate-server reset-owner-password --password 'the-new-password'
```

The command prints the Owner's username and ends every Session the Owner had open.

### There is no Create Account on the login card

The login card offers **Create Account** only when the Owner has turned on **Let anyone who can reach this server create their own account** under [Server Settings](/docs/user/features/owner/owner-home/#server-settings).
With it off, the Owner creates every account with **Add account**.

## Desktop app

### Windows or macOS warns before the first run

The installers are not code-signed yet, so both systems warn that the publisher is unknown.
[Install the desktop app](/docs/user/try/install-the-desktop-app/#install) has the steps for each system.

### Import and Export are missing

**Import** and **Export** are in the desktop app only.
The browser doesn't show them.

The Owner doesn't have them in either, because the Owner holds no messages.
Importing needs an account, and that account needs **Import** on under **Message Permissions**.

## Import

### "The backup is encrypted — fill Encryption password."

The iPhone backup was made with encryption, and the Import form's **Encryption password** is empty.
The password is the one chosen when the backup was made.

### "This backup is not encrypted. Clear Encryption password."

The iPhone backup was made without encryption, and the Import form's **Encryption password** holds a value.
Import continues once the field is empty.

### A WhatsApp import doesn't stop when cancelled

A run can't be cancelled while `wtsexporter` is working. It ends when the program finishes.
[WhatsApp](/docs/user/import-sources/whatsapp/#limits) lists this with the other limits.

## ffmpeg, ffprobe and wtsexporter

The desktop app runs three programs it doesn't include: `ffmpeg` and `ffprobe` for **Convert** and **Compress & Convert**, and `wtsexporter` for a WhatsApp import.
Each time it starts, it downloads the ones that are missing into its Tools Directory, in the background, and nothing needs doing.
The entries below are for when that download fails, or when a computer has no download.

[**Settings → System**](/docs/user/features/settings/system/#media) shows the full path of the Tools Directory under **Media**, such as `/home/sam/message-crate/tools` on Linux, and one line per program: found, with its path, or why not.
The Import form names a program the chosen import needs and can't use, with the reason, **Try again** and a link to this page.
**Try again** checks the Tools Directory and downloads what is missing at once, instead of at the next start.

### Import can't find wtsexporter

A WhatsApp import can't start without `wtsexporter`, and the Import form says so.
The app runs it only from the Tools Directory, so a `wtsexporter` installed anywhere else on the computer isn't used.

When the download failed, the reason on the Import form says why, and [The download failed](#the-download-failed) says what to do about it.
**Try again** on the Import form downloads it again.

Without a working connection, a copy downloaded on another computer works too.
The app downloads release `0.13.0-mc.2` of Message Crate's fork of WhatsApp Chat Exporter, from [its release page](https://github.com/messagecrate/WhatsApp-Chat-Exporter/releases/tag/0.13.0-mc.2), one file per computer:

| Computer | File |
|---|---|
| Linux, Intel or AMD (x64) | `wtsexporter_linux_x64` |
| macOS, Apple silicon | `wtsexporter_macos_arm64` |
| macOS, Intel | `wtsexporter_macos_x64` |
| Windows, Intel or AMD (x64) | `wtsexporter_win_x64.exe` |
| Windows on ARM | `wtsexporter_win_arm64.exe` |

The file is renamed to `wtsexporter`, or `wtsexporter.exe` on Windows, and put in the Tools Directory.
On Linux and macOS it also needs permission to run:

```bash title="Let wtsexporter run (Linux and macOS)"
chmod +x ~/message-crate/tools/wtsexporter
```

The app keeps a file put there by hand when it is this pinned file, and replaces anything else once a download succeeds; [Settings → System](/docs/user/features/settings/system/#media) states the rule.

#### Linux on ARM

The fork publishes no `wtsexporter` for Linux on ARM, so the app has nothing to download there.
The Import form says the app has no download of it for this computer, and offers no **Try again**, because nothing can be downloaded.
A `wtsexporter` built from the fork's release with `pipx`, which needs Python 3, works instead, linked into the Tools Directory:

```bash title="Install wtsexporter on Linux on ARM"
pipx install --force "whatsapp-chat-exporter[android_backup,crypt15] @ https://github.com/messagecrate/WhatsApp-Chat-Exporter/archive/refs/tags/0.13.0-mc.2.tar.gz"
ln -sf "$(pipx environment --value PIPX_BIN_DIR)/wtsexporter" ~/message-crate/tools/
```

The app never replaces it there, because it has no file of its own for this computer.
When a WhatsApp import fails as soon as it starts the program, with `a link to a pipx install whose Python interpreter has gone`, the Python that install used was removed, and an older `pipx` can't repair the install in place: run `pipx uninstall whatsapp-chat-exporter` first, then the two lines above.

### ffmpeg or ffprobe not found

**Convert** and **Compress & Convert** need both `ffmpeg` and `ffprobe`.
Without them an import with either choice still starts, because Staging reads the original files and needs neither program.
At the Staging Review, the **Convert media** or **Compress media** button stays disabled until both are there.
[Attachments and media](/docs/user/features/messages/attachments-and-media/#ffmpeg) covers what each choice does.

When the download failed, the reason on the Import form and at the Staging Review says why, and [The download failed](#the-download-failed) says what to do about it.

The app looks on the system `PATH`, then in the Tools Directory, and takes both programs from the same one.
It downloads neither when both are on `PATH`, because a person who installed ffmpeg chose that copy.
So installing ffmpeg with the system's package manager is the first fix, and it puts both programs on `PATH`:

| System | Command |
|---|---|
| Linux (Debian, Ubuntu) | `sudo apt install ffmpeg` |
| macOS | `brew install ffmpeg` |
| Windows | `winget install -e --id Gyan.FFmpeg` |

The app finds them the next time it looks, without a restart, except on Windows: `winget` changes `PATH` only for programs started after it, so the app must be started again.
On macOS, an app opened from the Dock or Finder doesn't see the `PATH` Homebrew sets in a terminal, so it doesn't find the ffmpeg `brew` installs.
It downloads its own copies into the Tools Directory instead, and those are the ones used.

Without a working connection, copies downloaded on another computer work too.
The app downloads release `b6.1.1` of `ffmpeg-static` from [its release page](https://github.com/eugeneware/ffmpeg-static/releases/tag/b6.1.1), two files per computer:

| Computer | ffmpeg | ffprobe |
|---|---|---|
| Linux, Intel or AMD (x64) | `ffmpeg-linux-x64` | `ffprobe-linux-x64` |
| Linux on ARM | `ffmpeg-linux-arm64` | `ffprobe-linux-arm64` |
| macOS, Apple silicon | `ffmpeg-darwin-arm64` | `ffprobe-darwin-arm64` |
| macOS, Intel | `ffmpeg-darwin-x64` | `ffprobe-darwin-x64` |
| Windows, x64 or ARM | `ffmpeg-win32-x64` | `ffprobe-win32-x64` |

The release also has each file as a `.gz`, which is smaller and must be unpacked first.
The files are renamed to `ffmpeg` and `ffprobe`, or `ffmpeg.exe` and `ffprobe.exe` on Windows, and both put in the Tools Directory.
On Linux and macOS they also need permission to run:

```bash title="Let ffmpeg and ffprobe run (Linux and macOS)"
chmod +x ~/message-crate/tools/ffmpeg ~/message-crate/tools/ffprobe
```

Both go in the same place, because the app takes them from one place only.
With one on `PATH` and the other only in the Tools Directory, **Settings → System** reports both as not used and says why.
The app keeps files put there by hand when they are these pinned files, and replaces anything else once a download succeeds; [Settings → System](/docs/user/features/settings/system/#media) states the rule.

#### Windows on ARM

Windows on ARM gets the x64 ffmpeg and ffprobe, because `ffmpeg-static` publishes no ARM build for Windows, and Windows runs x64 programs under emulation.
When they don't run, **Settings → System** says the program doesn't run on this computer, and `winget install -e --id Gyan.FFmpeg` puts a copy on `PATH` that the app uses instead.

### The download failed

`<program> download failed` in **Settings → System**, and the same reason on the Import form, say why.
A run that waited for the download ends with an error that gives the reason after `The <program> download failed.`
A failed download is tried again the next time the app starts, or at once with **Try again** on the Import form.

| The reason starts with | What it means | What fixes it |
|---|---|---|
| `No connection to the download's server` | The connection to `github.com`, where every file comes from, failed or dropped: no internet, a firewall or proxy in the way, or a connection cut off partway through. | **Try again** once the computer is online, or a copy put in the Tools Directory by hand. |
| `The download's server answered`, then a status such as `503 Service Unavailable` | GitHub refused the request or had trouble. `404 Not Found` means the pinned file is no longer there. | **Try again** later. A `404 Not Found` that stays belongs in an issue on [GitHub](https://github.com/messagecrate/message-crate/issues), and meanwhile a copy put in the Tools Directory by hand is used. |
| `The downloaded file's checksum did not match the one this app carries` | The file that arrived isn't the one the app was built to accept, so the app deleted it. A proxy that rewrites downloads, or a damaged transfer, does this. | **Try again**. A mismatch that repeats belongs in an issue on [GitHub](https://github.com/messagecrate/message-crate/issues), with the two checksums the reason names. |
| `The file could not be written to the Tools Directory` | The disk is full, or the Tools Directory can't be written to. The reason ends with the operating system's words. | Free space on that disk, or give the account that runs the app permission to write to the Tools Directory, then **Try again**. |
| `ffmpeg in the Tools Directory doesn't run on this computer`, or ffprobe or wtsexporter | The file passed its checksum and is in place, and the computer won't run it. The app doesn't download it again, because the download would be the same file. | For ffmpeg and ffprobe, the system's package manager, above. For `wtsexporter`, `chmod +x` on Linux and macOS. On macOS, see [macOS blocks a program](#macos-blocks-a-program). |
| `The download was interrupted` | The download stopped partway through. | **Try again**. |
| `The download could not be started` | The app couldn't start the download at all. | Starting the app again. |

### macOS blocks a program

macOS can block a program that came from the internet, most often one downloaded by hand in a browser.
A blocked `ffmpeg` or `ffprobe` shows in **Settings → System** as a program that doesn't run on this computer.
A blocked `wtsexporter` shows as found, and the WhatsApp import stops with an error when it starts the program.

**System Settings → Privacy & Security** lists a program macOS blocked, with **Open Anyway**, which allows it.
The same is done from a terminal by removing the mark macOS puts on a downloaded file:

```bash title="Let macOS run the programs in the Tools Directory"
xattr -d com.apple.quarantine ~/message-crate/tools/*
```

**Try again** on the Import form, or starting the app again, then finds the program.

## Convert

### "Choose a different output directory."

**Convert** can't write into the directory it reads from.
The message goes away once **Output directory** names a different directory.

## Getting help

Problems not listed here belong in an issue on [GitHub](https://github.com/messagecrate/message-crate/issues).

A useful issue names the operating system, the **Version** shown under **Server Settings**, the kind of backup, and the exact error text.
It must leave out passwords, API Tokens, phone numbers, and message text, because issues are public.
