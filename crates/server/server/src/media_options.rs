//! The compress options the server applies to the media it converts itself:
//! a browser preview (`process_assets`), and an import's media rewrite
//! (`import_media`).

use media::{CompressOptions, MaxResolution};

/// The server's compress options. The server has no Import form to read
/// them from, so they are named here, with the 1080p cap the server has
/// always used, rather than taken from the `media` crate's default, which
/// follows the desktop Import form.
pub(crate) fn server_compress_options() -> CompressOptions {
    CompressOptions {
        max_resolution: MaxResolution::P1080,
        ..CompressOptions::default()
    }
}
