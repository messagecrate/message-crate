# The server's log

What the server writes about its own work, where it keeps it, how much it
keeps, and who reads it. The decisions were made on #1340; the code is
`crates/server/server/src/logging.rs` and its `files.rs` and `read.rs`.

*The routes are `GET /v1/server/log-lines`, `GET /v1/server/log-files` and
`GET /v1/server/log-files/{id}`; their rules are in
[The HTTP interface](http-api.md).*

## Rotating files in the Data Directory

`serve` writes every line of its log to stderr, as it always has, and to files
in the `logs` directory of the Data Directory: `server-000001.log`,
`server-000002.log`, and so on. The newest file takes every line. A Message
Crate in Docker and the one the desktop app starts write the same files the
same way, because both are `serve` and both name a Data Directory.

Why files: a log has to be there when the database is not. A server whose
database fails to open, is locked, or is full still writes the lines that say
so, and a log kept in the database would lose exactly those. Files also keep
log writes off the database, which every request already writes to, so a busy
server does not slow its own imports by describing them.

Why the Data Directory: it is the one directory a Message Crate is sure to
have and to be allowed to write, in Docker (the volume) and in the desktop app
(its app-data directory) alike, and a copy of it is already the whole
installation.

Rejected: stderr alone. Docker keeps it only as long as the container, and the
desktop app has no window to show it in, so the owner of either had no way to
read it.

Rejected: a table in the database. It is the one store that is missing when
the log is needed most.

The other subcommands (`import`, `reset-demo`, `process-assets`, and the rest)
write to stderr only. They are run by hand from a shell that already shows
their output, and they are not the server.

## One line per event

Each file holds one event per line, in the stderr format without colour: the
time in UTC (RFC 3339, microseconds), the level, the request or work the event
belongs to, and what it says. A line break inside an event, from an error
chain or a message that holds one, is written as the two characters `\n`.
Every other control character is escaped: U+0000 to U+001F and U+007F as
`\x` and two hex digits (ESC as `\x1b`), and U+0080 to U+009F as `\u{..}`
(CSI as `\u{9b}`). A backslash is written as it is, so the escape is not
undone: a line holding `\x1b` may name a file whose name held those four
characters, as a line holding `\n` may have held those two.

Why: every line then starts with its time and level, so a line read from
anywhere in a file, backwards or forwards, is a whole event, and a person
reading the downloaded file with `grep` gets whole events too. A control
character written as it is would act on the terminal of a person reading the
file with `cat`, `less -r` or `tail`, clearing the screen or rewriting a line,
and a file name in a backup is enough to carry one
([#2275](https://github.com/messagecrate/message-crate/issues/2275)).
`tracing-subscriber` escapes them in an event's message but not in a field
written with `%`, so the file writer escapes them all. A terminal that
decodes UTF-8 can act on U+0080 to U+009F as well, and U+009B alone starts a
sequence as ESC `[` does.

## A refused request names its problem type

Every request leaves an `INFO` line with its method, path, status and
latency. A request the server refuses with a `4xx` leaves one more line,
written where its problem document is built, with the problem type's slug
and the status (`problem=invalid-credentials status=401`). It is at `WARN`
for `401 Unauthorized`, `403 Forbidden` and `429 Too Many Requests`, and at
`INFO` for every other refusal. The level follows from the status, in
`crates/server/server/src/problem.rs`, where each problem type is declared.
The problem's `detail` is never written.

Why `WARN` for those three: a run of them is what a password guesser or an
API token that lost its rights looks like, and the owner's Logs panel opens
at warnings and up. A `404 Not Found` or a refused search is a client asking
for something that is not there, which the owner has nothing to do about.

Why not `detail`: it can repeat what a person typed, such as a search word or
a value that broke a rule, and a line holds only outcomes (below). The slug
says which problem it was, and the request id joins the line to the answer
the client received.

## Trimmed by size only: 5 files of 50 MB

A file is closed and the next one started before a line would carry it past
50 MB (50,000,000 bytes). At most 5 files are kept, 250 MB in all: when one
more starts, the oldest is deleted. A restarted server writes on the end of the
newest file while it has room. A write that fails part-way, on a full disk,
is cut off the file again, so the file still ends where a line does and a full
disk loses only the lines it could not hold. A file a stopped server left
ending part-way through a line is not written on again: the next start begins
a new file, so no line is joined to the half line before it. Nothing is
deleted for its age.

Why size: the disk is what a log can run out of, and a size limit is the only
one that bounds it. A Message Crate on a small disk or a Raspberry Pi cannot
let a quiet week and a busy import day differ by gigabytes.

Why 5 of 50 MB: the figures are the maintainer's, on #1340. A 50 MB file is
small enough to download and open in an editor, and five of them hold the log
to 250 MB whatever level `RUST_LOG` sets.

Rejected: deleting by age (keep 14 days). A busy day still fills the disk, and
a quiet server throws away the lines that would explain its last failure.

A file is never renamed. Its number names it for as long as it exists, which
is what lets a line be addressed by where it is (below) while the log grows.

## Reading it: newest first, from a line

`GET /v1/server/log-lines` answers the newest lines first, with `level` (that
level and the more severe ones) and `text` (lines whose text, after the time
and the level, holds it, ignoring case).
A line's `id` is where it starts: its file's number times 2³², plus its byte
offset in the file, so a larger id is a newer line. `after={id}` reads the
lines older than that line. `GET /v1/server/log-files` lists the files, and
`GET /v1/server/log-files/{id}` downloads one whole, as `text/plain`, at the
length it had when the download started.

Why from a line, and not an offset: the server writes lines while the owner
reads them, its own answers included, so a count from the newest line moves
between pages. An id names a line that does not move.

A line that does not start with a time and a level is left out of the lines.
Only a write cut short leaves one, and the downloaded file still has it.

## The owner reads it, nobody else

The three routes take the owner's Session and refuse every other credential
with `403 Forbidden`.

Why the owner: the log is about the whole installation, every account's
requests in one stream, and the owner is the one who runs the installation
and is asked when it fails. An account holder is not shown other accounts'
requests, so no account reads it, and an API token never reads anything but
its account's own data.

The rule for an Import Run's log is separate: it stays on the computer that
ran the import, and the owner and the account that ran the import read it
there ([Import Run logs](import-run-logs.md)).

Owner Home's **Logs** panel shows the log a page at a time: the newest lines
first, older ones read as the list scrolls, filtered by level (errors,
warnings and up, or everything, opening at warnings and up) and by the text
typed into its search. Each file downloads as it is. Nothing reads new lines
on its own: the panel shows the log as it was when it was read.

## What a line never holds

A line never holds a password, a Session token or an API token, hashed or
not, message text, attachment bytes, or a contact's name or identities. The
owner reads the log, and the owner never reads content
(`docs/adr/0008-the-owner-holds-no-messages.md`), so the log may hold only
what the owner may see: ids, counts, sizes, times, routes and outcomes.

How it holds: the line each request writes names its path and query, and the
query keeps the value of a parameter only when it is a number, an id or a
fixed word (`after`, `around`, `before`, `deleted_account_id`, `level`,
`limit`, `mode`, `offset`, `sort`, `status`). Every other value is written as
`[hidden]`: `q`, a search over what the messages say; `media_link`, a
credential; the log's own `text` filter; and any parameter added later, until
someone adds it to the list. Every event the server writes is held to the same
rule by whoever writes it.

`crates/server/server/tests/server_log.rs` checks it: it runs `serve` at
`RUST_LOG=trace`, has an owner, an account and an API token log in, import a
conversation with a named contact and an attachment, search, open the
attachment through a media link, change a password, log in with a wrong
password and send a search the server refuses, and then fails if any of
those passwords, tokens (or their hashes), the message's words, the
attachment's bytes, or the contact's name, phone number or email address is in
any file of the log.
