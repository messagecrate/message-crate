//! Getting a pinned file and putting it in the Tools Directory.
//!
//! [`Fetch`] is the one step that reaches the network: it writes the asset
//! as it arrives. [`install`] does everything after it, the same for every
//! fetch: the asset's checksum, the unpack of a gzipped asset and the
//! program's checksum, the executable bit, and the rename over the program.
//! The app fetches over HTTP with [`HttpFetch`]; a test hands [`install`] a
//! fetch that writes fixed bytes, so the checks need no server.

use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::{DownloadError, Pinned, Stamp, Written, temp_prefix};

/// Gets a pinned asset.
pub(super) trait Fetch: Send {
    /// Write `pinned`'s asset to `into` as it arrives, telling `progress`
    /// the bytes received so far and the total when it is known.
    ///
    /// A failure to write to `into` is [`DownloadError::CouldNotWrite`].
    /// The asset is not checked here: [`install`] checks what was written.
    fn fetch(
        &self,
        pinned: &Pinned,
        into: &mut dyn Write,
        progress: &dyn Fn(u64, Option<u64>),
    ) -> Result<(), DownloadError>;
}

/// The fetch the app uses: a GET of the asset's address under `base`, which
/// is [`super::GITHUB`] outside tests.
pub(super) struct HttpFetch {
    base: String,
    /// The client, built at the first fetch so a check that downloads
    /// nothing builds none, and kept for the check's other downloads. The
    /// error when it could not be built.
    client: OnceLock<Result<reqwest::blocking::Client, String>>,
}

impl HttpFetch {
    /// A fetch from the releases under `base`.
    pub(super) fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            client: OnceLock::new(),
        }
    }
}

impl Fetch for HttpFetch {
    fn fetch(
        &self,
        pinned: &Pinned,
        into: &mut dyn Write,
        progress: &dyn Fn(u64, Option<u64>),
    ) -> Result<(), DownloadError> {
        // A client that can't be built is no network failure, so it is not
        // tried again after a wait.
        let client = self
            .client
            .get_or_init(|| download_client().map_err(|err| error_chain(&err)))
            .as_ref()
            .map_err(|why| DownloadError::CouldNotStart(why.clone()))?;
        let mut response = client
            .get(pinned.url(&self.base))
            .send()
            .map_err(|err| DownloadError::NoNetwork(error_chain(&err)))?;
        let status = response.status();
        if !status.is_success() {
            return Err(DownloadError::Http(status.as_u16()));
        }
        let total = response.content_length();
        progress(0, total);
        copy(
            &mut response,
            into,
            &|err| DownloadError::NoNetwork(error_chain(&err)),
            &|received| progress(received, total),
        )
    }
}

/// Copy `from` into `into`, telling `progress` the bytes copied so far after
/// each chunk.
///
/// A failure to read `from` is the error `read_error` makes of it, since
/// what that means depends on what is read; a failure to write `into` is
/// [`DownloadError::CouldNotWrite`].
fn copy(
    from: &mut dyn Read,
    into: &mut dyn Write,
    read_error: &dyn Fn(io::Error) -> DownloadError,
    progress: &dyn Fn(u64),
) -> Result<(), DownloadError> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut copied = 0u64;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(read_error(err)),
        };
        into.write_all(&buffer[..read])
            .map_err(|err| DownloadError::CouldNotWrite(err.to_string()))?;
        copied += read as u64;
        progress(copied);
    }
}

/// An error and the errors under it, as one line.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut line = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        line.push_str(": ");
        line.push_str(&cause.to_string());
        source = cause.source();
    }
    line
}

/// The client the downloads use: rustls, a `User-Agent` naming the app, 30
/// seconds to connect, and 60 seconds for any one read, so a stalled
/// download fails rather than hanging the check.
fn download_client() -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!(
            "MessageCrate/",
            env!("MESSAGE_CRATE_BUILD"),
            " (desktop app tool download)"
        ))
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60))
        .build()
}

/// A file that keeps the SHA-256 of what is written to it.
struct Hashing<'a> {
    file: &'a mut File,
    hasher: Sha256,
}

impl Write for Hashing<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Get `pinned` with `fetch` into `dir` and put it in place.
///
/// The asset is written to a temporary file in `dir` and its SHA-256
/// computed on the way. On a match it is unpacked when gzipped and the
/// program's SHA-256 checked, then made executable and renamed over the
/// program; on a mismatch, an asset that does not unpack, or any other
/// failure, the temporary files are deleted and the program there is left
/// as it was. `progress` hears the bytes received so far and the total when
/// it is known.
pub(super) fn install(
    fetch: &dyn Fetch,
    pinned: &Pinned,
    dir: &Path,
    progress: &dyn Fn(u64, Option<u64>),
) -> Result<Written, DownloadError> {
    let write_error = |err: io::Error| DownloadError::CouldNotWrite(err.to_string());
    let mut arrived = tempfile::Builder::new()
        .prefix(&temp_prefix(pinned.program, "download"))
        .tempfile_in(dir)
        .map_err(write_error)?;
    let mut hashing = Hashing {
        file: arrived.as_file_mut(),
        hasher: Sha256::new(),
    };
    fetch.fetch(pinned, &mut hashing, progress)?;
    let actual = hex::encode(hashing.hasher.finalize());
    if actual != pinned.sha256 {
        return Err(DownloadError::ChecksumMismatch {
            expected: pinned.sha256.to_string(),
            actual,
        });
    }
    let mut program = if pinned.gzipped {
        let mut unpacked = tempfile::Builder::new()
            .prefix(&temp_prefix(pinned.program, "unpack"))
            .tempfile_in(dir)
            .map_err(write_error)?;
        let packed = arrived.as_file_mut();
        packed.rewind().map_err(write_error)?;
        unpack(packed, &mut unpacked)?;
        unpacked.flush().map_err(write_error)?;
        let actual = file_io::file_sha256(unpacked.path()).map_err(write_error)?;
        if actual != pinned.program_sha256 {
            return Err(DownloadError::ChecksumMismatch {
                expected: pinned.program_sha256.to_string(),
                actual,
            });
        }
        unpacked
    } else {
        arrived
    };
    program.flush().map_err(write_error)?;
    program.as_file().sync_all().map_err(write_error)?;
    make_executable(program.path()).map_err(write_error)?;
    let target = dir.join(pinned.program.file_name());
    program
        .persist(&target)
        .map_err(|err| write_error(err.error))?;
    // Renaming keeps the size, modified time and inode, so the stamp taken
    // now is the one the next start sees.
    let stamp = Stamp::at(&target).map_err(write_error)?;
    Ok(Written::of(pinned, stamp))
}

/// Unpack the gzipped `packed` into `into`.
///
/// Bad gzip data, which flate2 reports as invalid input or data or an early
/// end, is [`DownloadError::CouldNotUnpack`], since the file that arrived
/// is at fault. Any other failure to read `packed`, such as the disk under
/// the Tools Directory, and a failure to write `into`, are
/// [`DownloadError::CouldNotWrite`].
pub(super) fn unpack(packed: &mut dyn Read, into: &mut dyn Write) -> Result<(), DownloadError> {
    copy(
        &mut flate2::read::GzDecoder::new(packed),
        into,
        &|err| match err.kind() {
            io::ErrorKind::InvalidInput
            | io::ErrorKind::InvalidData
            | io::ErrorKind::UnexpectedEof => DownloadError::CouldNotUnpack(err.to_string()),
            _ => DownloadError::CouldNotWrite(err.to_string()),
        },
        &|_| {},
    )
}

/// Give the file at `path` its executable bits.
#[cfg(unix)]
pub(super) fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

/// Windows runs any `.exe`, so there is no bit to set.
#[cfg(not(unix))]
pub(super) fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}
