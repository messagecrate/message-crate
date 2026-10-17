# media

FFmpeg wrapper used when an export copies, converts, or compresses attachment files. Convert/compress runs in `message-crate-core`'s attachment jobs, in the `staging` transcode pass, and in the server's import media processing.

Converters and the desktop app use this crate. Every function that runs `ffmpeg` or `ffprobe` takes them as a `MediaTools` value the caller passes in. `InstalledTools` is the real one: it looks on `PATH`, then in the Tools Directory the caller names when it builds it, and nowhere else. Both come from one place, because two builds would work on one file: both from `PATH` when both are there, else both from the Tools Directory. A test of what the code decides passes `testutil::FakeTools` instead, through the `testutil` feature, and needs no ffmpeg installed.

## Build and test

```bash
cargo test -p media
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. User options: https://messagecrate.app/docs/user/features/messages/attachments-and-media/

## License

Fair Core License. See the repository root `LICENSE.md`.
