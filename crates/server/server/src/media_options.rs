//! The compress options the server applies to an import's media rewrite
//! (`import_media`). A Preview is not compressed this way: it has a recipe
//! of its own, which every browser plays ([`media::make_preview`]).

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
