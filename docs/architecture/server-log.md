# The server's log

The server keeps a record of its own work: every request it answered, every
failure it met, and what it did at start-up. This page follows one line of
that record from the event that caused it to the owner reading it, and shows
why the pieces sit where they do. The constraints a change must keep are in
[The server's log: rules](rules/server-log.md).

## The shape

```mermaid
flowchart LR
    E[An event in the server:\na request answered,\nan error, a panic]
    T[tracing span\nrequest id, method, path]
    S[One subscriber]
    SE[stderr]
    F[Data Directory / logs\nserver-000001.log …]
    R["GET /v1/server/log-lines\nGET /v1/server/log-files"]
    P[Owner Home → Logs]
    E --> T --> S
    S --> SE
    S --> F
    F --> R --> P
```

Every line goes two places at once: to stderr, where a terminal or a
container runtime shows it, and to a set of rotating files in the `logs`
directory of the Data Directory. The owner reads the files back through
three routes, which the Logs panel of Owner Home calls. The Message Crate in
Docker and the one the desktop app starts are the same `serve` program with
the same Data Directory layout, so both keep the same files the same way.

## A line

```text
2026-10-08T12:00:00.123456Z  WARN request{id=7f3a method=GET path=/v1/contacts}: problem=invalid-credentials status=401
```

A line is one event: the time in UTC to the microsecond, the level, the span
the event happened under, and what it says. The span is what ties lines
together. A request runs under a span that names its id, method and path,
and every line written while that request is answered carries it, including
lines from work the request handed to another task, such as hashing an
import's messages or removing files. The same id is in the problem document
the client received, so a person who sees a failure on a screen can quote
its id and find the matching lines.

A line is always whole. A message that itself contains a line break is
written with the two characters `\n` in its place, and any other control
character is escaped, so a file can be read from any point, forwards or
backwards, and each line found is a complete event. The format is the one
`crates/libs/log-lines` defines, which the desktop app's Import Run logs
share, so one viewer reads both.

## What reaches the log

The server writes at four levels, and the owner's Logs panel opens at
warnings and up.

| Level | What is written there |
|---|---|
| `ERROR` | A `500 Internal Server Error`, with the failing step and the error's whole chain. A panic on any thread. A start-up failure. A response body that failed after its status was sent. |
| `WARN` | A refused request that looks like someone probing: `401 Unauthorized`, `403 Forbidden`, and a database too busy to answer. A refused login, with the account's id and the reason. |
| `INFO` | Every other request: its method, path, status and latency. Every other refusal, with its problem type. What `serve` did at start-up. |
| `DEBUG` | A successful health probe and a successful file of the website, which otherwise bury everything else. |

A failed database statement names the step it was doing, in a fixed phrase
such as `read the attachments of a page of messages`, so a `500` line reads
as the step and then the database's own message.

## What never reaches the log

The owner reads this log, and the owner never reads anyone's messages
([ADR 0008](../adr/0008-the-owner-holds-no-messages.md)). So a line holds
outcomes, never content: ids, counts, sizes, times, routes and statuses. A
search's words, a media link, a password, a token, message text, an
attachment's bytes and a contact's name or addresses are not written, and a
request's query string keeps only the values that are numbers, ids or fixed
words; everything else appears as `[hidden]`. A refused request names its
problem type but not the sentence the client was shown, because that
sentence can repeat what a person typed. An integration test runs a real
server through every kind of request and fails if any secret or any content
turns up in the files.

## The files

`serve` opens the log before it opens the database, so the lines that say
why a database could not be opened have somewhere to go. The newest file
takes every line. When a line would carry a file past 50 MB, that file is
closed and the next one starts; when a sixth file starts, the oldest is
deleted. The log is therefore never more than 250 MB, however much a busy
import day writes, and a quiet week deletes nothing. A file keeps its number
for as long as it exists.

The `logs` directory, like the Data Directory around it, is readable by the
user the server runs as and nobody else.

## Reading it back

The owner's Session is the one credential the three log routes accept. The
Logs panel asks for the newest lines first and reads older ones as it
scrolls, filtered by level and by a text search. Each line has an id that
says where it is: its file's number in the high bits and its byte offset in
the low bits, so a larger id is always a newer line, and a page of older
lines is asked for as "the lines before this id". The log is being written
while it is read, which is why pages are addressed by a line rather than by
a count from the top. Each file can also be downloaded whole as plain text.

The panel shows the log as it was when it was read; nothing streams.

## Where it lives

| Piece | Code |
|---|---|
| The subscriber, the panic hook, and opening the files | `crates/server/server/src/logging.rs` |
| Writing and rotating the files | `crates/server/server/src/logging/files.rs` |
| Line ids and reading pages back | `crates/server/server/src/logging/read.rs` |
| The line format, shared with Import Run logs | `crates/libs/log-lines` |
| The request span and spawning under it | `crates/server/server/src/request_id.rs` |
| What a request's line keeps of its URI, and body failures | `crates/server/server/src/middleware.rs` |
| The problem-type and `500` lines | `crates/server/server/src/problem.rs`, `api_error.rs` |
| The three routes | `crates/server/server/src/server_api/log_lines.rs`, `log_files.rs` |
| The Logs panel | `web/src/screens/owner/OwnerLogsPanel.tsx`, `web/src/components/logs/` |
| The content test | `crates/server/server/tests/server_log.rs` |
