//! Locate and run the external `wtsexporter` CLI.

use crate::ios_backup::DecryptedWhatsapp;
use anyhow::{Context, Result, bail};
use message_crate_core::LogSink;
use message_staging::scratch_disk_full;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod pin;
use pin::pinned_sha256_here;
pub use pin::{WTSEXPORTER_RELEASE, WTSEXPORTER_REPO, WtsexporterFile, pinned_wtsexporter};

/// Where a copy of `wtsexporter` put in the Tools Directory by hand comes
/// from: a `wtsexporter_<platform>` file of Message Crate's fork at release
/// `0.13.0-mc.2`, the first that records `full_key_id` and `reply_key_id`, the
/// ids a quoted reply is linked by, and `reaction_details`, each reaction
/// under the reactor's id, renamed to the name `resolve_wtsexporter` looks
/// for. Upstream records none of them.
const RELEASE_FILE_HINT: &str = "a wtsexporter_<platform> file from the messagecrate/WhatsApp-Chat-Exporter 0.13.0-mc.2 release, renamed to wtsexporter (wtsexporter.exe on Windows) and made executable (chmod +x on Linux and macOS)";

/// The sentence that sends a person to the user guide's section on a
/// `wtsexporter` that is missing or doesn't run, which says how to get one
/// by hand, including where the app has no download of its own. The desktop
/// app's `tool_downloads::troubleshooting` gives this same constant for
/// wtsexporter, so the heading's text lives here once.
pub const WTSEXPORTER_TROUBLESHOOTING: &str =
    "See \"Import can't find wtsexporter\" in Troubleshooting at messagecrate.app.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Android,
    Ios,
}

impl Platform {
    /// The wtsexporter command-line flag for this platform.
    pub fn as_flag(self) -> &'static str {
        match self {
            Self::Android => "-a",
            Self::Ios => "-i",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WtsexporterArgs {
    pub platform: Platform,
    /// Search root for relative defaults (`msgstore.db`, `wa.db`, …): the backup
    /// input, or the process cwd when the config names no input.
    pub input: PathBuf,
    /// Scratch directory for wtsexporter (media extract + JSON). Must outlive convert.
    pub work_dir: PathBuf,
    /// Key file path or crypt15 hex string (`-k`).
    pub key: Option<String>,
    pub backup: Option<PathBuf>,
    pub wa: Option<PathBuf>,
    pub media: Option<PathBuf>,
    pub db: Option<PathBuf>,
    pub is_business_app: bool,
}

impl WtsexporterArgs {
    /// Read WhatsApp's decrypted files instead of the iPhone backup they
    /// came from. No backup is passed: given an encrypted backup,
    /// wtsexporter asks for its password on a terminal the app does not
    /// have. Contacts chosen on the form still win over the backup's own.
    pub fn read_decrypted(&mut self, decrypted: DecryptedWhatsapp) {
        self.backup = None;
        self.input = decrypted.domain_dir.clone();
        self.db = Some(decrypted.database);
        self.media.get_or_insert(decrypted.domain_dir);
    }
}

/// The file name of `wtsexporter`, with `.exe` on Windows.
pub fn wtsexporter_file_name() -> &'static str {
    if cfg!(windows) {
        "wtsexporter.exe"
    } else {
        "wtsexporter"
    }
}

/// Where `wtsexporter` is: the app's own copy in `tools_dir`, the Tools
/// Directory, counting only a file whose SHA-256 is `pinned_sha256` where
/// that is `Some`. `Ok(None)` when there is no Tools Directory or no file in
/// it, and when the file there is not the pinned file, such as a copy still
/// being written or another build: it is treated as missing, and the
/// desktop app downloads the pinned file over it. The desktop app's status
/// passes the pin its download uses ([`pinned_wtsexporter`]), and a test
/// passes a pin of its own.
///
/// # Errors
///
/// Returns an error when the file is there but is not executable by this
/// account.
pub fn wtsexporter_in_dir(
    tools_dir: Option<&Path>,
    pinned_sha256: Option<&str>,
) -> Result<Option<PathBuf>> {
    Ok(match find_wtsexporter(tools_dir, pinned_sha256)? {
        Lookup::Found(path) => Some(path),
        Lookup::Missing | Lookup::NotPinned(_) => None,
    })
}

/// Locate `wtsexporter` (the Python WhatsApp export tool this crate shells
/// out to) in `tools_dir`, the Tools Directory the run names
/// ([`message_crate_core::RunContext::tools_dir`]).
///
/// # Errors
///
/// Returns an error when it is not there, or is not the pinned file.
pub(crate) fn resolve_wtsexporter(tools_dir: Option<&Path>) -> Result<PathBuf> {
    wtsexporter_in(tools_dir, pinned_sha256_here())
}

/// Why a wtsexporter in the Tools Directory that is not the file of
/// `release` is not used (#2692). The import's error and the desktop app's
/// failed download both start with it, so it reads the same in each.
pub fn not_pinned_reason(release: &str) -> String {
    format!(
        "The wtsexporter in the Tools Directory is not the file of release {release}, or is \
         still being copied there, so it is not used."
    )
}

/// What [`find_wtsexporter`] found.
#[derive(Debug, PartialEq, Eq)]
enum Lookup {
    /// No Tools Directory, or no file in it.
    Missing,
    /// A file that is not the pinned one: a copy still being written, or
    /// another build. It is not used.
    NotPinned(PathBuf),
    /// The file to run.
    Found(PathBuf),
}

/// `wtsexporter` in `tools_dir`, and nowhere else: it is always the app's
/// own copy, so neither `PATH` nor an environment variable is read (#1053).
///
/// Where the app has a download for this computer, `pinned_sha256` is the
/// pinned file's SHA-256, and a file with any other is
/// [`Lookup::NotPinned`] (#2692). The file is read once per version of it
/// ([`media::file_has_sha256`]), so a lookup of an unchanged file reads
/// nothing.
///
/// Where the app has none, such as Linux on ARM, `pinned_sha256` is `None`
/// and the file must only have an executable bit this account may run. It
/// is not run here: a `pipx` shim is slow to start. A shim is what is
/// linked there on such a computer. So a link to a `pipx` shim whose Python
/// has gone is found, and fails only when it is run, with the hint
/// [`run_wtsexporter`] gives (`docs/adr/0019`).
///
/// # Errors
///
/// Returns an error when the file is there, is the pinned one where there
/// is one, and is not executable by this account.
fn find_wtsexporter(tools_dir: Option<&Path>, pinned_sha256: Option<&str>) -> Result<Lookup> {
    let Some(tools_dir) = tools_dir else {
        return Ok(Lookup::Missing);
    };
    let candidate = tools_dir.join(wtsexporter_file_name());
    if !candidate.is_file() {
        return Ok(Lookup::Missing);
    }
    if let Some(sha256) = pinned_sha256
        && !media::file_has_sha256(&candidate, sha256)
    {
        return Ok(Lookup::NotPinned(candidate));
    }
    if !media::is_executable(&candidate) {
        bail!(
            "{} is not executable by this account. It must be {RELEASE_FILE_HINT}. \
             A copy another account owns needs that account to run chmod +x on it, \
             or to give this account ownership of it. On a disk mounted noexec no \
             program runs, whoever owns it.",
            candidate.display()
        );
    }
    Ok(Lookup::Found(candidate))
}

/// [`find_wtsexporter`], with a missing file, or one that is not the pinned
/// file, as an error.
///
/// # Errors
///
/// Returns an error naming the Tools Directory when the file is not in it,
/// saying there is none, saying the file there is not the pinned one, or
/// saying the file is not executable.
fn wtsexporter_in(tools_dir: Option<&Path>, pinned_sha256: Option<&str>) -> Result<PathBuf> {
    let executable = wtsexporter_file_name();
    match find_wtsexporter(tools_dir, pinned_sha256)? {
        Lookup::Found(found) => Ok(found),
        Lookup::NotPinned(path) => bail!(
            "{} It is {}, and must be {RELEASE_FILE_HINT}. The Import form's Try again \
             downloads that file. {WTSEXPORTER_TROUBLESHOOTING}",
            not_pinned_reason(WTSEXPORTER_RELEASE),
            path.display()
        ),
        Lookup::Missing => match tools_dir {
            None => bail!(
                "Could not find {executable}: no Tools Directory is set. The desktop app keeps \
                 {executable} in its Tools Directory."
            ),
            Some(tools_dir) => bail!(
                "Could not find {executable} in the Tools Directory, {}. Put {RELEASE_FILE_HINT} \
                 there. {WTSEXPORTER_TROUBLESHOOTING}",
                tools_dir.display()
            ),
        },
    }
}

/// Run wtsexporter in `args.work_dir`; write JSON to `json_out`.
/// Returns stderr+stdout for logging.
///
/// A failed run's whole output goes to `log` as a warning before the
/// failure is mapped, whatever the failure, so the Import Run's log holds
/// what wtsexporter said even when the error the person sees is the
/// free-space sentence (#1938).
///
/// # Errors
///
/// Returns an error when the work directory is missing, the process cannot start, or
/// wtsexporter exits with a non-zero status. When wtsexporter's output says
/// the disk is full, or writing the key file into the work directory fails
/// because the disk is full, the error is the free-space sentence for the
/// Scratch Directory.
pub(crate) fn run_wtsexporter(
    bin: &Path,
    args: &WtsexporterArgs,
    json_out: &Path,
    log: &LogSink,
) -> Result<String> {
    if !args.work_dir.is_dir() {
        bail!("work directory does not exist: {}", args.work_dir.display());
    }
    let out_dir = json_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let output = wtsexporter_command(bin, args, out_dir, json_out)?
        .output()
        .map_err(|err| {
            let hint = if err.kind() == std::io::ErrorKind::NotFound {
                format!(
                    " (often a link to a pipx install whose Python interpreter has gone. {WTSEXPORTER_TROUBLESHOOTING})"
                )
            } else {
                String::new()
            };
            anyhow::anyhow!("spawn {}: {err}{hint}", bin.display())
        })?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        if !combined.trim().is_empty() {
            log.warn(format!(
                "wtsexporter failed ({}). Its output:\n{}",
                output.status,
                combined.trim_end()
            ));
        }
        // wtsexporter writes everything into the work directory, under the
        // Scratch Directory: the decrypted msgstore.db, the extract and the
        // JSON. A decrypted Android database has no size until it is written,
        // so no check can measure it first; a disk that fills is reported
        // as a check would report it (#1820).
        if names_a_full_disk(&combined) {
            return Err(scratch_disk_full());
        }
        bail!(
            "wtsexporter failed ({}){}\n{}",
            output.status,
            if combined.trim().is_empty() { "" } else { ":" },
            combined.trim()
        );
    }
    if !json_out.is_file() {
        bail!(
            "wtsexporter finished but JSON missing at {}. Output:\n{}",
            json_out.display(),
            combined.trim()
        );
    }
    Ok(combined)
}

/// Whether wtsexporter's output says a write failed because its disk is
/// full. Python prints `[Errno 28]` for `ENOSPC` and `[WinError 112]` for
/// Windows' `ERROR_DISK_FULL` in every system language; the words after them
/// are matched too, in English, for output that carries the words alone.
fn names_a_full_disk(output: &str) -> bool {
    [
        "[Errno 28]",
        "[WinError 112]",
        "No space left on device",
        "There is not enough space on the disk",
    ]
    .iter()
    .any(|words| output.contains(words))
}

/// `err`, from a write into the work directory under the Scratch Directory,
/// as the free-space error when the disk is full, so a disk already full
/// before wtsexporter starts reads as one that fills while it runs.
/// Any other error keeps `what` as its context.
fn scratch_write_error(err: std::io::Error, what: String) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::StorageFull {
        scratch_disk_full()
    } else {
        anyhow::Error::new(err).context(what)
    }
}

/// The wtsexporter command for `args`, writing media to `out_dir` and JSON
/// to `json_out`. A hex key is written to a file in the work directory here.
///
/// # Errors
///
/// Returns an error when a forwarded path cannot be resolved or the key
/// file cannot be written.
fn wtsexporter_command(
    bin: &Path,
    args: &WtsexporterArgs,
    out_dir: &Path,
    json_out: &Path,
) -> Result<Command> {
    let paths = resolve_forwarded_paths(args)?;
    let mut cmd = Command::new(bin);
    // Scratch cwd so iOS/Android extract does not pollute the GUI launch directory.
    cmd.current_dir(&args.work_dir)
        .arg(args.platform.as_flag())
        .arg("--no-html")
        .arg("--no-banner")
        .arg("-o")
        .arg(out_dir)
        .arg("-j")
        .arg(json_out)
        // wtsexporter uses tqdm; without this, progress bars spam piped capture
        // (GUI only shows the dump after the process exits).
        .env("TQDM_DISABLE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(db) = &paths.db {
        cmd.arg("-d").arg(db);
    }
    match paths.key.as_deref() {
        // Key file path — the path itself is not secret, forward as-is.
        Some(key) if looks_like_path(key) => {
            cmd.arg("-k").arg(key);
        }
        // Hex key material — write the decoded bytes to a 0600 file in the
        // scratch work directory and pass the path, so the secret never appears in
        // the process command line (/proc/<pid>/cmdline).
        Some(key) => {
            let key_path = write_key_file(&args.work_dir, key)?;
            cmd.arg("-k").arg(&key_path);
        }
        None => {}
    }
    push_opt(&mut cmd, "-b", paths.backup.as_deref());
    push_opt(&mut cmd, "-w", paths.wa.as_deref());
    push_opt(&mut cmd, "-m", paths.media.as_deref());
    if args.is_business_app {
        cmd.arg("--business");
    }
    // Never pass `-c` (--move-media): wtsexporter would shutil.move the user's
    // media directory into the scratch work directory, which is deleted when the run
    // finishes — permanently destroying the original media. Always copy.
    Ok(cmd)
}

struct ForwardedPaths {
    key: Option<String>,
    backup: Option<PathBuf>,
    wa: Option<PathBuf>,
    media: Option<PathBuf>,
    db: Option<PathBuf>,
}

/// Whether wtsexporter extracts WhatsApp's files from an iPhone backup into
/// the work directory for `args`. It does when it is given a backup and no
/// media directory that exists; with one, it says "WhatsApp directory
/// already exists, skipping WhatsApp file extraction" and reads that.
///
/// # Errors
///
/// Returns an error when a forwarded path cannot be resolved.
pub(crate) fn extracts_ios_backup(args: &WtsexporterArgs) -> Result<bool> {
    if args.platform != Platform::Ios {
        return Ok(false);
    }
    let paths = resolve_forwarded_paths(args)?;
    Ok(paths.backup.is_some() && !paths.media.as_deref().is_some_and(Path::is_dir))
}

/// Absolutize user paths and fill Android/iOS defaults from `input` when missing.
///
/// An iPhone backup passed with `-b` fills nothing from `input`: wtsexporter
/// takes the database, contacts and media from the backup, and WhatsApp's
/// files at the backup directory's root are left over from an earlier extract.
fn resolve_forwarded_paths(args: &WtsexporterArgs) -> Result<ForwardedPaths> {
    let search = input_search_root(&args.input)?;
    let search_input = !(args.platform == Platform::Ios && args.backup.is_some());

    let db = match &args.db {
        Some(p) => Some(absolutize(p)?),
        None if !search_input => None,
        None if args.input.is_file() => Some(absolutize(&args.input)?),
        None => first_existing(&[
            search.join("msgstore.db"),
            search.join("ChatStorage.sqlite"),
        ]),
    };

    let wa = match &args.wa {
        Some(p) => Some(absolutize(p)?),
        None if !search_input => None,
        None => first_existing(&[
            search.join("wa.db"),
            search.join("ContactsV2.sqlite"),
            search.join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared/ContactsV2.sqlite"),
            search.join("AppDomainGroup-group.net.whatsapp.WhatsAppSMB.shared/ContactsV2.sqlite"),
        ]),
    };

    let media = match &args.media {
        Some(p) => Some(absolutize(p)?),
        None if !search_input => None,
        None => first_existing(&[
            search.join("WhatsApp"),
            search.join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared"),
            search.join("AppDomainGroup-group.net.whatsapp.WhatsAppSMB.shared"),
        ]),
    };

    let backup = match &args.backup {
        Some(p) => Some(absolutize(p)?),
        None if args.platform == Platform::Android => android_crypt_backup(&search),
        None => None,
    };

    // Do not pass `-k` when no backup is forwarded.
    let key = if backup.is_none() {
        None
    } else {
        match args.key.as_deref().and_then(message_ir::trimmed) {
            Some(k) if looks_like_path(k) => {
                let path = absolutize(Path::new(k))?;
                Some(path.to_string_lossy().into_owned())
            }
            Some(k) => Some(k.to_string()),
            None => None,
        }
    };

    Ok(ForwardedPaths {
        key,
        backup,
        wa,
        media,
        db,
    })
}

/// Directory used to resolve relative wtsexporter defaults (`msgstore.db`, and similar).
///
/// # Errors
///
/// Returns an error when `input` does not exist.
fn input_search_root(input: &Path) -> Result<PathBuf> {
    if input.is_dir() {
        return absolutize(input);
    }
    if input.is_file() {
        let parent = input
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        return absolutize(parent);
    }
    bail!("input path does not exist: {}", input.display());
}

/// First candidate path that exists on disk.
fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.exists()).cloned()
}

/// Android crypt file in `search` for `-b`, or `None` so wtsexporter defaults apply.
///
/// Prefers a decrypted `msgstore.db` file over any crypt name. Crypt names are
/// checked at the directory root only, in order: crypt12, crypt14, crypt15.
/// Directories with those names are ignored (`is_file`), matching the form probe.
pub(crate) fn android_crypt_backup(search: &Path) -> Option<PathBuf> {
    if search.join("msgstore.db").is_file() {
        return None;
    }
    [
        search.join("msgstore.db.crypt12"),
        search.join("msgstore.db.crypt14"),
        search.join("msgstore.db.crypt15"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// True when `s` looks like a filesystem path rather than a hex key string.
fn looks_like_path(s: &str) -> bool {
    s.contains('/') || s.contains('\\') || s.ends_with(".key") || Path::new(s).exists()
}

/// Make `path` absolute relative to the current working directory.
///
/// # Errors
///
/// Returns an error when the current working directory cannot be read.
fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = env::current_dir().context("resolve current working directory")?;
    Ok(cwd.join(path))
}

/// Append `flag` and `path` to `cmd` when `path` is `Some`.
fn push_opt(cmd: &mut Command, flag: &str, path: Option<&Path>) {
    if let Some(p) = path {
        cmd.arg(flag).arg(p);
    }
}

/// Write hex-encoded decryption key bytes to a 0600 file in the scratch work directory.
///
/// wtsexporter's `-k` accepts a hex string or a key file path (there is no
/// stdin key support upstream), so the file path is forwarded instead of the
/// hex string itself. The file lives in the disposable scratch dir and is
/// removed with it when the run finishes.
///
/// # Errors
///
/// Returns an error when the hex is invalid or the file cannot be written.
fn write_key_file(work_dir: &Path, hex_key: &str) -> Result<PathBuf> {
    let cleaned: String = hex_key.chars().filter(|c| !c.is_whitespace()).collect();
    // Deliberately do not echo the key material in the error message.
    let raw =
        hex::decode(&cleaned).with_context(|| "decryption key is not a hex string".to_string())?;
    let path = work_dir.join("decryption.key");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(&path)
        .map_err(|err| scratch_write_error(err, format!("create {}", path.display())))?;
    file.write_all(&raw)
        .map_err(|err| scratch_write_error(err, format!("write {}", path.display())))?;
    Ok(path)
}

#[cfg(test)]
mod tests;
