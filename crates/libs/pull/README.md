# message-crate-pull

Export messages from a running server into a local JSON Lines directory (`*.jsonl` plus `attachments/`), fetching each Asset the messages' attachments name.

The desktop app's **Export** screen uses this crate as a library, with the logged-in Session's token.

## Build and test

```bash
cargo test -p message-crate-pull
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

How this crate fits the export pipeline: https://messagecrate.app/docs/developer/message-transfer/

## License

Fair Core License. See the repository root `LICENSE.md`.
