# Database security audit

- **Audit:** database-security (code-audit skill `database-security`)
- **Date:** 2026-10-10
- **Commit:** `530dc1791` (branch `docs/initial-software-design-analysis`)
- **Scope:** the whole repository except `web-next/`, `vendor/`, `target/`, `node_modules/`. `vendor/sqlx-sqlite` is a byte-identical copy of `sqlx-sqlite` 0.8.6 with one dependency bump (`VENDORING.md`); it is noted, not audited.
- **Method:** read-only. Every dynamic SQL site in `crates/server/server/src` was read line by line and classified; the pool, transaction, schema, credential, logging and backup code was read in full where it matters; the desktop crates that open vendor SQLite files were checked for their open flags. Nothing was built, run, or opened as a database.

## 1. Summary

**Risk score: 3 / 10 (low).**

No SQL injection was found. Every statement the server runs is either a constant, a constant chosen by an enum or allow-list, or a template whose values travel as `?`/`$n` bindings. The search language compiles to SQL in one module and never writes typed text into the statement: text goes through `Sql::like` with `ESCAPE '\'` after `like_escape`, and full-text terms are FTS5 string literals bound as parameters. Credentials are handled correctly: Argon2id for passwords, SHA-256 of a 32-character OS-random token for Sessions and API tokens, an in-memory per-start HMAC key for Media Links. Tenant isolation is structural: every tenant table carries `account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE`, single-row reads go through `db/ownership.rs`, and assets live in a per-account directory. Request logging is an allow-list and is tested at `RUST_LOG=trace`.

What remains is about the data at rest and about operations, not about queries:

| ID | Severity | Title |
|----|----------|-------|
| DB-1 | Medium | A schema fingerprint mismatch drops every table, on the request path, with no copy kept |
| DB-2 | Medium | The owner's password is taken on the command line, and the docs show it that way |
| DB-3 | Low | No request timeout and no statement interrupt; four slow searches stall auth |
| DB-4 | Low | "Delete for good" leaves message text in free pages and the WAL |
| DB-5 | Low | The database file and data directory take the process umask |
| DB-6 | Low | Nothing is encrypted at rest; the documented backup is a plain tar.gz |
| DB-7 | Low | The Apple Messages Reader opens a backup's `Manifest.db` read-write |
| DB-8 | Low | `trusted_schema` is left on, so a supplied database file is trusted past `application_id` |
| DB-9 | Low | Token characters are drawn with a modulo bias |
| DB-10 | Low | Form credential stripping is a deny-list of two keys |
| DB-11 | Info | The Demo Account logs in with an empty password on a network-published server (by design) |

Counts: 0 Critical, 0 High, 2 Medium, 8 Low, 1 Informational.

## 2. Threat model and assumptions

- The product is "built for a home network, not for the open internet" (`README.md:130-131`). Passwords, Session tokens and messages cross the network as plain HTTP; the docs say so and require a reverse proxy with TLS and its own auth for anything beyond a trusted network (`docs/src/content/docs/docs/user/features/owner/run-on-another-machine.md:35-47`). Findings that only matter when the server is exposed say so.
- The Docker image binds `0.0.0.0:8080` (`config/config.docker.toml:17`) and runs as the unprivileged `node` user with the database in the `/app/data` volume (`Dockerfile:79-84`). The desktop app starts its own server at `127.0.0.1:8080` with data under the operating system's app-data directory (`src-tauri/src/local_server.rs:68`, `:965-966`), and a setting may open it to the network (`docs/adr/0018-the-desktop-app-starts-the-server-it-ships.md:39-43`).
- SQLite is the only engine (`docs/adr/0017-sqlite-is-the-only-database-engine.md`), in-process, so the skill's items about database users, network ACLs and TLS to the database do not apply as written; the operating-system file and the process boundary are the equivalents, and are judged as such.
- A rebuilt database is accepted by the project as the only kind of migration (`CLAUDE.md`, "No backwards compatibility"; `db/schema.rs:195-197`). DB-1 does not argue against rebuilding. It is about a rebuild that happens without a copy and from a request.

## 3. Dynamic SQL inventory

Every site where the SQL text is assembled at runtime, and what fills the hole. "Bound" means the value travels as a parameter. "Const" means a `&'static str` or enum method chosen in code. No site interpolates request input.

| Site | What is interpolated | Class |
|------|----------------------|-------|
| `search/bridge.rs:46-50` `Sql::like` | `column` from emitter constants; pattern bound | Const + bound; `ESCAPE '\'` |
| `search/bridge.rs:19-33` `push`, `bind_text`, `bind_int` | fragment constants; values bound | Const + bound |
| `search/emit.rs:204-227` `like_contains`, `like_escape` | escapes `\`, `%`, `_` before binding | Bound, escaped |
| `search/emit.rs:404-424` `text_match` | `column` constant; `Value::Keyword` matched by literal | Const |
| `search/emit.rs:1018-1037` `attachment_kind_sql` | `kind` is a parser `Value::Choice` from `spec.values` (`search/parse.rs:130`); only matched, never written | Const |
| `search/emit.rs:1085-1091` `source`/`service` | bound with `bind_text` even for fixed words | Bound |
| `search/fts.rs:11-13`, `:20-37`, `:40-49` | term quoted as FTS5 literal (`"`→`""`) and the whole MATCH expression bound | Bound, quoted |
| `search/mod.rs:152-160` `and_where` | fragment from `selection_where` (placeholders only) | Const + bound |
| `search/mod.rs:184-192` `compile_messages_of_conversations` | compiled Conversations `where_sql` | Compiled, bound |
| `db/conversation_messages.rs:111`, `:347`, `:354`, `:406`, `:822-825`, `:929`, `:952`, `:989`, `:996`, `:1035` | join constants, `Direction::sql()`, `placeholders(n)`, `where_sql` from the compiler; ids, timestamps, limit and offset bound | Const + bound |
| `db/contacts/read.rs:141-157`, `:177`, `:189-220`, `:296` | `ContactSort` enum → fixed strings; compiled `where_sql`; `LIMIT ? OFFSET ?` bound | Const + bound |
| `db/conversations.rs:147`, `:162` | same pattern | Const + bound |
| `paging.rs:117-163` `parse_sort` | sort keys matched against `accepted: &[(&str, K)]`; unknown key → 422 | Allow-list |
| `paging.rs:176-206` `page_params` | `MAX_LIST_LIMIT = 500`, `MAX_LIST_OFFSET = 50_000` | Capped |
| `db/audit_trail.rs:864-889`, `:906`, `:916-919`, `:966-972` | `Source::tag()` constants; `filter` one of three literals; `LIMIT {limit} OFFSET {offset}` are `usize` from `page_params` | Const + typed int |
| `db/attachment_versions.rs:196-200`, `:262-267`, `:295-300`, `:325-333`, `:356-371` | `Version::columns()` → `&'static str` triples (`:25-28`) | Const |
| `db/participants.rs:25-43` | `is_account_identity_sql("h", "$1")` constant builder | Const + bound |
| `db/exports.rs:324-329`, `db/imports.rs:983-988` | `status_sql` one of two literals; status bound | Const + bound |
| `db/handles.rs:457-480` | SQL fragments chosen by `IdentitiesOf` enum | Const |
| `db/import_contacts.rs:177` | `tally_sql("$1")` constant builder | Const + bound |
| `db/storage.rs:30-51`, `:165-175` | `select`, `from`, `account_column` are literals at every call (`:56` and siblings) | Const |
| `db/free_name.rs:65-80` `insert_sql` | `table` and `columns` from `NamedSetSpec.table: &'static str` (`db/named_membership.rs:63`, `:94-98`, `:135-139`) and literal callers (`db/saved_searches.rs:178`, `:281`; `db/contacts.rs:270`) | Const + bound |
| `db/ownership.rs:45-58` `owns_row` | `table: &'static str`; ids bound | Const + bound |
| `db/schema.rs:167`, `:183` | `APPLICATION_ID`, `SCHEMA_FINGERPRINT` integers | Const |
| `db/schema.rs:221` | table names read from `sqlite_master`, wrapped by `quote_ident` (`:253-255`) | Quoted identifier |
| `db/schema.rs:388` | index names from `MESSAGES_SECONDARY_INDEX_DDL` | Const |
| `db/staging.rs:869-882` `reset_id_map` | `table` and `extra_columns` literals at `:973`, `:1282`, `:1483`, `:1709` | Const |
| `db/staging.rs:1192`, `:1218` | two constants concatenated; account and id range bound | Const + bound |
| `dedupe.rs:446-456` `insert_content_key_rows` | `write!(sql, "(${}, ${})", …)` placeholder numbers; values bound; `SQLITE_IN_CHUNK` keeps under the bind limit | Bound |
| `dedupe.rs:674-700`, `:1145-1178` `apply_duplicate_flags` | `table` literal at `:376`, `:747`, `:969`; ids bound | Const + bound |
| `reset_demo.rs:1004-1006` | `VACUUM INTO $1` with the path bound | Bound |
| `reset_demo.rs:1384-1402` | table names from `sqlite_master` through `quote_ident`; account bound | Quoted identifier + bound |
| `db/sqlite_functions.rs:59-70` | replaces `lower()` with a Unicode fold; `SQLITE_DETERMINISTIC | SQLITE_INNOCUOUS`; NULL and non-UTF-8 handled (`:86-97`) | Safe function |
| `crates/helpers/imessage-reader/src/domain.rs:166-170` | `domain` bound as `?1` against `Manifest.db` | Bound |
| `crates/libs/ios-backup/src/backup.rs:62` | `Connection::open_with_flags(.., SQLITE_OPEN_READ_ONLY)` | Read-only open |
| `imessage-database 4.2.0` `tables/table.rs:198-203` (registry) | `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX` for `chat.db` | Read-only open |

Not SQL, checked because they looked like it: `declared_query.rs` validates HTTP query-parameter names against the OpenAPI document; `server.rs:1194-1250` `logged_uri` is log redaction.

`SELECT *` appears twice, both as `SELECT * FROM (SELECT <explicit list> …) AS c ORDER BY …` wrappers (`db/contacts/read.rs:189`, `db/conversations.rs:162`); no open `SELECT *` over a base table.

No `ATTACH`, `load_extension`, or `enable_load_extension` anywhere in the server crate. `src-tauri/` holds no SQL. The WhatsApp exporter never opens `msgstore.db` itself; it runs `wtsexporter` (`crates/exporters/whatsapp-exporter/src/run.rs:99-135`).

## 4. Findings

### DB-1. A schema fingerprint mismatch drops every table, on the request path, with no copy kept

- **Severity:** Medium
- **CWE:** CWE-693 (Protection Mechanism Failure); the destructive step runs without the copy that would make it recoverable.
- **Evidence:**
  - `crates/server/server/src/db/schema.rs:155-166`: `if stamped == SCHEMA_FINGERPRINT { return Ok(()); } if has_user_tables(conn).await? { tracing::warn!(… "The database's schema differs from this server's, so the database is rebuilt empty and its messages must be imported again"); } rebuild_schema(conn).await?;`
  - `db/schema.rs:206-226` `rebuild_schema`: `PRAGMA foreign_keys = OFF`, then `DROP TABLE IF EXISTS {quote_ident(table)}` for every table in `sqlite_master`.
  - `db/schema.rs:596-601` `ensure_accounts_schema`: `if user_version(conn).await? != SCHEMA_FINGERPRINT { ensure_schema(conn).await?; }`
  - Called on the request path: `server.rs:1617` in `resolve_auth_on_conn` (every authenticated request), `session_api.rs:273`, `accounts_api/api_tokens.rs:156`.
  - The only pre-check is `database_kind` on `application_id` (`db/schema.rs:150-153`); an empty file or a Message Crate file at another fingerprint passes.
  - No `VACUUM INTO`, copy, or rename precedes the drop. The only snapshot in the code is `reset_demo.rs:995-1016`, which belongs to `reset-demo`.
- **Why it matters:** the database holds every imported message. A running server that sees a different `user_version` on its next authenticated request empties it. `user_version` changes when another process stamps the same file: a second server binary of another release pointed at the same `--db`, a `reset-demo` or `create-database` run from a newer checkout against a live file, or a `cargo tauri dev` build sharing the installed app's data directory, which ADR 0018 names as the reason `data-dev` exists (`docs/adr/0018-…:31-33`). The project accepts rebuilds as the migration story; it does not have to accept them without a copy, and it does not have to let a request do it.
- **Exploitability:** not an attack, an operational hazard. Reproduction: start the server on a database; from another checkout at a different `SCHEMA_FINGERPRINT`, run `message-crate-server create-database --db <same file>` (or start a second server on it); make one authenticated request to the first server. The log shows the warning and the tables are gone.
- **Remediation:**
  1. Keep a copy before the drop when there are user tables. In `db/schema.rs` before `rebuild_schema(conn)`:
     ```rust
     if has_user_tables(conn).await? {
         let copy = path.with_extension(format!("db.before-rebuild-{stamped}"));
         sqlx::query("VACUUM INTO $1")
             .bind(copy.to_string_lossy().as_ref())
             .execute(&mut *conn)
             .await
             .with_context(|| format!("copy {} before rebuilding it", path.display()))?;
         tracing::warn!(copy = %copy.display(), "the previous database is kept beside the new one");
     }
     ```
     `migrate_schema` will need the path; `OpenDb::open_pool` (`open_db.rs:106-113`) has it in `cfg.paths.db`.
  2. Take the rebuild off the request path. `ensure_schema` already runs at startup (`open_db.rs:110`). Replace the per-request `ensure_accounts_schema` with a read that refuses: `if user_version(conn).await? != SCHEMA_FINGERPRINT { return Err(ApiError::Internal(anyhow!("the database was changed by another program; restart the server"))) }`. The design audit's F18 asks for the same removal for a different reason.
- **Defense in depth:** refuse to open a database whose `user_version` is newer than this binary's (a mismatch, not a handshake, so it stays within the project's rule); document that two servers must never share one file.

### DB-2. The owner's password is taken on the command line, and the docs show it that way

- **Severity:** Medium (Low when the host has one user)
- **CWE:** CWE-214 (Invocation of Process Using Visible Sensitive Information); CWE-532 for the shell history.
- **Evidence:**
  - `crates/server/server/src/cli.rs:89-91`: `/// Password for the owner … #[arg(long)] pub password: String,` (`create-owner`).
  - `cli.rs:101-103`: the same for `reset-owner-password`.
  - `docs/src/content/docs/docs/user/features/owner/troubleshooting.md:60`: `message-crate-server reset-owner-password --password 'the-new-password'`.
  - `docker/entrypoint-release.sh:21-23`: arguments are passed straight to `exec message-crate-server "$@"`, so `docker compose run --rm server reset-owner-password --password …` puts the password in the host's shell history and in the container's `/proc/<pid>/cmdline`.
- **Why it matters:** the owner's password is the one credential that reaches the accounts collection and server settings, and "the only thing between the network and every account" (`run-on-another-machine.md:46`). A password in argv is readable by every local user through `ps`, stays in `~/.bash_history` and in Docker's event log, and is the kind of secret a screen-share or a pasted terminal exposes.
- **Exploitability:** any other local user, or anyone who later reads the history file: `grep reset-owner-password ~/.bash_history`.
- **Remediation:** read the password from a file or standard input and keep the flag only for scripts that opt in.
  ```rust
  /// New password for the owner, read from this file (one line).
  #[arg(long, conflicts_with = "password_stdin")]
  pub password_file: Option<PathBuf>,
  /// Read the new password from standard input.
  #[arg(long)]
  pub password_stdin: bool,
  ```
  and in `run_reset_owner_password`: `let password = read_password(&args)?;` that trims one trailing newline. Update `troubleshooting.md:60` to `echo 'the-new-password' | message-crate-server reset-owner-password --password-stdin`, or an interactive prompt (`rpassword`) when stdin is a terminal.
- **Defense in depth:** the audit trail already records `password_set` by `command_line` (`db/audit_trail.rs:99`, `:146`), which is the right compensating control; keep it.

### DB-3. No request timeout and no statement interrupt; four slow searches stall auth

- **Severity:** Low (Medium if the server is reachable by untrusted users)
- **CWE:** CWE-400 (Uncontrolled Resource Consumption), CWE-770.
- **Evidence:**
  - `db/engine.rs:56`: `SqlitePoolOptions::new().max_connections(4)`; no `acquire_timeout`, `idle_timeout` or `max_lifetime` set. sqlx's defaults apply: `acquire_timeout: 30 s`, `idle_timeout: 10 min`, `max_lifetime: 30 min` (`sqlx-core-0.8.6/src/pool/options.rs:160-162`, cargo registry).
  - No `TimeoutLayer` or `tower_http::timeout` anywhere in `crates/server/server/src`; no `sqlite3_progress_handler` or `sqlite3_interrupt`.
  - `server.rs:1617` every authenticated request takes a pooled connection for `resolve_auth_on_conn` before the handler runs.
  - A free-text term on the Contacts and Conversations lists, and every `name:`/`filename:`-style word, compiles to `lower(column) LIKE lower(?) ESCAPE '\'` (`search/bridge.rs:48`), a full scan with a Unicode `lower()` per row (`db/sqlite_functions.rs`). Messages free text uses the FTS index instead (`search/fts.rs`).
- **Why it matters:** with four connections and no timeout, four concurrent slow list queries hold the pool; the fifth request, whatever it is, waits up to 30 s in `acquire` and then fails, login included. On a home network the only people who can do this hold a Session. If the server is exposed, anyone with an account, or the Demo Account (DB-11), can do it.
- **Exploitability:** PoC with a Session token: run four `GET /v1/contacts?q=<common letter>` requests at once against an archive with tens of thousands of contacts or conversations, then try `GET /v1/session`. The mitigations already present are real: `MAX_LIST_LIMIT 500`, `MAX_LIST_OFFSET 50 000` (`paging.rs:14`, `:19`), `MAX_JSON_BODY_BYTES` 32 MiB (`server.rs:1014`, `:1305`), and the FTS index for the heaviest list.
- **Remediation:**
  ```rust
  // db/engine.rs
  with_pragmas(
      SqlitePoolOptions::new()
          .max_connections(4)
          .acquire_timeout(std::time::Duration::from_secs(10)),
  )
  // server.rs http_app(): one layer for every /v1 route
  .layer(tower_http::timeout::TimeoutLayer::new(std::time::Duration::from_secs(30)))
  ```
  A timed-out request answers `408`/`503` as a Problem, which `docs/architecture/http-api.md` would need to name. For the statement itself, a progress handler registered through the same `sqlite3_auto_extension` hook as `db/sqlite_functions.rs` can call `sqlite3_progress_handler(db, 100_000, cancel_after_deadline, ..)` so a query stops when its request has gone.
- **Defense in depth:** keep imports on their own connection budget (design audit F7) so a long import cannot take two of the four.

### DB-4. "Delete for good" leaves message text in free pages and the WAL

- **Severity:** Low
- **CWE:** CWE-459 (Incomplete Cleanup), CWE-212 (Improper Removal of Sensitive Information Before Storage or Transfer).
- **Evidence:**
  - `db/engine.rs:17-38` sets `busy_timeout`, `foreign_keys`, `synchronous`, `temp_store`, `cache_size`; there is no `PRAGMA secure_delete`.
  - `VACUUM` runs only in `db/maintenance.rs:44`, called from `reset_demo.rs:2034` (the demo build). Trash deletion (`db/trash.rs`, "Permanent deletion … Trash is the only door", `:23-25`) and account deletion (`db/account_profile.rs:376`, `:424`) are plain `DELETE` statements.
  - `trash.rs`/`account_profile.rs` delete through `ON DELETE CASCADE` into `messages`, `attachments`, `message_versions` and the contentless FTS tables (`schema/sql/fts_virtual.sql`, `content=''`, `contentless_delete=1`).
- **Why it matters:** SQLite marks freed pages free; the bytes stay in `messagecrate.db` until the page is reused or the file is vacuumed, and a copy also sits in `messagecrate.db-wal` until the next checkpoint. Someone who copies the file after a person emptied the trash still reads the text with a hex editor or a forensic tool. The product calls this "delete for good" and records it in the audit trail as `trash_emptied` and `messages_deleted`, so the promise is stronger than the file keeps.
- **Exploitability:** read access to the file; `strings messagecrate.db | grep <deleted phrase>` after an empty-trash.
- **Remediation:** either
  ```rust
  // db/engine.rs with_pragmas, after busy_timeout
  sqlx::query("PRAGMA secure_delete = ON").execute(&mut *conn).await?;
  ```
  (SQLite then zeroes freed content as it frees it; the cost is extra writes on delete), or run `maintenance::vacuum_import_tables` and `PRAGMA wal_checkpoint(TRUNCATE)` at the end of `empty_trash`, `delete_trashed`, and `delete_account`. `secure_delete` is the simpler change and covers every path, including dedupe's `UPDATE … SET duplicate_of`.
- **Defense in depth:** document that the attachment files are unlinked, not shredded (`asset_store::remove_account_dir`, `asset_store.rs:469`), and that full-disk encryption is the control for that.

### DB-5. The database file and data directory take the process umask

- **Severity:** Low (only on a host shared by several operating-system users)
- **CWE:** CWE-276 (Incorrect Default Permissions).
- **Evidence:**
  - `db/engine.rs:41-45`: `SqliteConnectOptions::new().filename(path).create_if_missing(true)`; SQLite creates the file with mode `0644 & ~umask`.
  - `open_db.rs:96-101`: `std::fs::create_dir_all(parent)` for the database's directory; `src-tauri/src/local_server.rs:881`: `std::fs::create_dir_all(&launch.data_dir)`.
  - No `set_permissions`, `DirBuilderExt::mode`, or `OpenOptionsExt::mode` in `crates/server/server/src` (grep); the only `0o600` in the tree is the WhatsApp key file (`crates/exporters/whatsapp-exporter/src/wtsexporter.rs:519-523`), which shows the pattern the server could follow.
  - In Docker the volume is written by `node` (`Dockerfile:79-81`) and isolated by the container, so this is about a bare-metal or desktop install on a multi-user machine.
- **Why it matters:** with the common `umask 022`, `messagecrate.db`, its `-wal`, and every attachment under `data/<account>/assets/` are world-readable. Every message of every account is then readable by any local user without a password.
- **Exploitability:** `cat /path/to/data/messagecrate.db` as another user on the same machine.
- **Remediation:**
  ```rust
  // open_db.rs create_or_open, in place of create_dir_all(parent)
  #[cfg(unix)]
  {
      use std::os::unix::fs::DirBuilderExt;
      std::fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
  }
  #[cfg(not(unix))]
  std::fs::create_dir_all(parent)?;
  ```
  and the same for `data_dir` and each account directory in `asset_store.rs`. A `0700` directory makes the file mode moot, which is simpler than chasing every file SQLite creates.
- **Defense in depth:** the desktop app's app-data directory is already per-user on every platform, which is why this is Low.

### DB-6. Nothing is encrypted at rest; the documented backup is a plain tar.gz

- **Severity:** Low (accepted by the stated deployment model; the finding is that the docs do not say so)
- **CWE:** CWE-311 (Missing Encryption of Sensitive Data), CWE-312.
- **Evidence:**
  - The database is plain SQLite (`db/engine.rs`; no SQLCipher or similar in `Cargo.toml` of the server crate). Attachments are plain files sharded by SHA-256 (`asset_store.rs:228`, `assets_api.rs:298`). Message text is stored in `messages.body`/`subject` and indexed by a contentless FTS5 table, so there is one plaintext copy, not two.
  - `docs/…/run-on-another-machine.md:107-115`: the backup is `tar czf /backup/message-crate-data.tar.gz -C /data .`, moved by "a USB drive, a network share, or `scp`" (`:120`); no word on encrypting it.
  - `reset_demo.rs:1004`: `VACUUM INTO $1` writes a plaintext snapshot into a `tempfile::TempDir`; it is kept only when a rollback fails, and the error names where (`reset_demo.rs:699-712`).
- **Why it matters:** the archive is the product. A USB stick or a shared-drive copy of the tar is every message of every account in the clear. The home-network model relies on the disk being trusted; a backup leaves the disk.
- **Exploitability:** possession of the tar or the volume.
- **Remediation:** documentation first: in `run-on-another-machine.md`, encrypt the tar before it travels (`age -p` or `gpg -c`) and say that the volume's host should run full-disk encryption (FileVault, BitLocker, LUKS). Code second, if wanted: a `backup` server command that runs `VACUUM INTO` (the code at `reset_demo.rs:995-1016` already does this correctly, with the path bound) and then encrypts the result with a passphrase, so a backup is one consistent file rather than a stopped-container tar.
- **Defense in depth:** none needed beyond the above for the stated model.

### DB-7. The Apple Messages Reader opens a backup's `Manifest.db` read-write

- **Severity:** Low
- **CWE:** CWE-250 (Execution with Unnecessary Privileges), applied to the open mode.
- **Evidence:**
  - `crates/helpers/imessage-reader/src/domain.rs:86`: `let manifest = Connection::open(backup.manifest_db_path())?;` (rusqlite's default is `SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_CREATE`).
  - The same file is opened correctly elsewhere: `crates/libs/ios-backup/src/backup.rs:62`: `Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)`, and `imessage-database` opens `chat.db` with `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX` (registry `imessage-database-4.2.0/src/tables/table.rs:200-202`).
  - The query itself binds its one value (`domain.rs:166-170`).
- **Why it matters:** an iPhone backup is a person's only copy of their phone. A read-write open on a backup stored read-only fails where the read-only open at `backup.rs:62` succeeds; on a writable backup it can leave a `-journal` beside `Manifest.db` if the process dies mid-statement, and it would let a future bug write. `SQLITE_OPEN_CREATE` also means a mistyped backup path creates an empty `Manifest.db` instead of failing.
- **Exploitability:** none as an attack; it is a least-privilege gap in code that touches irreplaceable user data.
- **Remediation:**
  ```rust
  use rusqlite::OpenFlags;
  let manifest = Connection::open_with_flags(
      backup.manifest_db_path(),
      OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
  )?;
  ```
- **Defense in depth:** a Clippy `disallowed-methods` entry for `rusqlite::Connection::open` in the helper crate, as the server does for `sqlx::Connection::begin` (`db/write_tx.rs:15-17`).

### DB-8. `trusted_schema` is left on, so a supplied database file is trusted past `application_id`

- **Severity:** Low
- **CWE:** CWE-20 (Improper Input Validation) of the database file as input.
- **Evidence:**
  - `db/engine.rs:17-38`: no `PRAGMA trusted_schema = OFF`; no `SQLITE_DBCONFIG_DEFENSIVE` (grep over the crate for `trusted_schema`, `DEFENSIVE`, `query_only`, `cell_size_check`: none).
  - `open_db.rs:40-55` `inspect` reads `application_id` through a read-only connection first, and `OpenDb::open` refuses a foreign file (`:68-80`). That is a good check and it is the only one.
  - The server's own `lower()` replacement is registered `SQLITE_INNOCUOUS` (`db/sqlite_functions.rs:63`), so turning `trusted_schema` off costs nothing.
- **Why it matters:** `--db` and `[paths] db` accept any path; a restored backup is a file someone else may have made. With `trusted_schema=ON` (SQLite's default), views, triggers, `CHECK` constraints and indexes in that file may call any SQL function, including ones not marked innocuous, when the server reads it. `trusted_schema=OFF` and the defensive flag are SQLite's own recommendation for "database files that may have been tampered with".
- **Exploitability:** low; the attacker must get a crafted file in front of `serve`. The realistic path is a tampered backup restored under DB-6.
- **Remediation:**
  ```rust
  // db/engine.rs with_pragmas, first
  sqlx::query("PRAGMA trusted_schema = OFF").execute(&mut *conn).await?;
  ```
  `SQLITE_DBCONFIG_DEFENSIVE` needs the raw handle; the `sqlite3_auto_extension` hook in `db/sqlite_functions.rs:50-71` is the one place that has it: `ffi::sqlite3_db_config(db, ffi::SQLITE_DBCONFIG_DEFENSIVE, 1, ptr::null_mut::<c_int>())`.
- **Defense in depth:** `open_db.rs` could also run `PRAGMA quick_check` on a file whose `user_version` it did not write.

### DB-9. Token characters are drawn with a modulo bias

- **Severity:** Low (practically negligible; worth a one-line fix)
- **CWE:** CWE-331 (Insufficient Entropy), marginal.
- **Evidence:**
  - `db/session_tokens.rs:11`: `const TOKEN_ALPHANUM: &[u8] = b"ABC…xyz0123456789";` (62 symbols).
  - `db/session_tokens.rs:26-34`: `fill_random(&mut buf)?; for b in buf { suffix.push(TOKEN_ALPHANUM[(b as usize) % TOKEN_ALPHANUM.len()] as char); }` over 32 bytes from `rand::rngs::SysRng` (`:42-48`).
  - Used for Sessions (`mc-user-`) and API tokens (`mc-api-`, `db/api_tokens.rs:98-100`).
- **Why it matters:** 256 mod 62 = 8, so the first eight symbols (`A`–`H`) each come up 5/256 of the time and the rest 4/256. Entropy per character is about 5.95 bits instead of 5.954; the 32-character token still carries about 190 bits, far beyond brute force against a SHA-256 lookup. It is listed because the fix is trivial and a reviewer will otherwise ask.
- **Exploitability:** none in practice.
- **Remediation:** a 64-symbol alphabet removes the bias with a mask:
  ```rust
  const TOKEN_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
  suffix.push(TOKEN_ALPHABET[(b & 63) as usize] as char);
  ```
  `-` and `_` are URL- and header-safe.

### DB-10. Form credential stripping is a deny-list of two keys

- **Severity:** Low
- **CWE:** CWE-184 (Incomplete List of Disallowed Inputs).
- **Evidence:**
  - `imports_api/mod.rs:810`: `const FORM_CREDENTIAL_KEYS: [&str; 2] = ["backupPassword", "whatsappKey"];` removed by `strip_form_credentials` (`:813-821`) and applied on the write path at `:1293`: `let form = body.form.as_ref().map(strip_form_credentials);` before `imports.form_json` is written (`db/imports.rs:411`).
  - The form model carries `backup_password` and `whatsapp_key` as ordinary fields (`crates/core/message-crate-core/src/exporters.rs:186`, `:205`), and the web app holds both in component state (`web/src/screens/ImportScreen.tsx:210`, `:219`, `:803`, `:816`).
  - The server-side strip is the right place and it works today. The gap is that a third secret added to the form (a WhatsApp iOS backup password field, a future key file passphrase) is stored in `imports.form_json` until someone remembers the constant.
- **Why it matters:** `imports.form_json` is read back to restore the Import screen and lives as long as the Import Run row; a secret typed once would outlive the run in the database.
- **Exploitability:** none today. A regression risk.
- **Remediation:** make the stored shape explicit instead of subtracting from an open object. Either an allow-list of the keys the Import screen restores, or a `#[serde(skip_serializing)]` on the secret fields of the form type the client posts, plus a unit test that serializes a form with every secret set and asserts none appears. Keep the server-side strip as the second line.

### DB-11. The Demo Account logs in with an empty password on a network-published server

- **Severity:** Informational (by design; `docs/adr/0016-the-demo-account-is-fixed-not-configured.md`)
- **Evidence:** `schema/sql/accounts.sql` `password_hash TEXT` (nullable); `credentials.rs:170-176` "A missing or empty hash means the account has no password, so only an empty password is accepted"; `config/config.docker.toml:17` `bind = "0.0.0.0:8080"`; `account_profile::load_account_auth` grants `DEMO_ACCOUNT_PERMISSIONS` (export only), and `require_import_access`/`require_delete_access` refuse it by id (CLAUDE.md).
- **Why it matters:** on a Docker install reachable by the network, every device on the LAN can log in as `demo` and read and export the demo conversations. They are generated, not a person's, and the account cannot import or delete. The docs tell owners to keep registration off on a network (`run-on-another-machine.md:47`); they do not mention that `demo` is open. One sentence there, and a Server Setting to disable the Demo Account, would close the question. Not counted as a finding against the design.

## 5. What is done well

- One SQL builder (`search/bridge.rs::Sql`) with `push` for constants and `bind_*` for values, and the compiler never calls `push` with request text. The 4 100-line `search/tests.rs` fixes the SQL of every word.
- `like_escape` plus `ESCAPE '\'` (`search/emit.rs:218-227`, `bridge.rs:48`); FTS5 terms quoted and bound (`search/fts.rs:11-13`).
- `WriteTx` is the only transaction type, begun `BEGIN IMMEDIATE`, with Clippy refusing `sqlx::Connection::begin` and a test-only SQLite authorizer that fails any write to `messages`/`attachments` outside one (`db/write_tx.rs`, `db/write_guard.rs`).
- `PRAGMA foreign_keys = ON` on every connection; every tenant table has `account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE` (`schema/sql/*.sql`); `db/ownership.rs` answers "404 for another account's id" in one place; the single-message route filters `m.account_id = ? AND m.id = ?` (`messages_api.rs:209-212`); assets resolve under `assets_dir_for_account(account)` (`assets_api.rs:637-644`).
- Argon2id with the crate's defaults and a dummy hash for unknown usernames (`credentials.rs:148-194`); Sessions and API tokens stored as SHA-256 of a 190-bit random string, one Session per account rotated on login, 30-day TTL, API tokens with expiry, hint and revocation (`db/session_tokens.rs`, `db/api_tokens.rs`); the Media Link key is 32 OS-random bytes made at start and never written (`assets_api/media_links.rs:58-64`).
- Logging: query values hidden by allow-list (`server.rs:1194-1250`), `RUST_LOG` default `info` (`logging.rs:46`), sqlx logs SQL text only (statements at DEBUG, slow ones at WARN; `sqlx-core-0.8.6/src/connection.rs:194-201`), and the rule is tested at `trace` (`docs/architecture/server-log.md:135-140`).
- The WhatsApp key goes to a `0600` file in a scratch directory and `-k` takes the path, so it never appears in `/proc/<pid>/cmdline` (`wtsexporter.rs:315-326`, `:512-530`). The iPhone backup password goes to the GPL helper as a JSON line over stdin (`crates/libs/ios-backup/src/identity.rs:71-78`, `helper.rs:139-159`). Both are stripped from `imports.form_json` server-side (DB-10).
- `VACUUM INTO $1` with the path bound (`reset_demo.rs:1004-1006`); `quote_ident` for every identifier read from `sqlite_master` (`db/schema.rs:253-255`).
- Audit trail of logins, refused logins, password sets, permission changes, deletions, token lifecycle, address-book loads and exports, and every Import and Export Run with the credential used (`db/audit_trail.rs:93-112`; `schema/sql/accounts.sql` `audit_entries`, `imports.credential`).
- No committed secrets: `config/config.toml` is git-ignored (`.gitignore:32`), `.env.example` holds only `COMPOSE_FILE`, `UID`, `GID`.

## 6. Prioritized fixes

1. **DB-1:** `VACUUM INTO` a copy before `rebuild_schema` drops anything, and stop calling `ensure_accounts_schema` from `resolve_auth_on_conn`, `session_api.rs:273` and `api_tokens.rs:156`. Two small edits in `db/schema.rs` and three deletions; removes the one way the product loses an archive.
2. **DB-2:** `--password-stdin` / `--password-file` on `create-owner` and `reset-owner-password`; fix `troubleshooting.md:60`.
3. **DB-4 + DB-8:** two pragmas in `with_pragmas`: `PRAGMA secure_delete = ON` and `PRAGMA trusted_schema = OFF`. One edit in `db/engine.rs`, with the five-pragma test at `engine.rs:107-149` extended.
4. **DB-3:** `acquire_timeout(10 s)` on the pool and a `TimeoutLayer` on `/v1`.
5. **DB-5 + DB-7 + DB-9:** `0o700` for the data directory, read-only open for `Manifest.db`, 64-symbol token alphabet. Each is one line.

## 7. Checklist

| # | Item | Result | Evidence |
|---|------|--------|----------|
| 1 | Parameterized queries | **Pass** | Section 3; every value bound, identifiers const or allow-listed |
| 2 | Connection string security | **Pass** | File path only (`config.rs:212`, `engine.rs:41-45`); `config/config.toml` ignored (`.gitignore:32`); no `.env` secrets |
| 3 | Database user permissions | **N/A** | SQLite has no roles; Docker runs as `node` (`Dockerfile:81`); no separate read-only path exists (`open_db.rs::inspect` is the one read-only open) |
| 4 | Sensitive data encryption at rest | **Fail** | DB-6; accepted by the deployment model, undocumented |
| 5 | PII handling | **Pass** | Logs redacted (`server.rs:1194-1250`); form credentials stripped (`imports_api/mod.rs:810-821`); account deletion removes rows by cascade and the data directory (`accounts_api.rs:957-970`); Trash is the only door to deletion |
| 6 | Query timeout configuration | **Fail** | DB-3; no statement or request timeout |
| 7 | Connection pool settings | **Pass** | `max_connections(4)`; sqlx defaults acquire 30 s, idle 10 min, lifetime 30 min; bounded |
| 8 | Transaction handling | **Pass** | `WriteTx`/`BEGIN IMMEDIATE`, Clippy ban on `begin`, test authorizer (`db/write_tx.rs`, `db/write_guard.rs`) |
| 9 | Audit logging for sensitive operations | **Pass** | `db/audit_trail.rs:93-112`; centralization and immutability N/A for a home server; owner reads it over `/v1/server/log-lines` |
| 10 | NoSQL injection | **N/A** | No NoSQL |
| 11 | Row/tenant isolation | **Pass** | `account_id NOT NULL` + FK on every tenant table; `db/ownership.rs`; `ListCtx::account_col`; per-account asset directory |
| 12 | Least-privilege networking (database) | **N/A** | In-process SQLite; the HTTP API's `0.0.0.0` bind in Docker is documented (`config.docker.toml:11-17`) and out of this audit's layer |
| 13 | TLS in transit (database) | **N/A** | In-process; the API is plain HTTP by documented design |
| 14 | Secret management and rotation | **Pass** (DB-2 noted) | Argon2id; hashed tokens; Session rotates on login; Media Link key per start; API token expiry |
| 15 | Schema and integrity controls | **Pass** | `PRAGMA foreign_keys = ON`; `NOT NULL`, `UNIQUE`, `CHECK (id = 1)` singletons (`schema/sql/*.sql`) |
| 16 | Field-level minimization | **Pass** | `SELECT *` only over derived tables with explicit lists |
| 17 | Pagination and query limits | **Pass** | `MAX_LIST_LIMIT 500`, `MAX_LIST_OFFSET 50_000`, `LIMIT ? OFFSET ?` bound |
| 18 | Backup/restore security | **Fail** | DB-6; restore is documented and checked (`run-on-another-machine.md:126-140`), encryption is not |
| 19 | Data retention and deletion | **Fail** | DB-4; policy exists (Trash → delete for good), erasure is not secure |
| 20 | Migrations safety | **Fail** | DB-1; `application_id` check is the only guard, no copy, runs from a request |
| 21 | ORM raw-query escape hatch | **N/A** | No ORM; all SQL is hand-written and covered by item 1 |
| 22 | LIKE / regex input handling | **Pass** | `like_escape` + `ESCAPE '\'`; FTS5 literal quoting |
| 23 | Query timeouts and resource guards | **Fail** | DB-3; body limit 32 MiB and list caps are present |
| 24 | Audit and monitoring depth | **Fail** | A schema rebuild is a `tracing::warn!`, not an audit entry or alert (`db/schema.rs:159-165`); no anomaly detection (acceptable for the model, listed for completeness) |
| 25 | PII in logs/metrics | **Pass** | Allow-list redaction; sqlx logs SQL text without values; tested at `trace` |
| 26 | Indexing of sensitive data | **Pass** | `token_hash` indexed, never the token; FTS tables contentless (`fts_virtual.sql`) |
| 27 | Service/account lifecycle | **Pass** | API tokens expire (default 365 days), can be revoked, capped by the account's permissions; Sessions 30 days; Demo Account fixed and limited (DB-11) |
| 28 | Caching layers | **N/A** | No Redis/Memcached; browser-side TanStack Query keyed by account (ADR 0002) |
| 29 | Analytics/ETL exports | **N/A** | Export Runs are the account's own data, recorded in the audit trail; no third-party analytics |

## 8. Unable to verify

- **`asset_store::remove_account_dir` mechanism** (`crates/server/server/src/asset_store.rs:469`): the call is present on account deletion (`accounts_api.rs:957-962`); whether it is `remove_dir_all` directly or a rename-then-remove was not read. The function body proves it.
- **The web client's own stripping of `backupPassword`/`whatsappKey` before `POST /v1/imports`**, which `imports_api/mod.rs:806-807` says it does. `web/src/screens/import/useImportJob.ts` around line 258 would show it. The server strips regardless, so the finding (DB-10) does not depend on it.
- **`crates/server/server/tests/server_log.rs`** is cited by `docs/architecture/server-log.md:135-140` as the trace-level secret check; the test file itself was not read.
- **Sqlx-sqlite pool behaviour in `vendor/sqlx-sqlite`**: treated as the upstream 0.8.6 per `VENDORING.md`; the pool options come from `sqlx-core`, which is not vendored, and were read from the cargo registry.
- **Whether any production path in `crates/helpers/imessage-reader` other than `domain.rs:86` opens a SQLite file read-write.** The grep over `Connection::open` found only test modules and `get_connection` (read-only) elsewhere; a Clippy `disallowed-methods` rule (DB-7) would make this provable.

## 9. Cross-references to the design audit (`audits/2026-10-09-initial-software-design-analysis.md`)

- **F2** (`db/` returns `ApiError`): no security consequence found; `db/conversation_messages.rs:392` and `:402` construct validation errors for a bad `sort`, which is a layering question, not a data one.
- **F7** (dedupe SQL outside `db/`, import concurrency): the ~20 statements in `dedupe.rs` are all constants or placeholder builders with bound values (Section 3); placement is a maintainability issue, not an injection one. The shared 4-connection pool is why DB-3 matters.
- **F13** (`reset_demo.rs` holds SQL): the `VACUUM INTO` and the digest queries are parameterized and identifier-quoted; no finding.
- **F18** (schema check on every authenticated request): DB-1 is the security reading of the same call. F18 asks for its removal as a smell; DB-1 asks for it because the call can drop every table.
