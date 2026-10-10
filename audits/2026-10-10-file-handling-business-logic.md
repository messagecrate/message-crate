# File handling and business logic audit

- Audit: file-handling-business-logic (code-audit skill 0.1.0)
- Date: 2026-10-10
- Commit: `530dc1791` (branch `docs/initial-software-design-analysis`)
- Scope: the whole repository less `web-next/`, `vendor/`, `target/`, `node_modules/`, `docs/dist`
- Method: read-only review of the source. Nothing was built or run. Every claim below cites a file and line that was read; where the proving code was not read, the item is marked **Unable to verify** and names the file that would prove it.
- Cross-reference: `audits/2026-10-09-initial-software-design-analysis.md` (design audit). Its F12 (file helpers living in `message-ir`) and F17 (write queue loaded whole, media pass without a timeout) are not repeated; F17 is cited where its missing timeout becomes a security matter.

## 1. Threat model and assumptions

The server stores private message archives and their attachments. It speaks plain HTTP and is published to `127.0.0.1` by default; `docs/src/content/docs/docs/user/features/owner/run-on-another-machine.md:33-41` says a server on a home network is acceptable "where every device is trusted" and that anything beyond that "needs a reverse proxy with TLS in front of the server". `SECURITY.md` says both the desktop app and the server "run on machines you control". The audit therefore rates each finding against four attackers, and says which one it needs:

- **A hostile backup or attachment.** Media and conversation files come from phone backups. A contact in a group chat can send a crafted video or file; it lands in the person's backup and is then parsed by the exporters, uploaded, and converted by the server's media pass. A crafted conversation file (JSONL, CSV) can carry hostile paths and metadata. This attacker needs no account.
- **An authenticated account holder.** On a shared server (Docker on a LAN, registration open), another account holds a session or an import token and can call every `/v1/assets` route.
- **A local user on the same host.** Another Unix user on the machine the server or the desktop app runs on.
- **A network attacker.** Out of scope: the product documents plain HTTP and leaves TLS to a proxy.

Nothing here is reachable anonymously except the Demo Account (`CLAUDE.md`, "demo logs in with an empty password"), which holds synthetic data by design.

## 2. Summary

The file-handling code is careful and consistent. Every path the server builds from request data goes through a typed fingerprint (`Sha256`) or an integer id; every attachment path read from a conversation file goes through one guard (`message_ir::safe_attachment_path`); stored bytes are hashed while they are copied and again on every dedupe hit; symlinks are refused at open time; writes go through a temporary file, `fsync`, and a rename; and served bytes carry `X-Content-Type-Options: nosniff` and a fixed `Content-Disposition: attachment`. No path traversal, archive extraction, SHA-256 bypass, or stored-XSS path was found.

What remains is defence in depth: the media pass runs ffmpeg and ffprobe on attacker-supplied files with no deadline or resource limit; an authenticated importer can hold large request bodies in memory and on the temp disk with no concurrency bound; data directories take the process umask rather than an owner-only mode; and the declared MIME type is stored verbatim with no grammar check and no sniffing.

| Severity | Count |
|---|---|
| Critical | 0 |
| High | 0 |
| Medium | 2 |
| Low | 7 |

**Risk score: 3.5 / 10.** The two Medium findings need a hostile media file or a hostile account holder; the Low findings need a damaged database, a shared host, or an owner misconfiguration.

## 3. Findings

### M1. ffmpeg and ffprobe run on attacker-supplied media with no deadline, resource limit or sandbox

- **Severity:** Medium
- **CWE:** CWE-400 (uncontrolled resource consumption), CWE-20 (improper input validation, delegated to an external decoder)
- **Attacker:** a hostile attachment in the person's own backup; no account needed
- **Evidence:**
  - `crates/libs/media/src/tools.rs:416-470` `run_ffmpeg_with`: `Command::new(ffmpeg).args(QUIET_FFMPEG).args(args)` is spawned and waited on. The only way it ends early is the `stop: Option<&AtomicBool>` flag polled at `:434-456`; there is no wall-clock deadline, no `setrlimit`, no CPU or memory cap, and no `-protocol_whitelist`.
  - `crates/libs/media/src/versions.rs:89-103` `browser_shows` runs `probe_video` (ffprobe) on every asset whose declared type is `video/mp4`, where the declared type is whatever the import sent (finding L2).
  - `crates/server/server/src/process_assets.rs:921-945` `derive` calls `media::make_thumbnail` and `media::make_preview` on each queued original; `docs/architecture/media.md`, rule 4, queues every image and video of every Import Run automatically, in the server process, after each import.
  - `docker/Dockerfile:56-61` installs `ffmpeg` from Debian bookworm; the desktop app downloads a pinned static build (`src-tauri/src/tool_downloads.rs:257-266`, `:487-513`).
  - The design audit's F17 names the missing timeout as a throughput problem; this finding is the security side of the same fact.
- **Why it matters:** ffmpeg is the largest attack surface in the product and the only component that parses attacker-controlled binary formats. A video crafted to decode forever, or to allocate without bound, stalls the media pass for every account (it is one thread, `media.md` rule 4) and can exhaust the server's memory; a decoder bug in the ffmpeg build runs as the server user with read access to every account's data directory and the database.
- **Exploitability:** Send a crafted MP4 or HEIC to the target in any chat the target later backs up and imports. No credentials. A decode hang is trivial to produce (for example a file with a looping stream map); code execution depends on the ffmpeg build.
- **Remediation:**
  1. Give `run_ffmpeg_with` and the ffprobe call a deadline, killing the child when it passes:
     ```rust
     // crates/libs/media/src/tools.rs, in run_ffmpeg_with
     const FFMPEG_DEADLINE: Duration = Duration::from_secs(15 * 60);
     let started = Instant::now();
     loop {
         if let Some(status) = child.try_wait().context("wait for ffmpeg")? { break status; }
         if stopped() || started.elapsed() > FFMPEG_DEADLINE {
             let _ = child.kill(); let _ = child.wait(); drop(reader);
             bail!("ffmpeg stopped after {}s", started.elapsed().as_secs());
         }
         std::thread::sleep(pause); pause = (pause * 2).min(STOP_POLL_MAX);
     }
     ```
     Run the `stop.is_some()` branch unconditionally so the deadline applies to the plain `run_ffmpeg` path too.
  2. Limit what the child may read and allocate: pass `-protocol_whitelist file,pipe` and `-nostdin` with `QUIET_FFMPEG` (`tools.rs:370`), and on Unix set `RLIMIT_AS` and `RLIMIT_CPU` in `Command::pre_exec`, or run the Docker image with `--memory` and `--pids-limit`.
  3. Record the failure per asset (the queue already drops a failing asset, `media.md` rule 4) and never requeue the same fingerprint automatically after a deadline kill, so one file cannot stall every pass.
  4. Keep the pinned desktop build current; the manifest at `src-tauri/src/tool_downloads.rs:257-266` is the one place to bump it.

### M2. An authenticated importer can hold unbounded memory and temp disk

- **Severity:** Medium on a shared server (Docker on a LAN, or registration open); Low on the desktop app's own server
- **CWE:** CWE-770 (allocation without limits or throttling), CWE-400
- **Attacker:** an account holder with the `import` permission or an import token
- **Evidence:**
  - `crates/server/server/src/assets_api.rs:1275` `replace_asset_upload_part` reads the whole part into memory with `read_body_limited(request.into_body(), part_size)`. `part_size` is the upload's recorded size, by default `DEFAULT_PART_SIZE` = 64 MiB (`crates/server/server/src/asset_uploads.rs:18`). Nothing bounds how many part requests run at once.
  - `crates/server/server/src/imports_api/mod.rs:1731-1739` `create_import_batch` streams the JSONL body to `tempfile::tempdir()`, the system temp directory, up to `MAX_REQUEST_BODY_BYTES` = 512 MiB (`crates/server/server/src/server.rs:1021`). The import semaphore `MAX_CONCURRENT_IMPORTS = 2` (`mod.rs:1767-1772`) is taken inside `run_import_path` (`mod.rs:1785`), after the body is on disk, so any number of 512 MiB bodies can be streaming into `/tmp` at once.
  - `crates/server/server/src/jsonl.rs:62-82` `read_records` pushes every line into `Vec<String>` before `parse_ir_lines`, so a 512 MiB body becomes at least 512 MiB of strings plus the parsed records while it is staged.
  - Multipart staging has no per-account cap on open sessions: `start_upload` (`asset_uploads.rs:201-241`) checks only `bytes > limits.max_bytes` and sweeps sessions older than `STALE_UPLOAD_SECS` = 24 h (`crates/server/server/src/asset_store.rs:703`), so an importer can keep N x 512 MiB of parts on disk for a day.
  - `lookup_by_sha256` (`assets_api.rs:179-186`) re-hashes the stored file on every `HEAD /v1/assets/{sha256}` and every upload start (`:632-648`, `asset_uploads.rs:218`). The code documents this as deliberate (`:173-178`), and it means one probe of a 512 MiB asset reads 512 MiB from disk.
- **Why it matters:** On a server shared by several accounts, one account can take the server down for the others with its own credentials, and nothing records it as abuse. On the desktop app's server there is one account and the risk is only self-inflicted.
- **Exploitability:** With one import token: open 20 multipart uploads and send 20 parts of 64 MiB concurrently (1.3 GiB resident), or post 10 concurrent 512 MiB JSONL bodies (5 GiB in `/tmp`). Needs the `import` scope.
- **Remediation:**
  1. Take the import semaphore before streaming the body: move the `import_semaphore().acquire()` from `run_import_path` to the top of `create_import_batch`, or use `tempdir_in(&cfg.paths.data_dir)` so the body lands on the data disk whose free space the owner watches rather than `/tmp`.
  2. Stream a part to its file instead of buffering it: replace `read_body_limited` + `put_part(&body)` with `stream_body_to_file` into `part_path(session, part)` under the session lock, then record it in the manifest. This removes the 64 MiB resident buffer per request.
  3. Add a per-account `tokio::sync::Semaphore` for asset writes (for example 4) in `AppState`, taken in `replace_asset`, `replace_asset_upload_part` and `complete_asset_upload`.
  4. Cap open multipart sessions per account in `start_upload` (count `.incoming/*/*/manifest.json`, refuse past 16 with `AssetError::Invalid`).
  5. Parse the JSONL line by line into the staging tables rather than collecting `Vec<String>` first (`jsonl.rs:65-82`); `BufReader::lines()` already yields one line at a time.

### L1. Data directories and most files take the process umask, not an owner-only mode

- **Severity:** Low (needs another local user on the same host)
- **CWE:** CWE-732 (incorrect permission assignment for critical resource)
- **Evidence:**
  - The server never calls `set_permissions` or sets a `mode`: a grep of `crates/server/server/src` for `set_permissions`, `mode(0o`, `PermissionsExt`, `umask` matches nothing. Directories are made with `fs::create_dir_all` (`asset_uploads.rs:225`, `assets_api.rs:298`, `:1053`, `:1077`), the raw-upload temp with `File::create` (`server.rs:1763-1772`), parts with `fs::write` (`asset_uploads.rs:375`).
  - Stored originals, sidecars and Previews do get `0600`, because `shard_temp_file` (`asset_store.rs:765-769`) is a `tempfile::NamedTempFile`, which tempfile creates owner-only, and `persist` keeps that mode.
  - The desktop app makes the server's data directory with `std::fs::create_dir_all` (`src-tauri/src/local_server.rs:881`) under the app-data directory.
  - The project knows the pattern: `crates/core/message-crate-core/src/scratch.rs:181-186` `restrict_to_owner` sets `0o700` on scratch directories "because it holds decrypted message data or attachment payloads". The data directory, which holds the same data permanently, does not get it.
  - **Unable to verify:** the mode of `messagecrate.db`. sqlx opens it through SQLite's default (`0644` less umask) unless told otherwise; proving code is the `sqlx-sqlite` connect options in `~/.cargo/registry` and `crates/server/server/src/db/` where the pool is opened.
- **Why it matters:** On a multi-user Linux host with the default `umask 022`, the account directories are world-listable and the database, part files and upload temps world-readable; the database holds every message. The Docker image runs as `node` in its own container (`docker/Dockerfile:79-81`), which contains this; a bare-metal server or the desktop app on a shared workstation does not.
- **Remediation:**
  ```rust
  // at server start (cli.rs serve), before any directory is made
  #[cfg(unix)]
  unsafe { libc::umask(0o077); }
  ```
  and after each `create_dir_all` of `data_dir` or an account directory, `fs::set_permissions(dir, Permissions::from_mode(0o700))` as `scratch.rs:185` does. Do the same in `src-tauri/src/local_server.rs:881`.

### L2. The declared MIME type is stored verbatim, is immutable, and nothing sniffs the bytes

- **Severity:** Low
- **CWE:** CWE-434 (unrestricted upload of file with dangerous type), mitigated by serving headers; CWE-20
- **Evidence:**
  - `crates/server/server/src/assets_api.rs:994-1003` `require_content_type`: "Any type is accepted". `upload_content_type` (`server.rs:1698-1705`) takes the `Content-Type` base verbatim; `CreateAssetUploadRequest.mime` (`assets_api.rs:1117`) is any JSON string.
  - `store_mime_metadata` (`assets_api.rs:514-536`) trims and writes it to the `.{sha}.mime` sidecar with no grammar or length check; `persist_noclobber` (`:531-533`) means the first writer's type wins for that fingerprint forever. `read_mime_metadata` (`:500-511`) caps the read at 1024 bytes.
  - On serve, `stream_file` (`:951-965`) sets `Content-Type` from the sidecar if `HeaderValue::from_str` accepts it, and always adds `X-Content-Type-Options: nosniff` (`:966-969`) and `Content-Disposition: attachment; filename="asset"` (`:971-974`).
  - Type detection is by extension or declaration everywhere: `crates/libs/media/src/mime.rs:79-129` is a table; `crates/core/message-crate-core/src/attachment_jobs.rs:414-417` `mime_for_rel` uses the extension; `resolve_mime` (`assets_api.rs:539-547`) prefers the declaration. The only byte-level inspection is ffprobe on MP4s (`versions.rs:96-102`).
- **Why it matters:** The serving headers mean a `text/html` or `image/svg+xml` upload is downloaded, never rendered, when a URL is opened directly, and media elements do not execute scripts, so no stored XSS exists through this route. What remains: a wrong first declaration is permanent for that fingerprint and drives `shown_as_is` and the media pass (a non-MP4 declared `video/mp4` is ffprobed; an MP4 declared `application/octet-stream` never gets a Preview); a 1 MiB "mime" string is written to disk by a single request.
- **Remediation:**
  ```rust
  // assets_api.rs, before store_mime_metadata
  fn valid_media_type(s: &str) -> bool {
      s.len() <= 255 && s.split_once('/').is_some_and(|(t, sub)|
          !t.is_empty() && !sub.is_empty()
          && t.chars().chain(sub.chars()).all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c)))
  }
  ```
  Refuse with `AssetError::Invalid` otherwise. For the browser-shown set in `media::browser_shows`, confirm the declared type against the file's magic bytes (JPEG `FF D8 FF`, PNG `89 50 4E 47`, GIF `GIF8`, WebP `RIFF....WEBP`, MP3 `ID3` or `FF Fx`, MP4 `....ftyp`) before answering `shown_as_is = true`.

### L3. Preview and Thumbnail serving joins a database path without the `join_under` guard the delete path uses

- **Severity:** Low (needs a damaged or tampered database)
- **CWE:** CWE-22 (defence in depth)
- **Evidence:**
  - `crates/server/server/src/assets_api.rs:849-863` `stream_version`: `converted_dir.join(path)` where `path` is `derived_assets_path` or `thumbnail_assets_path` from `file_of` (`crates/server/server/src/db/attachment_versions.rs:316-340`).
  - The deletion path refuses a stored path that is not plain and relative: `paths_of` (`crates/server/server/src/asset_store.rs:336-343`) uses `join_under` (`:245-249`), which allows only `Component::Normal` parts.
  - Mitigations already in place: `stream_file` opens with `O_NOFOLLOW` (`assets_api.rs:924`) and refuses non-regular files (`:930-932`); the server writes every such path itself (`process_assets.rs:1160-1164` `derived_rel_path`).
- **Why it matters:** The database is trusted, but the code already treats it as untrusted on the delete side. An absolute or `..` path written into `attachments.derived_assets_path` by a bug or a tampered `messagecrate.db` would be opened and streamed to the account.
- **Remediation:**
  ```rust
  // assets_api.rs stream_version
  let Some(full) = crate::asset_store::join_under(&converted_dir, &path) else {
      return Err(ApiError::NotFound("asset has no stored version".into()));
  };
  stream_file(&full, mime_type, headers, None).await
  ```

### L4. The HTTP import resolves a conversation file's relative attachment path against the account's own asset store

- **Severity:** Low (same-account only)
- **CWE:** CWE-73 (external control of file name or path), contained by `safe_attachment_path`
- **Evidence:**
  - `crates/server/server/src/imports_api/mod.rs:1820` sets `asset_root: &assets_dir`, the account's originals directory, for every batch; `ImportOptions.asset_root` is documented as "Root for resolving relative attachment paths in JSONL" (`mod.rs:63`).
  - `crates/server/server/src/imports_api/staging.rs:128-132` `store_claimed_or_path` and `:90-111` `try_store_converted` resolve `att.path` with `safe_source(export_dir, rel, line)` (`:70-73`), then `hash_and_store(source)` or run the media rewrite on it.
  - `safe_attachment_path` keeps the path under its base but allows dot-prefixed names (`crates/libs/ir/src/attachment_path.rs:117-122`, `..hidden.jpg` is accepted by design).
- **Why it matters:** A JSONL line with `"path": ".incoming/<sha>-<n>.part"` or `"path": "aa/.<sha>.mime"` names a live upload temp or a MIME sidecar of the same account, and the import stores those bytes as a new Asset under their real hash. Nothing crosses an account boundary and nothing leaves `data_dir/<account>/assets`, so the effect is clutter, not disclosure. The HTTP import never carries attachment files (the route comment at `mod.rs:1756-1758` says so), so a `path` that resolves to a file is always a surprise.
- **Remediation:** For the HTTP route, treat `att.path` as a label only: pass an empty, dedicated directory as `asset_root` (for example `media_work.path()` from `stage_all_files`, `mod.rs:461`), or skip `safe_source` + `hash_and_store` when `opts.asset_root == opts.assets_dir`. Keep the path check so an unsafe path is still refused with `ImportFailure::UnsafeAttachmentPath`.

### L5. Upload temp names are predictable and created without `create_new`

- **Severity:** Low (server-owned directory, per-account, authenticated)
- **CWE:** CWE-377 (insecure temporary file), CWE-362
- **Evidence:**
  - `crates/server/server/src/asset_uploads.rs:86-91` `new_upload_id` is the nanosecond clock in hex; `session_dir` (`:107-112`) joins it; `start_upload` uses `create_dir_all` (`:225`), which succeeds on an existing directory, then `write_manifest` (`:233`) overwrites `manifest.json`.
  - `crates/server/server/src/assets_api.rs:1060-1068` names the raw-upload temp `{sha256}-{nanos}.part` and `create_dest_file` (`server.rs:1763-1772`) opens it with `File::create`, which truncates an existing file.
  - `write_manifest` (`asset_uploads.rs:160-168`) uses a fixed `manifest.json.tmp` sibling; it is written under the session lock (`:351`, `:413`) except in `start_upload` (`:233`), where the directory is new.
- **Why it matters:** Two requests in the same account that land on the same nanosecond share a directory or a temp file; the SHA-256 check at completion (`:384`, `:416`) means a wrong file is refused rather than stored, so the outcome is a failed upload, not corruption. A local attacker who can write into `.incoming/` would need to be the server user already.
- **Remediation:** Append random bytes to the id (`rand::rngs::SysRng`, already a dependency in `media_links.rs:60`) and use `OpenOptions::new().write(true).create_new(true)` for the `.part` file, mapping `AlreadyExists` to a retry with a new name.

### L6. Every upload start walks the whole `.incoming/` directory

- **Severity:** Low
- **CWE:** CWE-405 (asymmetric resource consumption)
- **Evidence:** `asset_uploads.rs:215-216` calls `crate::asset_store::sweep_incoming(assets_root, false)` on every `POST /v1/assets/{sha256}/uploads`; `sweep_incoming` (`asset_store.rs:712-743`) lists `.incoming/`, every `{sha}/` directory, and stats each session's manifest (`:851-917`).
- **Why it matters:** The cost of starting an upload grows with the number of abandoned sessions, which the same importer can create (M2). It is O(sessions) of `stat` calls per request, which is cheap until the directory holds thousands.
- **Remediation:** Run `sweep_incoming` on a timer (for example with `sweep_after_run`, `asset_store.rs:588`) or at most once per hour per account, not per request.

### L7. Nothing refuses a `data_dir` inside `static_dir`

- **Severity:** Low (owner misconfiguration only)
- **CWE:** CWE-552 (files accessible to external parties)
- **Evidence:** `crates/server/server/src/config.rs:107-134` `into_config` validates that `assets_dir` and `assets_converted_dir` are directory names and differ; nothing compares `paths.data_dir` with `server.static_dir`. `crates/server/server/src/server.rs:1302` serves `static_dir` whole with `ServeDir::new(static_dir)` and no authentication. Both paths are owner-configured (`config.rs:299-310`, `--static-dir` at `cli.rs:240-243`).
- **Why it matters:** An owner who sets `[paths] data_dir = "static/data"` publishes every account's originals (named by fingerprint, so they need to be guessed) and `messagecrate.db` to anyone who can reach the port.
- **Remediation:** In `Config::load`, after both are resolved, `bail!` when `data_dir.starts_with(&static_dir)` or `static_dir.starts_with(&data_dir)`.

### L8. The media pass's directory walk follows symlinked directories

- **Severity:** Low (the directory is one the app made)
- **CWE:** CWE-59 (link following)
- **Evidence:** `crates/libs/media/src/process.rs:225-258` `collect` and `remove_msgmedia_temps` use `path.is_dir()`, which follows a symlink, and push the target onto the walk stack. The walk runs over a run directory the desktop app created and marked (`src-tauri/src/run_directories.rs:217-298`), so a symlink there would have to be planted by the same user.
- **Remediation:** Use `entry.file_type()?.is_dir()` (does not follow) and skip `is_symlink()` entries, as `placeholders.rs:48-62` already does for the obfuscated export.

### L9. Mail header values: one path is **Unable to verify**

- **Severity:** Low (desktop-only export the person writes to their own disk)
- **CWE:** CWE-93 (CRLF injection), not confirmed
- **Evidence:**
  - The product's own `X-ME-*` headers are safe: `crates/libs/mail/src/lib.rs:667-683` `x_me_value` writes a value verbatim only when every byte is a space or ASCII graphic, and Q-encodes anything else, so a CR or LF in message metadata cannot break a header.
  - Addresses are safe: `synthetic_email` (`lib.rs:431-442`) builds the local part through `address_local_part` and `dot_atom`, whose comment at `:444-451` says "no space or line break is left in it".
  - Attachments are base64 MIME parts (`lib.rs:812-820`), so bytes cannot inject a boundary.
  - **Unable to verify:** `Subject` (`mail_subject`, `lib.rs:1039-1042`, built from a group title or contact name) and display names go through mail-builder 1.0.0's `Text` header (`~/.cargo/registry/src/*/mail-builder-1.0.0/src/headers/text.rs:39-52`), which picks an encoding with `get_encoding_type(bytes, true, false)` (`src/encoders/encode.rs:226-234`). Whether a bare `\r\n` in a group title is always Q- or B-encoded rather than folded into a new header is decided by `inline_encoding_type` in that file, which was not read.
- **Remediation:** Independent of the library's answer, strip control characters from `conversation_subject_label` and `peer_display_name` before they reach mail-builder:
  ```rust
  fn header_text(s: &str) -> String { s.chars().filter(|c| !c.is_control()).collect() }
  ```

## 4. What was checked and found sound

Recorded so the next audit does not redo it, and because the checklist in section 6 rests on it.

**Path construction from request data (server).**
- Every asset route path segment is a `Sha256` newtype that only `Sha256::parse` can make: exactly 64 hex digits, lower-cased, whitespace refused (`crates/server/server/src/assets_api.rs:95-152`). Shard and sidecar paths derive from it (`:166-171`, `asset_store.rs:224-229`).
- Upload ids are hex of at most 64 characters (`asset_uploads.rs:98-104`) before `session_dir` joins them.
- Log file downloads take an `i64` and build `server-{number:06}.log` (`crates/server/server/src/server_api/log_files.rs:74-80`, `crates/server/server/src/logging/files.rs:154-168`); no caller path reaches the file system.
- Account directories are `data_dir/<i64>` (`asset_store.rs:85-87`, `config.rs:271-287`); `assets_dir` and `assets_converted_dir` must be single directory names (`config.rs:79-116`).
- A stored `assets_path` is joined only through `join_under` on the delete side (`asset_store.rs:245-249`, `:336`); the serve side is L3.

**Attachment paths from conversation files.** One guard, `message_ir::safe_attachment_path` (`crates/libs/ir/src/attachment_path.rs:42-64`), refuses empty, absolute, `..`, root and drive-prefix components and is tested against ten escape shapes (`:76-98`). Every reader of a recorded path calls it: the server's staging (`imports_api/staging.rs:71`), the format writers that embed bytes (`crates/libs/ir-format/src/util.rs:66`), the desktop import's directory scan (`crates/libs/import/src/directory.rs:146`), the transcode pass (`crates/libs/staging/src/transcode.rs:382`), the staging summary (`crates/libs/staging/src/staging_summary.rs:307`), and the Export, which refuses a server-held path and falls back to `attachments/{sha256}` (`crates/libs/export/src/project.rs:263-287`, `run.rs:720-752`).

**Exporters reading untrusted backups.**
- WhatsApp: a media hint from wtsexporter's JSON is accepted only when the resolved file is inside an allowed root (`crates/exporters/whatsapp-exporter/src/emit.rs:514-549` `resolve_media_file`, `path_within_any`), with a test for `..` escape (`:787-797`). The decryption key goes to wtsexporter as a `0600` file in the scratch directory, never on the command line (`wtsexporter.rs:314-326`, `:510-530`). `--move-media` is never passed (`:334-336`).
- iMazing: attachments are found by listing the chat directory and matching names, never by joining a CSV cell onto a path (`crates/exporters/imazing-exporter/src/attachments.rs:78-122`); symlinks are not followed (test at `:502`).
- Apple Messages: the GPL helper reads attachments by Manifest.db file id and writes them to scratch under a name derived from the id's last segment plus a unique suffix (`crates/helpers/imessage-reader/src/backup.rs:278-282`). **Unable to verify:** on a Mac the path comes from `imessage-database`'s `resolved_attachment_path` (`crates/helpers/imessage-reader/src/attachments.rs:16`), a vendor crate not read; a hostile `chat.db` on the person's own Mac is not a realistic attacker.
- SMS Backup and Restore: the XML is streamed with quick-xml (`crates/libs/sbr/src/read.rs:824-833`, `:925-928`); each MMS part's base64 `data` is decoded once and hashed (`:379-398`). No per-part size cap exists, but the file is the person's own and the whole attribute must already be in memory as XML.
- Go SMS Pro: attachment bytes go to the shared writer with a type-table extension or `.bin` (`crates/exporters/go-sms-pro-exporter/src/attachments_emit.rs:12-43`).
- No archive extraction exists anywhere: the workspace depends on no `zip` or `tar` crate; `flate2` is used only to unpack a pinned, SHA-256-checked tool download (`src-tauri/Cargo.toml:57`, `tool_downloads.rs:498-512`).

**Symlinks.** `open_nofollow_read` opens with `O_NOFOLLOW` after a `symlink_metadata` check (`assets_api.rs:581-597`); `install_blob` refuses to install over a symlink or a non-file (`:305-317`); `stream_file` opens with `O_NOFOLLOW` and maps `ELOOP` to 404 (`:901-932`); the obfuscated export removes symlinks without following them (`crates/libs/ir-format/src/placeholders.rs:48-80`); run-directory deletion does the same (`src-tauri/src/run_directories.rs:416-436`).

**SHA-256 verification.** `copy_to_verified_temp` hashes the bytes as they are written and refuses a mismatch (`assets_api.rs:383-418`); a dedupe hit still hashes the source (`verify_source_digest`, `:367-376`, called at `:319`); `complete_upload` always hashes the assembled file (`asset_uploads.rs:384`, `:416`) and checks the total length (`:467-473`); `lookup_by_sha256` re-hashes the stored file before a client may skip sending bytes (`:179-186`). The Export verifies every fetched Asset's digest and discards a `200 OK` with other bytes (`crates/libs/export/src/http.rs:207-222`). The desktop app verifies a downloaded tool against its pinned SHA-256 before unpacking and the unpacked program against a second pinned SHA-256 before making it executable (`src-tauri/src/tool_downloads.rs:487-525`).

**Atomic writes and durability.** `message_ir::write_atomic`/`write_atomic_via`/`rename_into_place` write a sibling, `fsync` it, rename, and `fsync` the directory (`crates/libs/ir/src/durable.rs:26-116`). Stored originals, sidecars and Previews go through `shard_temp_file` + `persist_noclobber` (`assets_api.rs:325-356`, `:526-535`; `process_assets.rs:1176-1200`). The upload manifest is written through a temp and a rename (`asset_uploads.rs:160-168`). The Export writes each Asset through a per-fingerprint `.part` sibling (`crates/libs/export/src/part_file.rs:41-47`). The desktop download writes to a `tempfile` beside the chosen path, syncs, then `persist`s (`src-tauri/src/commands/download.rs:116-134`). The Demo Account reset backs up, installs, and rolls back both the database and the account directory (`crates/server/server/src/reset_demo.rs:1825-1990`).

**Size limits.** `limit_request_body` refuses a declared `Content-Length` over the cap before any handler runs and cuts off an undeclared body at the cap (`server.rs:1026-1052`): the attachment size limit from Server Settings for `PUT /v1/assets/{sha256}`, `MAX_REQUEST_BODY_BYTES` = 512 MiB elsewhere; a multipart part is held to its upload's part size (`assets_api.rs:1266-1275`). JSON bodies are held to 32 MiB (`server.rs:1014`, `:1305`); the address book to 8 MiB and UTF-8 (`crates/server/server/src/contacts_api/address_book.rs:70`, `:156-158`). The sidecar read is capped at 1024 bytes (`assets_api.rs:504`).

**Serving.** `stream_file` sets `Content-Type` from the sidecar or `application/octet-stream`, `X-Content-Type-Options: nosniff`, a fixed `Content-Disposition: attachment; filename="asset"` that never echoes a client name, `Accept-Ranges`, and a quoted-fingerprint `ETag` (`assets_api.rs:951-991`). Ranges: one `bytes=` range, suffix and open-ended forms, clamped to the file, `416` only when no byte is selected, `If-Range` compared strongly (`crates/server/server/src/assets_api/ranges.rs:35-103`). Log files and the address book are `text/plain` and `text/csv` attachments with server-chosen names (`log_files.rs:102-116`, `address_book.rs:267-277`); the CSV export prefixes formula-leading cells with `'` (`address_book.rs:207-210`).

**Media Links.** HMAC-SHA256 under a per-process random key (`crates/server/server/src/assets_api/media_links.rs:58-64`), over account, fingerprint, expiry and the live session's token hash (`:66-77`, `:216-249`), verified in constant time (`:92-94`), one hour (`:34`), only on the three byte routes, and only when no `Authorization` header is present (`:161-188`). The server stores nothing and logs `media_link=[hidden]` (`docs/architecture/http-api.md`, "Credentials and reach").

**Desktop app.** The webview never names a path the app writes: `save_download` and `save_file` take the path from the Save dialog alone (`src-tauri/src/commands/download.rs:40-48`, `paths.rs:227-283`); `save_download` accepts only an `http(s)` URL ending in `/v1/assets/{sha256}` with a `media_link` query and never repeats it in an error (`download.rs:58-75`). `open_path` opens only a run directory the app made, the Export Directory, or a log in the Logs Directory, after canonicalizing against the root (`paths.rs:183-199`, `:364-420`). Run directories are deleted only when recorded and still marked with the sentinel (`run_directories.rs:274-336`). Capabilities grant only `dialog` and `shell:allow-open` (`src-tauri/capabilities/default.json`); the CSP sets `object-src 'none'`, `frame-src 'none'`, `form-action 'none'` (`src-tauri/tauri.conf.json:24-37`). `web/src` has no `dangerouslySetInnerHTML`, `iframe`, `srcdoc`, `embed` or `object` sink (grep, excluding tests). Downloaded tools are made `0755` only after both checksums pass (`tool_downloads.rs:525-540`).

**Scratch and cleanup.** Scratch directories are `0700` (`crates/core/message-crate-core/src/scratch.rs:181-186`) and swept of directories whose lock is free (`:150-176`). Abandoned uploads are swept after 24 h (`asset_store.rs:699-743`), killed-write temps after 1 h (`:745-812`), unreferenced originals and versions after each Import Run under the database write lock (`:503-583`). Refused uploads remove their temp before answering (`assets_api.rs:1068-1071`, `:1088-1091`).

**File names from untrusted data.** Conversation file stems pass `sanitize_stem`, which keeps only `[A-Za-z0-9+_-]` (`crates/libs/ir/src/lib.rs:792-803`); attachment files are named `{utc-date}-{digest16}{ext}` (`crates/core/message-crate-core/src/attachments.rs:23-30`); `valid_filename` drops `null`/`none` (`ir/src/lib.rs:963-970`). The web app's suggested download name is the export's `original_name` or the path's last segment (`web/src/lib/attachmentMedia.ts:82-86`), handed to the OS Save dialog, which owns the final name.

## 5. Prioritised fixes

1. **Deadline and limits on ffmpeg/ffprobe** (M1): the one change that bounds the only untrusted-format parser in the product. `crates/libs/media/src/tools.rs:416-470`.
2. **Bound authenticated resource use** (M2): take the import semaphore before streaming the body, stream parts to disk instead of buffering, add a per-account write semaphore and a cap on open multipart sessions. `imports_api/mod.rs:1731-1785`, `assets_api.rs:1275`, `asset_uploads.rs:201-241`.
3. **Owner-only data directories** (L1): `umask(0o077)` at start and `0o700` on `data_dir` and each account directory, in the server and in `src-tauri/src/local_server.rs:881`.
4. **Validate the declared MIME type and guard the version path** (L2, L3): a grammar and length check in `store_mime_metadata`; `join_under` in `stream_version`.
5. **Treat the HTTP import's `path` as a label** (L4) and **refuse `data_dir` under `static_dir`** (L7): two small config and handler changes.

## 6. Checklist

| # | Check | Result | Where |
|---|---|---|---|
| 1 | File type validation (allow-list) | **N/A by design** | The server stores any type, since an archive must keep what was sent (`assets_api.rs:994-1003`). Serving headers compensate (section 4, "Serving"). See L2 for the gap this leaves. |
| 2 | File size limits | **Pass** | `server.rs:1026-1052`, `asset_uploads.rs:208-214`, `address_book.rs:70`, `server.rs:1014`, `:1021`. Concurrency, not size, is the gap (M2). |
| 3 | Filename sanitization | **Pass** | Stored names are fingerprints (`assets_api.rs:166-171`); export names pass `sanitize_stem` (`ir/src/lib.rs:792-803`) or are content-addressed (`attachments.rs:23-30`); served names are fixed (`assets_api.rs:971-974`). |
| 4 | Anti-virus scanning | **Fail (accepted)** | None exists. A self-hosted archive of the person's own messages has no natural place for one; the compensating control is that the server never executes stored bytes and serves them as downloads. Recorded, not recommended. |
| 5 | Storage outside the web root | **Pass** with L7 | `data_dir` and `static_dir` are separate paths (`config.rs:214-240`, `:164-165`); nothing stops an owner nesting them (L7). |
| 6 | Direct execution prevention | **Pass** | Stored files carry no extension (`assets_api.rs:1-7`); `Content-Disposition: attachment` and `nosniff` on every byte route (`:966-974`); only pinned, checksummed tools are made executable (`tool_downloads.rs:487-540`). |
| 7 | MIME type validation | **Fail** | Declared type stored verbatim and first-writer-wins (`assets_api.rs:514-536`). L2. |
| 8 | Magic number verification | **Fail** | No byte-level type detection; `media/src/mime.rs:79-129` is an extension table; only MP4 codec is probed (`versions.rs:96-102`). L2. |
| 9 | Image manipulation library vulnerabilities | **Partial** | No Rust image decoder is linked; all decoding is ffmpeg out of process, which contains a crash but not a hang, a memory blow-up, or a code-execution bug in the decoder (M1). |
| 10 | ZIP bomb protection | **N/A** | No archive is extracted. The one gzip unpack is of a pinned, SHA-256-checked file (`tool_downloads.rs:498-512`). |

## 7. Unable to verify

- mail-builder 1.0.0's treatment of a bare CR/LF inside a `Text` header value (L9): `~/.cargo/registry/src/*/mail-builder-1.0.0/src/encoders/encode.rs`, `inline_encoding_type` and `scan_encoding_type`.
- The file mode of `messagecrate.db` as sqlx creates it (L1): the `sqlx-sqlite` connect options and `crates/server/server/src/db/` where the pool is opened.
- The exact ffmpeg version in the Docker runtime image (M1): `docker/Dockerfile:56-61` installs Debian bookworm's package at build time.
- `imessage-database`'s `resolved_attachment_path` on macOS (section 4, Apple Messages): a vendor crate behind the GPL process boundary, not read.
- Whether any production deployment runs the server bare-metal on a multi-user host (L1's precondition): the documentation describes Docker and the desktop app only.
