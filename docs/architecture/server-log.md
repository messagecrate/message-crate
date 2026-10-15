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

Only the operating-system user the server runs as can read the files. The
server makes the Data Directory, and the `logs` directory in it, with mode
`0700`, and takes group and other access off a Data Directory that already
has it, each time it opens the database. Why: the log names every account's
routes and ids, and the Data Directory beside it holds every account's
attachments, so another user of the same computer must not read either. A
directory with no access for anyone else keeps everything inside it from
them, so the files in it keep the mode the umask gives them. Windows has no
such modes; there the directories take the access of the one they are made
in, which for the desktop app is the user's own app-data directory. Rules:
`crates/server/server/src/owner_only.rs`.

Rejected: stderr alone. Docker keeps it only as long as the container, and the
desktop app has no window to show it in, so the owner of either had no way to
read it.

Rejected: a table in the database. It is the one store that is missing when
the log is needed most.

`serve` opens the log before it touches the database, then writes every
line through it: where the log is, the database it opened and its journal
mode, a part-built Demo Account it removed, the attachment limits, a SIGTERM
it cannot catch, and the Demo Account build it runs on a new database or for
the owner. An error that stops `serve` after the log is open is written at
`ERROR` with its whole chain (`logging::write_failure`). Such errors include
a database that cannot be opened, another server holding the operation lock,
and an address that cannot be bound. When `RUST_LOG` leaves out `ERROR`
lines, the error and a panic go to stderr as Rust writes them instead, so
neither is lost. One line is on stderr alone: the one that says
where the server listens (`LISTENING_LINE` in
`crates/libs/serve-protocol`), which the desktop app and the tests read
there by how it starts.

A line that cannot be written to stderr is dropped, and the file still gets
it. Why: the desktop app reads its server's stderr through a pipe, and an
app that crashes takes the pipe's read end with it. Reporting the failed
write would panic, so every request would fail until the server noticed the
app was gone
([#1934](https://github.com/messagecrate/message-crate/issues/1934)).

Why: a Message Crate that will not start, or misbehaves from its first
minute, is when the owner needs these lines, and stderr is the place the
owner of Docker or the desktop app cannot read
([#2079](https://github.com/messagecrate/message-crate/issues/2079),
[#2018](https://github.com/messagecrate/message-crate/issues/2018)).

The other subcommands (`import`, `reset-demo`, `process-assets`, and the rest)
write to their own standard output and standard error only, never to the
server's log: their progress is their output (`crate::progress::Progress`).
They are run by hand from a shell that already shows their output, and they
are not the server.

## A panic is written as an `ERROR` line

A panic on any thread of the server, in a request handler, the media pass or
any other work, leaves one `ERROR` line, `A thread of the server panicked`,
with the panic's message and the file and line it happened at. The panic hook
`logging::init` installs writes it, under the span the panic happened in, so
a handler's panic carries its request's id. Rust's own hook, which writes to
stderr alone, runs as well only when `RUST_BACKTRACE` asks for a backtrace
or `RUST_LOG` leaves out the `ERROR` line.
A handler that panics is answered with a `500 Internal Server Error`
([The HTTP interface, "Failures"](http-api.md#failures)), which writes its
own `ERROR` line as every `500` does, so such a request leaves two.

Why: a panic was the one failure that reached stderr alone, so the owner's
Logs panel showed nothing for the request that broke
([#2060](https://github.com/messagecrate/message-crate/issues/2060),
[#2086](https://github.com/messagecrate/message-crate/issues/2086)).

The panic's message is written as Rust gives it. The server's own panics
carry fixed text, but a panic inside the standard library can quote a value,
such as the text a string was sliced from. That is the one place a line can
hold what the rule below keeps out, and it is accepted: a panic is a bug, and
its message is what makes it possible to fix.

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

## A routine answer is written at `DEBUG`

Every request leaves an `INFO` line with its method, path, status and
latency, except a successful `GET /health` and a successful file of the
website, which leave theirs at `DEBUG`. A failed probe and a file the
website doesn't have stay at `INFO`.

Why: the web app asks `/health` every 30 seconds in each open tab, and
loading the website asks for one file per asset. At `INFO` those lines bury
the requests that did something in the owner's Logs panel.

The health route and the website's fallback mark their responses
(`Routine` in `crates/server/server/src/server.rs`), and the trace layer
reads the mark. So the rule follows the route that answered, and no path is
written out a second time.

## A refused request names its problem type

A request the server refuses with a registered problem type (a `4xx`, or
one of the two `503 Service Unavailable` types, `server-stopping` and
`database-busy`) leaves one more line, written where its problem document is
built, with the problem type's slug and the status
(`problem=invalid-credentials status=401`). It is at `WARN` for
`401 Unauthorized`, `403 Forbidden` and `database-busy`, and at `INFO` for
every other refusal. The level follows from the problem type, by its status
and, for `database-busy`, by name, in `crates/server/server/src/problem.rs`,
where each problem type is declared.
The problem's `detail` is never written. A `database-busy` line also
carries the error's chain as `error`, as the `ERROR` line of a `500` does
(below).

Why `WARN` for those three statuses: a run of them is what a password
guesser or an API token that lost its rights looks like, and the owner's Logs
panel opens at warnings and up. Why `WARN` for `database-busy`: a run of it
says the server has more work than its database keeps up with, or that
something holds the database's lock and never lets it go, while one of them
is no bug, so it is not an `ERROR`. Why the chain: it names the step that
met the lock, so a lock that never clears can be told from a busy spell. A
`404 Not Found` or a refused search is a client asking for something that is
not there, which the owner has nothing to do about.

Why not `detail`: it can repeat what a person typed, such as a search word or
a value that broke a rule, and a line holds only outcomes (below). The slug
says which problem it was, and the request id joins the line to the answer
the client received.

A refused login leaves one more `WARN` line, beside its Audit Trail entry,
with the account's id (none for a username that names no account), the
reason (`unknown_username`, `wrong_password` or `account_disabled`) and the
connecting app and its Build.

Why: the Audit Trail is read one account at a time, so a run of password
guesses showed in the owner's Logs panel only as `invalid-credentials` lines
that do not say which account or why. Why not the username: it is what a person
typed, and a guesser's list of usernames is not an outcome.

## A failed request names the step that failed

A request that fails with `500 Internal Server Error` leaves one `ERROR`
line, written where its problem document is built, with the error's whole
chain, outermost first. A busy database answers `database-busy` instead,
and its refusal line, above, carries the chain at `WARN`. Each database
statement adds the step it does to that chain, in a few words (`read the
attachments of a page of messages`), so the line reads as the step and then
the database's own message. The step is a fixed phrase: it never holds a
value bound into the statement.
The trace layer writes no second `ERROR` line for the `5xx`, only the
response's `INFO` line.

A source whose text the error before it already is, or already ends with
after a `: `, is left out of the chain (`error_chain` in
`crates/server/server/src/server.rs`). A cause whose words only happen to
end the step's phrase is kept, because it still says something new. Why:
`sqlx`'s database error writes "error returned from database:" and the
database's message, then gives the same message again as its source, so
the line would name it twice.

Why: the request's span already names the route, but a route that runs
several statements gives the same database message for each, and a line
that does not say which statement failed sends the owner to the code to
guess. Why a fixed phrase: a bound value can be a search word or a message's
text, which a line never holds (below).

`db/conversation_messages.rs`, `db/conversations.rs` and
`db/contacts/read.rs` name every statement they run; the other `db/`
modules do not all name theirs yet
([#2667](https://github.com/messagecrate/message-crate/issues/2667)).

## A request's lines carry its id, spawned work included

Every line written while the server answers a request carries the request's
id, method and path, from the tracing span the request runs under. That
holds for a line written by work the request runs in a task of its own or
on the blocking pool, such as removing files or hashing an import's
messages: the server starts that work with `request_id::spawn_in_current_span` or
`request_id::spawn_blocking_in_current_span`, which run it under the span and subscriber of
the code that started it. `crates/server/server/clippy.toml` refuses
`tokio::spawn` and `tokio::task::spawn_blocking` in the server, so a new
spawn cannot leave them out.

Why: a tracing span does not cross a spawn on its own, and a warning that
names no request cannot be matched to the request that caused it or to the
error its client was shown
([#2186](https://github.com/messagecrate/message-crate/issues/2186)).

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

The path and query a line names are cut at 2,048 bytes, on a character
boundary, and end in `...[cut]` when they are (`logged_uri` in
`crates/server/server/src/server.rs`). Why: a request needs no credential to
leave its line, and a URI can be 64 KiB long, so without the cut anyone who
can reach the port could fill the 250 MB the log keeps with a few thousand
requests and push every other line off the disk
([#2088](https://github.com/messagecrate/message-crate/issues/2088)).

`crates/server/server/tests/server_log.rs` checks it: it runs `serve` at
`RUST_LOG=trace`, has an owner, an account and an API token log in, import a
conversation with a named contact and an attachment, search, open the
attachment through a media link, change a password, log in with a wrong
password and send a search the server refuses, and then fails if any of
those passwords, tokens (or their hashes), the message's words, the
attachment's bytes, or the contact's name, phone number or email address is in
any file of the log.
