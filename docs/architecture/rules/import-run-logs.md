# Import Run logs: rules

The constraints the desktop app's logs must keep satisfying: each Import
Run's log and the app's own log beside it. How a run's log travels from its
first line to the Logs panel is in [Import Run logs](../import-run-logs.md);
this document is what a change must not break. Each rule has an id,
`RUNLOG-n`, which code comments, tests and reviews cite. A rule's id never
changes; a rule that stops holding is removed, and its id is not reused.

The decisions were made on #1340, #1665 and #2185; the code is
`crates/libs/log-lines` (the line format, shared with the server's log),
`crates/core/message-crate-core/src/run_log.rs` (a run log's lines),
`src-tauri/src/app_directories.rs` (writing a run log),
`src-tauri/src/app_log.rs` (writing the desktop app's log) and
`src-tauri/src/run_logs.rs` (listing and reading both).

*The server's own log is a separate thing, with its own rules: [The server's
log](server-log.md).*

## RUNLOG-1 Who reads which log

| Log | Where it is | Who reads it | Where |
|---|---|---|---|
| The server's log | The Data Directory of the server | The owner | Owner Home → **Logs**, in a browser or the desktop app |
| An Import Run's log | The Logs Directory of the computer that ran the import | The owner, and the account that ran the run | Owner Home → **Logs** (the owner), and the run's row in Settings → Storage (the account), in the desktop app on that computer |
| The desktop app's log | The Logs Directory of the computer the app runs on | The owner | Owner Home → **Logs**, as **Desktop app**, in the desktop app on that computer |

A browser reads the server's log only, because a run log never leaves the
computer that ran the import.

Why the owner reads every run log: a run log holds the import's metadata,
what went wrong, what succeeded, counts, and the names of the files the run
read (RUNLOG-8). The owner already sees each
account's Import Runs and what each account holds
(`docs/adr/0008-the-owner-holds-no-messages.md`), so the run's log is the same
information at finer grain. Decided on #1665, which changed decision 5 on
#1340 ("a run log, the account that ran the import").

Why an account reads only its own: one account is not shown another
account's runs anywhere else either.

## RUNLOG-2 Kept on this computer, never deleted

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

## RUNLOG-3 The account line

The first line of a run's log names the run, the account that runs it, the
server's address and the Message Crate's id, written once the server has
created the run and before Staging writes anything:

```
2026-10-04T14:30:00.123456Z  INFO Import Run 42 by account 7 on http://127.0.0.1:8080, Message Crate 3f2a9c…
```

The listing reads it to decide who sees the log. The owner sees every log; an
account sees a log whose line names its own id and the id of the Message
Crate it is signed in to, because account 7 of another Message Crate is
another account. A log with no account line is the owner's alone.

The line is also what ties a run's directory in the Staging Directory to its
run (#2822). When an account opens Import, the desktop app lists the run
directories whose log names that account and the Message Crate it is signed
in to, asks the server how each run stands, and deletes the directory of
every run the server reports as closed. It adds one `INFO` line to the run's
log naming the run and the directory. A run that is running, a run the
server does not report, and a directory whose log has no account line all
keep their directory.

Why the log and not the server's `run_dir`: the server holds the path of a
running run only for Resume, and a directory is on one computer. The log is
on the computer that holds the directory, and it names the run.

Why the Message Crate's id and not the address: two Message Crates can answer
at one address. The desktop app's own server and a Docker one both answer at
`http://127.0.0.1:8080`, and a database rebuilt with `create-database` starts
its account and Import Run ids at the same numbers again. The server writes
the id (`GET /v1/server`, `id`) when its database is made and never changes
it. The address is in the line for a person to read, and one address can be
written several ways (`localhost` or `127.0.0.1`), so nothing compares it.

## RUNLOG-4 The desktop app's log

What the desktop app does and fails at outside an Import Run goes to one log
of its own, `desktop-app.log` in the Logs Directory: an event no window
received, a program in the Tools Directory whose record could not be written,
a server whose processes will not end with it, and the warnings the Apple
Messages Reader writes during the Import screen's identity check, which runs
before any Import Run exists. Its lines are in the format of RUNLOG 5, at
`WARN`, because each is something the app went on without. Nothing deletes
it, and every start of the app writes on into it. It is made when its first
line is written, so an app with nothing to say has none.

Why: a desktop starts the app with standard error thrown away, so a line
written only there was lost. The server's log and each run's log had a
place to go, and the app's own decisions outside a run had none
([#2185](https://github.com/messagecrate/message-crate/issues/2185)).

One line still goes to standard error: the one that says the Message Crate
Directory could not be made. Why: with no Message Crate Directory there is
no Logs Directory to write it in. A line written before the app has named
the Logs Directory, or one the log cannot take, goes there too, as the one
place left.

The owner reads it, in the desktop app on that computer, as **Desktop
app** in the Logs panel. It names no account, so the listing gives it to
the owner alone, as it does a run log with no account line. Like a run
log, it is a plain file that the computer's own user can open with or
without Message Crate (RUNLOG-7).

A line of it names only paths inside Message Crate's own directories, such
as the Tools Directory and the Scratch Directory. The path of a backup the
identity check reads is written as "the backup", in the form the window
gave it, which is the form the Reader names it in. The Reader's other lines
from the identity check are not written at all: they say how far it got,
and one of them names the phone, such as "Decrypted iOS backup: Sam's
iPhone". Why: the log is offered to the owner of whichever Message Crate
the app is signed in to, and a backup's path or a phone's name can name the
person whose phone it is.

## RUNLOG-5 One line per event, with its time and level

Each line starts with its time in UTC (RFC 3339, microseconds) and its level,
`ERROR`, `WARN` or `INFO`, then the text: the format of the server's log, so
one viewer reads both and the level filter means the same in each. Text that
holds a line break, such as the Upload's summary, is written as one line per
part, each with its time and level. Every other control character is
escaped as in the server's log, such as ESC as `\x1b` (`server-log.md`,
LOG-7): a file name or a tool's output can carry one, and a person
reading the log in a terminal would otherwise have it act on the screen.

- `ERROR`: what stopped, a stage that failed, a conversation not sent, an
  Import Errors row of kind `error`.
- `WARN`: what the run went on without, an attachment not uploaded, an
  Import Errors row of kind `skip`, a digest that did not match, a session
  the server stopped accepting. Also the whole output of a failed
  `wtsexporter` run, one line per line of it, written before the failure is
  turned into the error the window shows. Why: a full disk is shown as the
  free-space sentence for the Scratch Directory, and the output is the only
  record of which disk filled, or of a warning ahead of a different error
  ([#1938](https://github.com/messagecrate/message-crate/issues/1938)).
- `INFO`: what the run did.

## RUNLOG-6 Reading it

The desktop app reads a run log the way the server reads its own, with the
same code (`crates/libs/log-lines`): the newest lines first, walking back
from the id of the last line held a chunk at a time, filtered by level and
text. A line's id is its byte offset in the file, which never moves, because
the log is only ever appended to. A line still being written, with no line
break yet, is left out until it is whole. A download is the file byte for
byte.

A log written before its lines carried a time and a level has no line the
viewer can read. The viewer says so and offers the download, rather than
saying no line matches the filter.

## RUNLOG-7 A filter, not a guard

Who reads which log decides what each screen offers, not who can open a file.
The window tells the desktop app which account is signed in, on which Message
Crate, and whether it is the owner, and the desktop app takes its word. A run
log is a plain file in the Logs Directory, which any program run by the
computer's user can open. No other user of the computer can: the desktop app
makes the Logs Directory with mode `0700` and takes group and other access
off one that has it, each time it opens a run's log, because a run log names
the phone numbers and email addresses in its file names (RUNLOG-8). Windows
has no such modes, and there the Logs Directory takes the access of the
Message Crate Directory it is in, which is under the user's own profile
directory.

Why that is enough: everything on the computer runs as one user of the
operating system, who can read the Logs Directory with or without Message
Crate. The rule keeps one account's runs off another account's screens, as
the server keeps one account's Import Runs from another.

## RUNLOG-8 What a line holds

Like a line of the server's log (`server-log.md`, LOG-18),
a run log's line never holds a password, a Session token or an API token,
message text, or attachment bytes. Unlike a line of the server's log, it may
carry a conversation's identities in the file names it gives (below).

A run log does name the conversation files and attachments the run read, such
as `+15555550101.jsonl` and `attachments/IMG_1234.jpg`, because that is what
explains a failure. A conversation file is named for the people in it, so its
name can carry a phone number, an email address or a group's title. The owner
reads those names with the rest of the log, as ADR 0008 says for run logs.
Why: the log is a plain file in the Logs Directory. The operating-system user
who ran the import can open it with or without Message Crate (RUNLOG-7), and
the owner reads it in the desktop app running as that user. While the run
lasts, its directory in the Staging Directory holds the same names. A neutral
name on the owner's screen would protect nothing
([#1990](https://github.com/messagecrate/message-crate/issues/1990)).
