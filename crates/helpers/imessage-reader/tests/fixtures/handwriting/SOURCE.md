# Handwriting fixture

`hello.bin` and `hello.svg` are copied unchanged from the `imessage-database` crate.

- Repository: https://github.com/ReagentX/imessage-exporter
- Path: `imessage-database/test_data/handwritten_message/hello.bin` and `hello.svg`
- Tag: `4.2.0` (commit `f4021a1113bd47400ceaedfb79907ef2c63624a9`), the version in `Cargo.lock`
- Licence: GPL-3.0-or-later

`hello.bin` is the `payload_data` of a handwritten message that says "hello".
It holds no personal content.
`hello.svg` is what `HandwrittenMessage::render_svg` makes from it at that tag.

The files are GPL, like the crate that uses them.
They live in `imessage-reader` and nowhere else (ADR 0014).
A new `imessage-database` version may render the SVG differently.
The test then fails, and `hello.svg` is copied again from the new tag.
