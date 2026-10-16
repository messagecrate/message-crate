# ir

Shared conversation types for Message Crate: `ConversationDocument`, messages, attachments, and participants. This crate has no I/O and no formatting. Attachment bytes are never serialized to JSON; paths and hashes point at sidecar files.

Exporters, `ir-format`, `import`, `export`, and the server use this crate.

## Build and test

```bash
cargo test -p ir
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. Schema notes for contributors: [Common message](https://messagecrate.app/docs/developer/architecture/common-message/). JSON Lines layout for imports: https://messagecrate.app/docs/developer/reference/export-structure/

## License

Fair Core License. See the repository root `LICENSE.md`.
