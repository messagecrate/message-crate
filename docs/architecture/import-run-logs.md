# Import Run logs

Where the desktop app keeps each Import Run's log, what a line of it holds,
and who reads it. The decisions were made on #1340 and #1665; the code is
`crates/core/message-crate-core/src/run_log.rs` (the line format),
`src-tauri/src/app_directories.rs` (writing) and `src-tauri/src/run_logs.rs`
(listing and reading).

*The server's own log is a separate thing, with its own rules: [The server's
log](server-log.md).*

## Who reads which log

| Log | Where it is | Who reads it | Where |
|---|---|---|---|
| The server's log | The Data Directory of the server | The owner | Owner Home → **Logs**, in a browser or the desktop app |
| An Import Run's log | The Logs Directory of the computer that ran the import | The owner, and the account that ran the run | Owner Home → **Logs** (the owner), and the run's row in Settings → Storage (the account), in the desktop app on that computer |

A browser reads the server's log only, because a run log never leaves the
computer that ran the import.

Why the owner reads every run log: a run log holds the import's metadata,
what went wrong, what succeeded, and counts. The owner already sees each
account's Import Runs and what each account holds
(`docs/adr/0008-the-owner-holds-no-messages.md`), so the run's log is the same
information at finer grain. Decided on #1665, which changed decision 5 on
#1340 ("a run log, the account that ran the import").

Why an account reads only its own: one account is not shown another
account's runs anywhere else either.

## Kept on this computer, never deleted

Staging, Media and Upload each write their lines into one log per run in the
Logs Directory, named for the run's directory in the Staging Directory:
`staging-whatsapp-261004-143000` logs to `import-whatsapp-261004-143000.log`.
A resumed run writes on into the same log. Nothing deletes a log, so it
outlives the run's directory, which is deleted when the run ends.

Why: the run's directory holds the staged messages and goes when the run
ends, and the log is what explains the run afterwards. The long-term record of
a run is still the database, its Import Errors and notes; the log is the whole
of what the run said.

Why not upload it: a run log is the computer's record of its own work, and
the server already holds what is worth keeping from it.

## The account line

The first line of a run's log names the run, the account that runs it and
the server, written once the server has created the run and before Staging
writes anything:

```
2026-10-04T14:30:00.123456Z  INFO Import Run 42 by account 7 on http://127.0.0.1:8080
```

The listing reads it to decide who sees the log. The owner sees every log; an
account sees a log whose line names its own id on the server it is signed in
to, because account 7 of another server is another account. A log with no
account line is the owner's alone.

## One line per event, with its time and level

Each line starts with its time in UTC (RFC 3339, microseconds) and its level,
`ERROR`, `WARN` or `INFO`, then the text: the format of the server's log, so
one viewer reads both and the level filter means the same in each. Text that
holds a line break, such as the Upload's summary, is written as one line per
part, each with its time and level.

- `ERROR`: what stopped, a stage that failed, a conversation not sent, an
  Import Errors row of kind `error`.
- `WARN`: what the run went on without, an attachment not uploaded, an
  Import Errors row of kind `skip`, a digest that did not match, a session
  the server stopped accepting.
- `INFO`: what the run did.

## Reading it

The desktop app reads a run log the way the server reads its own: the newest
lines first, filtered by level and text, a page at a time from the id of the
last line held. A line's id is its byte offset in the file, which never moves,
because the log is only ever appended to. A download is the file as it is.

## What a line holds

ADR 0008's line holds for a run log as for the server's: a line never holds a
password, a Session token or an API token, message text, or attachment bytes.

A run log does name the conversation files and attachments the run read, such
as `+15555550101.jsonl` and `attachments/IMG_1234.jpg`, because that is what
explains a failure. A conversation file is named for the people in it, so its
name can carry a phone number, an email address or a group's title. Whether
the owner may read such a name is open in #1990.
