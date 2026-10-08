//! The download of ffmpeg, ffprobe and wtsexporter into the Tools Directory
//! (#1053, `docs/adr/0019`).
//!
//! At every start the app checks the Tools Directory in the background and
//! downloads what is missing or older than the release pinned here. Nothing
//! waits for it, and a failed download is tried again at the next start.
//!
//! - **ffmpeg and ffprobe** are not downloaded when both are on `PATH`,
//!   because a person who installed ffmpeg chose it.
//! - **Each file is pinned** to one release and one SHA-256 carried in
//!   [`pins`]. A download that does not match is deleted and the failure
//!   says the checksum did not match, because nobody clicked to start it.
//! - **One check at a time.** The check holds a lock on [`LOCK_FILE`] in
//!   the Tools Directory while it runs, and a second app open at the same
//!   time skips its check, so neither deletes the other's temporary file
//!   nor writes over the other's record.
//! - **A file is replaced only after its successor passed.** The new file
//!   is written under a temporary name in the Tools Directory, checked, and
//!   then renamed over the old one, so a failed download leaves the old one
//!   in use.
//!
//! Which file in the Tools Directory the app wrote is kept in
//! [`MANIFEST_FILE`] beside the programs: the release, the asset and its
//! checksum, the program's checksum, and the file's [`Stamp`]. One rule
//! holds for all three programs, because the Tools Directory belongs to the
//! app:
//!
//! - **The pinned one stays.** A file is the pinned one when its entry
//!   names the pinned release and its stamp is the one recorded, so no file
//!   is read again at start-up. One that does not run is not downloaded
//!   again, because the download would be the same file: Settings says it
//!   doesn't run until a newer pin or another file changes that.
//! - **A pinned file with no entry is adopted.** A file with no entry, or
//!   whose stamp changed, whose SHA-256 is the pinned program's gets an
//!   entry and stays. The pinned program's SHA-256 is carried beside the
//!   asset's, because ffmpeg's and ffprobe's assets are gzipped.
//! - **Anything else is replaced** after a good download, whoever put it
//!   there: a file from an older release, or any other file. Until the
//!   download passes, the old file stays in use, so with no internet a file
//!   put there by hand is still the one used.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};

mod pins;
pub use pins::pinned_for;

/// Where the pinned releases are downloaded from. Tests point the download
/// at a server of their own instead.
pub const GITHUB: &str = "https://github.com";

/// The record of the files the app wrote, in the Tools Directory.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The file the check holds a lock on while it runs, in the Tools Directory.
pub const LOCK_FILE: &str = ".check.lock";

/// The start of the temporary file the record is written to.
const MANIFEST_TEMP_PREFIX: &str = ".manifest-";

/// The steps a program's download writes a temporary file for: the file as
/// it arrives, and the program it unpacks to.
const TEMP_STEPS: [&str; 2] = ["download", "unpack"];

/// The start of the temporary file `program`'s download writes at `step`.
fn temp_prefix(program: Program, step: &str) -> String {
    format!(".{}.{step}-", program.name())
}

/// Delete the temporary files an interrupted check left in `dir`. A
/// temporary file is deleted when the check that wrote it ends, but an app
/// closed or killed mid-download runs no clean-up, and each one left is up
/// to about 110 MB.
fn delete_leftovers(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let prefixes: Vec<String> = [Program::Ffmpeg, Program::Ffprobe, Program::Wtsexporter]
        .into_iter()
        .flat_map(|program| TEMP_STEPS.map(|step| temp_prefix(program, step)))
        .chain([MANIFEST_TEMP_PREFIX.to_string()])
        .collect();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if prefixes
            .iter()
            .any(|prefix| name.starts_with(prefix.as_str()))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// A program the app keeps in the Tools Directory.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Program {
    /// ffmpeg.
    Ffmpeg,
    /// ffprobe.
    Ffprobe,
    /// wtsexporter.
    Wtsexporter,
}

impl Program {
    /// The program's name, as it is looked for.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
            Self::Wtsexporter => "wtsexporter",
        }
    }

    /// The program's file name in the Tools Directory, with `.exe` on
    /// Windows.
    pub fn file_name(self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.name())
        } else {
            self.name().to_string()
        }
    }
}

/// One program's file as a pinned release publishes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    /// The program the file is.
    pub program: Program,
    /// The GitHub project, `owner/name`.
    pub repo: &'static str,
    /// The release tag.
    pub release: &'static str,
    /// The release asset's name.
    pub asset: &'static str,
    /// The asset's SHA-256, in lowercase hex.
    pub sha256: &'static str,
    /// The program's SHA-256, in lowercase hex: the asset's when it is not
    /// gzipped, and what it unpacks to when it is.
    pub program_sha256: &'static str,
    /// Whether the asset is gzipped: the checksum is over the gzipped
    /// file, and the program is what it unpacks to.
    pub gzip: bool,
}

impl Pinned {
    /// The asset's address under `base`, which is [`GITHUB`] outside tests.
    pub fn url(&self, base: &str) -> String {
        format!(
            "{base}/{}/releases/download/{}/{}",
            self.repo, self.release, self.asset
        )
    }
}

/// One file the app wrote in the Tools Directory, as [`MANIFEST_FILE`]
/// records it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Written {
    /// The release it came from.
    pub release: String,
    /// The release asset it came from.
    pub asset: String,
    /// The asset's SHA-256.
    pub sha256: String,
    /// The program's SHA-256, which is the asset's when it is not gzipped.
    pub program_sha256: String,
    /// The program file as it was written, which tells the app's file from
    /// one put over it since.
    pub stamp: Stamp,
}

impl Written {
    /// What the app records after writing `pinned`'s program, as `stamp`.
    fn of(pinned: &Pinned, stamp: Stamp) -> Self {
        Self {
            release: pinned.release.to_string(),
            asset: pinned.asset.to_string(),
            sha256: pinned.sha256.to_string(),
            program_sha256: pinned.program_sha256.to_string(),
            stamp,
        }
    }

    /// Whether this is the file `pinned` publishes.
    fn is(&self, pinned: &Pinned) -> bool {
        self.release == pinned.release
            && self.asset == pinned.asset
            && self.sha256 == pinned.sha256
            && self.program_sha256 == pinned.program_sha256
    }
}

/// What tells the file the app wrote from another put in its place, without
/// reading it: a copy of the same size made since has a different modified
/// time, and on Unix a different inode.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Stamp {
    /// The file's size in bytes.
    pub size: u64,
    /// When the file was last modified, in nanoseconds since 1970, if the
    /// platform says.
    pub modified_ns: Option<u64>,
    /// The file's inode, on Unix.
    pub inode: Option<u64>,
}

impl Stamp {
    /// The stamp of the file `metadata` describes.
    fn of(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            Some(metadata.ino())
        };
        #[cfg(not(unix))]
        let inode = None;
        Self {
            size: metadata.len(),
            modified_ns: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|since| u64::try_from(since.as_nanos()).ok()),
            inode,
        }
    }

    /// The stamp of the file at `path`.
    fn at(path: &Path) -> io::Result<Self> {
        std::fs::metadata(path).map(|metadata| Self::of(&metadata))
    }
}

/// The files the app wrote, by program.
type Manifest = BTreeMap<Program, Written>;

/// The record in `dir`. One that is missing or can't be read is empty, and
/// every file is then taken as put there by someone else.
fn read_manifest(dir: &Path) -> Manifest {
    std::fs::read(dir.join(MANIFEST_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Write the record in `dir`, under a temporary name and then renamed, so a
/// half-written record is never read.
fn write_manifest(dir: &Path, manifest: &Manifest) -> io::Result<()> {
    let mut file = tempfile::Builder::new()
        .prefix(MANIFEST_TEMP_PREFIX)
        .tempfile_in(dir)?;
    serde_json::to_writer_pretty(&mut file, manifest)?;
    file.as_file().sync_all()?;
    file.persist(dir.join(MANIFEST_FILE))
        .map(|_| ())
        .map_err(|err| err.error)
}

/// What the start-up check does with one pinned program.
#[derive(Debug, PartialEq, Eq)]
enum Need {
    /// The file there is the pinned one and runs: it stays.
    Keep,
    /// The file there is the pinned one and does not run: it stays, and
    /// Settings says so, since downloading it again gives the same file.
    DoesNotRun,
    /// The file there is the pinned program with no entry, or with a stamp
    /// that changed: it stays and goes into the record.
    Adopt,
    /// The program is downloaded, and replaces whatever is there once the
    /// download passed.
    Download,
}

/// What to do about `pinned` in `dir`, given the record.
fn need(pinned: &Pinned, dir: &Path, manifest: &Manifest) -> Need {
    let path = dir.join(pinned.program.file_name());
    let Ok(stamp) = Stamp::at(&path) else {
        return Need::Download;
    };
    if let Some(written) = manifest.get(&pinned.program)
        && written.is(pinned)
        && written.stamp == stamp
    {
        return if runs(pinned.program, dir) {
            Need::Keep
        } else {
            Need::DoesNotRun
        };
    }
    if file_sha256(&path).is_ok_and(|sha256| sha256 == pinned.program_sha256) {
        Need::Adopt
    } else {
        Need::Download
    }
}

/// Whether `program` in `dir` runs, as the lookup that uses it decides:
/// ffmpeg and ffprobe answer `-version` (an answer `media` keeps until the
/// file changes), and wtsexporter is a file with an executable bit, which
/// is all its lookup asks because a `pipx` shim is slow to start.
fn runs(program: Program, dir: &Path) -> bool {
    match program {
        Program::Ffmpeg | Program::Ffprobe => media::tool_in_dir(dir, program.name()).is_some(),
        Program::Wtsexporter => is_executable(&dir.join(program.file_name())),
    }
}

/// Whether the file at `path` is a file with an executable bit.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Windows runs any `.exe`, so a file is enough.
#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The SHA-256 of the file at `path`, in lowercase hex.
fn file_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Why a download failed, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    /// No answer from the server: no network, a name that does not
    /// resolve, a connection cut off or timed out.
    NoNetwork(String),
    /// The server answered with a status other than success.
    Http(u16),
    /// The file arrived, and its SHA-256 is not the pinned one.
    ChecksumMismatch {
        /// The SHA-256 the app carries.
        expected: String,
        /// The SHA-256 of what arrived.
        actual: String,
    },
    /// The file could not be written to the Tools Directory.
    CouldNotWrite(String),
    /// The program passed its checksum and is in place, and does not run.
    DoesNotRun(Program),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoNetwork(why) => write!(f, "No connection to the download's server: {why}."),
            Self::Http(status) => {
                let reason = reqwest::StatusCode::from_u16(*status)
                    .ok()
                    .and_then(|status| status.canonical_reason())
                    .map(|reason| format!(" {reason}"))
                    .unwrap_or_default();
                write!(f, "The download's server answered {status}{reason}.")
            }
            Self::ChecksumMismatch { expected, actual } => write!(
                f,
                "The downloaded file's checksum did not match the one this app carries, so the file was deleted. \
                 SHA-256 expected {expected}, got {actual}."
            ),
            Self::CouldNotWrite(why) => {
                write!(
                    f,
                    "The file could not be written to the Tools Directory: {why}."
                )
            }
            Self::DoesNotRun(program @ (Program::Ffmpeg | Program::Ffprobe)) => write!(
                f,
                "{program} was downloaded but doesn't run on this computer. \
                 Install it with your package manager instead; the app uses the copy on PATH.",
                program = program.name()
            ),
            Self::DoesNotRun(Program::Wtsexporter) => write!(
                f,
                "wtsexporter was downloaded but doesn't run on this computer. \
                 See \"Import can't find wtsexporter\" in Troubleshooting at messagecrate.app."
            ),
        }
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

/// Download `pinned` from `url` into `dir` and put it in place.
///
/// The file is streamed to a temporary file in `dir` and its SHA-256
/// computed on the way. On a match it is unpacked when gzipped and the
/// program's SHA-256 checked, then made executable and renamed over the
/// program; on a mismatch, or any failure,
/// the temporary file is deleted and the program there is left as it was.
/// `progress` hears the bytes received so far and the total when the
/// server says it.
fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    pinned: &Pinned,
    dir: &Path,
    progress: &dyn Fn(u64, Option<u64>),
) -> Result<Written, DownloadError> {
    let write_error = |err: io::Error| DownloadError::CouldNotWrite(err.to_string());
    let mut response = client
        .get(url)
        .send()
        .map_err(|err| DownloadError::NoNetwork(error_chain(&err)))?;
    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::Http(status.as_u16()));
    }
    let total = response.content_length();
    let mut arrived = tempfile::Builder::new()
        .prefix(&temp_prefix(pinned.program, "download"))
        .tempfile_in(dir)
        .map_err(write_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    progress(received, total);
    loop {
        let read = match response.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(DownloadError::NoNetwork(error_chain(&err))),
        };
        hasher.update(&buffer[..read]);
        arrived.write_all(&buffer[..read]).map_err(write_error)?;
        received += read as u64;
        progress(received, total);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != pinned.sha256 {
        return Err(DownloadError::ChecksumMismatch {
            expected: pinned.sha256.to_string(),
            actual,
        });
    }
    let mut program = if pinned.gzip {
        let mut unpacked = tempfile::Builder::new()
            .prefix(&temp_prefix(pinned.program, "unpack"))
            .tempfile_in(dir)
            .map_err(write_error)?;
        let packed = arrived.as_file_mut();
        packed.rewind().map_err(write_error)?;
        io::copy(&mut flate2::read::GzDecoder::new(packed), &mut unpacked).map_err(write_error)?;
        unpacked.flush().map_err(write_error)?;
        let actual = file_sha256(unpacked.path()).map_err(write_error)?;
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

/// Give the file at `path` its executable bits.
#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

/// Windows runs any `.exe`, so there is no bit to set.
#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Where one program's download stands, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    /// Downloading: `received` bytes so far, of `total` when the server
    /// said.
    Downloading {
        /// Bytes received so far.
        received: u64,
        /// The file's size, when the server said.
        total: Option<u64>,
    },
    /// The last download failed, for `reason`.
    Failed {
        /// Why, as [`DownloadError`] says it.
        reason: String,
    },
}

/// The download state of each program, shared by the start-up check and
/// `tools_status`. A program with no entry is not downloading and has not
/// failed since the app started.
#[derive(Debug, Clone, Default)]
pub struct ToolDownloads(Arc<Mutex<HashMap<Program, DownloadState>>>);

impl ToolDownloads {
    /// Where `program`'s download stands, if anywhere.
    pub fn get(&self, program: Program) -> Option<DownloadState> {
        self.0
            .lock()
            .expect("tool downloads lock")
            .get(&program)
            .cloned()
    }

    fn set(&self, program: Program, state: DownloadState) {
        self.0
            .lock()
            .expect("tool downloads lock")
            .insert(program, state);
    }

    fn clear(&self, program: Program) {
        self.0.lock().expect("tool downloads lock").remove(&program);
    }
}

/// The state of `program` in place that does not run.
fn does_not_run(program: Program) -> DownloadState {
    DownloadState::Failed {
        reason: DownloadError::DoesNotRun(program).to_string(),
    }
}

/// Take the lock on [`LOCK_FILE`] in `dir`, making `dir` first. `None` when
/// another check holds it. The lock is let go when the file is closed.
fn lock_check(dir: &Path) -> io::Result<Option<File>> {
    std::fs::create_dir_all(dir)?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK_FILE))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(err)) => Err(err),
    }
}

/// Check `dir` against `pinned` and download, one after another, what is
/// missing or not the pinned release, from `base`. ffmpeg and ffprobe are
/// passed over when both are on `PATH`. Each download's progress and
/// failure go to `downloads`, and each file written goes into the record.
///
/// Another app's check running on `dir` already does the work, so this one
/// returns at once. When the lock can't be taken because the Tools Directory
/// can't be written, the check goes on and its downloads say why.
pub fn download_missing(dir: &Path, base: &str, pinned: &[Pinned], downloads: &ToolDownloads) {
    let _lock = match lock_check(dir) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => return,
        Err(_) => None,
    };
    delete_leftovers(dir);
    let ffmpeg_on_path = media::ffmpeg_on_path();
    let mut manifest = read_manifest(dir);
    let mut wanted = Vec::new();
    for pin in pinned {
        if ffmpeg_on_path && matches!(pin.program, Program::Ffmpeg | Program::Ffprobe) {
            continue;
        }
        match need(pin, dir, &manifest) {
            Need::Keep => {}
            Need::Adopt => {
                let path = dir.join(pin.program.file_name());
                let _ = make_executable(&path);
                if let Ok(stamp) = Stamp::at(&path) {
                    manifest.insert(pin.program, Written::of(pin, stamp));
                    let _ = write_manifest(dir, &manifest);
                }
                if !runs(pin.program, dir) {
                    downloads.set(pin.program, does_not_run(pin.program));
                }
            }
            Need::DoesNotRun => downloads.set(pin.program, does_not_run(pin.program)),
            Need::Download => wanted.push(pin),
        }
    }
    if wanted.is_empty() {
        return;
    }
    // Every program wanted shows as downloading from now on, before the
    // first request, so Settings opened while a connection is made, or
    // between two downloads, keeps asking until the last one ends.
    for pin in &wanted {
        downloads.set(
            pin.program,
            DownloadState::Downloading {
                received: 0,
                total: None,
            },
        );
    }
    let fail_all = |err: DownloadError| {
        for pin in &wanted {
            downloads.set(
                pin.program,
                DownloadState::Failed {
                    reason: err.to_string(),
                },
            );
        }
    };
    let client = match download_client() {
        Ok(client) => client,
        Err(err) => return fail_all(DownloadError::NoNetwork(error_chain(&err))),
    };
    if let Err(err) = std::fs::create_dir_all(dir) {
        return fail_all(DownloadError::CouldNotWrite(err.to_string()));
    }
    for pin in wanted {
        let program = pin.program;
        let progress = |received, total| {
            downloads.set(program, DownloadState::Downloading { received, total });
        };
        match download(&client, &pin.url(base), pin, dir, &progress) {
            Ok(written) => {
                manifest.insert(program, written);
                if runs(program, dir) {
                    downloads.clear(program);
                } else {
                    downloads.set(program, does_not_run(program));
                }
                if let Err(err) = write_manifest(dir, &manifest) {
                    // The program is in place and works; without the record
                    // the next start takes it as someone else's.
                    downloads.set(
                        program,
                        DownloadState::Failed {
                            reason: DownloadError::CouldNotWrite(format!(
                                "{} could not be written: {err}",
                                MANIFEST_FILE
                            ))
                            .to_string(),
                        },
                    );
                }
            }
            Err(err) => downloads.set(
                program,
                DownloadState::Failed {
                    reason: err.to_string(),
                },
            ),
        }
    }
}

/// Start the check of the Tools Directory `dir` on a thread of its own, for
/// this computer's platform, so login and browsing don't wait for it.
pub fn start(dir: PathBuf, downloads: ToolDownloads) {
    let pinned = pinned_for(std::env::consts::OS, std::env::consts::ARCH);
    let _ = std::thread::Builder::new()
        .name("tool-downloads".into())
        .spawn(move || download_missing(&dir, GITHUB, &pinned, &downloads));
}

#[cfg(test)]
mod tests;
