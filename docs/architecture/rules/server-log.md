# The server's log: rules

The constraints the server's log must keep satisfying. What the log is and
how a line travels from the event to the owner's screen is in
[The server's log](../server-log.md); this document is what a change must
not break. Each rule has an id, `LOG-n`, which code comments, tests and
reviews cite. A rule's id never changes; a rule that stops holding is
removed, and its id is not reused.

Each rule carries its reason and, where one was weighed, the alternative
turned down. Most were decided on #1340; the others name their issue.

## LOG-1 Keep the log in rotating files in the Data Directory, and on stderr

`serve` must write every line of its log to stderr and to files in the
`logs` directory of the Data Directory, named `server-000001.log`,
`server-000002.log`, and so on, the newest taking every line. The server in
Docker and the one the desktop app starts must write the same files the
same way: both are `serve`, and both name a Data Directory.

Why files: a log has to be there when the database is not. A server whose
database fails to open, is locked, or is full still writes the lines that
say so, and a log kept in the database would lose exactly those. Files also
keep log writes off the database, which every request already writes to.

Why the Data Directory: it is the one directory a Message Crate is sure to
have and to be allowed to write, in Docker and in the desktop app alike, and
a copy of it is already the whole installation.

Rejected: stderr alone. Docker keeps it only as long as the container, and
the desktop app has no window to show it in, so the owner of either had no
way to read it (#2079, #2018). Rejected: a table in the database, the one
store that is missing when the log is needed most.

Code: `crates/server/server/src/logging.rs`, `logging/files.rs`.

## LOG-2 Only the server's own user may read the log directory

The server must make the Data Directory and the `logs` directory in it with
mode `0700`, and must take group and other access off a Data Directory that
already has it, each time it opens the database. The files inside keep the
mode the umask gives them, because a directory nobody else can enter keeps
everything in it from them. Windows has no such modes; there the directories
take the access of the directory they are made in.

Why: the log names every account's routes and ids, and the Data Directory
beside it holds every account's attachments, so another user of the same
computer must not read either.

Code: `crates/server/server/src/owner_only.rs`.

## LOG-3 Open the log before the database, and write every start-up failure to it

`serve` must open the log before it touches the database and write every
line of its start through it: where the log is, the database it opened and
its journal mode, a part-built Demo Account it removed, the attachment
limits, a SIGTERM it cannot catch, and the Demo Account build. An error that
stops `serve` after the log is open must be written at `ERROR` with its
whole chain (`logging::write_failure`): a database that cannot be opened,
another server holding the operation lock, an address that cannot be bound.
When `RUST_LOG` leaves out `ERROR` lines, the error and a panic must still
reach stderr as Rust writes them. One line is on stderr alone: the one that
says where the server listens (`LISTENING_LINE` in
`crates/libs/serve-protocol`), which the desktop app and the tests read
there.

Why: a Message Crate that will not start, or misbehaves from its first
minute, is when the owner needs these lines, and stderr is the place the
owner of Docker or the desktop app cannot read (#2079, #2018).

Code: `crates/server/server/src/logging.rs`, `serve.rs`.

## LOG-4 A line stderr cannot take is dropped; the file still gets it

A write to stderr that fails must be dropped without a report, and the
same line must still reach the file.

Why: the desktop app reads its server's stderr through a pipe, and an app
that crashes takes the pipe's read end with it. Reporting the failed write
would panic, so every request would fail until the server noticed the app
was gone (#1934).

Code: `crates/server/server/src/logging.rs`, `init`.

## LOG-5 No subcommand but `serve` writes the server's log

`import`, `reset-demo`, `process-assets` and the other subcommands must
write to their own standard output and standard error only. Their progress
is their output (`crate::progress::Progress`).

Why: they are run by hand from a shell that already shows their output, and
they are not the server.

Code: `crates/server/server/src/progress.rs`.

## LOG-6 A panic is written as an `ERROR` line under its span

A panic on any thread of the server must leave one `ERROR` line, `A thread
of the server panicked`, with the panic's message and the file and line it
happened at, under the span the panic happened in, so a handler's panic
carries its request's id. Rust's own hook, which writes to stderr alone,
runs as well only when `RUST_BACKTRACE` asks for a backtrace or `RUST_LOG`
leaves out `ERROR`. A handler that panics is answered with `500 Internal
Server Error`, which writes its own `ERROR` line (LOG-11), so such a request
leaves two.

The panic's message is written as Rust gives it. A panic inside the
standard library can quote a value, such as the text a string was sliced
from. That is the one place a line can hold what LOG-18 keeps out, and it is
accepted: a panic is a bug, and its message is what makes it possible to
fix.

Why: a panic was the one failure that reached stderr alone, so the owner's
Logs panel showed nothing for the request that broke (#2060, #2086).

Code: `crates/server/server/src/logging.rs`, the hook `init` installs.

## LOG-7 One event per line, every control character escaped

Each file must hold one event per line, in the stderr format without color:
the time in UTC (RFC 3339, microseconds), the level, the request or work
the event belongs to, and what it says. A line break inside an event must
be written as the two characters `\n`. Every other control character must
be escaped: U+0000 to U+001F and U+007F as `\x` and two hex digits (ESC as
`\x1b`), and U+0080 to U+009F as `\u{..}` (CSI as `\u{9b}`). A backslash is
written as it is, so the escape is never undone.

Why: every line then starts with its time and level, so a line read from
anywhere in a file, backwards or forwards, is a whole event, and `grep` on
the downloaded file gets whole events too. A control character written as
it is would act on the terminal of a person reading the file with `cat`,
`less -r` or `tail`, and a file name in a backup is enough to carry one
(#2275). `tracing-subscriber` escapes them in an event's message but not in
a field written with `%`, so the file writer escapes them all. A terminal
that decodes UTF-8 can act on U+0080 to U+009F as well.

Code: `crates/server/server/src/logging/files.rs`; the line format is
`crates/libs/log-lines`.

## LOG-8 A routine answer is written at `DEBUG`

Every request must leave an `INFO` line with its method, path, status and
latency, except a successful `GET /health` and a successful file of the
website, which leave theirs at `DEBUG`. A failed probe and a file the
website does not have stay at `INFO`. The health route and the website's
fallback mark their responses (`Routine`), and the trace layer reads the
mark, so the rule follows the route that answered and no path is written
out a second time.

Why: the web app asks `/health` every 30 seconds in each open tab, and
loading the website asks for one file per asset. At `INFO` those lines bury
the requests that did something in the owner's Logs panel.

Code: `crates/server/server/src/middleware.rs`.

## LOG-9 A refused request names its problem type, never its detail

A request refused with a registered problem type (a `4xx`, or one of the
two `503 Service Unavailable` types, `server-stopping` and `database-busy`)
must leave one more line, written where its problem document is built, with
the problem type's slug and the status
(`problem=invalid-credentials status=401`). It must be at `WARN` for
`401 Unauthorized`, `403 Forbidden` and `database-busy`, and at `INFO` for
every other refusal; the level follows from the problem type where it is
declared. The problem's `detail`
must never be written. A `database-busy` line also carries the error's
chain as `error`, as the `ERROR` line of a `500` does.

Why `WARN` for those three: a run of `401` or `403` is what a password
guesser or an API token that lost its rights looks like, and the owner's
Logs panel opens at warnings and up. A run of `database-busy` says the
server has more work than its database keeps up with, or that something
holds the lock and never lets it go, while one of them is no bug, so it is
not an `ERROR`; the chain names the step that met the lock, so a lock that
never clears can be told from a busy spell. A `404 Not Found` or a refused
search is a client asking for something that is not there, which the owner
has nothing to do about.

Why not `detail`: it can repeat what a person typed, such as a search word
or a value that broke a rule, and a line holds only outcomes (LOG-18). The
slug says which problem it was, and the request id joins the line to the
answer the client received.

Code: `crates/server/server/src/problem.rs`, `api_error.rs`.

## LOG-10 A refused login names the account and the reason, never the username

A refused login must leave one `WARN` line, beside its Audit Trail entry,
with the account's id (none for a username that names no account), the
reason (`unknown_username`, `wrong_password` or `account_disabled`), and the
connecting app and its Build. The username must not be written.

Why: the Audit Trail is read one account at a time, so a run of password
guesses showed in the owner's Logs panel only as `invalid-credentials`
lines that do not say which account or why. The username is what a person
typed, and a guesser's list of usernames is not an outcome.

Code: `crates/server/server/src/session_api.rs`.

## LOG-11 A failed request names the step that failed

A request that fails with `500 Internal Server Error` must leave one `ERROR`
line, written where its problem document is built, with the error's whole
chain, outermost first. The trace layer must write no second `ERROR` line
for the `5xx`, only the response's `INFO` line. Each database statement in
`crates/server/server/src/db/` must add the step it does to that chain, in a
few words (`read the attachments of a page of messages`), as a fixed phrase
that never holds a value bound into the statement. A shared helper that
runs a statement for several callers leaves the step to each caller. The
pragmas the pool runs on each new connection name none: sqlx requires that
hook to return its own error type, which holds no step.

A source whose text the error before it already is, or already ends with
after a `: `, must be left out of the chain (`error_chain`). A cause whose
words only happen to end the step's phrase is kept, because it still says
something new.

Why the step: the request's span names the route, but a route that runs
several statements gives the same database message for each, and a line
that does not say which statement failed sends the owner to the code to
guess. Why a fixed phrase: a bound value can be a search word or a message's
text, which a line never holds (LOG-18). Why the deduplication: `sqlx`'s
database error writes "error returned from database:" and the database's
message, then gives the same message again as its source.

Code: `crates/server/server/src/api_error.rs`, `db/`.

## LOG-12 A body that fails after its status leaves an `ERROR` line

A response whose body fails after its status was sent, such as a download
that hits a disk error partway, must leave one `ERROR` line with the body's
error, under the request's span.

Why: the response's `INFO` line is written when the response starts, so it
says `status=200` for a download the client got cut off, and tower-http's
own classifier decides at the status and never looks at the body (#2870).

Code: `BodyFailures` and `log_body_failure` in
`crates/server/server/src/middleware.rs`.

## LOG-13 Every line carries its request's id, spawned work included

Every line written while the server answers a request must carry the
request's id, method and path, from the tracing span the request runs
under, including a line written by work the request runs in a task of its
own or on the blocking pool. The server must start such work with
`request_id::spawn_in_current_span` or
`request_id::spawn_blocking_in_current_span`. `tokio::spawn` and
`tokio::task::spawn_blocking` are refused in the server by
`crates/server/server/clippy.toml`.

Why: a tracing span does not cross a spawn on its own, and a warning that
names no request cannot be matched to the request that caused it or to the
error its client was shown (#2186).

Code: `crates/server/server/src/request_id.rs`.

## LOG-14 An Audit Trail entry keeps the id of the request that wrote it

An Audit Trail entry must keep its request's id in
`audit_entries.request_id` and hand it out as `request_id`, including an
entry written by work the request started: the two spawn helpers of LOG-13
carry the id as well as the span. An entry written outside a
request has none: a server command, the Demo Account build `serve` runs on
a new database, and the rows Import Runs, Export Runs and a Session that ran
out are read from.

Why: an entry says who did what and when, and a line says which request did
it and how it went, and with no key shared between them the owner matched
the two by time alone (#2276).

Code: `crates/server/server/src/db/audit_trail.rs`.

## LOG-15 Trim the log by size only: 5 files of 50 MB

A file must be closed and the next one started before a line would carry
it past 50 MB (50,000,000 bytes). At most 5 files are kept, 250 MB in all:
when one more starts, the oldest is deleted. A restarted server writes on
the end of the newest file while it has room. A write that fails part-way
must be cut off the file again, so the file ends where a line does. A file a
stopped server left ending part-way through a line must not be written on
again: the next start begins a new file. Nothing is deleted for its age, and
a file is never renamed.

Why size: the disk is what a log can run out of, and a size limit is the
only one that bounds it. A Message Crate on a small disk cannot let a quiet
week and a busy import day differ by gigabytes. Why 5 of 50 MB: a 50 MB
file is small enough to download and open in an editor, and five hold the
log to 250 MB whatever level `RUST_LOG` sets. Why never renamed: a file's
number names it for as long as it exists, which is what lets a line be
addressed by where it is (LOG-16) while the log grows.

Rejected: deleting by age (keep 14 days). A busy day still fills the disk,
and a quiet server throws away the lines that would explain its last
failure.

Code: `SERVER_LOG_LIMITS` in `crates/server/server/src/logging/files.rs`.

## LOG-16 The log is read newest first, from a line, never from an offset

`GET /v1/server/log-lines` must answer the newest lines first, filtered by
`level` (that level and the more severe ones) and `text` (lines whose text,
after the time and the level, holds it, ignoring case). A line's `id` is
where it starts: its file's number times 2³², plus its byte offset in the
file, so a larger id is a newer line, and `after={id}` reads the lines older
than that line. `GET /v1/server/log-files` lists the files, and
`GET /v1/server/log-files/{id}` downloads one whole, as `text/plain`, at the
length it had when the download started. A line that does not start with a
time and a level is left out of the lines; only a write cut short leaves
one, and the downloaded file still has it.

Why from a line: the server writes lines while the owner reads them, its
own answers included, so a count from the newest line moves between pages.
An id names a line that does not move.

Code: `crates/server/server/src/logging/read.rs`,
`server_api/log_lines.rs`, `server_api/log_files.rs`.

## LOG-17 The owner reads it, nobody else

The three routes must take the owner's Session and refuse every other
credential with `403 Forbidden`.

Why: the log is about the whole installation, every account's requests in
one stream, and the owner is the one who runs the installation and is asked
when it fails. An account holder is not shown other accounts' requests, so
no account reads it, and an API token never reads anything but its
account's own data. An Import Run's log follows a separate rule: it stays
on the computer that ran the import
([Import Run logs](import-run-logs.md)).

Code: `crates/server/server/src/server_api/`.

## LOG-18 A line never holds a secret or content

A line must never hold a password, a Session token or an API token, hashed
or not, message text, attachment bytes, or a contact's name or identities.
It may hold only what the owner may see: ids, counts, sizes, times, routes
and outcomes. The line each request writes names its path and query, and
the query keeps a parameter's value only when it is a number, an id or a
fixed word (`after`, `around`, `before`, `deleted_account_id`, `level`,
`limit`, `mode`, `offset`, `sort`, `status`); every other value is
written as `[hidden]` until someone adds it to the list: `q`, a search over
what the messages say; `media_link`, a credential; the log's own `text`
filter; and any parameter added later. The path and query
are cut at 2,048 bytes, on a character boundary, ending in `...[cut]`.
Every event the server writes is held to the same rule by whoever writes
it.

Why: the owner reads the log, and the owner never reads content
(`docs/adr/0008-the-owner-holds-no-messages.md`). Why the cut: a request
needs no credential to leave its line, and a URI can be 64 KiB long, so
without it anyone who can reach the port could fill the 250 MB the log
keeps with a few thousand requests and push every other line off the disk
(#2088).

Checked by `crates/server/server/tests/server_log.rs`, which runs `serve` at
`RUST_LOG=trace` through an owner, an account, an API token, an import with a
named contact and an attachment, a search, a media link, a password change,
a wrong password and a refused search, and fails if any of those secrets,
the message's words, the attachment's bytes, or the contact's name, phone
number or email address is in any file of the log.

Code: `logged_uri` in `crates/server/server/src/middleware.rs`.
