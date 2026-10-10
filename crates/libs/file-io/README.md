# file-io

File helpers that know nothing about conversations:

- `write_atomic` and `write_atomic_via` write a file through a temporary
  sibling, sync it, rename it into place and sync the directory, so a power
  loss leaves either the old file or the whole new one under its name.
- `rename_into_place` does the sync and the rename for a file another
  program wrote, such as an ffmpeg output.
- `file_sha256` streams a file through SHA-256 and returns the lowercase hex
  fingerprint that `digest_sha256` fields carry and assets are keyed on.

They sit in a leaf crate so `media`, the journal, the export and import
crates and the desktop app share one copy without depending on the
conversation model in `message-ir`.

## Build and test

```bash
cargo test -p file-io
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. It builds no binary.

## License

Fair Core License. See the repository root `LICENSE.md`.
