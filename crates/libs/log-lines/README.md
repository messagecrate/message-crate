# message-crate-log-lines

The line format of Message Crate's logs, defined once for the server, which
reads its own log back, and the desktop app, which writes each Import Run's
log and reads it back. A line is its time in UTC (RFC 3339, microseconds), its
level padded to five characters, and its text:

```text
2026-10-08T12:00:00.123456Z  WARN Did not upload a.jpg
```

That is what `tracing-subscriber` writes for the server, so one viewer reads
both logs and the level filter means the same in each. The crate holds the
writer (`format_lines`), the parser (`parse_line`), the level and text filter
(`LineFilter`), and the walk from a file's newest line back, a chunk at a time
(`lines_backward`).

The two sides kept their own copies of the parser and the filter before this
crate, and nothing would have caught them drifting apart. A server test now
checks that a line the desktop app writes is the line the subscriber writes.

The `schema` feature adds `utoipa::ToSchema` to `LogLevel` so it describes
itself in the OpenAPI document. The server turns it on; the desktop app's
crates leave it off and never build utoipa.

## Build and test

```bash
cargo test -p message-crate-log-lines
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

The rules: `docs/architecture/server-log.md` and
`docs/architecture/import-run-logs.md`. This crate is a library shared by the
server, `message-crate-core` (and through it `message-crate-import`) and the
desktop app. It builds no binary.

## License

Fair Core License. See the repository root `LICENSE.md`.
