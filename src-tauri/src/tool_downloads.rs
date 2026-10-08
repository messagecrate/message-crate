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
//!   this file. A download that does not match is deleted and the failure
//!   says the checksum did not match, because nobody clicked to start it.
//! - **A file is replaced only after its successor passed.** The new file
//!   is written under a temporary name in the Tools Directory, checked, and
//!   then renamed over the old one, so a failed download leaves the old one
//!   in use.
//!
//! Which file in the Tools Directory the app wrote is kept in
//! [`MANIFEST_FILE`] beside the programs: the release, asset and checksum
//! of each, with the size of the file written. A file whose size matches
//! its entry is the app's, and is current when its entry names the pinned
//! release, so no file is read again at start-up. A file with no entry, or
//! one whose size has changed, was put there by someone else:
//!
//! - ffmpeg or ffprobe that answers `-version` is left in place, as one a
//!   person linked in from Homebrew is.
//! - wtsexporter is kept only when its SHA-256 is the pinned one, and is
//!   then entered in the record. Any other wtsexporter is replaced, because
//!   its release can't be told from the file and the import reads only the
//!   pinned release's JSON.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// Where the pinned releases are downloaded from. Tests point the download
/// at a server of their own instead.
pub const GITHUB: &str = "https://github.com";

/// The record of the files the app wrote, in the Tools Directory.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The project the ffmpeg and ffprobe files come from.
const FFMPEG_STATIC: &str = "eugeneware/ffmpeg-static";

/// The `ffmpeg-static` release pinned: ffmpeg 6.1.1, a GPL build with
/// `libx264` and `libx265`.
const FFMPEG_RELEASE: &str = "b6.1.1";

/// The project the wtsexporter files come from: Message Crate's fork, whose
/// JSON says who sent each message and who is in each group.
const WTSEXPORTER_REPO: &str = "messagecrate/WhatsApp-Chat-Exporter";

/// The fork release pinned, the first with `full_key_id` and `reply_key_id`.
const WTSEXPORTER_RELEASE: &str = "0.13.0-mc.2";

/// The `.gz` file of ffmpeg and of ffprobe for one platform, with the
/// SHA-256 of each.
struct FfmpegFiles {
    /// `std::env::consts::OS`.
    os: &'static str,
    /// `std::env::consts::ARCH`.
    arch: &'static str,
    /// ffmpeg's asset and its SHA-256.
    ffmpeg: (&'static str, &'static str),
    /// ffprobe's asset and its SHA-256.
    ffprobe: (&'static str, &'static str),
}

/// The `ffmpeg-static` `b6.1.1` files, by platform.
///
/// The project publishes no checksum file. Each SHA-256 here was computed on
/// 2026-10-08 by downloading the asset with
/// `curl -L https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/<asset>`
/// and running `sha256sum` on it; `gzip -t` passed on each.
///
/// The `.gz` files are downloaded rather than the plain ones because they
/// are a third of the size (19 to 30 MB against 45 to 83 MB), and the check
/// is on the file as it arrives, before it is unpacked.
///
/// Windows on ARM gets the x64 files, which Windows runs under emulation.
/// Whether they run there is not checked yet (#1053).
const FFMPEG_FILES: &[FfmpegFiles] = &[
    FfmpegFiles {
        os: "linux",
        arch: "x86_64",
        ffmpeg: (
            "ffmpeg-linux-x64.gz",
            "bfe8a8fc511530457b528c48d77b5737527b504a3797a9bc4866aeca69c2dffa",
        ),
        ffprobe: (
            "ffprobe-linux-x64.gz",
            "25d9b6ccb05e3d9de9e04e31e2506d8dd7f9f0418981965ac6df12e8d3afd067",
        ),
    },
    FfmpegFiles {
        os: "linux",
        arch: "aarch64",
        ffmpeg: (
            "ffmpeg-linux-arm64.gz",
            "754a678672298bc68156adff58aa7385a592c2b30b1d0ae8750c45c915c4bac0",
        ),
        ffprobe: (
            "ffprobe-linux-arm64.gz",
            "2ab6aba60ee84412dff9188720703376cb4e7aaf7e0b5e43aa8249f2acae5bf8",
        ),
    },
    FfmpegFiles {
        os: "macos",
        arch: "x86_64",
        ffmpeg: (
            "ffmpeg-darwin-x64.gz",
            "929b375c1182d956c51f7ac25e0b2b0411fb01f6f407aa15c9758efeb4242106",
        ),
        ffprobe: (
            "ffprobe-darwin-x64.gz",
            "d4da574d6e2e197bd259b47d69cf262df9e312af24ad960444f6d806d3d4c186",
        ),
    },
    FfmpegFiles {
        os: "macos",
        arch: "aarch64",
        ffmpeg: (
            "ffmpeg-darwin-arm64.gz",
            "8923876afa8db5585022d7860ec7e589af192f441c56793971276d450ed3bbfa",
        ),
        ffprobe: (
            "ffprobe-darwin-arm64.gz",
            "d986a8ec7b030899fe66a8a288ed809a3543338705a3ce178cfb85869c5d80be",
        ),
    },
    FfmpegFiles {
        os: "windows",
        arch: "x86_64",
        ffmpeg: (
            "ffmpeg-win32-x64.gz",
            "8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77",
        ),
        ffprobe: (
            "ffprobe-win32-x64.gz",
            "f309e6223ad89d2fe54bccd420a7709b66fd27540674e92309578ed491a43c8d",
        ),
    },
    FfmpegFiles {
        os: "windows",
        arch: "aarch64",
        ffmpeg: (
            "ffmpeg-win32-x64.gz",
            "8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77",
        ),
        ffprobe: (
            "ffprobe-win32-x64.gz",
            "f309e6223ad89d2fe54bccd420a7709b66fd27540674e92309578ed491a43c8d",
        ),
    },
];

/// The wtsexporter file for each platform the fork builds for, as
/// `(os, arch, asset, sha256)`. The checksums are copied from the release's
/// `SHA256SUMS` asset, read on 2026-10-08; the app never fetches that file.
/// Linux on ARM has no file.
const WTSEXPORTER_FILES: &[(&str, &str, &str, &str)] = &[
    (
        "linux",
        "x86_64",
        "wtsexporter_linux_x64",
        "2756dfbcc7e320390e34244a8fc7160770d99d6fef17dea6a2d6ce269b62f71b",
    ),
    (
        "macos",
        "x86_64",
        "wtsexporter_macos_x64",
        "b0e885474108d810fa7ba31360275494fc93ebadae117859ba7e90fa705f45e3",
    ),
    (
        "macos",
        "aarch64",
        "wtsexporter_macos_arm64",
        "03f90f32e956a5df1cbfbc1a1a87524ab05450f645703df0c16d630671eda9bf",
    ),
    (
        "windows",
        "x86_64",
        "wtsexporter_win_x64.exe",
        "4f4c91ab7829fa4a78673b5124c354b7be0c6d7446689ac58c56461f7988707f",
    ),
    (
        "windows",
        "aarch64",
        "wtsexporter_win_arm64.exe",
        "aa154042bec5b96953dda8d3c6daabac4fbbd9bd51c28fd350ba416317d9ec10",
    ),
];

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

/// The pinned files for a computer whose `std::env::consts` are `os` and
/// `arch`: ffmpeg and ffprobe, then wtsexporter. A platform with no file
/// for a program gets nothing for it.
pub fn pinned_for(os: &str, arch: &str) -> Vec<Pinned> {
    let mut pinned = Vec::new();
    if let Some(files) = FFMPEG_FILES.iter().find(|f| f.os == os && f.arch == arch) {
        for (program, (asset, sha256)) in [
            (Program::Ffmpeg, files.ffmpeg),
            (Program::Ffprobe, files.ffprobe),
        ] {
            pinned.push(Pinned {
                program,
                repo: FFMPEG_STATIC,
                release: FFMPEG_RELEASE,
                asset,
                sha256,
                gzip: true,
            });
        }
    }
    if let Some(&(_, _, asset, sha256)) = WTSEXPORTER_FILES
        .iter()
        .find(|(file_os, file_arch, _, _)| *file_os == os && *file_arch == arch)
    {
        pinned.push(Pinned {
            program: Program::Wtsexporter,
            repo: WTSEXPORTER_REPO,
            release: WTSEXPORTER_RELEASE,
            asset,
            sha256,
            gzip: false,
        });
    }
    pinned
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
    /// The size of the program file written, which tells the app's file from
    /// one put over it since.
    pub size: u64,
}

impl Written {
    /// What the app records after writing `pinned`'s program, `size` bytes.
    fn of(pinned: &Pinned, size: u64) -> Self {
        Self {
            release: pinned.release.to_string(),
            asset: pinned.asset.to_string(),
            sha256: pinned.sha256.to_string(),
            size,
        }
    }

    /// Whether this is the file `pinned` publishes.
    fn is(&self, pinned: &Pinned) -> bool {
        self.release == pinned.release && self.asset == pinned.asset && self.sha256 == pinned.sha256
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
        .prefix(".manifest-")
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
    /// The file there stays.
    Keep,
    /// The file there is the pinned one, put there by someone else: it
    /// stays and goes into the record.
    Adopt(Written),
    /// The program is downloaded.
    Download,
}

/// What to do about `pinned` in `dir`, given the record.
fn need(pinned: &Pinned, dir: &Path, manifest: &Manifest) -> Need {
    let path = dir.join(pinned.program.file_name());
    let Ok(metadata) = std::fs::metadata(&path) else {
        return Need::Download;
    };
    if let Some(written) = manifest.get(&pinned.program)
        && written.size == metadata.len()
    {
        // The app wrote it. A newer pin replaces it, whoever uses it.
        return if written.is(pinned) {
            Need::Keep
        } else {
            Need::Download
        };
    }
    match pinned.program {
        Program::Ffmpeg | Program::Ffprobe => {
            if media::tool_in_dir(dir, pinned.program.name()).is_some() {
                Need::Keep
            } else {
                Need::Download
            }
        }
        Program::Wtsexporter => match file_sha256(&path) {
            Ok(sha256) if sha256 == pinned.sha256 => {
                Need::Adopt(Written::of(pinned, metadata.len()))
            }
            _ => Need::Download,
        },
    }
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
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoNetwork(why) => write!(
                f,
                "No connection to the download's server: {why}. It is tried again the next time the app starts."
            ),
            Self::Http(status) => {
                let reason = reqwest::StatusCode::from_u16(*status)
                    .ok()
                    .and_then(|status| status.canonical_reason())
                    .map(|reason| format!(" {reason}"))
                    .unwrap_or_default();
                write!(
                    f,
                    "The download's server answered {status}{reason}. It is tried again the next time the app starts."
                )
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
/// computed on the way. On a match it is unpacked when gzipped, made
/// executable, and renamed over the program; on a mismatch, or any failure,
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
    let name = pinned.program.name();
    let mut arrived = tempfile::Builder::new()
        .prefix(&format!(".{name}.download-"))
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
            .prefix(&format!(".{name}.unpack-"))
            .tempfile_in(dir)
            .map_err(write_error)?;
        let packed = arrived.as_file_mut();
        packed.rewind().map_err(write_error)?;
        io::copy(&mut flate2::read::GzDecoder::new(packed), &mut unpacked).map_err(write_error)?;
        unpacked
    } else {
        arrived
    };
    program.flush().map_err(write_error)?;
    program.as_file().sync_all().map_err(write_error)?;
    make_executable(program.path()).map_err(write_error)?;
    let size = program.as_file().metadata().map_err(write_error)?.len();
    program
        .persist(dir.join(pinned.program.file_name()))
        .map_err(|err| write_error(err.error))?;
    Ok(Written::of(pinned, size))
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

/// Check `dir` against `pinned` and download, one after another, what is
/// missing or not the pinned release, from `base`. ffmpeg and ffprobe are
/// passed over when both are on `PATH`. Each download's progress and
/// failure go to `downloads`, and each file written goes into the record.
pub fn download_missing(dir: &Path, base: &str, pinned: &[Pinned], downloads: &ToolDownloads) {
    let ffmpeg_on_path = media::ffmpeg_on_path();
    let mut manifest = read_manifest(dir);
    let mut wanted = Vec::new();
    for pin in pinned {
        if ffmpeg_on_path && matches!(pin.program, Program::Ffmpeg | Program::Ffprobe) {
            continue;
        }
        match need(pin, dir, &manifest) {
            Need::Keep => {}
            Need::Adopt(written) => {
                let _ = make_executable(&dir.join(pin.program.file_name()));
                manifest.insert(pin.program, written);
                let _ = write_manifest(dir, &manifest);
            }
            Need::Download => wanted.push(pin),
        }
    }
    if wanted.is_empty() {
        return;
    }
    let client = match download_client() {
        Ok(client) => client,
        Err(err) => {
            for pin in wanted {
                downloads.set(
                    pin.program,
                    DownloadState::Failed {
                        reason: DownloadError::NoNetwork(error_chain(&err)).to_string(),
                    },
                );
            }
            return;
        }
    };
    if let Err(err) = std::fs::create_dir_all(dir) {
        for pin in wanted {
            downloads.set(
                pin.program,
                DownloadState::Failed {
                    reason: DownloadError::CouldNotWrite(err.to_string()).to_string(),
                },
            );
        }
        return;
    }
    for pin in wanted {
        let program = pin.program;
        let progress = |received, total| {
            downloads.set(program, DownloadState::Downloading { received, total });
        };
        match download(&client, &pin.url(base), pin, dir, &progress) {
            Ok(written) => {
                manifest.insert(program, written);
                downloads.clear(program);
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
