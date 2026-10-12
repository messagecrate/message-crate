//! The download of ffmpeg, ffprobe and wtsexporter into the Tools Directory
//! (#1053, `docs/adr/0019`).
//!
//! At every start the app checks the Tools Directory in the background and
//! downloads what is missing or older than the release pinned here. Login
//! and browsing don't wait for it. A download that finds no network is
//! tried again a few times by the check itself, after a growing wait
//! ([`NO_NETWORK_WAITS`]); a download that failed is tried again at the next
//! start, or when **Try again** on the Import form asks ([`retry`]). An
//! import that needs a program still downloading waits for it
//! ([`ToolDownloads::wait_for`]).
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
//!   names the pinned release and its stamp is the one recorded, so ffmpeg
//!   and ffprobe are not read again at start-up. wtsexporter is read once
//!   per start, because its lookup counts it only by its SHA-256 (#2692).
//!   One that does not run is not downloaded again, because the download
//!   would be the same file: Settings says it doesn't run until a newer pin
//!   or another file changes that.
//! - **A pinned file with no entry is adopted.** A file with no entry, or
//!   whose stamp changed, whose SHA-256 is the pinned program's gets an
//!   entry and stays. The pinned program's SHA-256 is carried beside the
//!   asset's, because ffmpeg's and ffprobe's assets are gzipped.
//! - **Anything else is replaced** after a good download, whoever put it
//!   there: a file from an older release, or any other file. Until the
//!   download passes, an ffmpeg or ffprobe put there by hand is still the
//!   one used, so with no internet it still works.
//! - **Only the pinned wtsexporter counts** (#2692). The app needs the
//!   fork's pinned build, so a wtsexporter with any other SHA-256, such as
//!   a copy still being written or another build, is treated as missing
//!   until the download replaces it, by the import's lookup
//!   (`whatsapp_exporter::wtsexporter_in_dir`) and by the check alike
//!   ([`in_place`]). A failed download says so in its reason.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use media::InstalledTools;

mod fetch;
mod pins;
use fetch::{Fetch, HttpFetch, install, make_executable};
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
    pub gzipped: bool,
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
    /// Settings says so. Downloading it again gives the same file, and a
    /// file this account may not run, such as one another account took
    /// ownership of, is left for that account to fix, as an adopted one is.
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
    // The lookup reads wtsexporter whatever the record says, so the check
    // reads it too, or a file changed under the same stamp would show as
    // missing while every check keeps it.
    if not_the_pinned_file(pinned, dir) {
        return Need::Download;
    }
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
    if media::file_has_sha256(&path, pinned.program_sha256) {
        Need::Adopt
    } else {
        Need::Download
    }
}

/// Whether `program` in `dir` runs, as the lookup that uses it decides:
/// ffmpeg and ffprobe answer `-version` (an answer `media` keeps until the
/// file changes), and wtsexporter is a file with an executable bit this
/// account may run, which is all its lookup asks because a `pipx` shim is
/// slow to start. A bit alone is not enough: a root-owned `0o744` file has
/// one, and the app can't run it (#2793).
fn runs(program: Program, dir: &Path) -> bool {
    match program {
        Program::Ffmpeg | Program::Ffprobe => media::tool_in_dir(dir, program.name()).is_some(),
        Program::Wtsexporter => media::is_executable(&dir.join(program.file_name())),
    }
}

/// Whether `pin`'s program is a wtsexporter in `dir` that is not the pinned
/// file, such as a copy still being written or another build. Only the
/// pinned wtsexporter counts, as the WhatsApp import's lookup asks (#2692),
/// so such a file is not used. ffmpeg and ffprobe are never one. The file is
/// read once per version of it ([`media::file_has_sha256`]).
fn not_the_pinned_file(pin: &Pinned, dir: &Path) -> bool {
    let path = dir.join(pin.program.file_name());
    pin.program == Program::Wtsexporter
        && path.is_file()
        && !media::file_has_sha256(&path, pin.program_sha256)
}

/// Whether `pin`'s program in `dir` is in place for the lookup that uses
/// it: it [`runs`], and is not [`not_the_pinned_file`].
fn in_place(pin: &Pinned, dir: &Path) -> bool {
    runs(pin.program, dir) && !not_the_pinned_file(pin, dir)
}

/// The reason a download of `pin`'s program into `dir` failed with `err`.
/// A wtsexporter there that is not the pinned file is not used, so the
/// reason starts by saying so, because Settings shows wtsexporter as
/// missing though a file is there (#2692).
fn failure_reason(pin: &Pinned, dir: &Path, err: &DownloadError) -> String {
    if not_the_pinned_file(pin, dir) {
        format!(
            "{} {err}",
            whatsapp_exporter::not_pinned_reason(pin.release)
        )
    } else {
        err.to_string()
    }
}

/// The advice that ends the text of a program in the Tools Directory that
/// does not run: ffmpeg and ffprobe are found on `PATH` too. wtsexporter
/// has none here, because the run's error and Settings add its
/// troubleshooting section.
fn path_advice(program: Program) -> &'static str {
    match program {
        Program::Ffmpeg | Program::Ffprobe => {
            " Install it with your package manager instead. The app uses the copy on PATH."
        }
        Program::Wtsexporter => "",
    }
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
    /// The file could not be written to the Tools Directory, or read back
    /// from it.
    CouldNotWrite(String),
    /// The file arrived and passed its checksum, and is gzipped, and does
    /// not unpack.
    CouldNotUnpack(String),
    /// The program passed its checksum and is in place, and does not run.
    DoesNotRun(Program),
    /// The program found in place has no executable bit, and setting it
    /// failed, for the reason given.
    CouldNotMakeExecutable(Program, String),
    /// The check's thread could not be started, or the client the downloads
    /// go through could not be built, so nothing was downloaded.
    CouldNotStart(String),
    /// The check stopped partway through the download, such as on a panic
    /// in its thread, so the download never ended.
    Interrupted,
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
                    "The file could not be written to or read back from the Tools Directory: {why}."
                )
            }
            Self::CouldNotUnpack(why) => {
                write!(f, "The downloaded file could not be unpacked: {why}.")
            }
            Self::DoesNotRun(program) => write!(
                f,
                "{} in the Tools Directory doesn't run on this computer.{}",
                program.name(),
                path_advice(*program)
            ),
            Self::CouldNotMakeExecutable(program, why) => write!(
                f,
                "{} in the Tools Directory could not be marked as a program that runs: {why}.{}",
                program.name(),
                path_advice(*program)
            ),
            Self::CouldNotStart(why) => write!(f, "The download could not be started: {why}."),
            Self::Interrupted => f.write_str("The download was interrupted."),
        }
    }
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

/// The download state of each program, shared by the checks, `tools_status`
/// and an import waiting for a program. A program with no entry is not
/// downloading and has not failed since the app started.
#[derive(Debug, Clone, Default)]
pub struct ToolDownloads(Arc<Shared>);

/// The states, the signal a waiting import hears when one changes, and how
/// many checks run in this process.
#[derive(Debug, Default)]
struct Shared {
    states: Mutex<HashMap<Program, DownloadState>>,
    changed: Condvar,
    checks: AtomicUsize,
}

/// One check in this process, from the moment it is started until it ends,
/// owned by the thread that runs it.
///
/// While one exists [`ToolDownloads::checking`] says so, and the window
/// keeps asking for the status, because a program the check has not yet
/// looked at shows as missing. When it is dropped, at the end of the check
/// or on a panic in its thread, every program it marked that is still
/// downloading becomes failed with [`DownloadError::Interrupted`], so an
/// import waiting for one wakes and the Import form offers Try again.
#[derive(Debug)]
struct CheckRun {
    downloads: ToolDownloads,
    marked: Vec<Program>,
}

impl CheckRun {
    /// Show `program` as downloading, with nothing received yet.
    fn mark_downloading(&mut self, program: Program) {
        self.downloads.set(
            program,
            DownloadState::Downloading {
                received: 0,
                total: None,
            },
        );
        if !self.marked.contains(&program) {
            self.marked.push(program);
        }
    }
}

impl Drop for CheckRun {
    fn drop(&mut self) {
        {
            let mut states = self.downloads.states();
            for program in &self.marked {
                if matches!(states.get(program), Some(DownloadState::Downloading { .. })) {
                    states.insert(
                        *program,
                        DownloadState::Failed {
                            reason: DownloadError::Interrupted.to_string(),
                        },
                    );
                }
            }
        }
        self.downloads.0.checks.fetch_sub(1, Ordering::SeqCst);
        self.downloads.0.changed.notify_all();
    }
}

/// The waits before a download that found no network is tried again, one
/// before each new try (#2200): about two minutes in all, long enough for a
/// laptop opened before its Wi-Fi connects to come online. The programs of
/// one check share one round, which starts over only after a download
/// reaches its server, so when the network never comes the check ends
/// after one round, and an import waiting for a program hears it failed
/// within about two minutes.
const NO_NETWORK_WAITS: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

/// How often an import waiting for a download looks at its cancel flag when
/// no state changes.
const WAIT_TICK: Duration = Duration::from_millis(250);

impl ToolDownloads {
    fn states(&self) -> std::sync::MutexGuard<'_, HashMap<Program, DownloadState>> {
        self.0.states.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether a check runs in this process now, or has been started and not
    /// yet taken the lock.
    pub fn checking(&self) -> bool {
        self.0.checks.load(Ordering::SeqCst) > 0
    }

    /// Start a check: [`Self::checking`] holds until the returned run is
    /// dropped.
    fn begin_check(&self) -> CheckRun {
        self.0.checks.fetch_add(1, Ordering::SeqCst);
        CheckRun {
            downloads: self.clone(),
            marked: Vec::new(),
        }
    }

    /// Where `program`'s download stands, if anywhere.
    pub fn get(&self, program: Program) -> Option<DownloadState> {
        self.states().get(&program).cloned()
    }

    fn set(&self, program: Program, state: DownloadState) {
        self.states().insert(program, state);
        self.0.changed.notify_all();
    }

    fn clear(&self, program: Program) {
        self.states().remove(&program);
        self.0.changed.notify_all();
    }

    /// Wait while any of `programs` is downloading, so an import that needs
    /// a program still downloading goes on once it has arrived (#1053).
    ///
    /// `waiting` hears the program waited for and its progress each time it
    /// changes, for the import's progress line. `cancelled` is asked at
    /// least every [`WAIT_TICK`], so a Cancel ends the wait.
    ///
    /// Returns at once when none of `programs` is downloading: the import
    /// then finds the program or reports it missing as it always did.
    ///
    /// # Errors
    ///
    /// [`WaitError::Failed`] when a download waited for ended failed, with
    /// its reason; [`WaitError::Cancelled`] when `cancelled` said so.
    pub fn wait_for(
        &self,
        programs: &[Program],
        cancelled: &dyn Fn() -> bool,
        waiting: &mut dyn FnMut(Program, u64, Option<u64>),
    ) -> Result<(), WaitError> {
        let mut waited = Vec::new();
        let mut states = self.states();
        loop {
            let downloading = programs
                .iter()
                .find_map(|&program| match states.get(&program) {
                    Some(DownloadState::Downloading { received, total }) => {
                        Some((program, *received, *total))
                    }
                    _ => None,
                });
            let Some((program, received, total)) = downloading else {
                break;
            };
            if !waited.contains(&program) {
                waited.push(program);
            }
            drop(states);
            if cancelled() {
                return Err(WaitError::Cancelled);
            }
            waiting(program, received, total);
            states = self.states();
            // Woken by any change, or after a tick to look at the cancel flag.
            states = self
                .0
                .changed
                .wait_timeout(states, WAIT_TICK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        for program in waited {
            if let Some(DownloadState::Failed { reason }) = states.get(&program) {
                return Err(WaitError::Failed {
                    program,
                    reason: reason.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Why an import's wait for a download ended without the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitError {
    /// The download waited for failed, for `reason`.
    Failed {
        /// The program whose download failed.
        program: Program,
        /// Why, as [`DownloadError`] says it.
        reason: String,
    },
    /// The import was cancelled while it waited.
    Cancelled,
}

impl fmt::Display for WaitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed { program, reason } => write!(
                f,
                "The {} download failed. {reason} Try again on the Import form downloads it again. {}",
                program.name(),
                troubleshooting(*program)
            ),
            Self::Cancelled => fmt::Display::fmt(&message_crate_core::Cancelled, f),
        }
    }
}

impl std::error::Error for WaitError {}

/// The sentence that sends a person to the user guide's troubleshooting
/// section for `program`, which says how to install it or where to put a
/// copy by hand. The Import form and Settings link the same section. The
/// wtsexporter sentence is the exporter's, which puts it in its own errors,
/// so each heading's text is written once.
pub fn troubleshooting(program: Program) -> String {
    match program {
        Program::Ffmpeg | Program::Ffprobe => {
            "See \"ffmpeg or ffprobe not found\" in Troubleshooting at messagecrate.app."
                .to_string()
        }
        Program::Wtsexporter => whatsapp_exporter::WTSEXPORTER_TROUBLESHOOTING.to_string(),
    }
}

/// The state of `program` in place that does not run.
fn does_not_run(program: Program) -> DownloadState {
    DownloadState::Failed {
        reason: DownloadError::DoesNotRun(program).to_string(),
    }
}

/// What became of a program the start-up check adopts.
#[derive(Debug)]
enum Adopted {
    /// It runs. The error is why its executable bits could not be set,
    /// when that failed and it runs anyway.
    Runs(Option<io::Error>),
    /// Its executable bits were set, and it does not run.
    DoesNotRun,
    /// It does not run, and setting its executable bits failed for this
    /// reason.
    NotExecutable(io::Error),
}

/// What became of an adopted program, given the result of setting its
/// executable bits and whether it `runs` after that. A file that does not
/// run, and whose bits could not be set, is blamed on the bits even when
/// some are set: its mode can't tell whether this user may run it, such as
/// a file another user owns with only the owner's bit.
fn classify_adopted(made_executable: io::Result<()>, runs: bool) -> Adopted {
    match (runs, made_executable) {
        (true, result) => Adopted::Runs(result.err()),
        (false, Ok(())) => Adopted::DoesNotRun,
        (false, Err(err)) => Adopted::NotExecutable(err),
    }
}

/// Write the record in `dir`. A failure goes to the process log and fails
/// nothing: `program` is in place either way, and without its entry the
/// next start reads the file again and adopts it.
fn write_manifest_or_log(dir: &Path, manifest: &Manifest, program: Program) {
    if let Err(err) = write_manifest(dir, manifest) {
        eprintln!(
            "{} is in place in the Tools Directory, and {MANIFEST_FILE} could not be written: {err}",
            program.name()
        );
    }
}

/// Record `pin`'s program, adopted at `path` in `dir`, with the stamp of
/// `path`, and write the record in `dir`. A failure goes to the process log
/// and fails nothing: the program is in place either way, and without its
/// entry the next start reads the file again.
fn record_adopted_or_log(dir: &Path, path: &Path, manifest: &mut Manifest, pin: &Pinned) {
    let program = pin.program;
    match Stamp::at(path) {
        Ok(stamp) => {
            manifest.insert(program, Written::of(pin, stamp));
            write_manifest_or_log(dir, manifest, program);
        }
        Err(err) => {
            let name = program.name();
            eprintln!(
                "{name} is in place in the Tools Directory, and was not recorded in {MANIFEST_FILE} \
                 because {name}'s size and modified time could not be read: {err}"
            );
        }
    }
}

/// The lock on [`LOCK_FILE`] a check holds, let go when it is dropped.
///
/// It is let go by unlocking, not only by closing the file. A child this
/// process starts on another thread holds every descriptor the process had
/// open from its fork until its exec, and a lock belongs to the open file
/// the descriptors share, so a lock let go only by closing would stay held
/// for that instant, and a check that starts then would read it as another
/// check running and do nothing.
#[derive(Debug)]
struct CheckLock(File);

impl Drop for CheckLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Take the lock on [`LOCK_FILE`] in `dir`, making `dir` first. `None` when
/// another check holds it.
fn lock_check(dir: &Path) -> io::Result<Option<CheckLock>> {
    std::fs::create_dir_all(dir)?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK_FILE))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(CheckLock(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(err)) => Err(err),
    }
}

/// Check `dir` against `pinned` and download, one after another, what is
/// missing or not the pinned release, with `fetch`. ffmpeg and ffprobe are
/// passed over when both are on `PATH`. Each download's progress and
/// failure go to the downloads `run` belongs to, and each file written goes
/// into the record.
///
/// Another app's check running on `dir` already does the work, so this one
/// returns at once. When the lock can't be taken because the Tools Directory
/// can't be written, the check goes on and its downloads say why.
///
/// `wait` pauses the check before it tries a download again that found no
/// network: [`std::thread::sleep`] in the app.
fn download_missing(
    dir: &Path,
    installed: &InstalledTools,
    fetch: &dyn Fetch,
    pinned: &[Pinned],
    mut run: CheckRun,
    wait: &dyn Fn(Duration),
) {
    let lock = match lock_check(dir) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => return,
        Err(_) => None,
    };
    check(dir, installed, fetch, pinned, &mut run, lock, wait);
}

/// What [`retry`] did.
#[derive(Debug)]
pub enum Retry {
    /// A check started, on the thread this carries.
    Started(JoinHandle<()>),
    /// Another check holds the lock on the Tools Directory: the start-up
    /// check, an earlier Try again, or another app's check. Nothing started,
    /// and the check already running brings the programs.
    AlreadyRunning,
    /// The check's thread could not be started: each program in `not_found`
    /// that has a pin fails with [`DownloadError::CouldNotStart`].
    CouldNotStart,
}

/// Start the check of `dir` again on a thread of its own, as **Try again**
/// on the Import form asks (#1053), and return without waiting for it. The
/// files are downloaded from the releases under `base`, which is [`GITHUB`]
/// outside tests.
///
/// `not_found` are the programs the window says are not found. They show as
/// downloading before this returns, so the window, which asks again only
/// while a download runs, keeps asking until the check has decided each one.
///
/// Returns [`Retry::Started`] with the check's thread, or says why no check
/// started: [`Retry::AlreadyRunning`] or [`Retry::CouldNotStart`].
pub fn retry(
    dir: PathBuf,
    installed: InstalledTools,
    base: String,
    pinned: Vec<Pinned>,
    downloads: &ToolDownloads,
    not_found: &[Program],
) -> Retry {
    let fetch = Box::new(HttpFetch::new(base));
    retry_with(
        dir,
        installed,
        fetch,
        pinned,
        downloads,
        not_found,
        |work| {
            std::thread::Builder::new()
                .name("tool-downloads-retry".into())
                .spawn(work)
        },
    )
}

/// The work [`retry`] hands to a new thread.
type Work = Box<dyn FnOnce() + Send>;

/// [`retry`], getting the files with `fetch` and starting the check's
/// thread with `spawn`, so a test can make the start fail.
fn retry_with(
    dir: PathBuf,
    installed: InstalledTools,
    fetch: Box<dyn Fetch>,
    pinned: Vec<Pinned>,
    downloads: &ToolDownloads,
    not_found: &[Program],
    spawn: impl FnOnce(Work) -> io::Result<JoinHandle<()>>,
) -> Retry {
    let lock = match lock_check(&dir) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => return Retry::AlreadyRunning,
        Err(_) => None,
    };
    let mut run = downloads.begin_check();
    let marked: Vec<Program> = pinned
        .iter()
        .map(|pin| pin.program)
        .filter(|program| not_found.contains(program))
        .collect();
    for &program in &marked {
        run.mark_downloading(program);
    }
    // The run goes to the thread only once the thread has started. Were it
    // moved into the closure, a failed start would drop it there, and its
    // drop would mark these programs interrupted, and wake a waiting import
    // with that reason, before the cause is known.
    let (hand_over, handed) = std::sync::mpsc::sync_channel::<CheckRun>(1);
    let spawned = spawn(Box::new(move || {
        if let Ok(mut run) = handed.recv() {
            check(
                &dir,
                &installed,
                &*fetch,
                &pinned,
                &mut run,
                lock,
                &std::thread::sleep,
            );
        }
    }));
    match spawned {
        Ok(handle) => {
            // The thread waits on `handed` until this arrives, so the send
            // can't fail.
            let _ = hand_over.send(run);
            Retry::Started(handle)
        }
        // Each program fails with the thread's error first, so the run's
        // drop, which fails only the ones still downloading, leaves them.
        Err(err) => {
            for program in marked {
                downloads.set(
                    program,
                    DownloadState::Failed {
                        reason: DownloadError::CouldNotStart(err.to_string()).to_string(),
                    },
                );
            }
            drop(run);
            Retry::CouldNotStart
        }
    }
}

/// The check [`download_missing`] and [`retry`] run as `run`, holding
/// `_lock`, or no lock when the Tools Directory can't be written. Every
/// pinned program ends with its state decided: none when it is in place and
/// runs, failed with the reason otherwise. `wait` is the pause before a
/// download that found no network is tried again.
fn check(
    dir: &Path,
    installed: &InstalledTools,
    fetch: &dyn Fetch,
    pinned: &[Pinned],
    run: &mut CheckRun,
    _lock: Option<CheckLock>,
    wait: &dyn Fn(Duration),
) {
    // A clone of the handle (one `Arc`), because `run` is borrowed mutably
    // below to mark the programs wanted as downloading.
    let downloads = &run.downloads.clone();
    delete_leftovers(dir);
    let ffmpeg_on_path = installed.on_path();
    let mut manifest = read_manifest(dir);
    let mut wanted = Vec::new();
    for pin in pinned {
        if ffmpeg_on_path && matches!(pin.program, Program::Ffmpeg | Program::Ffprobe) {
            downloads.clear(pin.program);
            continue;
        }
        match need(pin, dir, &manifest) {
            Need::Keep => downloads.clear(pin.program),
            Need::Adopt => {
                let program = pin.program;
                let path = dir.join(program.file_name());
                let made_executable = make_executable(&path);
                match classify_adopted(made_executable, runs(program, dir)) {
                    Adopted::Runs(None) => downloads.clear(program),
                    Adopted::Runs(Some(err)) => {
                        eprintln!(
                            "{} in the Tools Directory runs, and its executable bits could not be set: {err}",
                            program.name()
                        );
                        downloads.clear(program);
                    }
                    Adopted::DoesNotRun => downloads.set(program, does_not_run(program)),
                    // Left out of the record, so the next start sets the
                    // bits again and, if that fails again, says why again.
                    Adopted::NotExecutable(err) => {
                        downloads.set(
                            program,
                            DownloadState::Failed {
                                reason: DownloadError::CouldNotMakeExecutable(
                                    program,
                                    err.to_string(),
                                )
                                .to_string(),
                            },
                        );
                        continue;
                    }
                }
                record_adopted_or_log(dir, &path, &mut manifest, pin);
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
        run.mark_downloading(pin.program);
    }
    // Each program's file as the check found it. One that appears or
    // changes later, while an earlier program's tries wait included, was
    // put there by someone else, such as a copy put in by hand.
    let found: HashMap<Program, Option<Stamp>> = wanted
        .iter()
        .map(|pin| {
            let path = dir.join(pin.program.file_name());
            (pin.program, Stamp::at(&path).ok())
        })
        .collect();
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
    if let Err(err) = std::fs::create_dir_all(dir) {
        return fail_all(DownloadError::CouldNotWrite(err.to_string()));
    }
    let mut no_network_waits = NO_NETWORK_WAITS.iter();
    for pin in wanted {
        let program = pin.program;
        let progress = |received, total| {
            downloads.set(program, DownloadState::Downloading { received, total });
        };
        let mut attempt = || install(fetch, pin, dir, &progress);
        let show_downloading = || {
            downloads.set(
                program,
                DownloadState::Downloading {
                    received: 0,
                    total: None,
                },
            );
        };
        let path = dir.join(program.file_name());
        let was_put_in_place = || {
            Stamp::at(&path)
                .ok()
                .is_some_and(|now| found.get(&program) != Some(&Some(now)))
                && in_place(pin, dir)
        };
        // Put in place while an earlier program's tries waited.
        if was_put_in_place() {
            downloads.clear(program);
            continue;
        }
        let tried = try_until_answered(
            &mut no_network_waits,
            wait,
            show_downloading,
            was_put_in_place,
            &mut attempt,
        );
        let result = match tried {
            Tried::Done(result) => result,
            // The lookup finds it, so nothing waits for the download.
            Tried::PutInPlace => {
                downloads.clear(program);
                continue;
            }
        };
        if !matches!(result, Err(DownloadError::NoNetwork(_))) {
            // The download reached its server, so the network is there:
            // the next program that finds none gets the whole round again.
            no_network_waits = NO_NETWORK_WAITS.iter();
        }
        match result {
            Ok(written) => {
                manifest.insert(program, written);
                if runs(program, dir) {
                    downloads.clear(program);
                } else {
                    downloads.set(program, does_not_run(program));
                }
                // The program is in place and the lookup finds it, so its
                // download did not fail when the record can't be written.
                write_manifest_or_log(dir, &manifest, program);
            }
            Err(err) => downloads.set(
                program,
                DownloadState::Failed {
                    reason: failure_reason(pin, dir, &err),
                },
            ),
        }
    }
}

/// What [`try_until_answered`] came to.
enum Tried<T> {
    /// The last try's result: one that reached the server, a failure for
    /// any other reason included, or [`DownloadError::NoNetwork`] once the
    /// waits were spent.
    Done(Result<T, DownloadError>),
    /// During a wait the program was put in place by someone else, and it
    /// runs, so no try is needed.
    PutInPlace,
}

/// Run `attempt` until it gets past [`DownloadError::NoNetwork`], waiting
/// the next of `waits` before each new try. `show_downloading` runs before
/// each wait. After each wait `was_put_in_place` is asked whether the program
/// is now there without the download, and when it is the tries end.
fn try_until_answered<T>(
    waits: &mut std::slice::Iter<'static, Duration>,
    wait: &dyn Fn(Duration),
    show_downloading: impl Fn(),
    was_put_in_place: impl Fn() -> bool,
    attempt: &mut dyn FnMut() -> Result<T, DownloadError>,
) -> Tried<T> {
    loop {
        match attempt() {
            Err(DownloadError::NoNetwork(why)) => match waits.next() {
                Some(&pause) => {
                    show_downloading();
                    wait(pause);
                    if was_put_in_place() {
                        return Tried::PutInPlace;
                    }
                }
                None => return Tried::Done(Err(DownloadError::NoNetwork(why))),
            },
            answered => return Tried::Done(answered),
        }
    }
}

/// Start the check of the Tools Directory `dir` on a thread of its own, for
/// this computer's platform, so login and browsing don't wait for it.
///
/// The check counts as running from before the thread starts, so a window
/// that asks for the status at once keeps asking until the check ends.
pub fn start(dir: PathBuf, installed: InstalledTools, downloads: &ToolDownloads) {
    let pinned = pinned_for(std::env::consts::OS, std::env::consts::ARCH);
    let run = downloads.begin_check();
    let _ = std::thread::Builder::new()
        .name("tool-downloads".into())
        .spawn(move || {
            download_missing(
                &dir,
                &installed,
                &HttpFetch::new(GITHUB),
                &pinned,
                run,
                &std::thread::sleep,
            )
        });
}

#[cfg(test)]
mod tests;
