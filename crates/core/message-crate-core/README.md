# message-crate-core

Shared form models (`Form`, `Exporter`, `ExporterConfig`), the run's parts every exporter takes (`RunContext`, `OutputConfig`, and its own `SourceConfig` variant's config), the shared export pipeline (`run_pipeline`, `finish_run`, `ExportReport`), attachment jobs, cancellation (`CancelFlag`), and progress events (`ProgressEvent`).

The Tauri desktop app in `src-tauri/` uses this crate. Exporter libraries use the same config types.

## Build and test

```bash
cargo test -p message-crate-core
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library used by the desktop app and the converters. The Slint GUI that once used it is gone; its screens are recorded in
[docs/gui/legacy-slint-gui.md](../../../docs/gui/legacy-slint-gui.md).

## License

Fair Core License. See the repository root `LICENSE.md`.
