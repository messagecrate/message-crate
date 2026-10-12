# Outbound connections

What Message Crate connects to beyond the computer it runs on. The published
statement is on [What Message Crate is](../src/content/docs/docs/user/try/what-is-message-crate.md),
"What leaves the computer"; this document is the rule behind it (#2273).

## The rule

Message Crate sends no telemetry, crash reports, or usage data. Exactly two
outbound connections exist, both from the desktop app:

- **The Message Crate server the app uses.** The desktop app, `message-crate-import`,
  and `message-crate-export` talk to the server address they are given and to
  no other host.
- **GitHub, for the Tools Directory.** `src-tauri/src/tool_downloads.rs`
  downloads ffmpeg, ffprobe, and wtsexporter from `GITHUB`
  (`https://github.com`), the one outbound host written in the code. The check
  runs in the background at every start and downloads a program only when it
  is missing or different from the pinned release, and each file is checked
  against a SHA-256 carried in `tool_downloads/pins.rs`, which takes
  wtsexporter's from the WhatsApp exporter's `wtsexporter/pin.rs`. The rules for that
  download are in
  [ADR 0019](../adr/0019-the-desktop-app-downloads-the-programs-it-needs.md).

The server and the website make no outbound connections. The website's links
to messagecrate.app and GitHub are links: nothing is fetched from those hosts
until a person clicks one.

Why: Message Crate holds a person's private message history, and whether it
reports anything about that history is the first thing a person deciding to
trust it asks. A rule written here turns any new outbound connection, whether
a crash reporter, an update check, or a dependency that phones home, into a
visible change to this document instead of a side effect nobody noticed.

## Changing the rule

A change that adds an outbound host, or makes an existing one carry more than
a file request, changes this document and the published section in the same
pull request, and says so in the changelog.
