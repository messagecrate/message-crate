---
title: Config and accounts
description: Instance config.toml, per-account data paths, and multi-tenant accounts.
---

## Instance config

Copy [`config/config.toml.example`](https://github.com/messagecrate/message-crate/blob/main/config/config.toml.example)
to `config/config.toml` (gitignored).

```toml title="config/config.toml"
[paths]
db = "data/messagecrate.db"
data_dir = "data"
assets_dir = "assets"
assets_converted_dir = "assets_converted"

[server]
bind = "127.0.0.1:8080"
cors_origins = [
  "http://localhost:5173",
  "http://127.0.0.1:5173",
  "https://tauri.localhost",
  "http://tauri.localhost",
  "tauri://localhost",
]
```

- A relative path resolves against the directory above the config file's directory: the repository root for `config/config.toml`. The rule holds for every path in the file (`db`, `data_dir`, `static_dir`) and for the flags that replace one of them (`--db`, `--static-dir`), so `--db data/messagecrate.db` names the same file as `db = "data/messagecrate.db"` whatever directory the command runs in. `serve --data-dir` reads no config file, so there `--data-dir` and `--static-dir` resolve against the directory the server is started in.
- `assets_dir` and `assets_converted_dir` are each one directory name, not a path: an account's originals are in `data_dir/<account_id>/<assets_dir>` and its Previews in `data_dir/<account_id>/<assets_converted_dir>`. The server refuses a value that is absolute, contains a `/`, `\` or `:`, starts with `.` (`.` and `..` included), ends in `.` or a space, or is empty, with an error naming the key, and refuses the two keys being the same name in any letter case. Why: an absolute path, or a Windows drive such as `C:assets`, would put every account's attachments in one directory; a separator or `..` would reach outside the account's own; the server keeps `.removing` beside the attachments and deletes what is in it; Windows drops a trailing `.` or space, and macOS and Windows ignore letter case by default, so `assets.` or `Assets` could be another key's directory; and originals and Previews are cleaned up under different rules, so they need a directory each.
- `[server]` is required for `serve`.
- `reset-demo` rebuilds the Demo Account in the database `db` names and never writes the config file.
- `cors_origins` lists origins allowed on top of the three the packaged desktop app runs from (`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`), which the server allows whether or not you name them. The website the server serves is same-origin and needs no entry either, so an empty list is the right setting for most installs. Add the Vite origins (`http://localhost:5173`, `http://127.0.0.1:5173`) when running the dev UI against this server.
- `static_dir` is the directory holding the built website, served at `/`. It defaults to `static`. Everything in it is served to anyone who can reach the server, without a login, so `serve` refuses to start when `data_dir` or `db` is inside it, or it is inside `data_dir`. The paths are compared after every symlink and `..` is resolved.
- `serve` runs without a config file when given `--data-dir <directory>`: the database is `messagecrate.db` in that directory, the accounts' files sit beside it, and every `[server]` key has its default. `--bind` and `--static-dir` override `bind` and `static_dir`, with or without a config file. The desktop app starts the server this way, and passes `--tools-dir` with its Tools Directory, where ffmpeg and ffprobe are looked for when they are not on `PATH`. The config file has no key for it, because a server started by hand finds ffmpeg on `PATH` or makes no Previews or Thumbnails.
- Source names are **not** listed in TOML — each import registers its own
  source slug for that account in the database. The files on disk carry no
  source: an account keeps one directory of originals and one of previews.

### Which file is the database

The server opens only a Message Crate database, and it changes no other file.
A Message Crate database carries SQLite's `application_id` set to `0x4d734372` (the bytes `MsCr`), which the server writes into every database it builds.
A file without that mark, such as Apple's `chat.db` or another program's backup, is refused by name before any statement changes it, because rebuilding it would drop that program's tables.

Only `serve`, `create-database` and `reset-demo` make a new database, at a path where no file exists or in an empty file.
Every other command (`import`, `imports discard`, `dedupe-cross-source`, `process-assets`, `create-owner`, `reset-owner-password`) stops with an error naming the path when no database is there, so a mistyped `--db` creates nothing.

A Message Crate database stamped with another Schema Fingerprint is still rebuilt empty on open, as after any schema change.
Before the rebuild drops its tables, the server copies it with `VACUUM INTO` to `<database>.before-rebuild-<old fingerprint>` beside it, and refuses to rebuild when the copy fails.
`reset-demo` refuses a database at another fingerprint before it takes a snapshot, because the snapshot would be rebuilt and its copy kept in the reset's work directory, which is deleted.
`serve` keeps the copy beside the database and rebuilds it, and a reset can follow.
The schema is made sure of only when the pool opens and when `reset-demo` builds a database, never by a request, so a request never rebuilds a database the server is serving.

### Video and audio conversions

`[server]` also accepts one optional conversion setting:

| Key | Default | Description |
|-----|---------|-------------|
| `transcode_stall_secs` | `120` (2 minutes) | How many seconds a video or audio conversion may go without moving forward in the file before ffmpeg is stopped. Must be greater than 0; a file that sets 0 is refused with an error naming the key. |

A conversion that keeps moving runs as long as it needs, so a long video on a slow computer is never cut off for its length.
One that stops moving for this many seconds is stopped, and so is one that runs past twice the file's length, as looping on a damaged file.
Reading a file's details, reading the one frame of a Thumbnail or of an image's Preview, and the check that finds ffmpeg have a fixed limit of 15 seconds that no key changes, because each takes well under a second on a working file.
`serve`, `process-assets` and `import` read the key; a server started with `--data-dir` and the desktop app's own conversions use the default.
Why the rules: `docs/architecture/media.md`.

### Server asset limits

`[server]` also accepts three optional upload settings:

| Key | Default | Description |
|-----|---------|-------------|
| `asset_part_size` | `16777216` (16 MiB) | Largest part of a multipart upload. Must be greater than 0. Keep under ~100 MiB for Cloudflare-proxied setups, and small enough that a whole part arrives inside `upload_piece_secs`: 16 MiB arrives inside the default at about 0.45 Mbit/s. The part size a client is told is this or the attachment size limit, whichever is smaller. |
| `upload_stall_secs` | `60` | Seconds an upload (an import batch, an attachment, or a part of one) may go with no byte arriving before the server stops reading it. Must be greater than 0. |
| `upload_piece_secs` | `300` (5 minutes) | Seconds a whole upload may take to arrive, from the start of the read, before the server stops reading it. Must be greater than 0. A full 16 MiB import batch arrives in time at about 0.45 Mbit/s. |

The server refuses a config file that sets either time to 0, naming the key.
An upload that meets either limit is refused with `408 Request Timeout` and the [`upload-stalled`](/docs/developer/reference/errors/upload-stalled/) problem type, and none of it is stored. Running the import again continues it.
Why: an import batch holds one of the server's two import slots while it arrives, so a client that stops sending part-way, such as a laptop that goes to sleep, must not hold it for long.
`GET /v1/server` states both limits as `upload_stall_secs` and `upload_piece_secs`, and the desktop app's Upload waits for each upload's answer as long as the server says it may take, plus 5 minutes for the server to save it.
A server started with `--data-dir`, as the desktop app starts its own, uses the defaults.

The server refuses a config file that carries a section or key it does not use.
Every command that loads the file, `serve` included, stops with an error that names each unknown key and its section, for example ``[server] has a key the server does not use: `bnd` ``.
Why: a misspelt key would otherwise load as its default, and a removed key would sit in the file looking as though it still held.

The attachment size limit is not a config key, and a file that still sets `[server] asset_max_bytes` is refused with a message saying where the limit is set now.
It is the largest attachment the server accepts, as a single `PUT /v1/assets/{sha256}` body or as the total declared bytes of a multipart upload.
It is a Server Setting stored in the database: 512 MiB until the Owner changes it under **Server Settings**, or a program with the Owner's Session sends `PATCH /v1/server/settings` with `asset_max_bytes` in bytes.
A change holds from the next upload, with no restart.
A multipart upload already in progress keeps the part size it started with, so lowering the limit does not break it.
`GET /v1/server` reports the limit as `asset_max_bytes` to any client, with no credential, because the desktop app reads it before Staging.

The limit is whatever the Owner set, and a part is never larger than the limit.
A limit below `asset_part_size` is accepted, and the server then hands out parts the size of the limit.
The part size is worked out on each upload, so neither a change to the limit nor an edit to `asset_part_size` can leave a server that does not start.
The server refuses only a limit of zero, or one above 9223372036854775807, with `422 Unprocessable Entity`.

No other request body is held to the attachment size limit.
Each part of a multipart upload is held to the part size the upload was given when it started.
Every other request body has a cap fixed in the server: 32 KiB for `POST /v1/session`, `POST /v1/accounts` and `POST /v1/server/claim`, 32 MiB for any other JSON body, 8 MiB for an address book loaded with `POST /v1/contacts`, 16 MiB for an import batch, the most the desktop app sends in one, and 512 MiB for any other body.
Why: the Owner's limit must never reach the login, or the settings change that would raise it again.
A body over its cap is refused with `413 Payload Too Large` and the [`payload-too-large`](/docs/developer/reference/errors/payload-too-large/) problem type.

### Logging

The server writes its log to stderr through `tracing`: one `INFO` line per HTTP response with the method, path, status and latency (at `DEBUG` for a successful `GET /health` or file of the website), one `ERROR` line with the full cause chain behind every `500`, and `WARN` lines for work the server could not complete but did not fail the request over. `RUST_LOG` sets the level and accepts the usual filter syntax, for example `RUST_LOG=debug` or `RUST_LOG=message_crate_server=debug,tower_http=info`. Unset, the level is `info`. The `import`, `dedupe-cross-source`, `process-assets` and `reset-demo` subcommands print their progress to stdout as before; that is their output, not the log.

`serve` also writes every line to files in the `logs` directory of the Data Directory (`[paths] data_dir`, or `serve --data-dir`): `server-000001.log`, `server-000002.log` and so on, the highest number the newest. It starts the next file before a line would carry the newest past 50 MB, keeps at most 5 files (250 MB in all), and deletes the oldest when one more starts. No config key changes these limits. `RUST_LOG` decides how many lines there are, not how much disk they may take. Docker and the desktop app write the same files, the first in its data volume and the second in its app-data directory. A line is one event, with a line break inside it written as `\n`, and its query string keeps only numbers and fixed words, so no search, password, token, message text or contact name reaches the files. The owner reads them through `GET /v1/server/log-lines` (newest first, with `level`, `text` and `after`), `GET /v1/server/log-files` and `GET /v1/server/log-files/{id}`. No other account can. Why, and the rest of the rules: `docs/architecture/server-log.md` in the repository.

## Per-account asset files

Created on first use if missing:

- `data/<account_id>/assets/`
- `data/<account_id>/assets_converted/`

An attachment file is named by the SHA-256 of its bytes, so a file imported
from two sources is stored once. It is removed when no message of the
account, from any source, still names it. `assets_converted/` holds what the
server makes from the originals: a Thumbnail of every image and video, and a
Preview of each attachment browsers often cannot show.

## Accounts

Rows are scoped by `account_id` in a shared `messagecrate.db`. The owner is
always account `1` and the demo account `2`; every other account takes an id
from `100` up.

- Web login uses username + password (Argon2id hash in `accounts.password_hash`).
  A username is 1 to 128 characters: letters A to Z in either case, digits,
  `_`, `-` or `.`.
  The login matches it without regard to the case of its letters.
  Letters from other scripts are refused, because a name such as `аdmin`
  with a Cyrillic `а` would look the same as `admin` while naming another
  account. The preferred name carries a name in any script.
  An account created before 0.11.0 keeps a username with letters from
  another script, and logs in with it.
  Those letters must be typed in the case they were stored in, because the
  login folds the case of A to Z only.
  An account may have no password (`password_hash` NULL); an empty password is
  accepted only for those accounts.
- Guesses at one account's password are rate-limited to 20 per 60 seconds:
  every login as the account, under any spelling of its username, and every
  wrong current password sent to change the owner's password or to delete the
  account.
  Creating an account and claiming Message Crate are each limited to 20 attempts
  per 60 seconds across the whole server, whatever the username.
- Each account can create named **API tokens** for programs that call the HTTP API
  (stored hashed; shown once when created). GUI sessions use a separate rotating
  token.
- Four columns on `accounts` govern what a logged-in session may do, each
  enforced by a guard in `server.rs` rather than left as decoration:
  - `disabled` — may not log in; an existing session or API token for a
    disabled account stops working immediately.
  - `can_import`, `can_export`, `can_delete` — may call the import endpoints,
    the export endpoints, and the endpoints that destroy message data,
    respectively. New accounts default to all three. A named API token
    carries import and export only, never delete: permanent deletion is a
    person's act, so it always needs a logged-in session.
  The owner sets another account's flags from Owner Home
  (`PATCH /v1/accounts/{id}`), resets a password
  (`PUT /v1/accounts/{id}/password`), deletes an account's messages
  (`DELETE /v1/accounts/{id}/messages`) or the account
  (`DELETE /v1/accounts/{id}`). The owner has no flags of its own: it holds
  no messages, and nothing can disable or delete it. `create-owner` and
  `reset-owner-password` on the server CLI are the only ways to make or
  recover the owner's login.
- The Demo Account: username `demo`, never a password. `serve` adds it to a
  database that does not exist yet; `reset-demo` rebuilds it; the login card's
  **Explore Demo Account** button logs in as it.

See [Account and Profile](/docs/user/features/settings/account-and-profile/).
